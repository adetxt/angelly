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

        init(initialCommit: Bool = true) throws {
            try FileManager.default.createDirectory(at: checkout, withIntermediateDirectories: true)
            try git(["init", "-b", "main"])
            try git(["config", "user.name", "Worktree Tests"])
            try git(["config", "user.email", "worktrees@example.invalid"])
            try git(["config", "core.hooksPath", "/dev/null"])
            if initialCommit { try git(["commit", "--allow-empty", "-m", "Initial"]) }
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

    @Test(arguments: ["git@github.com:owner/repo.git", "https://github.com/owner/repo.git",
                      "ssh://git@github.com/owner/repo.git"])
    func ownerAvatarUsesOriginAndFollowsRemoteChanges(remote: String) async throws {
        let repo = try Repository()
        try repo.git(["remote", "add", "upstream", "https://github.com/upstream/repo.git"])
        try repo.git(["remote", "add", "origin", remote])
        let status = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(status.ownerAvatarURL == URL(string: "https://github.com/owner.png?size=32"))

        try repo.git(["remote", "set-url", "origin", "https://github.com/new-owner/repo.git"])
        try repo.git(["checkout", "--detach"])
        let detached = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(detached.ownerAvatarURL == URL(string: "https://github.com/new-owner.png?size=32"))
    }

    @Test func ownerAvatarSupportsLinkedWorktreesWithoutOrigin() async throws {
        let repo = try Repository()
        try repo.git(["remote", "add", "upstream", "https://github.com/owner/repo.git"])
        let linked = try await repo.service.create(in: repo.checkout, root: repo.root,
                                                   folderName: "feature", branch: "feature", newBranch: true)
        let status = try #require(try await repo.service.tabStatus(from: linked))
        #expect(status.ownerAvatarURL == URL(string: "https://github.com/owner.png?size=32"))
    }

    @Test func ownerAvatarIsAbsentForLocalAndUnsupportedRepositories() async throws {
        let repo = try Repository()
        let local = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(local.ownerAvatarURL == nil)
        try repo.git(["remote", "add", "origin", "https://git.example.invalid/owner/repo.git"])
        let unsupported = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(unsupported.ownerAvatarURL == nil)
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

    @Test func discoversRepositoriesFromTerminalsAndWorktreeRootWithoutDuplicates() async throws {
        let first = try Repository()
        let second = try Repository()
        let linked = try await first.service.create(in: first.checkout, root: first.root,
                                                     folderName: "feature", branch: "feature", newBranch: true)
        let nested = second.checkout.appendingPathComponent("nested")
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: first.root.appendingPathComponent("not a repo/empty"),
                                                withIntermediateDirectories: true)

        let snapshots = await first.service.loadRepositories(from: [linked, nested, second.checkout],
                                                              worktreeRoot: first.root)
        #expect(Set(snapshots.map(\.directory)) == Set([
            first.checkout.resolvingSymlinksInPath(), second.checkout.resolvingSymlinksInPath()
        ]))
        #expect(snapshots.count == 2)
        #expect(snapshots.first { $0.directory == first.checkout.resolvingSymlinksInPath() }?.worktrees.count == 2)

        let fromRoot = await first.service.loadRepositories(from: [], worktreeRoot: first.root)
        #expect(fromRoot.map(\.directory) == [first.checkout.resolvingSymlinksInPath()])
    }

    @Test func missingWorktreeRootStillDiscoversTerminalRepository() async throws {
        let repo = try Repository()
        let snapshots = await repo.service.loadRepositories(from: [repo.checkout], worktreeRoot: repo.root)
        #expect(snapshots.map(\.directory) == [repo.checkout.resolvingSymlinksInPath()])
        let empty = await repo.service.loadRepositories(from: [repo.directory], worktreeRoot: repo.root)
        #expect(empty.isEmpty)
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

    @Test func lineChangesCompareFinalCheckoutAgainstHeadWithoutDoubleCounting() async throws {
        let repo = try Repository()
        let file = repo.checkout.appendingPathComponent("tracked")
        try "original\nkeep\n".write(to: file, atomically: true, encoding: .utf8)
        try repo.git(["add", "."])
        try repo.git(["commit", "-m", "Tracked file"])
        try "staged\nkeep\nextra\n".write(to: file, atomically: true, encoding: .utf8)
        try repo.git(["add", "."])
        try "final\nkeep\n".write(to: file, atomically: true, encoding: .utf8)
        let status = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(status.lineChanges == GitLineChanges(additions: 1, deletions: 1))

        try "original\nkeep\n".write(to: file, atomically: true, encoding: .utf8)
        let restored = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(restored.hasChanges)
        #expect(restored.lineChanges == GitLineChanges(additions: 0, deletions: 0))
    }

    @Test func lineChangesHandleRenamesAndPathsContainingTabsAndNewlines() async throws {
        let repo = try Repository()
        let original = "old name\nwith\ttab"
        let renamed = "new name\nwith\ttab"
        let content = (1...20).map { "line \($0)\n" }.joined()
        try content.write(to: repo.checkout.appendingPathComponent(original), atomically: true, encoding: .utf8)
        try repo.git(["add", "."])
        try repo.git(["commit", "-m", "Original"])
        try repo.git(["mv", "--", original, renamed])
        try (content + "added\n").write(to: repo.checkout.appendingPathComponent(renamed), atomically: true, encoding: .utf8)
        let status = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(status.lineChanges == GitLineChanges(additions: 1, deletions: 0))
    }

    @Test func lineChangesIncludeUntrackedTextAndExcludeBinaryIgnoredAndSymlinkTargets() async throws {
        let repo = try Repository()
        let tracked = repo.checkout.appendingPathComponent("binary")
        try Data([0, 10, 11]).write(to: tracked)
        try "ignored\n".write(to: repo.checkout.appendingPathComponent(".gitignore"), atomically: true, encoding: .utf8)
        try repo.git(["add", "."])
        try repo.git(["commit", "-m", "Binary and ignore rules"])
        try Data([0, 10, 11, 10]).write(to: tracked)
        let nested = repo.checkout.appendingPathComponent("nested")
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try "first\nlast".write(to: nested.appendingPathComponent("untracked\nfile"), atomically: true, encoding: .utf8)
        try Data([0, 10, 10]).write(to: nested.appendingPathComponent("untracked binary"))
        try "ignored\nlines\n".write(to: repo.checkout.appendingPathComponent("ignored"), atomically: true, encoding: .utf8)
        let outside = repo.directory.appendingPathComponent("outside")
        try "outside\nlines\n".write(to: outside, atomically: true, encoding: .utf8)
        try FileManager.default.createSymbolicLink(at: repo.checkout.appendingPathComponent("link"), withDestinationURL: outside)
        try FileManager.default.createSymbolicLink(at: repo.checkout.appendingPathComponent("directory link"),
                                                 withDestinationURL: repo.directory)
        let status = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(status.lineChanges == GitLineChanges(additions: 2, deletions: 0))
    }

    @Test func lineChangesCountStagedAndUntrackedFilesOnUnbornBranches() async throws {
        let repo = try Repository(initialCommit: false)
        let staged = repo.checkout.appendingPathComponent("staged")
        try "one\ntwo\n".write(to: staged, atomically: true, encoding: .utf8)
        try repo.git(["add", "."])
        try "one\n".write(to: staged, atomically: true, encoding: .utf8)
        try "untracked".write(to: repo.checkout.appendingPathComponent("untracked"), atomically: true, encoding: .utf8)
        let status = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(status.branch == "main")
        #expect(status.lineChanges == GitLineChanges(additions: 2, deletions: 0))
    }

    @Test func oversizedUntrackedFilesKeepBranchContextWithUnavailableLineChanges() async throws {
        let repo = try Repository()
        try Data(repeating: 10, count: 16 * 1_024 * 1_024 + 1)
            .write(to: repo.checkout.appendingPathComponent("large"))
        let status = try #require(try await repo.service.tabStatus(from: repo.checkout))
        #expect(status.branch == "main")
        #expect(status.hasChanges)
        #expect(status.lineChanges == nil)
        #expect(status.detail.contains("Line changes unavailable"))
    }

    @Test func numstatReadsBinaryAndRenameRecordsWithoutParsingTheirPaths() throws {
        let output = "2\t3\tordinary\nname\0-\t-\tbinary\0" +
            "1\t0\t\0old\nname\0new\tname\0"
        let counts = try GitWorktreeService.parseNumstat(output)
        #expect(counts.additions == 3)
        #expect(counts.deletions == 3)
        for malformed in ["1\t2\tmissing terminator", "1\t0\t\0old\0", "-1\t0\tfile\0", "-\t1\tfile\0"] {
            #expect(throws: (any Error).self) { try GitWorktreeService.parseNumstat(malformed) }
        }
    }
}
