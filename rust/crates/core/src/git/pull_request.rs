use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use url::Url;

use super::runner;
use super::status::{GitPullRequestStatus, PullRequestState};
use crate::http::{HttpClient, HttpRequest, UreqClient};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    directory: PathBuf,
    branch: String,
}

#[derive(Clone)]
struct Entry {
    status: GitPullRequestStatus,
    expiry: Instant,
}

struct Pending {
    slot: Mutex<Option<Entry>>,
    ready: Condvar,
}

#[derive(Default)]
struct State {
    cache: HashMap<Key, Entry>,
    pending: HashMap<Key, Arc<Pending>>,
}

/// Looks up the pull request of a checkout's branch, preferring the `gh` CLI and
/// falling back to the public GitHub API. Results are cached and concurrent lookups
/// for the same branch share one request.
pub struct GitHubPullRequestService {
    executable: Option<PathBuf>,
    git: PathBuf,
    environment: HashMap<String, String>,
    http: Arc<dyn HttpClient>,
    state: Mutex<State>,
    api_unavailable_until: Mutex<Option<SystemTime>>,
}

const CLI_TTL: Duration = Duration::from_secs(15);
const ANONYMOUS_API_TTL: Duration = Duration::from_secs(300);

impl GitHubPullRequestService {
    pub fn new() -> Self {
        Self::with(None, std::env::vars().collect(), UreqClient::shared())
    }

    pub fn with(executable: Option<PathBuf>, environment: HashMap<String, String>, http: Arc<dyn HttpClient>) -> Self {
        Self {
            executable,
            git: PathBuf::from("git"),
            environment,
            http,
            state: Mutex::new(State::default()),
            api_unavailable_until: Mutex::new(None),
        }
    }

    pub fn git_executable(mut self, git: impl Into<PathBuf>) -> Self {
        self.git = git.into();
        self
    }

