import Foundation

struct GitTabStatus: Sendable {
    let directory: URL
    let branch: String?
    let head: String
    let isLinkedWorktree: Bool
    let hasChanges: Bool
    let hasConflicts: Bool
    let ahead: Int
    let behind: Int
    var pullRequest: GitPullRequestStatus

    var titlebarBranch: String { branch ?? "Detached \(head.prefix(7))" }

    enum Indicator: Sendable {
        case standard, open, draft, merged, conflict

        var assetName: String? {
            switch self {
            case .open: "GitPullRequest"
            case .merged: "GitMerge"
            case .conflict: "GitPullRequestConflict"
            case .standard, .draft: nil
            }
        }
    }

    var indicator: Indicator {
        if hasConflicts { return .conflict }
        guard case let .pullRequest(_, state, _) = pullRequest else { return .standard }
        switch state {
        case .open: return .open
        case .draft: return .draft
        case .merged: return .merged
        case .conflict: return .conflict
        case .closed: return .standard
        }
    }

    mutating func preservePullRequest(from old: Self?) {
        guard let old, old.directory == directory, old.branch == branch,
              old.pullRequest != .notApplicable, pullRequest != .notApplicable else { return }
        pullRequest = old.pullRequest
    }

    var detail: String {
        var lines = [isLinkedWorktree ? "Linked worktree" : "Main repository checkout", directory.path,
                     branch.map { "Branch: \($0)" } ?? "Detached HEAD: \(head)",
                     hasChanges ? "Uncommitted changes" : "Working tree clean"]
        if hasConflicts { lines.append("Unresolved merge conflicts in working tree") }
        if ahead > 0 { lines.append("\(ahead) commits ahead of upstream") }
        if behind > 0 { lines.append("\(behind) commits behind upstream") }
        if let label = pullRequest.label { lines.append(label) }
        if pullRequest == .unavailable {
            lines.append("GitHub PR lookup requires gh and gh auth login. Check authentication and connectivity.")
        }
        return lines.joined(separator: "\n")
    }

    static func parse(_ output: String, directory: URL, isLinkedWorktree: Bool, hasRemote: Bool) throws -> Self {
        let records = output.split(separator: "\0")
        // Only leading records are headers: a rename's second path can begin with '# '.
        let headers = records.prefix { $0.hasPrefix("# ") }
        guard let head = headers.first(where: { $0.hasPrefix("# branch.oid ") }),
              let branch = headers.first(where: { $0.hasPrefix("# branch.head ") }) else {
            throw GitWorktreeService.Failure(message: "Couldn't read Git's branch status.")
        }
        let name = String(branch.dropFirst("# branch.head ".count))
        var ahead = 0
        var behind = 0
        if let tracking = headers.first(where: { $0.hasPrefix("# branch.ab ") }) {
            let counts = tracking.dropFirst("# branch.ab ".count).split(separator: " ")
            guard counts.count == 2, let added = Int(counts[0]), let removed = Int(counts[1]),
                  added >= 0, removed <= 0, removed != Int.min else {
                throw GitWorktreeService.Failure(message: "Couldn't read Git's upstream status.")
            }
            ahead = added
            behind = -removed
        }
        let detached = name == "(detached)"
        var hasConflicts = false
        var index = headers.count
        while index < records.count {
            if records[index].hasPrefix("u ") { hasConflicts = true }
            if records[index].hasPrefix("2 ") { index += 1 }
            index += 1
        }
        return Self(directory: directory, branch: detached ? nil : name,
                    head: String(head.dropFirst("# branch.oid ".count)), isLinkedWorktree: isLinkedWorktree,
                    hasChanges: records.count > headers.count, hasConflicts: hasConflicts, ahead: ahead, behind: behind,
                    pullRequest: detached || !hasRemote ? .notApplicable : .loading)
    }
}

enum GitPullRequestStatus: Equatable, Sendable {
    enum State: String, Sendable {
        case open = "Open"
        case draft = "Draft"
        case merged = "Merged"
        case closed = "Closed"
        case conflict = "Conflict"
    }

    case loading
    case notApplicable
    case noPullRequest
    case unavailable
    case pullRequest(number: Int, state: State, url: URL)

    var label: String? {
        switch self {
        case .loading: "PR …"
        case .notApplicable: nil
        case .noPullRequest: "No PR"
        case .unavailable: "PR unavailable"
        case let .pullRequest(number, state, _): "#\(number) \(state.rawValue)"
        }
    }
}

actor GitHubPullRequestService {
    static let shared = GitHubPullRequestService()

    private struct Key: Hashable {
        let directory: URL
        let branch: String
    }

    private struct Entry {
        let status: GitPullRequestStatus
        let date: Date
    }

    private let executable: URL?
    private var cache: [Key: Entry] = [:]

    init(executable: URL? = nil) { self.executable = executable }

    func status(in directory: URL, branch: String) -> GitPullRequestStatus {
        let key = Key(directory: directory, branch: branch)
        cache = cache.filter { Date().timeIntervalSince($0.value.date) < 60 }
        if let entry = cache[key] { return entry.status }
        let status: GitPullRequestStatus
        if let executable = executable ?? Self.findExecutable(),
           let result = try? GitCommandRunner.run(
            executable: executable, arguments: ["pr", "view", "--json", "number,state,isDraft,url,headRefName,mergeable"],
            directory: directory, timeout: 10) {
            status = Self.parse(output: result.output, exitCode: result.exitCode, branch: branch)
        } else {
            status = .unavailable
        }
        cache[key] = Entry(status: status, date: Date())
        return status
    }

    static func parse(output: String, exitCode: Int32, branch: String) -> GitPullRequestStatus {
        guard exitCode == 0 else {
            let prefix = "no pull requests found for branch "
            let message = output.trimmingCharacters(in: .whitespacesAndNewlines)
            if exitCode == 1, message.hasPrefix(prefix),
               let name = try? JSONDecoder().decode(String.self, from: Data(message.dropFirst(prefix.count).utf8)),
               name.split(separator: ":", maxSplits: 1).last.map(String.init) == branch {
                return .noPullRequest
            }
            return .unavailable
        }
        struct PullRequest: Decodable {
            let number: Int
            let state: String
            let isDraft: Bool
            let url: URL
            let headRefName: String
            let mergeable: String?
        }
        guard let request = try? JSONDecoder().decode(PullRequest.self, from: Data(output.utf8)),
              request.number > 0, request.url.scheme == "https", request.headRefName == branch else { return .unavailable }
        let state: GitPullRequestStatus.State
        switch request.state {
        case "OPEN": state = request.mergeable == "CONFLICTING" ? .conflict : (request.isDraft ? .draft : .open)
        case "MERGED": state = .merged
        case "CLOSED": state = .closed
        default: return .unavailable
        }
        return .pullRequest(number: request.number, state: state, url: request.url)
    }

    private static func findExecutable() -> URL? {
        let paths = ["/opt/homebrew/bin", "/usr/local/bin"] +
            (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map(String.init)
        return paths.map { URL(fileURLWithPath: $0).appendingPathComponent("gh") }
            .first { FileManager.default.isExecutableFile(atPath: $0.path) }
    }
}
