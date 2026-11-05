import AppKit
import Foundation
import Testing
@testable import Ghostty

@Suite
struct GitTabStatusTests {
    @Test func lineChangesAreIncludedInTooltipAndContextEquality() throws {
        let directory = URL(fileURLWithPath: "/repo")
        var status = try GitTabStatus.parse("# branch.oid abcdef123456\0# branch.head main\0",
                                           directory: directory, isLinkedWorktree: false, hasRemote: false)
        #expect(status.lineChanges == GitLineChanges(additions: 0, deletions: 0))
        let clean = TerminalDirectoryContext(workingDirectory: directory, gitStatus: status)
        status.lineChanges = GitLineChanges(additions: 123, deletions: 465)
        #expect(status.detail.contains("123 lines added, 465 lines deleted"))
        #expect(clean != TerminalDirectoryContext(workingDirectory: directory, gitStatus: status))
        status.lineChanges = nil
        #expect(status.detail.contains("Line changes unavailable"))
    }

    @Test func subprocessOutputLimitRejectsOversizedResults() throws {
        #expect(throws: (any Error).self) {
            try GitCommandRunner.run(executable: URL(fileURLWithPath: "/usr/bin/printf"),
                                     arguments: ["%4096s", ""], timeout: 1, maxOutputBytes: 1_024)
        }
        let result = try GitCommandRunner.run(executable: URL(fileURLWithPath: "/usr/bin/printf"),
                                              arguments: ["small"], timeout: 1, maxOutputBytes: 1_024)
        #expect(result.output == "small")
    }

    @Test func titlebarContextChangesWhenPullRequestChanges() throws {
        let directory = URL(fileURLWithPath: "/repo")
        var status = try GitTabStatus.parse("# branch.oid abcdef123456\0# branch.head main\0",
                                             directory: directory, isLinkedWorktree: false, hasRemote: true)
        let loading = TerminalDirectoryContext(workingDirectory: directory, gitStatus: status)
        status.pullRequest = .pullRequest(number: 42, state: .open,
                                         url: URL(string: "https://github.com/example/repo/pull/42")!)
        let open = TerminalDirectoryContext(workingDirectory: directory, gitStatus: status)
        #expect(loading != open)
        status.pullRequest = .pullRequest(number: 42, state: .merged,
                                         url: URL(string: "https://github.com/example/repo/pull/42")!)
        let merged = TerminalDirectoryContext(workingDirectory: directory, gitStatus: status)
        #expect(open != merged)
    }

    @Test func pullRequestIconsAreAvailableAndTintable() throws {
        #expect(try #require(NSImage(named: "GitBranch")).isTemplate)
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

    @Test func unavailableTooltipDescribesAPIAuthentication() throws {
        var status = try GitTabStatus.parse("# branch.oid abcdef123456\0# branch.head feature\0",
                                           directory: URL(fileURLWithPath: "/repo"),
                                           isLinkedWorktree: false, hasRemote: true)
        status.pullRequest = .unavailable
        #expect(status.detail.contains("GITHUB_TOKEN"))
        #expect(!status.detail.contains("requires gh"))
    }

    private final class PullRequestRepository {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)

        init(remote: String = "git@github.com:owner/repo.git") throws {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try git(["init", "-b", "feature/test"])
            try git(["remote", "add", "origin", remote])
        }

        deinit { try? FileManager.default.removeItem(at: directory) }

        func git(_ arguments: [String]) throws {
            let result = try GitCommandRunner.run(executable: URL(fileURLWithPath: "/usr/bin/git"),
                                                  arguments: arguments, directory: directory, timeout: 5)
            #expect(result.exitCode == 0)
        }

        func cli(output: String, exitCode: Int) throws -> URL {
            let executable = directory.appendingPathComponent("gh")
            try output.write(to: directory.appendingPathComponent("gh-output"), atomically: true, encoding: .utf8)
            try "#!/bin/sh\n/bin/cat \"$0-output\"\nexit \(exitCode)\n"
                .write(to: executable, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: executable.path)
            return executable
        }
    }

    private actor PullRequestAPI {
        var requests: [URLRequest] = []
        let list: String
        let closedList: String
        let detail: String
        let code: Int
        let detailCode: Int
        let headers: [String: String]

        init(list: String = "[]", closedList: String = "[]", detail: String = "{}",
             code: Int = 200, detailCode: Int = 200, headers: [String: String] = [:]) {
            self.list = list
            self.closedList = closedList
            self.detail = detail
            self.code = code
            self.detailCode = detailCode
            self.headers = headers
        }

        func load(_ request: URLRequest) async throws -> (Data, URLResponse) {
            requests.append(request)
            let url = try #require(request.url)
            let state = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?
                .first { $0.name == "state" }?.value
            let output = url.path.hasSuffix("/pulls") ? (state == "closed" ? closedList : list) : detail
            let status = url.path.hasSuffix("/pulls") ? code : detailCode
            return (Data(output.utf8), try #require(HTTPURLResponse(url: url, statusCode: status,
                                                                  httpVersion: nil, headerFields: headers)))
        }
    }

    private static func apiPullRequest(state: String = "open", draft: Bool = false,
                                       merged: Bool = false, mergeable: String = "true",
                                       branch: String = "feature/test", owner: String = "owner") -> String {
        """
        {"number":42,"state":"\(state)","draft":\(draft),"merged_at":\(merged ? "\"2026-01-01T00:00:00Z\"" : "null"),
         "mergeable":\(mergeable),"html_url":"https://github.com/owner/repo/pull/42",
         "head":{"ref":"\(branch)","repo":{"full_name":"\(owner)/repo"}}}
        """
    }

    @Test(arguments: ["https://github.com/owner/repo.git", "git@github.com:owner/repo.git",
                      "ssh://git@github.com/owner/repo.git", "https://github.com/owner/repo"])
    func parsesGitHubRemote(remote: String) {
        #expect(GitHubPullRequestService.repositoryName(from: remote) == "owner/repo")
    }

    @Test(arguments: ["https://github.com.evil.invalid/owner/repo", "git@evil.invalid:owner/repo.git",
                      "file:///owner/repo", "https://github.com/owner/repo/extra",
                      "https://github.com/owner/..", "https://github.com/owner/repo?query=1"])
    func rejectsUnsupportedRemote(remote: String) {
        #expect(GitHubPullRequestService.repositoryName(from: remote) == nil)
    }

    @Test func missingCliFallsBackToPublicAPIAndCachesResult() async throws {
        let repo = try PullRequestRepository()
        let api = PullRequestAPI()
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/missing/gh"),
                                               environment: [:], load: { try await api.load($0) })
        async let first = service.status(in: repo.directory, branch: "feature/test")
        async let second = service.status(in: repo.directory, branch: "feature/test")
        #expect(await first == .noPullRequest)
        #expect(await second == .noPullRequest)
        #expect(await service.status(in: repo.directory, branch: "feature/test") == .noPullRequest)
        let requests = await api.requests
        #expect(requests.count == 2)
        let request = try #require(requests.first)
        #expect(request.value(forHTTPHeaderField: "Authorization") == nil)
        #expect(request.url?.host == "api.github.com")
        #expect(request.url?.path == "/repos/owner/repo/pulls")
        let query = URLComponents(url: try #require(request.url), resolvingAgainstBaseURL: false)?.queryItems
        #expect(query?.first { $0.name == "head" }?.value == "owner:feature/test")
        #expect(query?.first { $0.name == "state" }?.value == "open")
    }

    @Test(arguments: [("open", false, false, "true", "Open"),
                      ("open", true, false, "true", "Draft"),
                      ("open", true, false, "false", "Conflict"),
                      ("open", false, false, "null", "Open"),
                      ("closed", false, true, "false", "Merged"),
                      ("closed", false, false, "true", "Closed")])
    func apiPreservesPullRequestStates(state: String, draft: Bool, merged: Bool,
                                       mergeable: String, label: String) async throws {
        let repo = try PullRequestRepository()
        let pull = Self.apiPullRequest(state: state, draft: draft, merged: merged, mergeable: mergeable)
        let api = PullRequestAPI(list: state == "open" ? "[\(pull)]" : "[]",
                                 closedList: state == "closed" ? "[\(pull)]" : "[]", detail: pull)
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/missing/gh"),
                                               environment: [:], load: { try await api.load($0) })
        #expect(await service.status(in: repo.directory, branch: "feature/test").label == "#42 \(label)")
    }

    @Test func apiFindsForkPullRequestAndUsesToken() async throws {
        let repo = try PullRequestRepository(remote: "https://github.com/fork/repo.git")
        try repo.git(["remote", "add", "upstream", "git@github.com:owner/repo.git"])
        try repo.git(["config", "branch.feature/test.remote", "upstream"])
        try repo.git(["config", "branch.feature/test.merge", "refs/heads/main"])
        let pull = Self.apiPullRequest(owner: "fork")
        let api = PullRequestAPI(list: "[\(pull)]", detail: pull)
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/usr/bin/false"),
                                               environment: ["GH_TOKEN": "gh-test", "GITHUB_TOKEN": "github-test"],
                                               load: { try await api.load($0) })
        #expect(await service.status(in: repo.directory, branch: "feature/test").label == "#42 Open")
        let requests = await api.requests
        #expect(requests.count == 2)
        for request in requests {
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer gh-test")
            #expect(request.url?.path.hasPrefix("/repos/owner/repo/pulls") == true)
        }
        let url = try #require(requests.first?.url)
        #expect(URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?
            .first { $0.name == "head" }?.value == "fork:feature/test")
    }

    @Test(arguments: [401, 403, 404, 429, 500])
    func apiErrorsAreUnavailable(code: Int) async throws {
        let repo = try PullRequestRepository()
        let api = PullRequestAPI(code: code)
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/missing/gh"),
                                               environment: ["GITHUB_TOKEN": "test-token"],
                                               load: { try await api.load($0) })
        #expect(await service.status(in: repo.directory, branch: "feature/test") == .unavailable)
        #expect(await api.requests.first?.value(forHTTPHeaderField: "Authorization") == "Bearer test-token")
    }

    @Test(arguments: ["not JSON", "{}", "[\(apiPullRequest(branch: "other"))]",
                      "[\(apiPullRequest(owner: "other"))]"])
    func malformedOrUnrelatedAPIResultsAreUnavailable(list: String) async throws {
        let repo = try PullRequestRepository()
        let api = PullRequestAPI(list: list)
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/missing/gh"),
                                               environment: [:], load: { try await api.load($0) })
        #expect(await service.status(in: repo.directory, branch: "feature/test") == .unavailable)
    }

    @Test func unsupportedRemoteDoesNotSendToken() async throws {
        let repo = try PullRequestRepository(remote: "https://git.example.invalid/owner/repo")
        let api = PullRequestAPI()
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/missing/gh"),
                                               environment: ["GH_TOKEN": "secret"], load: { try await api.load($0) })
        #expect(await service.status(in: repo.directory, branch: "feature/test") == .unavailable)
        #expect(await api.requests.isEmpty)
    }

    @Test(arguments: [false, true])
    func successfulCliDoesNotCallAPI(noPullRequest: Bool) async throws {
        let repo = try PullRequestRepository()
        let output = noPullRequest ? "no pull requests found for branch \"feature/test\"\n" : """
        {"number":42,"state":"OPEN","isDraft":false,"url":"https://github.com/owner/repo/pull/42","headRefName":"feature/test"}
        """
        let cli = try repo.cli(output: output, exitCode: noPullRequest ? 1 : 0)
        let api = PullRequestAPI()
        let service = GitHubPullRequestService(executable: cli, environment: [:], load: { try await api.load($0) })
        let status = await service.status(in: repo.directory, branch: "feature/test")
        #expect(status.label == (noPullRequest ? "No PR" : "#42 Open"))
        #expect(await api.requests.isEmpty)
    }

    @Test(arguments: [200, 503])
    func apiDetailFailureIsUnavailable(code: Int) async throws {
        let repo = try PullRequestRepository()
        let api = PullRequestAPI(list: "[\(Self.apiPullRequest())]", detailCode: code)
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/missing/gh"),
                                               environment: [:], load: { try await api.load($0) })
        #expect(await service.status(in: repo.directory, branch: "feature/test") == .unavailable)
        #expect(await api.requests.count == 2)
    }

    @Test func apiRateLimitBacksOffAcrossBranches() async throws {
        let repo = try PullRequestRepository()
        let reset = String(Int(Date().addingTimeInterval(600).timeIntervalSince1970))
        let api = PullRequestAPI(code: 403, headers: ["X-RateLimit-Remaining": "0", "X-RateLimit-Reset": reset])
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/missing/gh"),
                                               environment: [:], load: { try await api.load($0) })
        #expect(await service.status(in: repo.directory, branch: "feature/test") == .unavailable)
        #expect(await service.status(in: repo.directory, branch: "other") == .unavailable)
        #expect(await api.requests.count == 1)
    }

    @Test func apiUsesConfiguredPushRemote() async throws {
        let repo = try PullRequestRepository()
        try repo.git(["remote", "add", "contributor", "ssh://git@github.com/fork/repo.git"])
        try repo.git(["config", "branch.feature/test.pushRemote", "contributor"])
        let pull = Self.apiPullRequest(owner: "fork")
        let api = PullRequestAPI(list: "[\(pull)]", detail: pull)
        let service = GitHubPullRequestService(executable: URL(fileURLWithPath: "/missing/gh"),
                                               environment: [:], load: { try await api.load($0) })
        #expect(await service.status(in: repo.directory, branch: "feature/test").label == "#42 Open")
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