    fn api_token(&self) -> Option<&str> {
        ["GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .filter_map(|k| self.environment.get(*k))
            .map(String::as_str)
            .find(|v| !v.is_empty())
    }

    pub fn status(&self, directory: &Path, branch: &str) -> GitPullRequestStatus {
        let key = Key { directory: directory.to_path_buf(), branch: branch.to_string() };
        let waiting = {
            let mut state = self.state.lock().unwrap();
            let now = Instant::now();
            state.cache.retain(|_, e| e.expiry > now);
            if let Some(entry) = state.cache.get(&key) {
                return entry.status.clone();
            }
            match state.pending.get(&key) {
                Some(pending) => Some(pending.clone()),
                None => {
                    state.pending.insert(
                        key.clone(),
                        Arc::new(Pending { slot: Mutex::new(None), ready: Condvar::new() }),
                    );
                    None
                }
            }
        };

        if let Some(pending) = waiting {
            let mut slot = pending.slot.lock().unwrap();
            while slot.is_none() {
                slot = pending.ready.wait(slot).unwrap();
            }
            return slot.as_ref().unwrap().status.clone();
        }

        let entry = self.lookup(directory, branch);
        let pending = {
            let mut state = self.state.lock().unwrap();
            state.cache.insert(key.clone(), entry.clone());
            state.pending.remove(&key)
        };
        if let Some(pending) = pending {
            *pending.slot.lock().unwrap() = Some(entry.clone());
            pending.ready.notify_all();
        }
        entry.status
    }

    fn lookup(&self, directory: &Path, branch: &str) -> Entry {
        let executable = self.executable.clone().or_else(find_gh_executable);
        if let Some(executable) = executable {
            let args: Vec<String> = ["pr", "view", branch, "--json", "number,state,isDraft,url,headRefName,mergeable"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            if let Ok(result) = runner::run(&executable, &args, Some(directory), Some(Duration::from_secs(10)), None) {
                let status = parse_cli_output(&result.output, result.exit_code, branch);
                if status != GitPullRequestStatus::Unavailable {
                    return Entry { status, expiry: Instant::now() + CLI_TTL };
                }
            }
        }
        let status = self.api_status(directory, branch);
        // Anonymous GitHub access allows only 60 requests/hour, shared across all tabs.
        let ttl = if self.api_token().is_none() && status != GitPullRequestStatus::Unavailable {
            ANONYMOUS_API_TTL
        } else {
            CLI_TTL
        };
        Entry { status, expiry: Instant::now() + ttl }
    }

    fn git_output(&self, directory: &Path, args: &[&str]) -> Option<String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let result = runner::run(&self.git, &args, Some(directory), Some(Duration::from_secs(5)), Some(65_536)).ok()?;
        (result.exit_code == 0).then(|| result.output.trim().to_string())
    }

    fn api_status(&self, directory: &Path, branch: &str) -> GitPullRequestStatus {
        use GitPullRequestStatus::*;
        if self
            .api_unavailable_until
            .lock()
            .unwrap()
            .is_some_and(|until| until > SystemTime::now())
        {
            return Unavailable;
        }
        let Some(remote_output) = self.git_output(directory, &["remote"]) else { return Unavailable };
        let remotes: Vec<&str> = remote_output.split('\n').filter(|r| !r.is_empty()).collect();
        let has = |name: &str| remotes.contains(&name);

        let configured_head = self
            .git_output(directory, &["config", "--get", &format!("branch.{branch}.pushRemote")])
            .or_else(|| self.git_output(directory, &["config", "--get", "remote.pushDefault"]))
            .or_else(|| has("origin").then(|| "origin".to_string()))
            .or_else(|| self.git_output(directory, &["config", "--get", &format!("branch.{branch}.remote")]));

        let Some(base_remote) = ["upstream", "origin"]
            .into_iter()
            .find(|r| has(r))
            .or_else(|| remotes.first().copied())
        else {
            return Unavailable;
        };
        let head_remote = configured_head
            .filter(|r| has(r))
            .unwrap_or_else(|| if has("origin") { "origin".to_string() } else { base_remote.to_string() });

        let (Some(base_url), Some(head_url)) = (
            self.git_output(directory, &["remote", "get-url", "--", base_remote]),
            self.git_output(directory, &["remote", "get-url", "--", &head_remote]),
        ) else {
            return Unavailable;
        };
        let (Some(base), Some(head)) = (repository_name(&base_url), repository_name(&head_url)) else {
            return Unavailable;
        };
        let Some(owner) = head.split('/').next() else { return Unavailable };

        for state in ["open", "closed"] {
            let mut url = Url::parse(&format!("https://api.github.com/repos/{base}/pulls")).unwrap();
            url.query_pairs_mut()
                .append_pair("head", &format!("{owner}:{branch}"))
                .append_pair("state", state)
                .append_pair("sort", "created")
                .append_pair("direction", "desc")
                .append_pair("per_page", "1");
            let Some(data) = self.api_data(&url) else { return Unavailable };
            let Ok(requests) = serde_json::from_slice::<Vec<ApiPullRequest>>(&data) else { return Unavailable };
            let Some(mut request) = requests.into_iter().next() else { continue };
            if request.state != state || request.status(branch, &head, &base) == Unavailable {
                return Unavailable;
            }
            if state == "open" {
                // The list endpoint omits mergeability; the detail endpoint computes it.
                let detail_url = Url::parse(&format!("https://api.github.com/repos/{base}/pulls/{}", request.number)).unwrap();
                let Some(data) = self.api_data(&detail_url) else { return Unavailable };
                let Ok(detail) = serde_json::from_slice::<ApiPullRequest>(&data) else { return Unavailable };
                if detail.number != request.number {
                    return Unavailable;
                }
                request = detail;
            }
            return request.status(branch, &head, &base);
        }
        NoPullRequest
    }

    fn api_data(&self, url: &Url) -> Option<Vec<u8>> {
        let mut request = HttpRequest::get(url.as_str())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "Ghostty");
        if let Some(token) = self.api_token() {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = self.http.get(&request).ok()?;
        let limited = matches!(response.status, 403 | 429)
            && (response.status == 429
                || response.header("X-RateLimit-Remaining") == Some("0")
                || response.header("Retry-After").is_some());
        if limited {
            let now = SystemTime::now();
            let now_secs = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            let reset = response.header("X-RateLimit-Reset").and_then(|v| v.parse::<f64>().ok());
            let retry = response.header("Retry-After").and_then(|v| v.parse::<f64>().ok());
            let wait = (reset.map(|r| r - now_secs).unwrap_or(0.0)).max(retry.unwrap_or(0.0));
            let wait = if wait.is_finite() { wait.max(60.0) } else { 60.0 };
            *self.api_unavailable_until.lock().unwrap() = Some(now + Duration::from_secs_f64(wait));
        }
        (response.status == 200).then_some(response.body)
    }
}

impl Default for GitHubPullRequestService {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct ApiPullRequest {
    number: i64,
    state: String,
    draft: bool,
    merged_at: Option<String>,
    html_url: String,
    mergeable: Option<bool>,
    head: ApiHead,
}

#[derive(Deserialize)]
struct ApiHead {
    #[serde(rename = "ref")]
    reference: String,
    repo: Option<ApiRepository>,
}

#[derive(Deserialize)]
struct ApiRepository {
    full_name: String,
}

impl ApiPullRequest {
    fn status(&self, branch: &str, repository: &str, base: &str) -> GitPullRequestStatus {
        let unavailable = GitPullRequestStatus::Unavailable;
        let Ok(url) = Url::parse(&self.html_url) else { return unavailable };
        let head_matches = self
            .head
            .repo
            .as_ref()
            .is_some_and(|r| r.full_name.eq_ignore_ascii_case(repository));
        if self.number <= 0
            || self.head.reference != branch
            || !head_matches
            || url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url.path().eq_ignore_ascii_case(&format!("/{base}/pull/{}", self.number))
        {
            return unavailable;
        }
        let state = match self.state.as_str() {
            "open" => {
                if self.mergeable == Some(false) {
                    PullRequestState::Conflict
                } else if self.draft {
                    PullRequestState::Draft
                } else {
                    PullRequestState::Open
                }
            }
            "closed" => {
                if self.merged_at.is_none() {
                    PullRequestState::Closed
                } else {
                    PullRequestState::Merged
                }
            }
            _ => return unavailable,
        };
        GitPullRequestStatus::PullRequest { number: self.number as u64, state, url }
    }
}

/// `owner/repo` for a GitHub remote URL, or `None` for anything else.
pub fn repository_name(remote: &str) -> Option<String> {
    let value = match remote.strip_prefix("git@github.com:") {
        Some(rest) => format!("ssh://git@github.com/{rest}"),
        None => remote.to_string(),
    };
    let url = Url::parse(&value).ok()?;
    if !url.host_str()?.eq_ignore_ascii_case("github.com")
        || !["https", "ssh", "git"].contains(&url.scheme())
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let mut path = url.path();
    path = path.strip_suffix(".git").unwrap_or(path);
    let parts: Vec<&str> = path.split('/').collect();
    let allowed = |c: char| c.is_ascii_alphanumeric() || "-_.".contains(c);
    if parts.len() != 3
        || !parts[0].is_empty()
        || !parts[1..]
            .iter()
            .all(|p| !p.is_empty() && *p != "." && *p != ".." && p.chars().all(allowed))
    {
        return None;
    }
    Some(parts[1..].join("/"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliPullRequest {
    number: i64,
    state: String,
    is_draft: bool,
    url: String,
    head_ref_name: String,
    mergeable: Option<String>,
}

/// Interprets the output of `gh pr view --json ...`.
pub fn parse_cli_output(output: &str, exit_code: i32, branch: &str) -> GitPullRequestStatus {
    use GitPullRequestStatus::*;
    if exit_code != 0 {
        let prefix = "no pull requests found for branch ";
        let message = output.trim();
        if exit_code == 1 {
            if let Some(rest) = message.strip_prefix(prefix) {
                if let Ok(name) = serde_json::from_str::<String>(rest) {
                    if name.splitn(2, ':').last() == Some(branch) {
                        return NoPullRequest;
                    }
                }
            }
        }
        return Unavailable;
    }
    let Ok(request) = serde_json::from_str::<CliPullRequest>(output) else { return Unavailable };
    let Ok(url) = Url::parse(&request.url) else { return Unavailable };
    if request.number <= 0 || url.scheme() != "https" || request.head_ref_name != branch {
        return Unavailable;
    }
    let state = match request.state.as_str() {
        "OPEN" => {
            if request.mergeable.as_deref() == Some("CONFLICTING") {
                PullRequestState::Conflict
            } else if request.is_draft {
                PullRequestState::Draft
            } else {
                PullRequestState::Open
            }
        }
        "MERGED" => PullRequestState::Merged,
        "CLOSED" => PullRequestState::Closed,
        _ => return Unavailable,
    };
    PullRequest { number: request.number as u64, state, url }
}

fn find_gh_executable() -> Option<PathBuf> {
    let mut dirs: Vec<OsString> = if cfg!(windows) {
        std::env::var_os("ProgramFiles").map(|p| Path::new(&p).join("GitHub CLI").into_os_string()).into_iter().collect()
    } else {
        vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into()]
    };
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path).map(OsString::from));
    }
    let name = if cfg!(windows) { "gh.exe" } else { "gh" };
    dirs.into_iter().map(|d| Path::new(&d).join(name)).find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{FnHttpClient, HttpError, HttpResponse};
    use std::process::Command;
    use std::sync::Mutex as StdMutex;

