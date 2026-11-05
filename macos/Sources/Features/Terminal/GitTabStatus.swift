import Foundation

struct GitTabStatus: Equatable, Sendable {
    let directory: URL
    let branch: String?
    let head: String
    let isLinkedWorktree: Bool
    let hasChanges: Bool
    let hasConflicts: Bool
    let ahead: Int
    let behind: Int
    var lineChanges: GitLineChanges? = GitLineChanges(additions: 0, deletions: 0)
    var pullRequest: GitPullRequestStatus
    var ownerAvatarURL: URL?

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
        if let lineChanges {
            if lineChanges.additions > 0 || lineChanges.deletions > 0 {
                lines.append("\(lineChanges.additions) lines added, \(lineChanges.deletions) lines deleted")
            }
        } else {
            lines.append("Line changes unavailable")
        }
        if ahead > 0 { lines.append("\(ahead) commits ahead of upstream") }
        if behind > 0 { lines.append("\(behind) commits behind upstream") }
        if let label = pullRequest.label { lines.append(label) }
        if pullRequest == .unavailable {
            lines.append("Check connectivity and GitHub access. For private repositories, use gh auth login or set GH_TOKEN / GITHUB_TOKEN.")
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

struct GitLineChanges: Equatable, Sendable {
    let additions: Int
    let deletions: Int
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
        let expiry: Date
    }

    private let executable: URL?
    private let environment: [String: String]
    private let load: @Sendable (URLRequest) async throws -> (Data, URLResponse)
    private var cache: [Key: Entry] = [:]
    private var pending: [Key: Task<Entry, Never>] = [:]
    private var apiUnavailableUntil: Date?

    private var apiToken: String? {
        [environment["GH_TOKEN"], environment["GITHUB_TOKEN"]].compactMap { $0 }.first { !$0.isEmpty }
    }

    init(executable: URL? = nil, environment: [String: String] = ProcessInfo.processInfo.environment,
         load: @escaping @Sendable (URLRequest) async throws -> (Data, URLResponse) = {
             try await apiSession.data(for: $0)
         }) {
        self.executable = executable
        self.environment = environment
        self.load = load
    }

    func status(in directory: URL, branch: String) async -> GitPullRequestStatus {
        let key = Key(directory: directory, branch: branch)
        cache = cache.filter { $0.value.expiry > Date() }
        if let entry = cache[key] { return entry.status }
        if let task = pending[key] { return await task.value.status }
        let task = Task { await lookup(in: directory, branch: branch) }
        pending[key] = task
        let entry = await task.value
        pending[key] = nil
        cache[key] = entry
        return entry.status
    }

    private func lookup(in directory: URL, branch: String) async -> Entry {
        if let executable = executable ?? Self.findExecutable(),
           let result = try? GitCommandRunner.run(
            executable: executable, arguments: ["pr", "view", "--json", "number,state,isDraft,url,headRefName,mergeable"],
            directory: directory, timeout: 10) {
            let status = Self.parse(output: result.output, exitCode: result.exitCode, branch: branch)
            if status != .unavailable { return Entry(status: status, expiry: Date().addingTimeInterval(60)) }
        }
        let status = await apiStatus(in: directory, branch: branch)
        // Anonymous GitHub access allows only 60 requests/hour, shared across all tabs.
        let lifetime: TimeInterval = apiToken == nil && status != .unavailable ? 300 : 60
        return Entry(status: status, expiry: Date().addingTimeInterval(lifetime))
    }

    private final class APIDelegate: NSObject, URLSessionTaskDelegate {
        func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                        newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
            // Keep tokens on the fixed GitHub API endpoint, including when a repository moves.
            completionHandler(nil)
        }
    }

