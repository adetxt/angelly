import Foundation
import Testing
@testable import Ghostty

@Suite
struct GitWorktreeServiceTests {
    private final class Repository {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("Angelly worktrees \(UUID().uuidString)")
        let service = GitWorktreeService()
        var checkout: URL { directory.appendingPathComponent("repository") }
        var root: URL { directory.appendingPathComponent("worktrees") }

        init() throws {
            try FileManager.default.createDirectory(at: checkout, withIntermediateDirectories: true)
            try git(["init", "-b", "main"])
            try git(["config", "user.name", "Worktree Tests"])
            try git(["config", "user.email", "worktrees@example.invalid"])
            try git(["config", "core.hooksPath", "/dev/null"])
            try git(["commit", "--allow-empty", "-m", "Initial"])
        }

        deinit { try? FileManager.default.removeItem(at: directory) }

        @discardableResult
        func git(_ arguments: [String]) throws -> String {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
            process.arguments = ["-C", checkout.path] + arguments
            var environment = ProcessInfo.processInfo.environment.filter { !$0.key.hasPrefix("GIT_") }
            environment["GIT_CONFIG_GLOBAL"] = "/dev/null"
            environment["GIT_CONFIG_NOSYSTEM"] = "1"
            process.environment = environment
            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = pipe
            process.standardInput = FileHandle.nullDevice
            try process.run()
            let output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            process.waitUntilExit()
            guard process.terminationStatus == 0 else {
                throw NSError(domain: "GitWorktreeTests", code: Int(process.terminationStatus),
                              userInfo: [NSLocalizedDescriptionKey: output])
            }
            return output
        }
    }

