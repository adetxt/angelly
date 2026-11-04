import AppKit
import Foundation
import Testing
@testable import Ghostty

@Suite
struct GitTabStatusTests {
    @Test func pullRequestIconsAreAvailableAndTintable() throws {
        for indicator in [GitTabStatus.Indicator.open, .merged, .conflict] {
            let name = try #require(indicator.assetName)
            let icon = try #require(NSImage(named: name))
            #expect(icon.isTemplate)
        }
    }

    @Test func readsBranchChangesAndTrackingCounts() throws {
        let status = try GitTabStatus.parse(
            "# branch.oid abcdef123456\0# branch.head feature/test\0# branch.upstream origin/main\0# branch.ab +3 -2\0? file\nwith newline\0",
            directory: URL(fileURLWithPath: "/repo"), isLinkedWorktree: true, hasRemote: true)
        #expect(status.branch == "feature/test")
        #expect(status.isLinkedWorktree)
        #expect(status.hasChanges)
        #expect(status.ahead == 3)
        #expect(status.behind == 2)
        #expect(status.titlebarBranch == "feature/test")
    }

    @Test func distinguishesDetachedAndUnbornBranches() throws {
        let directory = URL(fileURLWithPath: "/repo")
        let detached = try GitTabStatus.parse("# branch.oid abcdef123456\0# branch.head (detached)\0",
                                             directory: directory, isLinkedWorktree: false, hasRemote: true)
        #expect(detached.branch == nil)
        #expect(detached.titlebarBranch.contains("abcdef1"))
        #expect(detached.pullRequest == .notApplicable)
        let unborn = try GitTabStatus.parse("# branch.oid (initial)\0# branch.head main\0",
                                           directory: directory, isLinkedWorktree: false, hasRemote: false)
        #expect(unborn.branch == "main")
        #expect(!unborn.hasChanges)
        #expect(unborn.pullRequest == .notApplicable)
    }

    @Test func renamePathsCannotSpoofBranchHeaders() throws {
        let status = try GitTabStatus.parse(
            "# branch.oid abcdef123456\0# branch.head main\0" +
            "2 R. N... 100644 100644 100644 abc abc R100 new\0# branch.head spoofed\0",
            directory: URL(fileURLWithPath: "/repo"), isLinkedWorktree: false, hasRemote: false)
        #expect(status.branch == "main")
        #expect(status.hasChanges)
        #expect(!status.hasConflicts)
    }

    @Test func unresolvedConflictsUseRedIndicatorWithoutAPullRequest() throws {
        var status = try GitTabStatus.parse(
            "# branch.oid abcdef123456\0# branch.head feature\0u UU N... 100644 100644 100644 100644 abc abc abc file\0",
            directory: URL(fileURLWithPath: "/repo"), isLinkedWorktree: true, hasRemote: false)
        #expect(status.hasConflicts)
        #expect(status.indicator == .conflict)
        status.pullRequest = .pullRequest(number: 42, state: .merged, url: URL(string: "https://github.com/owner/repo/pull/42")!)
        #expect(status.indicator == .conflict)

        let renamed = try GitTabStatus.parse(
            "# branch.oid abcdef123456\0# branch.head main\0" +
            "2 R. N... 100644 100644 100644 abc abc R100 new\0u original-name\0",
            directory: URL(fileURLWithPath: "/repo"), isLinkedWorktree: false, hasRemote: false)
        #expect(!renamed.hasConflicts)
    }

    @Test func indicatorFollowsPullRequestState() throws {
        var status = try GitTabStatus.parse("# branch.oid abcdef123456\0# branch.head feature\0",
                                           directory: URL(fileURLWithPath: "/repo"), isLinkedWorktree: true, hasRemote: true)
        for request in [GitPullRequestStatus.loading, .notApplicable, .noPullRequest, .unavailable] {
            status.pullRequest = request
            #expect(status.indicator == .standard)
        }
        let url = URL(string: "https://github.com/owner/repo/pull/42")!
        for (state, indicator) in [(GitPullRequestStatus.State.open, GitTabStatus.Indicator.open),
                                   (.draft, .draft), (.merged, .merged), (.conflict, .conflict), (.closed, .standard)] {
            status.pullRequest = .pullRequest(number: 42, state: state, url: url)
            #expect(status.indicator == indicator)
        }
    }

    @Test func githubMergeConflictsOverrideOpenAndDraftButNotMerged() {
        for draft in [false, true] {
            let output = """
            {"number":42,"state":"OPEN","isDraft":\(draft),"mergeable":"CONFLICTING","url":"https://github.com/owner/repo/pull/42","headRefName":"feature"}
            """
            #expect(GitHubPullRequestService.parse(output: output, exitCode: 0, branch: "feature").label == "#42 Conflict")
        }
        let merged = """
        {"number":42,"state":"MERGED","isDraft":false,"mergeable":"CONFLICTING","url":"https://github.com/owner/repo/pull/42","headRefName":"feature"}
        """
        #expect(GitHubPullRequestService.parse(output: merged, exitCode: 0, branch: "feature").label == "#42 Merged")
    }

