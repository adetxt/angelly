import AppKit
import Testing
@testable import Ghostty

@Suite(.serialized)
@MainActor
struct ClosedWindowRecoveryTests {
    private class RedButtonController: TerminalController {
        override func closeWindow(_ sender: Any?) { closeWindowImmediately(origin: .user) }
    }

    private class ConfirmationController: TerminalController {
        var confirmationCount = 0
        var pendingConfirmation: (() -> Void)?

        override func confirmClose(
            messageText: String, informativeText: String, confirmButtonTitle: String = "Close",
            completion: @escaping () -> Void
        ) {
            confirmationCount += 1
            pendingConfirmation = completion
        }
    }

    private func runningController(_ app: AppDelegate) throws -> ConfirmationController {
        var config = Ghostty.SurfaceConfiguration()
        config.command = "/bin/sleep 60"
        let view = Ghostty.SurfaceView(try #require(app.ghostty.app), baseConfig: config)
        try #require(view.needsConfirmQuit)
        return ConfirmationController(app.ghostty, withSurfaceTree: .init(view: view))
    }

    private func withStore(_ body: (AppDelegate, ClosedWindowStore) throws -> Void) throws {
        let app = try #require(NSApp.delegate as? AppDelegate)
        let original = app.closedWindowStore
        let originalUndo = app.undoManager
        app.undoManager = ExpiringUndoManager()
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let store = ClosedWindowStore(directoryURL: directory)
        try store.setPersistenceEnabled(true)
        app.closedWindowStore = store
        let existing = Set(TerminalController.all.map(ObjectIdentifier.init))
        defer {
            app.undoManager.disableUndoRegistration()
            for controller in TerminalController.all where !existing.contains(ObjectIdentifier(controller)) {
                controller.closeWindowImmediately()
            }
            app.closedWindowStore = original
            app.undoManager.removeAllActions()
            app.undoManager = originalUndo
            try? FileManager.default.removeItem(at: directory)
        }
        try body(app, store)
    }

    @Test func closeTwoWindowsKeepsBothAndUndoConsumesOnlyItsEntry() throws {
        try withStore { app, store in
            let first = TerminalController(app.ghostty)
            let second = TerminalController(app.ghostty)
            app.undoManager.removeAllActions()
            app.undoManager.groupsByEvent = false
            defer { app.undoManager.groupsByEvent = true }
            app.undoManager.beginUndoGrouping()
            first.closeWindowImmediately(origin: .user)
            app.undoManager.endUndoGrouping()
            let firstID = try #require(store.entries.first?.id)
            app.undoManager.beginUndoGrouping()
            second.closeWindowImmediately(origin: .user)
            app.undoManager.endUndoGrouping()
            #expect(store.entries.count == 2)
            app.undoManager.undo()
            #expect(store.entries.map(\.id) == [firstID])
        }
    }

    @Test func persistentRecoveryPreventsStaleUndoFromDuplicatingWindow() throws {
        try withStore { app, store in
            let controller = TerminalController(app.ghostty)
            app.undoManager.removeAllActions()
            app.undoManager.groupsByEvent = false
            defer { app.undoManager.groupsByEvent = true }
            app.undoManager.beginUndoGrouping()
            controller.closeWindowImmediately(origin: .user)
            app.undoManager.endUndoGrouping()
            let snapshot = try #require(store.entries.first)
            let recovered = try TerminalController.reopen(snapshot, ghostty: app.ghostty)
            try store.consume(id: snapshot.id)
            let count = TerminalController.all.count
            app.undoManager.undo()
            #expect(TerminalController.all.count == count)
            #expect(store.entries.isEmpty)
        }
    }

    @Test func wholeTabGroupCreatesOneRecordAndAutomaticCloseCreatesNone() throws {
        try withStore { app, store in
            let first = TerminalController(app.ghostty)
            let second = try #require(TerminalController.newTab(app.ghostty, from: first.window))
            first.closeWindowImmediately(origin: .user)
            #expect(store.entries.count == 1)
            #expect(store.entries.first?.tabs.count == 2)
            let automatic = TerminalController(app.ghostty)
            automatic.closeWindowImmediately()
            #expect(store.entries.count == 1)
        }
    }

    @Test func disabledHistoryDoesNotBlockUserClose() throws {
        try withStore { app, store in
            try store.setPersistenceEnabled(false)
            let controller = TerminalController(app.ghostty)
            let window = try #require(controller.window)
            controller.closeWindowImmediately(origin: .user)
            #expect(!window.isVisible)
            #expect(store.entries.isEmpty)
            #expect(!FileManager.default.fileExists(atPath: store.fileURL.path))
        }
    }

    @Test func menuRemainsAvailableWithoutAKeyWindow() throws {
        try withStore { app, store in
            let controller = TerminalController(app.ghostty)
            controller.closeWindowImmediately(origin: .user)
            let item = NSMenuItem(title: "Reopen Closed Window", action: #selector(app.reopenClosedWindow(_:)), keyEquivalent: "")
            #expect(app.validateMenuItem(item))
            try store.clear()
            #expect(!app.validateMenuItem(item))
        }
    }

    @Test func redTrafficLightClosesAndCapturesTheWholeWindow() throws {
        try withStore { app, store in
            let first = RedButtonController(app.ghostty)
            _ = try #require(TerminalController.newTab(app.ghostty, from: first.window))
            let window = try #require(first.window)
            window.makeKeyAndOrderFront(nil)
            let red = try #require(window.standardWindowButton(.closeButton))
            #expect(red.action == #selector(TerminalController.closeWindow(_:)))
            red.performClick(nil)
            #expect(store.entries.count == 1)
            #expect(store.entries.first?.tabs.count == 2)
        }
    }

    @Test func explicitTerminalCloseRecordsButProcessExitDoesNot() throws {
        try withStore { app, store in
            let explicit = TerminalController(app.ghostty)
            let view = try #require(explicit.surfaceTree.first)
            explicit.closeSurface(view, withConfirmation: false, origin: .user)
            #expect(store.entries.count == 1)
            let automatic = TerminalController(app.ghostty)
            let exited = try #require(automatic.surfaceTree.first)
            automatic.closeSurface(exited, withConfirmation: false, origin: .automatic)
            #expect(store.entries.count == 1)
        }
    }

    @Test func closeAllRecordsEachLogicalWindowOnce() throws {
        try withStore { app, store in
            let first = TerminalController(app.ghostty)
            let tab = try #require(TerminalController.newTab(app.ghostty, from: first.window))
            let second = TerminalController(app.ghostty)
            TerminalController.closeAllWindowsImmediately([first, tab, second])
            #expect(store.entries.count == 2)
            #expect(store.entries.map { $0.tabs.count }.sorted() == [1, 2])
        }
    }

    @Test func cancelingProcessConfirmationDoesNotCaptureOrClose() throws {
        try withStore { app, store in
            let controller = try runningController(app)
            controller.requestCloseWindow(origin: .user)
            #expect(controller.confirmationCount == 1)
            #expect(controller.pendingConfirmation != nil)
            #expect(store.entries.isEmpty)
            controller.pendingConfirmation = nil
            #expect(!controller.surfaceTree.isEmpty)
            #expect(store.entries.isEmpty)
        }
    }

    @Test func changedTabMembershipRequiresFreshConfirmationBeforeCapture() throws {
        try withStore { app, store in
            let controller = try runningController(app)
            controller.requestCloseWindow(origin: .user)
            let acceptOriginal = try #require(controller.pendingConfirmation)
            controller.pendingConfirmation = nil
            let window = try #require(controller.window)
            let added = NSWindow(
                contentRect: window.frame, styleMask: [.titled, .closable], backing: .buffered, defer: false)
            added.isReleasedWhenClosed = false
            defer {
                added.tabGroup?.removeWindow(added)
                added.close()
            }
            try #require(window.addTabbedWindowSafely(added, ordered: .above))
            acceptOriginal()
            #expect(controller.confirmationCount == 2)
            #expect(store.entries.isEmpty)
            #expect(!controller.surfaceTree.isEmpty)
            controller.pendingConfirmation = nil
        }
    }
}
