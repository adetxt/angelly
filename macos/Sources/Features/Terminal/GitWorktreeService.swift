import Foundation
import Darwin

struct GitWorktree: Identifiable, Sendable {
    let directory: URL
    let branch: String?
    let isMain: Bool
    let isBare: Bool
    let isLocked: Bool
    let isPrunable: Bool

    var id: URL { directory }
    var canRemove: Bool { !isMain && !isBare && !isLocked }
}

struct GitWorktreeSnapshot: Sendable {
    let directory: URL
    let worktrees: [GitWorktree]
    let branches: [String]
}

/// Serializes Git operations across all manager sheets, away from the main actor.
actor GitWorktreeService {
    static let shared = GitWorktreeService()

    struct Failure: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    func load(from directory: URL) throws -> GitWorktreeSnapshot {
        let output = try git(["worktree", "list", "--porcelain", "-z"], in: directory)
        let worktrees = try Self.parse(output)
        guard let main = worktrees.first else { throw Failure(message: "Git returned no worktrees.") }
        let branches = try git(["for-each-ref", "--format=%(refname:strip=2)", "refs/heads/"], in: directory)
            .split(separator: "\n").map(String.init)
        return GitWorktreeSnapshot(directory: main.directory, worktrees: worktrees, branches: branches)
    }

    func loadRepositories(from directories: [URL], worktreeRoot: URL) -> [GitWorktreeSnapshot] {
        let files = FileManager.default
        func folders(in directory: URL) -> [URL] {
            let children = (try? files.contentsOfDirectory(at: directory, includingPropertiesForKeys: [.isDirectoryKey],
                                                          options: [.skipsHiddenFiles])) ?? []
            return children.filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
        }

        // Created worktrees live at <root>/<repository>/<worktree>; don't scan arbitrary directory trees.
        let groups = folders(in: worktreeRoot)
        let candidates = directories + groups + groups.flatMap { folders(in: $0) }
        var repositories: [URL: GitWorktreeSnapshot] = [:]
        var visited: Set<URL> = []
        for candidate in candidates {
            let directory = candidate.resolvingSymlinksInPath()
            guard visited.insert(directory).inserted, let snapshot = try? load(from: directory) else { continue }
            repositories[snapshot.directory] = snapshot
            visited.formUnion(snapshot.worktrees.map(\.directory))
        }
        return repositories.values.sorted { $0.directory.path.localizedStandardCompare($1.directory.path) == .orderedAscending }
    }

    func tabStatus(from directory: URL) throws -> GitTabStatus? {
        guard (try? git(["rev-parse", "--is-inside-work-tree"], in: directory, timeout: 5)) == "true\n" else { return nil }
        let root = try git(["rev-parse", "--show-toplevel"], in: directory, timeout: 5)
        let checkout = URL(fileURLWithPath: String(root.dropLast())).resolvingSymlinksInPath()
        let worktrees = try Self.parse(git(["worktree", "list", "--porcelain", "-z"], in: checkout, timeout: 5))
        guard let worktree = worktrees.first(where: { $0.directory == checkout }) else { return nil }
        let output = try git(["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=normal"], in: checkout, timeout: 5)
        let remotes = try git(["remote"], in: checkout, timeout: 5)
        var status = try GitTabStatus.parse(output, directory: checkout, isLinkedWorktree: !worktree.isMain,
                                           hasRemote: !remotes.isEmpty)
        let remoteNames = remotes.split(separator: "\n").map(String.init)
        if let remote = remoteNames.contains("origin") ? "origin" : remoteNames.first,
           let remoteURL = try? git(["remote", "get-url", "--", remote], in: checkout, timeout: 5),
           let repository = GitHubPullRequestService.repositoryName(
            from: remoteURL.trimmingCharacters(in: .whitespacesAndNewlines)),
           let owner = repository.split(separator: "/").first {
            status.ownerAvatarURL = URL(string: "https://github.com/\(owner).png?size=32")
        }
        if status.hasChanges {
            do {
                // Compare the final working copy with HEAD so staged edits aren't counted twice.
                let base = status.head == "(initial)"
                    ? try git(["hash-object", "-t", "tree", "/dev/null"], in: checkout, timeout: 5)
                        .trimmingCharacters(in: .whitespacesAndNewlines)
                    : "HEAD"
                let counts = try Self.parseNumstat(git(["diff", "--numstat", "-z", "--find-renames", "--no-ext-diff",
                                                        "--no-textconv", "--ignore-submodules=all", base, "--"],
                                                       in: checkout, timeout: 5, maxOutputBytes: 1_048_576))
                let untracked = try untrackedAdditions(in: checkout)
                guard untracked <= Int.max - counts.additions else {
                    throw Failure(message: "Couldn't read Git's line changes.")
                }
                status.lineChanges = GitLineChanges(additions: counts.additions + untracked, deletions: counts.deletions)
            } catch {
                status.lineChanges = nil
            }
        }
        return status
    }

    static func parseNumstat(_ output: String) throws -> (additions: Int, deletions: Int) {
        guard output.isEmpty || output.last == "\0" else {
            throw Failure(message: "Couldn't read Git's line changes.")
        }
        let records = output.split(separator: "\0", omittingEmptySubsequences: false)
        var additions = 0
        var deletions = 0
        var index = 0
        while index < records.count - 1 {
            let fields = records[index].split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false)
            guard fields.count == 3 else { throw Failure(message: "Couldn't read Git's line changes.") }
            if fields[0] != "-" || fields[1] != "-" {
                guard let added = Int(fields[0]), let deleted = Int(fields[1]), added >= 0, deleted >= 0,
                      added <= Int.max - additions, deleted <= Int.max - deletions else {
                    throw Failure(message: "Couldn't read Git's line changes.")
                }
                additions += added
                deletions += deleted
            }
            // A rename has two further NUL-delimited paths, even if they contain tabs or newlines.
            if fields[2].isEmpty {
                guard index + 2 < records.count - 1 else {
                    throw Failure(message: "Couldn't read Git's renamed paths.")
                }
                index += 2
            }
            index += 1
        }
        return (additions, deletions)
    }

    private func untrackedAdditions(in directory: URL) throws -> Int {
        let paths = try git(["ls-files", "--others", "--exclude-standard", "-z"], in: directory, timeout: 5, maxOutputBytes: 1_048_576)
            .split(separator: "\0")
        guard paths.count <= 1_000 else {
            throw Failure(message: "Too many untracked files to count line changes.")
        }
        let root = open(directory.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
        guard root >= 0 else { throw CocoaError(.fileReadUnknown) }
        defer { close(root) }
        var remainingBytes = 16 * 1_024 * 1_024
        var additions = 0
        let deadline = Date().addingTimeInterval(2)
        for path in paths {
            guard Date() < deadline else { throw Failure(message: "Counting untracked line changes timed out.") }
            let descriptor = try Self.openUntrackedFile(String(path), relativeTo: root)
            guard descriptor >= 0 else { continue }
            let file = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
            defer { try? file.close() }
            var metadata = stat()
            guard fstat(descriptor, &metadata) == 0 else { throw CocoaError(.fileReadUnknown) }
            guard metadata.st_mode & mode_t(S_IFMT) == mode_t(S_IFREG) else { continue }
            var lines = 0
            var bytesRead = 0
            var lastByte: UInt8?
            while let chunk = try file.read(upToCount: min(32_768, remainingBytes + 1)), !chunk.isEmpty {
                guard Date() < deadline else { throw Failure(message: "Counting untracked line changes timed out.") }
                remainingBytes -= chunk.count
                guard remainingBytes >= 0 else {
                    throw Failure(message: "Untracked files are too large to count line changes.")
                }
                // Git identifies binary content by a NUL in its first 8,000 bytes.
                if bytesRead < 8_000, chunk.prefix(8_000 - bytesRead).contains(0) {
                    lines = 0
                    lastByte = nil
                    break
                }
                bytesRead += chunk.count
                lines += chunk.reduce(0) { $0 + ($1 == 10 ? 1 : 0) }
                lastByte = chunk.last
            }
            additions += lines + (lastByte != nil && lastByte != 10 ? 1 : 0)
        }
        return additions
    }

    private static func openUntrackedFile(_ path: String, relativeTo root: Int32) throws -> Int32 {
        let components = path.split(separator: "/", omittingEmptySubsequences: false)
        guard !components.isEmpty, components.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." }) else { return -1 }
        var parent = dup(root)
        guard parent >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        for (index, component) in components.enumerated() {
            let flags = O_RDONLY | O_NOFOLLOW | O_NONBLOCK | (index < components.count - 1 ? O_DIRECTORY : 0)
            let descriptor = openat(parent, String(component), flags)
            close(parent)
            guard descriptor >= 0 else {
                if errno == ELOOP || errno == ENOENT || errno == ENOTDIR { return -1 }
                throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
            }
            parent = descriptor
        }
        return parent
    }

    func create(
        in repository: URL,
        root: URL,
        folderName: String,
        branch: String,
        newBranch: Bool
    ) throws -> URL {
        guard !folderName.isEmpty, folderName != ".", folderName != "..",
              !folderName.contains("/"), !folderName.contains("\0") else {
            throw Failure(message: "Enter a folder name without path separators.")
        }
        guard !branch.isEmpty, !branch.hasPrefix("-"), branch != "HEAD", !branch.contains("\0") else {
            throw Failure(message: "Enter a valid branch name.")
        }
        try git(["check-ref-format", "refs/heads/\(branch)"], in: repository)
        if !newBranch {
            try git(["show-ref", "--verify", "--quiet", "refs/heads/\(branch)"], in: repository)
        }

        let snapshot = try load(from: repository)
        let parent = root.appendingPathComponent(snapshot.directory.lastPathComponent, isDirectory: true)
        let destination = parent.appendingPathComponent(folderName, isDirectory: true)
        // Even an empty directory may belong to the user; Git itself would reuse it.
        guard !FileManager.default.fileExists(atPath: destination.path),
              (try? FileManager.default.destinationOfSymbolicLink(atPath: destination.path)) == nil else {
            throw Failure(message: "The destination already exists. Choose a different folder name.")
        }
        try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        let arguments = newBranch
            ? ["worktree", "add", "-b", branch, "--", destination.path, "HEAD"]
            : ["worktree", "add", "--", destination.path, branch]
        try git(arguments, in: repository)
        return destination.resolvingSymlinksInPath()
    }

    func remove(in repository: URL, directory: URL) throws {
        let snapshot = try load(from: repository)
        let target = directory.resolvingSymlinksInPath()
        guard let worktree = snapshot.worktrees.first(where: { $0.directory == target }), worktree.canRemove else {
            throw Failure(message: "The main checkout, bare repositories, and locked worktrees cannot be removed here. Refresh the list and try again.")
        }
        // Git's removal check excludes ignored files, including local secrets and state.
        if FileManager.default.fileExists(atPath: target.path) || !worktree.isPrunable {
            let ignored = try git(["ls-files", "--others", "--ignored", "--exclude-standard", "--directory", "-z"], in: target)
            guard ignored.isEmpty else {
                throw Failure(message: "This worktree contains ignored files, such as build output or local configuration. Move or remove those files before removing the worktree.")
            }
        }
        // Git checks dirty/untracked files and locks again when executing; never force.
        try git(["worktree", "remove", "--", target.path], in: snapshot.directory)
    }

    private static func parse(_ output: String) throws -> [GitWorktree] {
        var worktrees: [GitWorktree] = []
        var fields: [String] = []
        for field in output.split(separator: "\0", omittingEmptySubsequences: false) {
            if !field.isEmpty {
                fields.append(String(field))
                continue
            }
            guard !fields.isEmpty else { continue }
            guard let path = fields.first, path.hasPrefix("worktree ") else {
                throw Failure(message: "Couldn't read Git's worktree list.")
            }
            let branch = fields.first { $0.hasPrefix("branch refs/heads/") }
                .map { String($0.dropFirst("branch refs/heads/".count)) }
            worktrees.append(GitWorktree(
                directory: URL(fileURLWithPath: String(path.dropFirst("worktree ".count)))
                    .resolvingSymlinksInPath(),
                branch: branch,
                isMain: worktrees.isEmpty,
                isBare: fields.contains("bare"),
                isLocked: fields.contains { $0 == "locked" || $0.hasPrefix("locked ") },
                isPrunable: fields.contains { $0 == "prunable" || $0.hasPrefix("prunable ") }))
            fields.removeAll(keepingCapacity: true)
        }
        return worktrees
    }

    @discardableResult
    private func git(
        _ arguments: [String], in directory: URL, timeout: TimeInterval? = nil, maxOutputBytes: Int? = nil
    ) throws -> String {
        let result: GitCommandRunner.Result
        do {
            result = try GitCommandRunner.run(executable: URL(fileURLWithPath: "/usr/bin/git"),
                                               arguments: ["-C", directory.path] + arguments, timeout: timeout,
                                               maxOutputBytes: maxOutputBytes)
        } catch {
            throw Failure(message: "Couldn't run Git. Install the Xcode Command Line Tools and try again.\n\(error.localizedDescription)")
        }
        guard result.exitCode == 0 else {
            let detail = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
            throw Failure(message: detail.isEmpty ? "Git couldn't complete the operation (exit \(result.exitCode))." : detail)
        }
        return result.output
    }
}