    fn cli(output: &str) -> String {
        output.to_string()
    }

    fn label(output: &str, branch: &str) -> Option<String> {
        parse_cli_output(output, 0, branch).label()
    }

    #[test]
    fn github_merge_conflicts_override_open_and_draft_but_not_merged() {
        for draft in [false, true] {
            let out = format!(
                r#"{{"number":42,"state":"OPEN","isDraft":{draft},"mergeable":"CONFLICTING","url":"https://github.com/owner/repo/pull/42","headRefName":"feature"}}"#
            );
            assert_eq!(label(&out, "feature").as_deref(), Some("#42 Conflict"));
        }
        let merged = r#"{"number":42,"state":"MERGED","isDraft":false,"mergeable":"CONFLICTING","url":"https://github.com/owner/repo/pull/42","headRefName":"feature"}"#;
        assert_eq!(label(merged, "feature").as_deref(), Some("#42 Merged"));
    }

    #[test]
    fn reads_pull_request_state() {
        for (state, draft, expected) in [("OPEN", false, "Open"), ("OPEN", true, "Draft"), ("MERGED", false, "Merged"), ("CLOSED", false, "Closed")] {
            let out = format!(
                r#"{{"number":42,"state":"{state}","isDraft":{draft},"url":"https://github.com/owner/repo/pull/42","headRefName":"feature"}}"#
            );
            assert_eq!(label(&out, "feature"), Some(format!("#42 {expected}")));
        }
    }

