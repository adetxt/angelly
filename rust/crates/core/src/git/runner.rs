use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::error::GitError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub exit_code: i32,
    pub output: String,
}

/// Runs subprocesses for the Git services. Output goes through a private file so
/// descendants that inherit the output handle cannot keep a timed-out call waiting.
pub fn run(
    executable: &Path,
    arguments: &[String],
    directory: Option<&Path>,
    timeout: Option<Duration>,
    max_output_bytes: Option<usize>,
) -> Result<RunResult, GitError> {
    let scratch = Scratch::new()?;
    let output_path = scratch.path.join("output");
    let file = create_private_file(&output_path)?;

    let mut command = Command::new(executable);
    command
        .args(arguments)
        .env_clear()
        .envs(environment(std::env::vars_os()))
        .stdin(Stdio::null())
        .stdout(Stdio::from(file.try_clone()?))
        .stderr(Stdio::from(file));
    if let Some(dir) = directory {
        command.current_dir(dir);
    }
    let mut child = command.spawn()?;

    let deadline = timeout.map(|t| Instant::now() + t);
    let mut pause = Duration::from_millis(1);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(Duration::from_millis(20));
    };

    let mut reader = File::open(&output_path)?;
    let mut data = Vec::new();
    match max_output_bytes {
        Some(max) => {
            if max == usize::MAX {
                return Err(GitError::new("Invalid command output limit."));
            }
            (&mut reader).take(max as u64 + 1).read_to_end(&mut data)?;
            if data.len() > max {
                return Err(GitError::new("The command returned too much output."));
            }
        }
        None => {
            reader.read_to_end(&mut data)?;
        }
    }
    let output = String::from_utf8(data)
        .map_err(|_| GitError::new("The command returned text that isn't valid UTF-8."))?;
    Ok(RunResult { exit_code: status.code().unwrap_or(-1), output })
}

/// The environment for Git and `gh` subprocesses. Repository overrides are dropped so
/// commands cannot be redirected away from the tab's checkout.
pub fn environment<I>(inherited: I) -> HashMap<OsString, OsString>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    let mut env: HashMap<OsString, OsString> = inherited
        .into_iter()
        .filter(|(k, _)| {
            let key = k.to_string_lossy();
            !key.starts_with("GIT_") && key != "GH_REPO" && key != "GH_HOST"
        })
        .collect();
    for (k, v) in [
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_OPTIONAL_LOCKS", "0"),
        ("GH_PROMPT_DISABLED", "1"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
        ("NO_COLOR", "1"),
        ("LC_ALL", "C"),
    ] {
        env.insert(k.into(), v.into());
    }
    env
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self, GitError> {
        let path = std::env::temp_dir().join(format!("angelly-git-{}", uuid::Uuid::new_v4()));
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = fs::DirBuilder::new();
        builder.create(&path)?;
        Ok(Self { path })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn output_limit_rejects_oversized_results() {
        let big = run(Path::new("/usr/bin/printf"), &s(&["%4096s", ""]), None, Some(Duration::from_secs(1)), Some(1024));
        assert!(big.is_err());
        let small = run(Path::new("/usr/bin/printf"), &s(&["small"]), None, Some(Duration::from_secs(1)), Some(1024)).unwrap();
        assert_eq!(small.output, "small");
    }

    #[test]
    fn timeout_is_bounded() {
        let start = Instant::now();
        let result = run(Path::new("/bin/sleep"), &s(&["30"]), None, Some(Duration::from_millis(100)), None).unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn descendants_cannot_keep_timeout_waiting_for_output() {
        let start = Instant::now();
        let result = run(Path::new("/bin/sh"), &s(&["-c", "sleep 5 & wait"]), None, Some(Duration::from_millis(100)), None).unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn environment_ignores_repository_overrides_and_keeps_authentication() {
        let pairs = [("GIT_DIR", "/other"), ("GH_REPO", "owner/other"), ("GH_HOST", "other.invalid"), ("GH_TOKEN", "test-token")]
            .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let env = environment(pairs);
        assert!(!env.contains_key(&OsString::from("GIT_DIR")));
        assert!(!env.contains_key(&OsString::from("GH_REPO")));
        assert!(!env.contains_key(&OsString::from("GH_HOST")));
        assert_eq!(env[&OsString::from("GH_TOKEN")], OsString::from("test-token"));
        assert_eq!(env[&OsString::from("GH_PROMPT_DISABLED")], OsString::from("1"));
    }
}
