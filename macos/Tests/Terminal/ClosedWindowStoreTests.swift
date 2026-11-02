import AppKit
import Testing
@testable import Ghostty

@Suite(.serialized)
@MainActor
struct ClosedWindowStoreTests {
    private func fixture() throws -> (URL, ClosedWindowSnapshot) {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
        let controller = TerminalController(ghostty)
        defer { controller.closeWindowImmediately() }
        return (directory, try ClosedWindowSnapshot.capture(from: controller))
    }

    @Test func closingNewWindowDoesNotReplaceOlderWindow() throws {
        let (directory, first) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        let firstTicket = try store.append(first)
        let second = try fixture().1
        _ = try store.append(second)

        let reloaded = ClosedWindowStore(directoryURL: directory)
        try reloaded.setPersistenceEnabled(true)
        #expect(reloaded.entries.map(\.id) == [second.id, first.id])
        try store.consume(id: first.id)
        #expect(firstTicket.isRestored)
        #expect(store.entries.map(\.id) == [second.id])
    }

    @Test func pruningDoesNotInvalidateLiveUndoTicket() throws {
        let (directory, first) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        let ticket = try store.append(first)
        for _ in 0..<20 {
            var next = first
            next.id = UUID()
            _ = try store.append(next)
        }
        #expect(store.entries.count == 20)
        #expect(!store.entries.contains { $0.id == first.id })
        #expect(!ticket.isRestored)
    }

    @Test func disabledPersistenceClearsAndDoesNotCapture() throws {
        let (directory, snapshot) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        _ = try store.append(snapshot)
        try store.setPersistenceEnabled(false)
        #expect(store.entries.isEmpty)
        #expect(!store.isPersistenceEnabled)
        #expect(!FileManager.default.fileExists(atPath: store.fileURL.path))
    }

    @Test func corruptHistoryCannotBeOverwrittenByClosingAnotherWindow() throws {
        let (directory, snapshot) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let store = ClosedWindowStore(directoryURL: directory)
        let corrupt = Data("invalid history".utf8)
        try corrupt.write(to: store.fileURL)
        #expect(throws: (any Error).self) { try store.setPersistenceEnabled(true) }
        #expect(throws: (any Error).self) { try store.append(snapshot) }
        #expect(try Data(contentsOf: store.fileURL) == corrupt)
        try store.clear()
        _ = try store.append(snapshot)
        #expect(store.entries.count == 1)
    }

    @Test func failedWritePreservesPreviousHistory() throws {
        let (directory, snapshot) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        _ = try store.append(snapshot)
        try FileManager.default.removeItem(at: store.fileURL)
        try FileManager.default.createDirectory(at: store.fileURL, withIntermediateDirectories: false)
        var next = snapshot
        next.id = UUID()
        #expect(throws: (any Error).self) { try store.append(next) }
        #expect(store.entries.map(\.id) == [snapshot.id])
    }

    @Test func byteLimitEvictsOldEntriesAndRejectsAnOversizedWindow() throws {
        let (directory, snapshot) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        func padded(_ bytes: Int) throws -> ClosedWindowSnapshot {
            var state = try #require(JSONSerialization.jsonObject(with: snapshot.tabs[0].stateData) as? [String: Any])
            state["padding"] = String(repeating: "x", count: bytes)
            return ClosedWindowSnapshot(
                id: UUID(), closedAt: snapshot.closedAt, title: snapshot.title, frame: snapshot.frame,
                selectedTabIndex: 0, tabs: [.init(stateVersion: TerminalRestorableState.version,
                                               stateData: try JSONSerialization.data(withJSONObject: state))])
        }
        let large = try padded(7 * 1024 * 1024)
        _ = try store.append(large)
        _ = try store.append(try padded(7 * 1024 * 1024))
        #expect(store.entries.count == 1)
        #expect(store.entries[0].id != large.id)
        let previous = store.entries.map(\.id)
        #expect(throws: (any Error).self) { try store.append(padded(8 * 1024 * 1024)) }
        #expect(store.entries.map(\.id) == previous)
        #expect(try Data(contentsOf: store.fileURL).count <= ClosedWindowStore.maximumBytes)
    }

