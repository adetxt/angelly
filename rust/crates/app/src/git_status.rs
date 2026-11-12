//! Git status of each tab's working directory, refreshed off the UI thread.
//!
//! Statuses refresh when the set of directories changes and at most every two seconds
//! otherwise. Pull request lookups run per branch after each refresh.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use angelly_core::git::{error::GitError, GitHubPullRequestService, GitPullRequestStatus, GitService, GitTabStatus};
use angelly_core::http::{HttpClient, HttpRequest, UreqClient};
use gpui_kit::*;
use url::Url;

use crate::workspace::{TabId, Workspace};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

pub struct GitStatuses {
    by_directory: HashMap<PathBuf, GitTabStatus>,
    tab_directory: HashMap<TabId, PathBuf>,
    refreshed_at: HashMap<PathBuf, Instant>,
    refreshing: HashSet<PathBuf>,
    errors: HashMap<PathBuf, String>,
    requested_pull_requests: HashSet<(PathBuf, String)>,
    avatars: HashMap<Url, Option<Arc<Image>>>,
    service: Arc<GitService>,
    pull_requests: Arc<GitHubPullRequestService>,
    /// Pull request and avatar lookups reach the network; off for the self test.
    pub network: bool,
}

impl GitStatuses {
    pub fn new(network: bool) -> Self {
        GitStatuses {
            by_directory: HashMap::new(),
            tab_directory: HashMap::new(),
            refreshed_at: HashMap::new(),
            refreshing: HashSet::new(),
            errors: HashMap::new(),
            requested_pull_requests: HashSet::new(),
            avatars: HashMap::new(),
            service: Arc::new(GitService::default()),
            pull_requests: Arc::new(GitHubPullRequestService::new()),
            network,
        }
    }

    pub fn directory_for(&self, tab: TabId) -> Option<&Path> {
        self.tab_directory.get(&tab).map(PathBuf::as_path)
    }

    pub fn status_for(&self, tab: TabId) -> Option<&GitTabStatus> {
        self.tab_directory.get(&tab).and_then(|dir| self.by_directory.get(dir))
    }

    pub fn error_for(&self, tab: TabId) -> Option<&str> {
        self.tab_directory.get(&tab).and_then(|dir| self.errors.get(dir)).map(String::as_str)
    }

    fn finish_refresh(&mut self, dir: PathBuf, result: Result<Option<GitTabStatus>, GitError>) -> bool {
        self.refreshing.remove(&dir);
        if !self.tab_directory.values().any(|live| live == &dir) {
            return false;
        }
        self.refreshed_at.insert(dir.clone(), Instant::now());
        match result {
            Ok(Some(mut status)) => {
                status.preserve_pull_request(self.by_directory.get(&dir));
                self.by_directory.insert(dir.clone(), status);
                self.errors.remove(&dir);
            }
            Ok(None) => {
                self.by_directory.remove(&dir);
                self.errors.remove(&dir);
            }
            Err(error) => {
                self.errors.insert(dir, error.to_string());
            }
        }
        true
    }

    /// The avatar of the repository owner, once fetched.
    pub fn avatar(&self, url: &Url) -> Option<Arc<Image>> {
        self.avatars.get(url).cloned().flatten()
    }
}

impl Default for GitStatuses {
    fn default() -> Self {
        Self::new(true)
    }
}

fn avatar_format(bytes: &[u8]) -> Option<ImageFormat> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some(ImageFormat::Png),
        [0xFF, 0xD8, ..] => Some(ImageFormat::Jpeg),
        _ => None,
    }
}

/// Downloads an avatar, following the one redirect GitHub answers with.
fn fetch_avatar(url: &Url) -> Option<Image> {
    let client = UreqClient::new();
    let mut response = client.get(&HttpRequest::get(url.as_str())).ok()?;
    if (300..400).contains(&response.status) {
        let location = Url::parse(response.header("location")?).ok()?;
        if location.scheme() != "https" {
            return None;
        }
        response = client.get(&HttpRequest::get(location.as_str())).ok()?;
    }
    if response.status != 200 {
        return None;
    }
    let format = avatar_format(&response.body)?;
    Some(Image::from_bytes(format, response.body))
}

impl Workspace {
    /// Working directory of each tab, when the shell reported an absolute one.
    fn tab_directories(&self, cx: &App) -> HashMap<TabId, PathBuf> {
        self.tabs
            .iter()
            .filter_map(|tab| {
                let cwd = tab.view.read(cx).cwd().filter(|p| p.is_absolute())
                    .or_else(|| self.chrome.git.directory_for(tab.id).map(Path::to_path_buf))?;
                Some((tab.id, cwd))
            })
            .collect()
    }