    @Test func discoversRepositoryFromSubdirectoryAndLinkedCheckout() async throws {
        let repo = try Repository()
        let subdirectory = repo.checkout.appendingPathComponent("nested")
        try FileManager.default.createDirectory(at: subdirectory, withIntermediateDirectories: true)
        let initial = try await repo.service.load(from: subdirectory)
        #expect(initial.worktrees.count == 1)
        #expect(initial.worktrees.first?.isMain == true)
        #expect(initial.branches == ["main"])

        let path = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                folderName: "feature", branch: "feature/test", newBranch: true)
        let linked = try await repo.service.load(from: path)
        #expect(linked.directory == repo.checkout.resolvingSymlinksInPath())
        #expect(linked.worktrees.count == 2)
        #expect(linked.worktrees.contains { $0.directory == path && $0.branch == "feature/test" })
    }

    @Test func createsExistingBranchAndPreservesSpecialPathCharacters() async throws {
        let repo = try Repository()
        try repo.git(["branch", "existing"])
        let folder = "space ' quote\nline"
        let path = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                folderName: folder, branch: "existing", newBranch: false)
        let snapshot = try await repo.service.load(from: repo.checkout)
        #expect(snapshot.worktrees.contains { $0.directory == path && $0.branch == "existing" })
    }

    @Test func newBranchStartsAtSourceCheckoutHead() async throws {
        let repo = try Repository()
        let source = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                  folderName: "source", branch: "source", newBranch: true)
        try repo.git(["-C", source.path, "commit", "--allow-empty", "-m", "Linked checkout commit"])
        let expected = try repo.git(["-C", source.path, "rev-parse", "HEAD"])
        let destination = try await repo.service.create(in: source, root: repo.root,
                                                       folderName: "child", branch: "child", newBranch: true)
        #expect(try repo.git(["-C", destination.path, "rev-parse", "HEAD"]) == expected)
    }

    @Test func refusesRemovalOfIgnoredFiles() async throws {
        let repo = try Repository()
        try ".env\n".write(to: repo.checkout.appendingPathComponent(".gitignore"), atomically: true, encoding: .utf8)
        try repo.git(["add", ".gitignore"])
        try repo.git(["commit", "-m", "Ignore local secrets"])
        let path = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                folderName: "feature", branch: "feature", newBranch: true)
        let secrets = path.appendingPathComponent(".env")
        try "keep".write(to: secrets, atomically: true, encoding: .utf8)
        await #expect(throws: (any Error).self) {
            try await repo.service.remove(in: repo.checkout, directory: path)
        }
        #expect(FileManager.default.fileExists(atPath: secrets.path))
    }

    @Test func refusesDuplicateDestinationAndCheckedOutBranch() async throws {
        let repo = try Repository()
        let path = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                folderName: "feature", branch: "feature", newBranch: true)
        try "keep".write(to: path.appendingPathComponent("untracked"), atomically: true, encoding: .utf8)
        await #expect(throws: (any Error).self) {
            try await repo.service.create(in: repo.checkout, root: repo.root,
                                          folderName: "feature", branch: "other", newBranch: true)
        }
        await #expect(throws: (any Error).self) {
            try await repo.service.create(in: repo.checkout, root: repo.root,
                                          folderName: "main-again", branch: "main", newBranch: false)
        }
        #expect(try String(contentsOf: path.appendingPathComponent("untracked"), encoding: .utf8) == "keep")
    }

    @Test func rejectsInvalidNamesWithoutCreatingBranches() async throws {
        let repo = try Repository()
        for folder in ["..", ".", "../escape", "/absolute", ""] {
            await #expect(throws: (any Error).self) {
                try await repo.service.create(in: repo.checkout, root: repo.root,
                                              folderName: folder, branch: "safe", newBranch: true)
            }
        }
        for branch in ["--detach", "@{-1}", "bad name", "HEAD", ""] {
            await #expect(throws: (any Error).self) {
                try await repo.service.create(in: repo.checkout, root: repo.root,
                                              folderName: "safe", branch: branch, newBranch: true)
            }
        }
        #expect(try repo.git(["branch", "--format=%(refname:short)"]) == "main\n")
    }

    @Test func removesCleanWorktreeAndKeepsBranch() async throws {
        let repo = try Repository()
        let path = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                folderName: "feature", branch: "feature", newBranch: true)
        try await repo.service.remove(in: repo.checkout, directory: path)
        #expect(!FileManager.default.fileExists(atPath: path.path))
        #expect(try repo.git(["branch", "--list", "feature"]).contains("feature"))
    }

    @Test func refusesRemovalOfDirtyLockedAndMainWorktrees() async throws {
        let repo = try Repository()
        let path = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                folderName: "feature", branch: "feature", newBranch: true)
        let file = path.appendingPathComponent("untracked")
        try "keep".write(to: file, atomically: true, encoding: .utf8)
        await #expect(throws: (any Error).self) {
            try await repo.service.remove(in: repo.checkout, directory: path)
        }
        #expect(FileManager.default.fileExists(atPath: file.path))
        try FileManager.default.removeItem(at: file)
        try repo.git(["worktree", "lock", "--reason", "test", path.path])
        #expect(try await repo.service.load(from: repo.checkout).worktrees.last?.isLocked == true)
        await #expect(throws: (any Error).self) {
            try await repo.service.remove(in: repo.checkout, directory: path)
        }
        await #expect(throws: (any Error).self) {
            try await repo.service.remove(in: repo.checkout, directory: repo.checkout)
        }
        #expect(FileManager.default.fileExists(atPath: path.path))
    }

    @Test func rejectsNonRepository() async throws {
        let repo = try Repository()
        await #expect(throws: (any Error).self) {
            try await repo.service.load(from: repo.directory)
        }
    }

    @Test func tabStatusDistinguishesMainLinkedAndPlainDirectories() async throws {
        let repo = try Repository()
        let main = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(!main.isLinkedWorktree)
        #expect(main.branch == "main")
        #expect(!main.hasChanges)
        #expect(try await repo.service.tabStatus(from: repo.directory) == nil)

        let linked = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                  folderName: "linked", branch: "feature", newBranch: true)
        let nested = linked.appendingPathComponent("nested\nfolder")
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try "change".write(to: nested.appendingPathComponent("file"), atomically: true, encoding: .utf8)
        let status = try #require(try await repo.service.tabStatus(from: nested))
        #expect(status.directory == linked)
        #expect(status.isLinkedWorktree)
        #expect(status.branch == "feature")
        #expect(status.hasChanges)
    }
}