    @Test func rejectsMalformedStatus() {
        #expect(throws: (any Error).self) {
            try GitTabStatus.parse("unexpected output", directory: URL(fileURLWithPath: "/repo"),
                                   isLinkedWorktree: false, hasRemote: false)
        }
    }

    @Test func addingRemoteStartsLookupAndBranchChangesDiscardPreviousPR() throws {
        let directory = URL(fileURLWithPath: "/repo")
        let output = "# branch.oid abcdef123456\0# branch.head feature\0"
        var old = try GitTabStatus.parse(output, directory: directory, isLinkedWorktree: false, hasRemote: false)
        var status = try GitTabStatus.parse(output, directory: directory, isLinkedWorktree: false, hasRemote: true)
        status.preservePullRequest(from: old)
        #expect(status.pullRequest == .loading)

        old.pullRequest = .noPullRequest
        status.preservePullRequest(from: old)
        #expect(status.pullRequest == .noPullRequest)
        var otherBranch = try GitTabStatus.parse("# branch.oid abcdef123456\0# branch.head other\0",
                                                directory: directory, isLinkedWorktree: false, hasRemote: true)
        otherBranch.preservePullRequest(from: old)
        #expect(otherBranch.pullRequest == .loading)
        var noRemote = try GitTabStatus.parse(output, directory: directory, isLinkedWorktree: false, hasRemote: false)
        noRemote.preservePullRequest(from: old)
        #expect(noRemote.pullRequest == .notApplicable)
    }

    @Test(arguments: [("OPEN", false, "Open"), ("OPEN", true, "Draft"),
                      ("MERGED", false, "Merged"), ("CLOSED", false, "Closed")])
    func readsPullRequestState(state: String, draft: Bool, label: String) {
        let output = """
        {"number":42,"state":"\(state)","isDraft":\(draft),"url":"https://github.com/owner/repo/pull/42","headRefName":"feature"}
        """
        let status = GitHubPullRequestService.parse(output: output, exitCode: 0, branch: "feature")
        #expect(status.label == "#42 \(label)")
    }

    @Test func lookupFailureIsDifferentFromNoPullRequest() {
        #expect(GitHubPullRequestService.parse(output: "no pull requests found for branch \"feature\"\n",
                                               exitCode: 1, branch: "feature") == .noPullRequest)
        #expect(GitHubPullRequestService.parse(output: "no pull requests found for branch \"owner:feature\"\n",
                                               exitCode: 1, branch: "feature") == .noPullRequest)
        #expect(GitHubPullRequestService.parse(output: "no pull requests found for branch \"owner:other\"\n",
                                               exitCode: 1, branch: "feature") == .unavailable)
        for output in ["authentication required", "network timeout", "invalid JSON"] {
            #expect(GitHubPullRequestService.parse(output: output, exitCode: 1, branch: "feature") == .unavailable)
        }
        #expect(GitHubPullRequestService.parse(output: "not JSON", exitCode: 0, branch: "feature") == .unavailable)
    }

    @Test func rejectsPullRequestForAnotherBranch() {
        let output = """
        {"number":42,"state":"OPEN","isDraft":false,"url":"https://github.com/owner/repo/pull/42","headRefName":"other"}
        """
        #expect(GitHubPullRequestService.parse(output: output, exitCode: 0, branch: "feature") == .unavailable)
    }

    @Test func missingGitHubCliIsUnavailable() async {
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/does-not-exist/gh"))
        #expect(await service.status(in: URL(fileURLWithPath: "/tmp"), branch: "feature") == .unavailable)
    }

    @Test func subprocessTimeoutIsBounded() throws {
        let start = Date()
        let result = try GitCommandRunner.run(executable: URL(fileURLWithPath: "/bin/sleep"),
                                              arguments: ["30"], timeout: 0.1)
        #expect(result.exitCode != 0)
        #expect(Date().timeIntervalSince(start) < 3)
    }

    @Test func subprocessDescendantsCannotKeepTimeoutWaitingForOutput() throws {
        let start = Date()
        let result = try GitCommandRunner.run(executable: URL(fileURLWithPath: "/bin/sh"),
                                              arguments: ["-c", "sleep 5 & wait"], timeout: 0.1)
        #expect(result.exitCode != 0)
        #expect(Date().timeIntervalSince(start) < 3)
    }

    @Test func subprocessIgnoresRepositoryOverridesAndKeepsAuthentication() {
        let environment = GitCommandRunner.environment(from: ["GIT_DIR": "/other", "GH_REPO": "owner/other",
                                                              "GH_HOST": "other.invalid", "GH_TOKEN": "test-token"])
        #expect(environment["GIT_DIR"] == nil)
        #expect(environment["GH_REPO"] == nil)
        #expect(environment["GH_HOST"] == nil)
        #expect(environment["GH_TOKEN"] == "test-token")
        #expect(environment["GH_PROMPT_DISABLED"] == "1")
    }
}
