use std::path::PathBuf;

use url::Url;

use super::error::GitError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitLineChanges {
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestState {
    Open,
    Draft,
    Merged,
    Closed,
    Conflict,
}

impl PullRequestState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Draft => "Draft",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::Conflict => "Conflict",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitPullRequestStatus {
    Loading,
    NotApplicable,
    NoPullRequest,
    Unavailable,
    PullRequest { number: u64, state: PullRequestState, url: Url },
}

impl GitPullRequestStatus {
    pub fn label(&self) -> Option<String> {
        match self {
            Self::Loading => Some("PR …".into()),
            Self::NotApplicable => None,
            Self::NoPullRequest => Some("No PR".into()),
            Self::Unavailable => Some("PR unavailable".into()),
            Self::PullRequest { number, state, .. } => Some(format!("#{number} {}", state.label())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    Standard,
    Open,
    Draft,
    Merged,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTabStatus {
    pub directory: PathBuf,
    pub branch: Option<String>,
    pub head: String,
    pub is_linked_worktree: bool,
    pub has_changes: bool,
    pub has_conflicts: bool,
    pub ahead: u64,
    pub behind: u64,
    pub line_changes: Option<GitLineChanges>,
    pub pull_request: GitPullRequestStatus,
    pub pull_request_stale: bool,
    pub owner_avatar_url: Option<Url>,
}

impl GitTabStatus {
    pub fn titlebar_branch(&self) -> String {
        match &self.branch {
            Some(b) => b.clone(),
            None => format!("Detached {}", self.head.chars().take(7).collect::<String>()),
        }
    }

    pub fn indicator(&self) -> Indicator {
        if self.has_conflicts {
            return Indicator::Conflict;
        }
        match &self.pull_request {
            GitPullRequestStatus::PullRequest { state, .. } => match state {
                PullRequestState::Open => Indicator::Open,
                PullRequestState::Draft => Indicator::Draft,
                PullRequestState::Merged => Indicator::Merged,
                PullRequestState::Conflict => Indicator::Conflict,
                PullRequestState::Closed => Indicator::Standard,
            },
            _ => Indicator::Standard,
        }
    }

    /// Keeps the previous pull request lookup when the same branch in the same checkout
    /// is refreshed, so the titlebar does not flicker back to a loading state.
    pub fn preserve_pull_request(&mut self, old: Option<&GitTabStatus>) {
        let Some(old) = old else { return };
        if old.directory == self.directory
            && old.branch == self.branch
            && old.pull_request != GitPullRequestStatus::NotApplicable
            && self.pull_request != GitPullRequestStatus::NotApplicable
        {
            self.pull_request = old.pull_request.clone();
            self.pull_request_stale = old.pull_request_stale;
        }
    }

    pub fn update_pull_request(&mut self, request: GitPullRequestStatus) {
        self.pull_request_stale = request == GitPullRequestStatus::Unavailable
            && matches!(self.pull_request, GitPullRequestStatus::PullRequest { .. } | GitPullRequestStatus::NoPullRequest);
        if !self.pull_request_stale {
            self.pull_request = request;
        }
    }

    pub fn detail(&self) -> String {
        let mut lines = vec![
            if self.is_linked_worktree { "Linked worktree" } else { "Main repository checkout" }.to_string(),
            self.directory.display().to_string(),
            match &self.branch {
                Some(b) => format!("Branch: {b}"),
                None => format!("Detached HEAD: {}", self.head),
            },
            if self.has_changes { "Uncommitted changes" } else { "Working tree clean" }.to_string(),
        ];
        if self.has_conflicts {
            lines.push("Unresolved merge conflicts in working tree".into());
        }
        match self.line_changes {
            Some(c) if c.additions > 0 || c.deletions > 0 => {
                lines.push(format!("{} lines added, {} lines deleted", c.additions, c.deletions));
            }
            Some(_) => {}
            None => lines.push("Line changes unavailable".into()),
        }
        if self.ahead > 0 {
            lines.push(format!("{} commits ahead of upstream", self.ahead));
        }
        if self.behind > 0 {
            lines.push(format!("{} commits behind upstream", self.behind));
        }
        if let Some(label) = self.pull_request.label() {
            lines.push(label);
        }
        if self.pull_request_stale {
            lines.push("GitHub refresh failed; showing last known PR status. Retrying automatically.".into());
        }
        if self.pull_request == GitPullRequestStatus::Unavailable {
            lines.push(
                "Check connectivity and GitHub access. For private repositories, use gh auth login or set GH_TOKEN / GITHUB_TOKEN."
                    .into(),
            );
        }
        lines.join("\n")
    }

    /// Parses `git status --porcelain=v2 --branch -z` output.
    pub fn parse(
        output: &str,
        directory: PathBuf,
        is_linked_worktree: bool,
        has_remote: bool,
    ) -> Result<Self, GitError> {
        let records: Vec<&str> = output.split('\0').filter(|r| !r.is_empty()).collect();
        // Only leading records are headers: a rename's second path can begin with '# '.
        let header_count = records.iter().take_while(|r| r.starts_with("# ")).count();
        let headers = &records[..header_count];

        let find = |prefix: &str| headers.iter().find_map(|h| h.strip_prefix(prefix));
        let (Some(head), Some(name)) = (find("# branch.oid "), find("# branch.head ")) else {
            return Err(GitError::new("Couldn't read Git's branch status."));
        };

        let (mut ahead, mut behind) = (0, 0);
        if let Some(tracking) = find("# branch.ab ") {
            let err = || GitError::new("Couldn't read Git's upstream status.");
            let counts: Vec<&str> = tracking.split(' ').filter(|c| !c.is_empty()).collect();
            if counts.len() != 2 {
                return Err(err());
            }
            let added: i64 = counts[0].parse().map_err(|_| err())?;
            let removed: i64 = counts[1].parse().map_err(|_| err())?;
            if added < 0 || removed > 0 || removed == i64::MIN {
                return Err(err());
            }
            ahead = added as u64;
            behind = (-removed) as u64;
        }

        let detached = name == "(detached)";
        let mut has_conflicts = false;
        let mut index = header_count;
        while index < records.len() {
            if records[index].starts_with("u ") {
                has_conflicts = true;
            }
            if records[index].starts_with("2 ") {
                index += 1;
            }
            index += 1;
        }

        Ok(Self {
            directory,
            branch: if detached { None } else { Some(name.to_string()) },
            head: head.to_string(),
            is_linked_worktree,
            has_changes: records.len() > header_count,
            has_conflicts,
            ahead,
            behind,
            line_changes: Some(GitLineChanges { additions: 0, deletions: 0 }),
            pull_request: if detached || !has_remote {
                GitPullRequestStatus::NotApplicable
            } else {
                GitPullRequestStatus::Loading
            },
            owner_avatar_url: None,
            pull_request_stale: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(output: &str, linked: bool, remote: bool) -> Result<GitTabStatus, GitError> {
        GitTabStatus::parse(output, PathBuf::from("/repo"), linked, remote)
    }

    fn pr_url() -> Url {
        Url::parse("https://github.com/owner/repo/pull/42").unwrap()
    }

    #[test]
    fn line_changes_appear_in_detail() {
        let mut status = parse("# branch.oid abcdef123456\0# branch.head main\0", false, false).unwrap();
        assert_eq!(status.line_changes, Some(GitLineChanges { additions: 0, deletions: 0 }));
        status.line_changes = Some(GitLineChanges { additions: 123, deletions: 465 });
        assert!(status.detail().contains("123 lines added, 465 lines deleted"));
        status.line_changes = None;
        assert!(status.detail().contains("Line changes unavailable"));
    }

    #[test]
    fn reads_branch_changes_and_tracking_counts() {
        let status = parse(
            "# branch.oid abcdef123456\0# branch.head feature/test\0# branch.upstream origin/main\0# branch.ab +3 -2\0? file\nwith newline\0",
            true,
            true,
        )
        .unwrap();
        assert_eq!(status.branch.as_deref(), Some("feature/test"));
        assert!(status.is_linked_worktree);
        assert!(status.has_changes);
        assert_eq!((status.ahead, status.behind), (3, 2));
        assert_eq!(status.titlebar_branch(), "feature/test");
    }

    #[test]
    fn distinguishes_detached_and_unborn_branches() {
        let detached = parse("# branch.oid abcdef123456\0# branch.head (detached)\0", false, true).unwrap();
        assert_eq!(detached.branch, None);
        assert!(detached.titlebar_branch().contains("abcdef1"));
        assert_eq!(detached.pull_request, GitPullRequestStatus::NotApplicable);
        let unborn = parse("# branch.oid (initial)\0# branch.head main\0", false, false).unwrap();
        assert_eq!(unborn.branch.as_deref(), Some("main"));
        assert!(!unborn.has_changes);
        assert_eq!(unborn.pull_request, GitPullRequestStatus::NotApplicable);
    }

    #[test]
    fn rename_paths_cannot_spoof_branch_headers() {
        let status = parse(
            "# branch.oid abcdef123456\0# branch.head main\02 R. N... 100644 100644 100644 abc abc R100 new\0# branch.head spoofed\0",
            false,
            false,
        )
        .unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert!(status.has_changes);
        assert!(!status.has_conflicts);
    }

    #[test]
    fn unresolved_conflicts_use_conflict_indicator() {
        let mut status = parse(
            "# branch.oid abcdef123456\0# branch.head feature\0u UU N... 100644 100644 100644 100644 abc abc abc file\0",
            true,
            false,
        )
        .unwrap();
        assert!(status.has_conflicts);
        assert_eq!(status.indicator(), Indicator::Conflict);
        status.pull_request = GitPullRequestStatus::PullRequest { number: 42, state: PullRequestState::Merged, url: pr_url() };
        assert_eq!(status.indicator(), Indicator::Conflict);

        let renamed = parse(
            "# branch.oid abcdef123456\0# branch.head main\02 R. N... 100644 100644 100644 abc abc R100 new\0u original-name\0",
            false,
            false,
        )
        .unwrap();
        assert!(!renamed.has_conflicts);
    }

    #[test]
    fn indicator_follows_pull_request_state() {
        let mut status = parse("# branch.oid abcdef123456\0# branch.head feature\0", true, true).unwrap();
        for request in [
            GitPullRequestStatus::Loading,
            GitPullRequestStatus::NotApplicable,
            GitPullRequestStatus::NoPullRequest,
            GitPullRequestStatus::Unavailable,
        ] {
            status.pull_request = request;
            assert_eq!(status.indicator(), Indicator::Standard);
        }
        for (state, indicator) in [
            (PullRequestState::Open, Indicator::Open),
            (PullRequestState::Draft, Indicator::Draft),
            (PullRequestState::Merged, Indicator::Merged),
            (PullRequestState::Conflict, Indicator::Conflict),
            (PullRequestState::Closed, Indicator::Standard),
        ] {
            status.pull_request = GitPullRequestStatus::PullRequest { number: 42, state, url: pr_url() };
            assert_eq!(status.indicator(), indicator);
        }
    }

    #[test]
    fn rejects_malformed_status() {
        assert!(parse("unexpected output", false, false).is_err());
    }

    #[test]
    fn adding_remote_starts_lookup_and_branch_changes_discard_previous_pr() {
        let output = "# branch.oid abcdef123456\0# branch.head feature\0";
        let mut old = parse(output, false, false).unwrap();
        let mut status = parse(output, false, true).unwrap();
        status.preserve_pull_request(Some(&old));
        assert_eq!(status.pull_request, GitPullRequestStatus::Loading);

        old.pull_request = GitPullRequestStatus::NoPullRequest;
        status.preserve_pull_request(Some(&old));
        assert_eq!(status.pull_request, GitPullRequestStatus::NoPullRequest);

        let mut other = parse("# branch.oid abcdef123456\0# branch.head other\0", false, true).unwrap();
        other.preserve_pull_request(Some(&old));
        assert_eq!(other.pull_request, GitPullRequestStatus::Loading);

        let mut no_remote = parse(output, false, false).unwrap();
        no_remote.preserve_pull_request(Some(&old));
        assert_eq!(no_remote.pull_request, GitPullRequestStatus::NotApplicable);
    }

    #[test]
    fn unavailable_tooltip_describes_api_authentication() {
        let mut status = parse("# branch.oid abcdef123456\0# branch.head feature\0", false, true).unwrap();
        status.pull_request = GitPullRequestStatus::Unavailable;
        assert!(status.detail().contains("GITHUB_TOKEN"));
        assert!(!status.detail().contains("requires gh"));
    }

    #[test]
    fn failed_pr_refresh_preserves_known_result_and_marks_it_stale_until_recovery() {
        let mut status = parse("# branch.oid abc\0# branch.head feature\0", false, true).unwrap();
        let known = GitPullRequestStatus::PullRequest { number: 42, state: PullRequestState::Open, url: pr_url() };
        for request in [known, GitPullRequestStatus::NoPullRequest] {
            status.update_pull_request(request.clone());
            status.update_pull_request(GitPullRequestStatus::Unavailable);
            assert_eq!(status.pull_request, request);
            assert!(status.pull_request_stale);
            assert!(status.detail().contains("last known PR status"));
            let mut refreshed = parse("# branch.oid def\0# branch.head feature\0", false, true).unwrap();
            refreshed.preserve_pull_request(Some(&status));
            assert!(refreshed.pull_request_stale);
            status.update_pull_request(GitPullRequestStatus::NoPullRequest);
            assert!(!status.pull_request_stale);
        }
        let mut new_branch = parse("# branch.oid abc\0# branch.head other\0", false, true).unwrap();
        new_branch.preserve_pull_request(Some(&status));
        new_branch.update_pull_request(GitPullRequestStatus::Unavailable);
        assert_eq!(new_branch.pull_request, GitPullRequestStatus::Unavailable);
        assert!(!new_branch.pull_request_stale);
    }
}