    #[test]
    fn lookup_failure_is_different_from_no_pull_request() {
        use GitPullRequestStatus::*;
        assert_eq!(parse_cli_output("no pull requests found for branch \"feature\"\n", 1, "feature"), NoPullRequest);
        assert_eq!(parse_cli_output("no pull requests found for branch \"owner:feature\"\n", 1, "feature"), NoPullRequest);
        assert_eq!(parse_cli_output("no pull requests found for branch \"owner:other\"\n", 1, "feature"), Unavailable);
        for out in ["authentication required", "network timeout", "invalid JSON"] {
            assert_eq!(parse_cli_output(out, 1, "feature"), Unavailable);
        }
        assert_eq!(parse_cli_output("not JSON", 0, "feature"), Unavailable);
    }

    #[test]
    fn rejects_pull_request_for_another_branch() {
        let out = r#"{"number":42,"state":"OPEN","isDraft":false,"url":"https://github.com/owner/repo/pull/42","headRefName":"other"}"#;
        assert_eq!(parse_cli_output(out, 0, "feature"), GitPullRequestStatus::Unavailable);
    }

    #[test]
    fn parses_github_remotes() {
        for remote in [
            "https://github.com/owner/repo.git",
            "git@github.com:owner/repo.git",
            "ssh://git@github.com/owner/repo.git",
            "https://github.com/owner/repo",
        ] {
            assert_eq!(repository_name(remote).as_deref(), Some("owner/repo"), "{remote}");
        }
        for remote in [
            "https://github.com.evil.invalid/owner/repo",
            "git@evil.invalid:owner/repo.git",
            "file:///owner/repo",
            "https://github.com/owner/repo/extra",
            "https://github.com/owner/..",
            "https://github.com/owner/repo?query=1",
        ] {
            assert_eq!(repository_name(remote), None, "{remote}");
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn repository(remote: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "feature/test"]);
        git(dir.path(), &["remote", "add", "origin", remote]);
        dir
    }