    @Test func failedConsumptionHidesEntryAndCanBeRetried() throws {
        let (directory, snapshot) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        let ticket = try store.append(snapshot)
        try FileManager.default.removeItem(at: store.fileURL)
        try FileManager.default.createDirectory(at: store.fileURL, withIntermediateDirectories: false)
        #expect(throws: (any Error).self) { try store.consume(id: snapshot.id) }
        #expect(ticket.isRestored)
        #expect(store.entries.isEmpty)
        #expect(store.hasHistory)
        try FileManager.default.removeItem(at: store.fileURL)
        try store.consume(id: snapshot.id)
        let reloaded = ClosedWindowStore(directoryURL: directory)
        try reloaded.setPersistenceEnabled(true)
        #expect(reloaded.entries.isEmpty)
    }

    @Test func filesHaveOwnerOnlyPermissions() throws {
        let (directory, snapshot) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        _ = try store.append(snapshot)
        let file = try FileManager.default.attributesOfItem(atPath: store.fileURL.path)
        let parent = try FileManager.default.attributesOfItem(atPath: directory.path)
        #expect((file[.posixPermissions] as? NSNumber)?.intValue == 0o600)
        #expect((parent[.posixPermissions] as? NSNumber)?.intValue == 0o700)
    }

    @Test(arguments: ["schema", "selection", "tabVersion"])
    func rejectsUnsupportedHistoryWithoutLaunchingTerminals(kind: String) throws {
        let (directory, snapshot) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var window = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(snapshot)) as? [String: Any])
        if kind == "selection" { window["selectedTabIndex"] = 99 }
        if kind == "tabVersion" {
            var tabs = try #require(window["tabs"] as? [[String: Any]])
            tabs[0]["stateVersion"] = 999
            window["tabs"] = tabs
        }
        let data = try JSONSerialization.data(withJSONObject: ["version": kind == "schema" ? 999 : 1, "windows": [window]])
        let store = ClosedWindowStore(directoryURL: directory)
        try data.write(to: store.fileURL)
        let before = TerminalController.all.count
        #expect(throws: (any Error).self) { try store.setPersistenceEnabled(true) }
        #expect(throws: (any Error).self) { try store.append(snapshot) }
        #expect(TerminalController.all.count == before)
        #expect(try Data(contentsOf: store.fileURL) == data)
    }

    @Test func rejectsAnOversizedHistoryFileBeforeDecoding() throws {
        let (directory, _) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let store = ClosedWindowStore(directoryURL: directory)
        try Data(repeating: 0, count: ClosedWindowStore.maximumBytes + 1).write(to: store.fileURL)
        #expect(throws: (any Error).self) { try store.setPersistenceEnabled(true) }
        #expect(store.entries.isEmpty)
    }

    @Test func failedDeletionStillDisablesRecoveryAndCanBeRetried() throws {
        let (directory, snapshot) = try fixture()
        defer {
            try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
            try? FileManager.default.removeItem(at: directory)
        }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        _ = try store.append(snapshot)
        try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: directory.path)
        #expect(throws: (any Error).self) { try store.setPersistenceEnabled(false) }
        #expect(!store.isPersistenceEnabled)
        #expect(store.entries.isEmpty)
        #expect(throws: (any Error).self) { try store.append(snapshot) }
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
        try store.setPersistenceEnabled(false)
        #expect(!FileManager.default.fileExists(atPath: store.fileURL.path))
    }

    @Test func staleConsumptionAfterClearCannotRecreateHistory() throws {
        let (directory, snapshot) = try fixture()
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        _ = try store.append(snapshot)
        try store.clear()
        try store.consume(id: snapshot.id)
        #expect(store.entries.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: store.fileURL.path))
    }
}
