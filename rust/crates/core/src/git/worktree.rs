use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use url::Url;

use super::error::GitError;
use super::pull_request::repository_name;
use super::runner;
use super::status::{GitLineChanges, GitTabStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktree {
    pub directory: PathBuf,
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_bare: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
}

impl GitWorktree {
    pub fn can_remove(&self) -> bool {
        !self.is_main && !self.is_bare && !self.is_locked
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktreeSnapshot {
    pub directory: PathBuf,
    pub worktrees: Vec<GitWorktree>,
    pub branches: Vec<String>,
}

/// The default folder name for a worktree of `branch`: separators are flattened.
pub fn default_folder_name(branch: &str) -> String {
    branch.replace('/', "-")
}

pub fn resolve_symlinks(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

const SHORT: Duration = Duration::from_secs(5);
const NUMSTAT_LIMIT: usize = 1_048_576;
const MAX_UNTRACKED_FILES: usize = 1_000;
const MAX_UNTRACKED_BYTES: usize = 16 * 1024 * 1024;
const UNTRACKED_DEADLINE: Duration = Duration::from_secs(2);

/// Git operations over worktrees and tab status. Worktree operations are serialized;
/// read-only status lookups may run independently for different checkouts.
pub struct GitService {
    git: PathBuf,
    serial: Mutex<()>,
}

impl Default for GitService {
    fn default() -> Self {
        Self::new("git")
    }
}

impl GitService {
    pub fn new(git: impl Into<PathBuf>) -> Self {
        Self { git: git.into(), serial: Mutex::new(()) }
    }

    pub fn load(&self, directory: &Path) -> Result<GitWorktreeSnapshot, GitError> {
        let _guard = self.serial.lock().unwrap();
        self.load_unlocked(directory)
    }

    fn load_unlocked(&self, directory: &Path) -> Result<GitWorktreeSnapshot, GitError> {
        let output = self.git(&["worktree", "list", "--porcelain", "-z"], directory, None, None)?;
        let worktrees = parse_worktrees(&output)?;
        let main = worktrees.first().ok_or_else(|| GitError::new("Git returned no worktrees."))?;
        let branches = self
            .git(&["for-each-ref", "--format=%(refname:strip=2)", "refs/heads/"], directory, None, None)?
            .split('\n')
            .filter(|b| !b.is_empty())
            .map(str::to_string)
            .collect();
        Ok(GitWorktreeSnapshot { directory: main.directory.clone(), worktrees, branches })
    }

    /// Discovers repositories from terminal directories and from the worktree root,
    /// where created worktrees live at `<root>/<repository>/<worktree>`.
    pub fn load_repositories(&self, directories: &[PathBuf], worktree_root: &Path) -> Vec<GitWorktreeSnapshot> {
        let _guard = self.serial.lock().unwrap();
        fn folders(directory: &Path) -> Vec<PathBuf> {
            let Ok(entries) = std::fs::read_dir(directory) else { return Vec::new() };
            let mut result: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            result.sort();
            result
        }

        let groups = folders(worktree_root);
        let mut candidates: Vec<PathBuf> = directories.to_vec();
        candidates.extend(groups.iter().cloned());
        candidates.extend(groups.iter().flat_map(|g| folders(g)));

        let mut repositories: HashMap<PathBuf, GitWorktreeSnapshot> = HashMap::new();
        let mut visited: HashSet<PathBuf> = HashSet::new();
        for candidate in candidates {
            let directory = resolve_symlinks(&candidate);
            if !visited.insert(directory.clone()) {
                continue;
            }
            let Ok(snapshot) = self.load_unlocked(&directory) else { continue };
            visited.extend(snapshot.worktrees.iter().map(|w| w.directory.clone()));
            repositories.insert(snapshot.directory.clone(), snapshot);
        }
        let mut result: Vec<_> = repositories.into_values().collect();
        result.sort_by(|a, b| natural_cmp(&a.directory.to_string_lossy(), &b.directory.to_string_lossy()));
        result
    }

    /// Branch, change and tracking status of the checkout containing `directory`, or
    /// `None` when it isn't inside a work tree.
    pub fn tab_status(&self, directory: &Path) -> Result<Option<GitTabStatus>, GitError> {
        let inside = runner::run(
            &self.git,
            &["rev-parse".into(), "--is-inside-work-tree".into()],
            Some(directory),
            Some(SHORT),
            Some(NUMSTAT_LIMIT),
        )?;
        if inside.exit_code == 128 && inside.output.starts_with("fatal: not a git repository") {
            return Ok(None);
        }
        if inside.exit_code != 0 {
            return Err(GitError::new("Couldn't determine the checkout's Git status."));
        }
        if inside.output.trim() == "false" {
            return Ok(None);
        }
        if inside.output.trim() != "true" {
            return Err(GitError::new("Couldn't read Git's checkout status."));
        }
        let root = self.git(&["rev-parse", "--show-toplevel"], directory, Some(SHORT), None)?;
        let checkout = resolve_symlinks(Path::new(root.strip_suffix('\n').unwrap_or(&root)));
        let worktrees = parse_worktrees(&self.git(&["worktree", "list", "--porcelain", "-z"], &checkout, Some(SHORT), None)?)?;
        let Some(worktree) = worktrees.iter().find(|w| w.directory == checkout) else { return Ok(None) };
        let output = self.git(
            &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=normal"],
            &checkout,
            Some(SHORT),
            None,
        )?;
        let remotes = self.git(&["remote"], &checkout, Some(SHORT), None)?;
        let mut status = GitTabStatus::parse(&output, checkout.clone(), !worktree.is_main, !remotes.is_empty())?;

        let remote_names: Vec<&str> = remotes.split('\n').filter(|r| !r.is_empty()).collect();
        let remote = if remote_names.contains(&"origin") { Some("origin") } else { remote_names.first().copied() };
        if let Some(remote) = remote {
            if let Ok(url) = self.git(&["remote", "get-url", "--", remote], &checkout, Some(SHORT), None) {
                if let Some(repository) = repository_name(url.trim()) {
                    if let Some(owner) = repository.split('/').next() {
                        status.owner_avatar_url = Url::parse(&format!("https://github.com/{owner}.png?size=32")).ok();
                    }
                }
            }
        }

        if status.has_changes {
            status.line_changes = self.line_changes(&checkout, &status.head).ok();
        }
        Ok(Some(status))
    }

    fn line_changes(&self, checkout: &Path, head: &str) -> Result<GitLineChanges, GitError> {
        // Compare the final working copy with HEAD so staged edits aren't counted twice.
        let base = if head == "(initial)" {
            self.git(&["hash-object", "-t", "tree", "/dev/null"], checkout, Some(SHORT), None)?.trim().to_string()
        } else {
            "HEAD".to_string()
        };
        let output = self.git(
            &[
                "diff", "--numstat", "-z", "--find-renames", "--no-ext-diff", "--no-textconv",
                "--ignore-submodules=all", &base, "--",
            ],
            checkout,
            Some(SHORT),
            Some(NUMSTAT_LIMIT),
        )?;
        let (additions, deletions) = parse_numstat(&output)?;
        let untracked = self.untracked_additions(checkout)?;
        let additions = additions
            .checked_add(untracked)
            .ok_or_else(|| GitError::new("Couldn't read Git's line changes."))?;
        Ok(GitLineChanges { additions, deletions })
    }

    fn untracked_additions(&self, directory: &Path) -> Result<u64, GitError> {
        let output = self.git(
            &["ls-files", "--others", "--exclude-standard", "-z"],
            directory,
            Some(SHORT),
            Some(NUMSTAT_LIMIT),
        )?;
        let paths: Vec<&str> = output.split('\0').filter(|p| !p.is_empty()).collect();
        if paths.len() > MAX_UNTRACKED_FILES {
            return Err(GitError::new("Too many untracked files to count line changes."));
        }
        let root = open_root(directory)?;
        let mut remaining = MAX_UNTRACKED_BYTES;
        let mut additions = 0u64;
        let deadline = Instant::now() + UNTRACKED_DEADLINE;
        let timed_out = || GitError::new("Counting untracked line changes timed out.");
        for path in paths {
            if Instant::now() >= deadline {
                return Err(timed_out());
            }
            let Some(mut file) = open_untracked_file(path, &root)? else { continue };
            if !file.metadata()?.is_file() {
                continue;
            }
            let (mut lines, mut bytes_read, mut last_byte) = (0u64, 0usize, None::<u8>);
            let mut buf = vec![0u8; 32_768];
            loop {
                let want = buf.len().min(remaining + 1);
                let n = file.read(&mut buf[..want])?;
                if n == 0 {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(timed_out());
                }
                if n > remaining {
                    return Err(GitError::new("Untracked files are too large to count line changes."));
                }
                remaining -= n;
                let chunk = &buf[..n];
                // Git identifies binary content by a NUL in its first 8,000 bytes.
                if bytes_read < 8_000 && chunk[..chunk.len().min(8_000 - bytes_read)].contains(&0) {
                    lines = 0;
                    last_byte = None;
                    break;
                }
                bytes_read += n;
                lines += chunk.iter().filter(|b| **b == b'\n').count() as u64;
                last_byte = chunk.last().copied();
            }
            additions += lines + u64::from(last_byte.is_some_and(|b| b != b'\n'));
        }
        Ok(additions)
    }

    /// Creates a worktree at `<root>/<repository>/<folder_name>` and returns its path.
    pub fn create(
        &self,
        repository: &Path,
        root: &Path,
        folder_name: &str,
        branch: &str,
        new_branch: bool,
    ) -> Result<PathBuf, GitError> {
        let _guard = self.serial.lock().unwrap();
        if folder_name.is_empty()
            || folder_name == "."
            || folder_name == ".."
            || folder_name.contains('/')
            || folder_name.contains('\0')
        {
            return Err(GitError::new("Enter a folder name without path separators."));
        }
        if branch.is_empty() || branch.starts_with('-') || branch == "HEAD" || branch.contains('\0') {
            return Err(GitError::new("Enter a valid branch name."));
        }
        self.git(&["check-ref-format", &format!("refs/heads/{branch}")], repository, None, None)?;
        if !new_branch {
            self.git(&["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")], repository, None, None)?;
        }

        let snapshot = self.load_unlocked(repository)?;
        let repo_name = snapshot
            .directory
            .file_name()
            .ok_or_else(|| GitError::new("Git returned no worktrees."))?;
        let parent = root.join(repo_name);
        let destination = parent.join(folder_name);
        // Even an empty directory may belong to the user; Git itself would reuse it.
        if std::fs::symlink_metadata(&destination).is_ok() {
            return Err(GitError::new("The destination already exists. Choose a different folder name."));
        }
        std::fs::create_dir_all(&parent)?;
        let dest = destination.to_string_lossy().into_owned();
        if new_branch {
            self.git(&["worktree", "add", "-b", branch, "--", &dest, "HEAD"], repository, None, None)?;
        } else {
            self.git(&["worktree", "add", "--", &dest, branch], repository, None, None)?;
        }
        Ok(resolve_symlinks(&destination))
    }

    /// Removes a clean, unlocked linked worktree. Never forces: Git re-checks dirty and
    /// untracked files, and ignored files are refused here because Git's own check
    /// excludes them.
    pub fn remove(&self, repository: &Path, directory: &Path) -> Result<(), GitError> {
        let _guard = self.serial.lock().unwrap();
        let snapshot = self.load_unlocked(repository)?;
        let target = resolve_symlinks(directory);
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|w| w.directory == target && w.can_remove())
            .ok_or_else(|| {
                GitError::new("The main checkout, bare repositories, and locked worktrees cannot be removed here. Refresh the list and try again.")
            })?;
        if target.exists() || !worktree.is_prunable {
            let ignored = self.git(
                &["ls-files", "--others", "--ignored", "--exclude-standard", "--directory", "-z"],
                &target,
                None,
                None,
            )?;
            if !ignored.is_empty() {
                return Err(GitError::new("This worktree contains ignored files, such as build output or local configuration. Move or remove those files before removing the worktree."));
            }
        }
        self.git(&["worktree", "remove", "--", &target.to_string_lossy()], &snapshot.directory, None, None)?;
        Ok(())
    }

    /// Drops administrative data of worktrees whose directories no longer exist.
    pub fn prune(&self, repository: &Path) -> Result<(), GitError> {
        let _guard = self.serial.lock().unwrap();
        self.git(&["worktree", "prune"], repository, None, None)?;
        Ok(())
    }

    fn git(
        &self,
        arguments: &[&str],
        directory: &Path,
        timeout: Option<Duration>,
        max_output: Option<usize>,
    ) -> Result<String, GitError> {
        let mut args = vec!["-C".to_string(), directory.to_string_lossy().into_owned()];
        args.extend(arguments.iter().map(|a| a.to_string()));
        let result = runner::run(&self.git, &args, None, timeout, max_output).map_err(|e| {
            GitError::new(format!(
                "Couldn't run Git. Install Git and try again.\n{}",
                e.message
            ))
        })?;
        if result.exit_code != 0 {
            let detail = result.output.trim();
            return Err(GitError::new(if detail.is_empty() {
                format!("Git couldn't complete the operation (exit {}).", result.exit_code)
            } else {
                detail.to_string()
            }));
        }
        Ok(result.output)
    }
}

/// Sums additions and deletions from `git diff --numstat -z`. Binary files count as
/// zero and renames carry two extra NUL-delimited paths.
pub fn parse_numstat(output: &str) -> Result<(u64, u64), GitError> {
    let err = || GitError::new("Couldn't read Git's line changes.");
    if !(output.is_empty() || output.ends_with('\0')) {
        return Err(err());
    }
    let records: Vec<&str> = output.split('\0').collect();
    let (mut additions, mut deletions) = (0u64, 0u64);
    let mut index = 0;
    while index + 1 < records.len() {
        let fields: Vec<&str> = records[index].splitn(3, '\t').collect();
        if fields.len() != 3 {
            return Err(err());
        }
        if fields[0] != "-" || fields[1] != "-" {
            let added: u64 = fields[0].parse().map_err(|_| err())?;
            let deleted: u64 = fields[1].parse().map_err(|_| err())?;
            additions = additions.checked_add(added).ok_or_else(err)?;
            deletions = deletions.checked_add(deleted).ok_or_else(err)?;
        }
        if fields[2].is_empty() {
            if index + 2 >= records.len() - 1 {
                return Err(GitError::new("Couldn't read Git's renamed paths."));
            }
            index += 2;
        }
        index += 1;
    }
    Ok((additions, deletions))
}

/// Parses `git worktree list --porcelain -z`. The first entry is the main checkout.
pub fn parse_worktrees(output: &str) -> Result<Vec<GitWorktree>, GitError> {
    let mut worktrees: Vec<GitWorktree> = Vec::new();
    let mut fields: Vec<&str> = Vec::new();
    for field in output.split('\0') {
        if !field.is_empty() {
            fields.push(field);
            continue;
        }
        if fields.is_empty() {
            continue;
        }
        let path = fields[0]
            .strip_prefix("worktree ")
            .ok_or_else(|| GitError::new("Couldn't read Git's worktree list."))?;
        let branch = fields
            .iter()
            .find_map(|f| f.strip_prefix("branch refs/heads/"))
            .map(str::to_string);
        worktrees.push(GitWorktree {
            directory: resolve_symlinks(Path::new(path)),
            branch,
            is_main: worktrees.is_empty(),
            is_bare: fields.contains(&"bare"),
            is_locked: fields.iter().any(|f| *f == "locked" || f.starts_with("locked ")),
            is_prunable: fields.iter().any(|f| *f == "prunable" || f.starts_with("prunable ")),
        });
        fields.clear();
    }
    Ok(worktrees)
}

/// Orders like Finder: digit runs compare numerically, case is ignored.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s.trim_start_matches('0').to_string()
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(&nb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().collect::<String>().cmp(&y.to_lowercase().collect::<String>());
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(unix)]
mod nofollow {
    use std::fs::File;
    use std::os::fd::FromRawFd;
    use std::path::Path;

    use crate::git::error::GitError;

    pub(super) struct Fd(libc::c_int);

    impl Drop for Fd {
        fn drop(&mut self) {
            unsafe { libc::close(self.0) };
        }
    }

    pub(super) fn open_root(directory: &Path) -> Result<Fd, GitError> {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(directory.as_os_str().as_bytes())
            .map_err(|_| GitError::new("Couldn't read Git's line changes."))?;
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Fd(fd))
    }

    /// Opens a path below `root` without following any symlink, or returns `None` for
    /// symlinks, missing entries and invalid components.
    pub(super) fn open_untracked_file(path: &str, root: &Fd) -> Result<Option<File>, GitError> {
        let components: Vec<&str> = path.split('/').collect();
        if components.is_empty() || components.iter().any(|c| c.is_empty() || *c == "." || *c == "..") {
            return Ok(None);
        }
        let mut parent = unsafe { libc::dup(root.0) };
        if parent < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        for (index, component) in components.iter().enumerate() {
            let flags = libc::O_RDONLY
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | if index < components.len() - 1 { libc::O_DIRECTORY } else { 0 };
            let name = std::ffi::CString::new(*component).map_err(|_| GitError::new("Couldn't read Git's line changes."))?;
            let descriptor = unsafe { libc::openat(parent, name.as_ptr(), flags) };
            unsafe { libc::close(parent) };
            if descriptor < 0 {
                let err = std::io::Error::last_os_error();
                return match err.raw_os_error() {
                    Some(libc::ELOOP) | Some(libc::ENOENT) | Some(libc::ENOTDIR) => Ok(None),
                    _ => Err(err.into()),
                };
            }
            parent = descriptor;
        }
        Ok(Some(unsafe { File::from_raw_fd(parent) }))
    }
}

#[cfg(not(unix))]
mod nofollow {
    use std::fs::File;
    use std::path::{Path, PathBuf};

    use crate::git::error::GitError;

    pub(super) struct Fd(PathBuf);

    pub(super) fn open_root(directory: &Path) -> Result<Fd, GitError> {
        let metadata = std::fs::symlink_metadata(directory)?;
        if metadata.is_symlink() || !metadata.is_dir() {
            return Err(GitError::new("Couldn't read Git's line changes."));
        }
        Ok(Fd(directory.to_path_buf()))
    }

    /// Opens a path below `root`, or returns `None` for symlinks and reparse points,
    /// missing entries and invalid components.
    pub(super) fn open_untracked_file(path: &str, root: &Fd) -> Result<Option<File>, GitError> {
        let components: Vec<&str> = path.split('/').collect();
        if components.is_empty() || components.iter().any(|c| c.is_empty() || *c == "." || *c == ".." || c.contains(['\\', ':'])) {
            return Ok(None);
        }
        let mut current = root.0.clone();
        for (index, component) in components.iter().enumerate() {
            current.push(component);
            let metadata = match std::fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            if metadata.is_symlink() || is_reparse_point(&metadata) {
                return Ok(None);
            }
            let last = index == components.len() - 1;
            if metadata.is_dir() == last {
                return Ok(None);
            }
        }
        Ok(Some(File::open(&current)?))
    }

    fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
}

use nofollow::{open_root, open_untracked_file};

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    struct Repository {
        dir: tempfile::TempDir,
        service: GitService,
    }

    impl Repository {
        fn new() -> Self {
            Self::with_initial_commit(true)
        }

        fn with_initial_commit(initial: bool) -> Self {
            let dir = tempfile::Builder::new().prefix("Angelly worktrees ").tempdir().unwrap();
            let repo = Self { dir, service: GitService::default() };
            std::fs::create_dir_all(repo.checkout()).unwrap();
            repo.git(&["init", "-q", "-b", "main"]);
            repo.git(&["config", "user.name", "Worktree Tests"]);
            repo.git(&["config", "user.email", "worktrees@example.invalid"]);
            repo.git(&["config", "core.hooksPath", "/dev/null"]);
            if initial {
                repo.git(&["commit", "--allow-empty", "-m", "Initial"]);
            }
            repo
        }

        fn checkout(&self) -> PathBuf {
            self.dir.path().join("repository")
        }

        fn root(&self) -> PathBuf {
            self.dir.path().join("worktrees")
        }

        fn git(&self, args: &[&str]) -> String {
            let out = Command::new("git")
                .arg("-C")
                .arg(self.checkout())
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap()
        }

        fn create(&self, folder: &str, branch: &str, new_branch: bool) -> Result<PathBuf, GitError> {
            self.service.create(&self.checkout(), &self.root(), folder, branch, new_branch)
        }

        fn write(&self, rel: &str, contents: impl AsRef<[u8]>) {
            let path = self.checkout().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }

        fn status(&self) -> GitTabStatus {
            self.service.tab_status(&self.checkout()).unwrap().unwrap()
        }
    }

    fn canon(p: &Path) -> PathBuf {
        resolve_symlinks(p)
    }

    #[test]
    fn default_folder_name_flattens_branch_separators() {
        assert_eq!(default_folder_name("feature/login/ui"), "feature-login-ui");
        assert_eq!(default_folder_name("main"), "main");
    }

    #[test]
    fn owner_avatar_uses_origin_and_follows_remote_changes() {
        for remote in ["git@github.com:owner/repo.git", "https://github.com/owner/repo.git", "ssh://git@github.com/owner/repo.git"] {
            let repo = Repository::new();
            repo.git(&["remote", "add", "upstream", "https://github.com/upstream/repo.git"]);
            repo.git(&["remote", "add", "origin", remote]);
            assert_eq!(repo.status().owner_avatar_url, Url::parse("https://github.com/owner.png?size=32").ok());

            repo.git(&["remote", "set-url", "origin", "https://github.com/new-owner/repo.git"]);
            repo.git(&["checkout", "--detach"]);
            assert_eq!(repo.status().owner_avatar_url, Url::parse("https://github.com/new-owner.png?size=32").ok());
        }
    }

    #[test]
    fn owner_avatar_supports_linked_worktrees_without_origin() {
        let repo = Repository::new();
        repo.git(&["remote", "add", "upstream", "https://github.com/owner/repo.git"]);
        let linked = repo.create("feature", "feature", true).unwrap();
        let status = repo.service.tab_status(&linked).unwrap().unwrap();
        assert_eq!(status.owner_avatar_url, Url::parse("https://github.com/owner.png?size=32").ok());
    }

    #[test]
    fn owner_avatar_is_absent_for_local_and_unsupported_repositories() {
        let repo = Repository::new();
        assert_eq!(repo.status().owner_avatar_url, None);
        repo.git(&["remote", "add", "origin", "https://git.example.invalid/owner/repo.git"]);
        assert_eq!(repo.status().owner_avatar_url, None);
    }

    #[test]
    fn discovers_repository_from_subdirectory_and_linked_checkout() {
        let repo = Repository::new();
        let nested = repo.checkout().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let initial = repo.service.load(&nested).unwrap();
        assert_eq!(initial.worktrees.len(), 1);
        assert!(initial.worktrees[0].is_main);
        assert_eq!(initial.branches, vec!["main"]);

        let path = repo.create("feature", "feature/test", true).unwrap();
        let linked = repo.service.load(&path).unwrap();
        assert_eq!(linked.directory, canon(&repo.checkout()));
        assert_eq!(linked.worktrees.len(), 2);
        assert!(linked.worktrees.iter().any(|w| w.directory == path && w.branch.as_deref() == Some("feature/test")));
    }

    #[test]
    fn discovers_repositories_from_terminals_and_worktree_root_without_duplicates() {
        let first = Repository::new();
        let second = Repository::new();
        let linked = first.create("feature", "feature", true).unwrap();
        let nested = second.checkout().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(first.root().join("not a repo/empty")).unwrap();

        let snapshots = first.service.load_repositories(&[linked, nested, second.checkout()], &first.root());
        let dirs: HashSet<_> = snapshots.iter().map(|s| s.directory.clone()).collect();
        assert_eq!(dirs, HashSet::from([canon(&first.checkout()), canon(&second.checkout())]));
        assert_eq!(snapshots.len(), 2);
        let primary = snapshots.iter().find(|s| s.directory == canon(&first.checkout())).unwrap();
        assert_eq!(primary.worktrees.len(), 2);

        let from_root = first.service.load_repositories(&[], &first.root());
        assert_eq!(from_root.iter().map(|s| s.directory.clone()).collect::<Vec<_>>(), vec![canon(&first.checkout())]);
    }

    #[test]
    fn missing_worktree_root_still_discovers_terminal_repository() {
        let repo = Repository::new();
        let snapshots = repo.service.load_repositories(&[repo.checkout()], &repo.root());
        assert_eq!(snapshots.iter().map(|s| s.directory.clone()).collect::<Vec<_>>(), vec![canon(&repo.checkout())]);
        assert!(repo.service.load_repositories(&[repo.dir.path().to_path_buf()], &repo.root()).is_empty());
    }

    #[test]
    fn creates_existing_branch_and_preserves_special_path_characters() {
        let repo = Repository::new();
        repo.git(&["branch", "existing"]);
        let path = repo.create("space ' quote\nline", "existing", false).unwrap();
        let snapshot = repo.service.load(&repo.checkout()).unwrap();
        assert!(snapshot.worktrees.iter().any(|w| w.directory == path && w.branch.as_deref() == Some("existing")));
    }

    #[test]
    fn new_branch_starts_at_source_checkout_head() {
        let repo = Repository::new();
        let source = repo.create("source", "source", true).unwrap();
        let git_in = |dir: &Path, args: &[&str]| -> String {
            let out = Command::new("git")
                .arg("-C").arg(dir).args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap()
        };
        git_in(&source, &["commit", "--allow-empty", "-m", "Linked checkout commit"]);
        let expected = git_in(&source, &["rev-parse", "HEAD"]);
        let destination = repo.service.create(&source, &repo.root(), "child", "child", true).unwrap();
        assert_eq!(git_in(&destination, &["rev-parse", "HEAD"]), expected);
    }

    #[test]
    fn refuses_removal_of_ignored_files() {
        let repo = Repository::new();
        repo.write(".gitignore", ".env\n");
        repo.git(&["add", ".gitignore"]);
        repo.git(&["commit", "-m", "Ignore local secrets"]);
        let path = repo.create("feature", "feature", true).unwrap();
        let secrets = path.join(".env");
        std::fs::write(&secrets, "keep").unwrap();
        assert!(repo.service.remove(&repo.checkout(), &path).is_err());
        assert!(secrets.exists());
    }

    #[test]
    fn refuses_duplicate_destination_and_checked_out_branch() {
        let repo = Repository::new();
        let path = repo.create("feature", "feature", true).unwrap();
        std::fs::write(path.join("untracked"), "keep").unwrap();
        assert!(repo.create("feature", "other", true).is_err());
        assert!(repo.create("main-again", "main", false).is_err());
        assert_eq!(std::fs::read_to_string(path.join("untracked")).unwrap(), "keep");
    }

    #[test]
    fn rejects_invalid_names_without_creating_branches() {
        let repo = Repository::new();
        for folder in ["..", ".", "../escape", "/absolute", ""] {
            assert!(repo.create(folder, "safe", true).is_err(), "{folder}");
        }
        for branch in ["--detach", "@{-1}", "bad name", "HEAD", ""] {
            assert!(repo.create("safe", branch, true).is_err(), "{branch}");
        }
        assert_eq!(repo.git(&["branch", "--format=%(refname:short)"]), "main\n");
    }

    #[test]
    fn removes_clean_worktree_and_keeps_branch() {
        let repo = Repository::new();
        let path = repo.create("feature", "feature", true).unwrap();
        repo.service.remove(&repo.checkout(), &path).unwrap();
        assert!(!path.exists());
        assert!(repo.git(&["branch", "--list", "feature"]).contains("feature"));
    }

    #[test]
    fn refuses_removal_of_dirty_locked_and_main_worktrees() {
        let repo = Repository::new();
        let path = repo.create("feature", "feature", true).unwrap();
        let file = path.join("untracked");
        std::fs::write(&file, "keep").unwrap();
        assert!(repo.service.remove(&repo.checkout(), &path).is_err());
        assert!(file.exists());
        std::fs::remove_file(&file).unwrap();
        repo.git(&["worktree", "lock", "--reason", "test", &path.to_string_lossy()]);
        assert!(repo.service.load(&repo.checkout()).unwrap().worktrees.last().unwrap().is_locked);
        assert!(repo.service.remove(&repo.checkout(), &path).is_err());
        assert!(repo.service.remove(&repo.checkout(), &repo.checkout()).is_err());
        assert!(path.exists());
    }

    #[test]
    fn prunes_worktrees_whose_directories_are_gone() {
        let repo = Repository::new();
        let path = repo.create("feature", "feature", true).unwrap();
        std::fs::remove_dir_all(&path).unwrap();
        let before = repo.service.load(&repo.checkout()).unwrap();
        assert!(before.worktrees.iter().any(|w| w.is_prunable));
        repo.service.prune(&repo.checkout()).unwrap();
        let after = repo.service.load(&repo.checkout()).unwrap();
        assert_eq!(after.worktrees.len(), 1);
    }

    #[test]
    fn rejects_non_repository() {
        let repo = Repository::new();
        assert!(repo.service.load(repo.dir.path()).is_err());
    }

    #[test]
    fn tab_status_distinguishes_main_linked_and_plain_directories() {
        let repo = Repository::new();
        let main = repo.status();
        assert!(!main.is_linked_worktree);
        assert_eq!(main.branch.as_deref(), Some("main"));
        assert!(!main.has_changes);
        assert_eq!(repo.service.tab_status(repo.dir.path()).unwrap(), None);

        let linked = repo.create("linked", "feature", true).unwrap();
        let nested = linked.join("nested\nfolder");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("file"), "change").unwrap();
        let status = repo.service.tab_status(&nested).unwrap().unwrap();
        assert_eq!(status.directory, linked);
        assert!(status.is_linked_worktree);
        assert_eq!(status.branch.as_deref(), Some("feature"));
        assert!(status.has_changes);
    }

    #[test]
    fn missing_git_or_directory_is_a_failure_not_a_non_repository() {
        let repo = Repository::new();
        assert!(GitService::new(repo.dir.path().join("missing-git")).tab_status(&repo.checkout()).is_err());
        assert!(repo.service.tab_status(&repo.dir.path().join("missing-directory")).is_err());
    }

    #[test]
    fn status_reads_do_not_wait_for_serial_worktree_operations() {
        let repo = Repository::new();
        let service = std::sync::Arc::new(GitService::default());
        let guard = service.serial.lock().unwrap();
        let reader = service.clone();
        let directory = repo.checkout();
        let (send, receive) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || send.send(reader.tab_status(&directory)).unwrap());
        let result = receive.recv_timeout(Duration::from_secs(3));
        drop(guard);
        thread.join().unwrap();
        assert!(result.unwrap().unwrap().is_some());
    }

    #[test]
    fn line_changes_compare_final_checkout_against_head_without_double_counting() {
        let repo = Repository::new();
        repo.write("tracked", "original\nkeep\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-m", "Tracked file"]);
        repo.write("tracked", "staged\nkeep\nextra\n");
        repo.git(&["add", "."]);
        repo.write("tracked", "final\nkeep\n");
        assert_eq!(repo.status().line_changes, Some(GitLineChanges { additions: 1, deletions: 1 }));

        repo.write("tracked", "original\nkeep\n");
        let restored = repo.status();
        assert!(restored.has_changes);
        assert_eq!(restored.line_changes, Some(GitLineChanges { additions: 0, deletions: 0 }));
    }

    #[test]
    fn line_changes_handle_renames_and_paths_containing_tabs_and_newlines() {
        let repo = Repository::new();
        let original = "old name\nwith\ttab";
        let renamed = "new name\nwith\ttab";
        let content: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        repo.write(original, &content);
        repo.git(&["add", "."]);
        repo.git(&["commit", "-m", "Original"]);
        repo.git(&["mv", "--", original, renamed]);
        repo.write(renamed, format!("{content}added\n"));
        assert_eq!(repo.status().line_changes, Some(GitLineChanges { additions: 1, deletions: 0 }));
    }

    #[test]
    fn line_changes_include_untracked_text_and_exclude_binary_ignored_and_symlink_targets() {
        let repo = Repository::new();
        repo.write("binary", [0u8, 10, 11]);
        repo.write(".gitignore", "ignored\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-m", "Binary and ignore rules"]);
        repo.write("binary", [0u8, 10, 11, 10]);
        repo.write("nested/untracked\nfile", "first\nlast");
        repo.write("nested/untracked binary", [0u8, 10, 10]);
        repo.write("ignored", "ignored\nlines\n");
        let outside = repo.dir.path().join("outside");
        std::fs::write(&outside, "outside\nlines\n").unwrap();
        std::os::unix::fs::symlink(&outside, repo.checkout().join("link")).unwrap();
        std::os::unix::fs::symlink(repo.dir.path(), repo.checkout().join("directory link")).unwrap();
        assert_eq!(repo.status().line_changes, Some(GitLineChanges { additions: 2, deletions: 0 }));
    }

    #[test]
    fn line_changes_count_staged_and_untracked_files_on_unborn_branches() {
        let repo = Repository::with_initial_commit(false);
        repo.write("staged", "one\ntwo\n");
        repo.git(&["add", "."]);
        repo.write("staged", "one\n");
        repo.write("untracked", "untracked");
        let status = repo.status();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.line_changes, Some(GitLineChanges { additions: 2, deletions: 0 }));
    }

    #[test]
    fn oversized_untracked_files_keep_branch_context_with_unavailable_line_changes() {
        let repo = Repository::new();
        repo.write("large", vec![10u8; 16 * 1024 * 1024 + 1]);
        let status = repo.status();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert!(status.has_changes);
        assert_eq!(status.line_changes, None);
        assert!(status.detail().contains("Line changes unavailable"));
    }

    #[test]
    fn numstat_reads_binary_and_rename_records_without_parsing_their_paths() {
        let output = "2\t3\tordinary\nname\0-\t-\tbinary\01\t0\t\0old\nname\0new\tname\0";
        assert_eq!(parse_numstat(output).unwrap(), (3, 3));
        for malformed in ["1\t2\tmissing terminator", "1\t0\t\0old\0", "-1\t0\tfile\0", "-\t1\tfile\0"] {
            assert!(parse_numstat(malformed).is_err(), "{malformed:?}");
        }
    }

    #[test]
    fn natural_ordering_compares_digit_runs_numerically() {
        assert_eq!(natural_cmp("a2", "a10"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("B", "a"), std::cmp::Ordering::Greater);
    }
}