    struct Api {
        requests: StdMutex<Vec<HttpRequest>>,
        list: String,
        closed_list: String,
        detail: String,
        code: u16,
        detail_code: u16,
        headers: Vec<(String, String)>,
    }

    impl Default for Api {
        fn default() -> Self {
            Self {
                requests: StdMutex::new(Vec::new()),
                list: "[]".into(),
                closed_list: "[]".into(),
                detail: "{}".into(),
                code: 200,
                detail_code: 200,
                headers: Vec::new(),
            }
        }
    }

    impl Api {
        fn client(self: &Arc<Self>) -> Arc<dyn HttpClient> {
            let api = self.clone();
            Arc::new(FnHttpClient(move |req: &HttpRequest| -> Result<HttpResponse, HttpError> {
                api.requests.lock().unwrap().push(req.clone());
                let url = Url::parse(&req.url).unwrap();
                let state = url.query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.into_owned());
                let is_list = url.path().ends_with("/pulls");
                let (body, status) = if is_list {
                    (if state.as_deref() == Some("closed") { &api.closed_list } else { &api.list }, api.code)
                } else {
                    (&api.detail, api.detail_code)
                };
                let mut response = HttpResponse::new(status, body.as_bytes());
                for (k, v) in &api.headers {
                    response = response.with_header(k, v);
                }
                Ok(response)
            }))
        }

        fn count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    fn api_pull(state: &str, draft: bool, merged: bool, mergeable: &str, branch: &str, owner: &str) -> String {
        format!(
            r#"{{"number":42,"state":"{state}","draft":{draft},"merged_at":{},"mergeable":{mergeable},"html_url":"https://github.com/owner/repo/pull/42","head":{{"ref":"{branch}","repo":{{"full_name":"{owner}/repo"}}}}}}"#,
            if merged { "\"2026-01-01T00:00:00Z\"" } else { "null" }
        )
    }

    fn service(api: &Arc<Api>, env: &[(&str, &str)]) -> GitHubPullRequestService {
        GitHubPullRequestService::with(
            Some(PathBuf::from("/missing/gh")),
            env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            api.client(),
        )
    }