    private static let apiSession: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 10
        configuration.timeoutIntervalForResource = 15
        return URLSession(configuration: configuration, delegate: APIDelegate(), delegateQueue: nil)
    }()

    private struct APIPullRequest: Decodable {
        struct Head: Decodable {
            struct Repository: Decodable { let fullName: String }
            let ref: String
            let repo: Repository?
        }
        let number: Int
        let state: String
        let draft: Bool
        let mergedAt: String?
        let htmlUrl: URL
        let mergeable: Bool?
        let head: Head

        func status(branch: String, repository: String, base: String) -> GitPullRequestStatus {
            guard number > 0, head.ref == branch,
                  head.repo?.fullName.lowercased() == repository.lowercased(),
                  htmlUrl.scheme == "https", htmlUrl.host == "github.com",
                  htmlUrl.path.lowercased() == "/\(base)/pull/\(number)".lowercased() else { return .unavailable }
            let status: GitPullRequestStatus.State
            switch state {
            case "open": status = mergeable == false ? .conflict : (draft ? .draft : .open)
            case "closed": status = mergedAt == nil ? .closed : .merged
            default: return .unavailable
            }
            return .pullRequest(number: number, state: status, url: htmlUrl)
        }
    }

    private func apiStatus(in directory: URL, branch: String) async -> GitPullRequestStatus {
        if let until = apiUnavailableUntil, until > Date() { return .unavailable }
        func git(_ arguments: [String]) -> String? {
            guard let result = try? GitCommandRunner.run(executable: URL(fileURLWithPath: "/usr/bin/git"),
                                                        arguments: arguments, directory: directory,
                                                        timeout: 5, maxOutputBytes: 65_536),
                  result.exitCode == 0 else { return nil }
            return result.output.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        guard let remoteOutput = git(["remote"]) else { return .unavailable }
        let remotes = remoteOutput.split(separator: "\n").map(String.init)
        let configuredHead = git(["config", "--get", "branch.\(branch).pushRemote"])
            ?? git(["config", "--get", "remote.pushDefault"])
            ?? (remotes.contains("origin") ? "origin" : nil)
            ?? git(["config", "--get", "branch.\(branch).remote"])
        guard let baseRemote = ["upstream", "origin"].first(where: remotes.contains) ?? remotes.first else {
            return .unavailable
        }
        let headRemote = configuredHead.flatMap { remotes.contains($0) ? $0 : nil }
            ?? (remotes.contains("origin") ? "origin" : baseRemote)
        guard let baseURL = git(["remote", "get-url", "--", baseRemote]),
              let headURL = git(["remote", "get-url", "--", headRemote]),
              let base = Self.repositoryName(from: baseURL),
              let head = Self.repositoryName(from: headURL),
              let owner = head.split(separator: "/").first else { return .unavailable }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do {
            // Search open PRs first so a newer closed PR cannot hide an older open one.
            for state in ["open", "closed"] {
                var components = URLComponents(string: "https://api.github.com/repos/\(base)/pulls")!
                components.queryItems = [URLQueryItem(name: "head", value: "\(owner):\(branch)"),
                                         URLQueryItem(name: "state", value: state),
                                         URLQueryItem(name: "sort", value: "created"),
                                         URLQueryItem(name: "direction", value: "desc"),
                                         URLQueryItem(name: "per_page", value: "1")]
                let data = try await apiData(at: components.url!)
                let requests = try decoder.decode([APIPullRequest].self, from: data)
                guard var request = requests.first else { continue }
                guard request.state == state,
                      request.status(branch: branch, repository: head, base: base) != .unavailable else {
                    return .unavailable
                }
                if state == "open" {
                    // The list endpoint omits mergeability; the detail endpoint computes it.
                    let data = try await apiData(at: URL(string: "https://api.github.com/repos/\(base)/pulls/\(request.number)")!)
                    let detail = try decoder.decode(APIPullRequest.self, from: data)
                    guard detail.number == request.number else { return .unavailable }
                    request = detail
                }
                return request.status(branch: branch, repository: head, base: base)
            }
            return .noPullRequest
        } catch {
            return .unavailable
        }
    }

    private func apiData(at url: URL) async throws -> Data {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue("2022-11-28", forHTTPHeaderField: "X-GitHub-Api-Version")
        request.setValue("Ghostty", forHTTPHeaderField: "User-Agent")
        if let token = apiToken {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        let (data, response) = try await load(request)
        guard let response = response as? HTTPURLResponse else { throw URLError(.badServerResponse) }
        if [403, 429].contains(response.statusCode),
           response.statusCode == 429 || response.value(forHTTPHeaderField: "X-RateLimit-Remaining") == "0" ||
            response.value(forHTTPHeaderField: "Retry-After") != nil {
            let now = Date()
            let reset = response.value(forHTTPHeaderField: "X-RateLimit-Reset").flatMap(TimeInterval.init)
            let retry = response.value(forHTTPHeaderField: "Retry-After").flatMap(TimeInterval.init)
            let wait = max(reset.map { $0 - now.timeIntervalSince1970 } ?? 0, retry ?? 0)
            apiUnavailableUntil = now.addingTimeInterval(wait.isFinite ? max(60, wait) : 60)
        }
        guard response.statusCode == 200 else {
            throw URLError(.badServerResponse)
        }
        return data
    }

    static func repositoryName(from remote: String) -> String? {
        let value = remote.hasPrefix("git@github.com:")
            ? "ssh://git@github.com/" + remote.dropFirst("git@github.com:".count) : remote
        guard let url = URLComponents(string: value), url.host?.lowercased() == "github.com",
              ["https", "ssh", "git"].contains(url.scheme), url.port == nil,
              url.query == nil, url.fragment == nil else { return nil }
        var path = url.path
        if path.hasSuffix(".git") { path.removeLast(4) }
        let parts = path.split(separator: "/", omittingEmptySubsequences: false)
        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.")
        guard parts.count == 3, parts[0].isEmpty,
              parts.dropFirst().allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." &&
                  $0.unicodeScalars.allSatisfy(allowed.contains) }) else { return nil }
        return parts.dropFirst().joined(separator: "/")
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
