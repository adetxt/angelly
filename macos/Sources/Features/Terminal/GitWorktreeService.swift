import Foundation

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

    func tabStatus(from directory: URL) throws -> GitTabStatus? {
        guard (try? git(["rev-parse", "--is-inside-work-tree"], in: directory, timeout: 5)) == "true\n" else { return nil }
        let root = try git(["rev-parse", "--show-toplevel"], in: directory, timeout: 5)
        let checkout = URL(fileURLWithPath: String(root.dropLast())).resolvingSymlinksInPath()
        let worktrees = try Self.parse(git(["worktree", "list", "--porcelain", "-z"], in: checkout, timeout: 5))
        guard let worktree = worktrees.first(where: { $0.directory == checkout }) else { return nil }
        let output = try git(["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=normal"], in: checkout, timeout: 5)
        let remotes = try git(["remote"], in: checkout, timeout: 5)
        return try GitTabStatus.parse(output, directory: checkout, isLinkedWorktree: !worktree.isMain,
                                      hasRemote: !remotes.isEmpty)
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
    private func git(_ arguments: [String], in directory: URL, timeout: TimeInterval? = nil) throws -> String {
        let result: GitCommandRunner.Result
        do {
            result = try GitCommandRunner.run(executable: URL(fileURLWithPath: "/usr/bin/git"),
                                               arguments: ["-C", directory.path] + arguments, timeout: timeout)
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