    #[test]
    fn missing_cli_falls_back_to_public_api_and_caches_result() {
        let repo = repository("git@github.com:owner/repo.git");
        let api = Arc::new(Api::default());
        let svc = Arc::new(service(&api, &[]));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let svc = svc.clone();
                let dir = repo.path().to_path_buf();
                std::thread::spawn(move || svc.status(&dir, "feature/test"))
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), GitPullRequestStatus::NoPullRequest);
        }
        assert_eq!(svc.status(repo.path(), "feature/test"), GitPullRequestStatus::NoPullRequest);
        assert_eq!(api.count(), 2);
        let requests = api.requests.lock().unwrap();
        let first = &requests[0];
        assert!(first.header_value("Authorization").is_none());
        let url = Url::parse(&first.url).unwrap();
        assert_eq!(url.host_str(), Some("api.github.com"));
        assert_eq!(url.path(), "/repos/owner/repo/pulls");
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["head"], "owner:feature/test");
        assert_eq!(query["state"], "open");
    }

    #[test]
    fn api_preserves_pull_request_states() {
        for (state, draft, merged, mergeable, expected) in [
            ("open", false, false, "true", "Open"),
            ("open", true, false, "true", "Draft"),
            ("open", true, false, "false", "Conflict"),
            ("open", false, false, "null", "Open"),
            ("closed", false, true, "false", "Merged"),
            ("closed", false, false, "true", "Closed"),
        ] {
            let repo = repository("git@github.com:owner/repo.git");
            let pull = api_pull(state, draft, merged, mergeable, "feature/test", "owner");
            let api = Arc::new(Api {
                list: if state == "open" { format!("[{pull}]") } else { "[]".into() },
                closed_list: if state == "closed" { format!("[{pull}]") } else { "[]".into() },
                detail: pull,
                ..Api::default()
            });
            let status = service(&api, &[]).status(repo.path(), "feature/test");
            assert_eq!(status.label(), Some(format!("#42 {expected}")));
        }
    }

    #[test]
    fn api_finds_fork_pull_request_and_uses_token() {
        let repo = repository("https://github.com/fork/repo.git");
        git(repo.path(), &["remote", "add", "upstream", "git@github.com:owner/repo.git"]);
        git(repo.path(), &["config", "branch.feature/test.remote", "upstream"]);
        git(repo.path(), &["config", "branch.feature/test.merge", "refs/heads/main"]);
        let pull = api_pull("open", false, false, "true", "feature/test", "fork");
        let api = Arc::new(Api { list: format!("[{pull}]"), detail: pull, ..Api::default() });
        let svc = GitHubPullRequestService::with(
            Some(PathBuf::from("/usr/bin/false")),
            [("GH_TOKEN", "gh-test"), ("GITHUB_TOKEN", "github-test")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            api.client(),
        );
        assert_eq!(svc.status(repo.path(), "feature/test").label().as_deref(), Some("#42 Open"));
        let requests = api.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for r in requests.iter() {
            assert_eq!(r.header_value("Authorization"), Some("Bearer gh-test"));
            assert!(Url::parse(&r.url).unwrap().path().starts_with("/repos/owner/repo/pulls"));
        }
        let url = Url::parse(&requests[0].url).unwrap();
        assert_eq!(url.query_pairs().find(|(k, _)| k == "head").unwrap().1, "fork:feature/test");
    }

    #[test]
    fn api_errors_are_unavailable() {
        for code in [401, 403, 404, 429, 500] {
            let repo = repository("git@github.com:owner/repo.git");
            let api = Arc::new(Api { code, ..Api::default() });
            let svc = service(&api, &[("GITHUB_TOKEN", "test-token")]);
            assert_eq!(svc.status(repo.path(), "feature/test"), GitPullRequestStatus::Unavailable);
            assert_eq!(api.requests.lock().unwrap()[0].header_value("Authorization"), Some("Bearer test-token"));
        }
    }

    #[test]
    fn malformed_or_unrelated_api_results_are_unavailable() {
        for list in [
            "not JSON".to_string(),
            "{}".to_string(),
            format!("[{}]", api_pull("open", false, false, "true", "other", "owner")),
            format!("[{}]", api_pull("open", false, false, "true", "feature/test", "other")),
        ] {
            let repo = repository("git@github.com:owner/repo.git");
            let api = Arc::new(Api { list, ..Api::default() });
            assert_eq!(service(&api, &[]).status(repo.path(), "feature/test"), GitPullRequestStatus::Unavailable);
        }
    }

    #[test]
    fn unsupported_remote_does_not_send_token() {
        let repo = repository("https://git.example.invalid/owner/repo");
        let api = Arc::new(Api::default());
        let svc = service(&api, &[("GH_TOKEN", "secret")]);
        assert_eq!(svc.status(repo.path(), "feature/test"), GitPullRequestStatus::Unavailable);
        assert_eq!(api.count(), 0);
    }

    #[test]
    fn successful_cli_does_not_call_api() {
        use std::os::unix::fs::PermissionsExt;
        for no_pr in [false, true] {
            let repo = repository("git@github.com:owner/repo.git");
            let output = if no_pr {
                cli("no pull requests found for branch \"feature/test\"\n")
            } else {
                cli(r#"{"number":42,"state":"OPEN","isDraft":false,"url":"https://github.com/owner/repo/pull/42","headRefName":"feature/test"}"#)
            };
            let script = repo.path().join("gh");
            std::fs::write(repo.path().join("gh-output"), output).unwrap();
            std::fs::write(&script, format!("#!/bin/sh\n/bin/cat \"$0-output\"\nexit {}\n", if no_pr { 1 } else { 0 })).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
            let api = Arc::new(Api::default());
            let svc = GitHubPullRequestService::with(Some(script), HashMap::new(), api.client());
            let status = svc.status(repo.path(), "feature/test");
            assert_eq!(status.label().as_deref(), Some(if no_pr { "No PR" } else { "#42 Open" }));
            assert_eq!(api.count(), 0);
        }
    }

    #[test]
    fn api_detail_failure_is_unavailable() {
        for detail_code in [200, 503] {
            let repo = repository("git@github.com:owner/repo.git");
            let api = Arc::new(Api {
                list: format!("[{}]", api_pull("open", false, false, "true", "feature/test", "owner")),
                detail_code,
                ..Api::default()
            });
            assert_eq!(service(&api, &[]).status(repo.path(), "feature/test"), GitPullRequestStatus::Unavailable);
            assert_eq!(api.count(), 2);
        }
    }

    #[test]
    fn api_rate_limit_backs_off_across_branches() {
        let repo = repository("git@github.com:owner/repo.git");
        let reset = (SystemTime::now() + Duration::from_secs(600)).duration_since(UNIX_EPOCH).unwrap().as_secs();
        let api = Arc::new(Api {
            code: 403,
            headers: vec![
                ("X-RateLimit-Remaining".into(), "0".into()),
                ("X-RateLimit-Reset".into(), reset.to_string()),
            ],
            ..Api::default()
        });
        let svc = service(&api, &[]);
        assert_eq!(svc.status(repo.path(), "feature/test"), GitPullRequestStatus::Unavailable);
        assert_eq!(svc.status(repo.path(), "other"), GitPullRequestStatus::Unavailable);
        assert_eq!(api.count(), 1);
    }

    #[test]
    fn api_uses_configured_push_remote() {
        let repo = repository("git@github.com:owner/repo.git");
        git(repo.path(), &["remote", "add", "contributor", "ssh://git@github.com/fork/repo.git"]);
        git(repo.path(), &["config", "branch.feature/test.pushRemote", "contributor"]);
        let pull = api_pull("open", false, false, "true", "feature/test", "fork");
        let api = Arc::new(Api { list: format!("[{pull}]"), detail: pull, ..Api::default() });
        assert_eq!(service(&api, &[]).status(repo.path(), "feature/test").label().as_deref(), Some("#42 Open"));
    }
}