    /// Re-reads the tabs' directories and refreshes their git statuses when the
    /// directories changed, two seconds passed, or `force` is set.
    pub fn refresh_git(&mut self, force: bool, cx: &mut Context<Self>) {
        let directories = self.tab_directories(cx);
        let wanted: HashSet<PathBuf> = directories.values().cloned().collect();
        let git = &mut self.chrome.git;
        if git.tab_directory != directories {
            git.tab_directory = directories;
            cx.notify();
        }
        git.by_directory.retain(|dir, _| wanted.contains(dir));
        git.refreshed_at.retain(|dir, _| wanted.contains(dir));
        git.errors.retain(|dir, _| wanted.contains(dir));
        for dir in wanted {
            let git = &mut self.chrome.git;
            let stale = git.refreshed_at.get(&dir).is_none_or(|at| at.elapsed() >= REFRESH_INTERVAL);
            if git.refreshing.contains(&dir) || !(force || stale) {
                continue;
            }
            git.refreshing.insert(dir.clone());
            let service = git.service.clone();
            cx.spawn(async move |this, cx| {
                let directory = dir.clone();
                let result = cx.background_executor().spawn(async move { service.tab_status(&directory) }).await;
                this.update(cx, |this, cx| {
                    if !this.chrome.git.finish_refresh(dir.clone(), result) {
                        return;
                    }
                    this.refresh_network_lookups(&dir, cx);
                    cx.notify();
                }).ok();
            }).detach();
        }
    }

    /// Pull request status per branch and owner avatars for the current statuses.
    fn refresh_network_lookups(&mut self, directory: &Path, cx: &mut Context<Self>) {
        if !self.chrome.git.network {
            return;
        }
        if let Some(status) = self.chrome.git.by_directory.get(directory).cloned() {
            if let Some(url) = status.owner_avatar_url.clone() {
                self.fetch_avatar(url, cx);
            }
            let Some(branch) = status.branch.clone() else { return };
            if status.pull_request == GitPullRequestStatus::NotApplicable {
                return;
            }
            let key = (status.directory.clone(), branch);
            if !self.chrome.git.requested_pull_requests.insert(key.clone()) {
                return;
            }
            let service = self.chrome.git.pull_requests.clone();
            cx.spawn(async move |this, cx| {
                let (directory, branch) = key.clone();
                let request = cx.background_executor().spawn(async move { service.status(&directory, &branch) }).await;
                this.update(cx, |this, cx| {
                    let git = &mut this.chrome.git;
                    git.requested_pull_requests.remove(&key);
                    for status in git.by_directory.values_mut() {
                        if status.directory == key.0
                            && status.branch.as_deref() == Some(key.1.as_str())
                            && status.pull_request != GitPullRequestStatus::NotApplicable
                        {
                            status.update_pull_request(request.clone());
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn fetch_avatar(&mut self, url: Url, cx: &mut Context<Self>) {
        if self.chrome.git.avatars.contains_key(&url) {
            return;
        }
        self.chrome.git.avatars.insert(url.clone(), None);
        cx.spawn(async move |this, cx| {
            let target = url.clone();
            let image = cx.background_executor().spawn(async move { fetch_avatar(&target) }).await;
            this.update(cx, |this, cx| {
                if let Some(image) = image {
                    this.chrome.git.avatars.insert(url, Some(Arc::new(image)));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Checks the tabs' directories every second so a `cd` shows up in the titlebar quickly.
    pub fn start_git_polling(&mut self, cx: &mut Context<Self>) {
        self.refresh_git(false, cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            if this.update(cx, |this, cx| this.refresh_git(false, cx)).is_err() {
                break;
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{GitStatuses, GitTabStatus, GitPullRequestStatus, GitError};
    use std::path::{Path, PathBuf};

    fn status(directory: &Path) -> GitTabStatus {
        GitTabStatus::parse("# branch.oid abc123\0# branch.head feature\0", directory.into(), false, true).unwrap()
    }

    #[test]
    fn refresh_failure_keeps_status_until_successful_non_repository_result() {
        let mut git = GitStatuses::new(false);
        let dir = PathBuf::from("/repo");
        git.tab_directory.insert(1, dir.clone());
        let mut known = status(&dir);
        known.pull_request = GitPullRequestStatus::NoPullRequest;
        git.finish_refresh(dir.clone(), Ok(Some(known.clone())));
        git.finish_refresh(dir.clone(), Err(GitError::new("temporary failure")));
        assert_eq!(git.status_for(1), Some(&known));
        assert_eq!(git.error_for(1), Some("temporary failure"));

        git.finish_refresh(dir.clone(), Ok(Some(status(&dir))));
        assert_eq!(git.status_for(1), Some(&known));
        assert_eq!(git.error_for(1), None);
        git.finish_refresh(dir, Ok(None));
        assert_eq!(git.status_for(1), None);
    }

    #[test]
    fn completed_directory_publishes_without_waiting_for_other_directories() {
        let mut git = GitStatuses::new(false);
        let fast = PathBuf::from("/fast");
        let slow = PathBuf::from("/slow");
        git.tab_directory.insert(1, fast.clone());
        git.tab_directory.insert(2, slow.clone());
        git.refreshing.extend([fast.clone(), slow.clone()]);
        git.finish_refresh(fast.clone(), Ok(Some(status(&fast))));
        assert!(git.status_for(1).is_some());
        assert!(git.refreshing.contains(&slow));
        assert!(!git.refreshing.contains(&fast));
        git.tab_directory.remove(&2);
        assert!(!git.finish_refresh(slow.clone(), Ok(Some(status(&slow)))));
        assert!(!git.by_directory.contains_key(&slow));
        assert!(!git.refreshing.contains(&slow));
    }
}
