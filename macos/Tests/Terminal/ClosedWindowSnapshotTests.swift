import AppKit
import Testing
@testable import Ghostty

@Suite(.serialized)
@MainActor
struct ClosedWindowSnapshotTests {
    @Test func capturesOrderedTabsWithoutDecodingTerminals() throws {
        let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
        let first = TerminalController(ghostty)
        let window = try #require(first.window)
        let second = try #require(TerminalController.newTab(ghostty, from: window))
        defer { first.closeWindowImmediately() }
        first.titleOverride = "Work"
        second.titleOverride = "Server"
        window.tabGroup?.selectedWindow = window
        let before = TerminalController.all.count

        let snapshot = try ClosedWindowSnapshot.capture(from: first)
        let decoded = try JSONDecoder().decode(
            ClosedWindowSnapshot.self, from: JSONEncoder().encode(snapshot))
        try decoded.validate()

        #expect(decoded.tabs.count == 2)
        #expect(decoded.selectedTabIndex == 0)
        #expect(decoded.title == "Work")
        #expect(TerminalController.all.count == before)
    }

    @Test func rejectsCustomCommandWindow() throws {
        let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
        var config = Ghostty.SurfaceConfiguration()
        config.command = "/bin/sleep 60"
        let controller = TerminalController(ghostty, withBaseConfig: config)
        defer { controller.closeWindowImmediately() }
        #expect(throws: (any Error).self) { try ClosedWindowSnapshot.capture(from: controller) }
    }

    @Test func recoveryPreservesLayoutWithFreshIdentities() throws {
        let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
        let first = TerminalController(ghostty)
        let firstWindow = try #require(first.window as? TerminalWindow)
        let second = try #require(TerminalController.newTab(ghostty, from: firstWindow))
        let group = TabGroupStore.shared.create()
        firstWindow.sidebarGroup = group
        (second.window as? TerminalWindow)?.sidebarGroup = group
        first.titleOverride = "Work"
        second.titleOverride = "Server"
        firstWindow.tabColor = .purple
        let original = try #require(first.surfaceTree.first)
        let app = try #require(ghostty.app)
        let split = Ghostty.SurfaceView(app, baseConfig: .init())
        first.surfaceTree = try first.surfaceTree.inserting(view: split, at: original, direction: .right)
        first.focusedSurface = split
        let originalIDs = Set([first, second].flatMap { $0.surfaceTree.map(\.id) })
        defer { first.closeWindowImmediately() }

        let snapshot = try ClosedWindowSnapshot.capture(from: first)
        let result = try TerminalController.reopen(snapshot, ghostty: ghostty)
        defer { result.controllers.first?.closeWindowImmediately() }
        #expect(result.controllers.map(\.titleOverride) == ["Work", "Server"])
        #expect(result.controllers[0].window?.tabGroup?.windows.count == 2)
        #expect(result.controllers[0].surfaceTree.count == 2)
        #expect(result.controllers[0].focusedSurface?.id != split.id)
        #expect((result.controllers[0].window as? TerminalWindow)?.tabColor == .purple)
        let recoveredGroup = try #require((result.controllers[0].window as? TerminalWindow)?.sidebarGroup)
        #expect(recoveredGroup != group)
        #expect((result.controllers[1].window as? TerminalWindow)?.sidebarGroup == recoveredGroup)
        #expect(TabGroupStore.shared.snapshot(recoveredGroup)?.info.name == TabGroupStore.shared.snapshot(group)?.info.name)
        #expect(originalIDs.isDisjoint(with: result.controllers.flatMap { $0.surfaceTree.map(\.id) }))
        #expect(firstWindow.tabGroup?.windows.count == 2)
    }

    @Test func missingDirectoryFallsBackWithoutReusingSavedIdentity() throws {
        let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
        let controller = TerminalController(ghostty)
        defer { controller.closeWindowImmediately() }
        let original = try ClosedWindowSnapshot.capture(from: controller)
        var state = try #require(JSONSerialization.jsonObject(with: original.tabs[0].stateData) as? [String: Any])
        var tree = try #require(state["surfaceTree"] as? [String: Any])
        var root = try #require(tree["root"] as? [String: Any])
        var view = try #require(root["view"] as? [String: Any])
        view["pwd"] = "/nonexistent-\(UUID().uuidString)"
        root["view"] = view
        tree["root"] = root
        state["surfaceTree"] = tree
        let snapshot = ClosedWindowSnapshot(
            id: original.id, closedAt: original.closedAt, title: original.title,
            frame: NSRect(x: 100000, y: 100000, width: 640, height: 480), selectedTabIndex: 0,
            tabs: [.init(stateVersion: TerminalRestorableState.version,
                         stateData: try JSONSerialization.data(withJSONObject: state))])
        let result = try TerminalController.reopen(snapshot, ghostty: ghostty)
        defer { result.controllers.first?.closeWindowImmediately() }
        #expect(result.usedDirectoryFallback)
        let restoredWindow = try #require(result.controllers.first?.window)
        #expect(restoredWindow.frame.origin != snapshot.frame.origin)
    }

    @Test func invalidPayloadIsRejectedBeforeCreatingAnyTerminals() throws {
        let invalid = ClosedWindowSnapshot(
            id: UUID(), closedAt: Date(), title: "Invalid", frame: NSRect(x: 0, y: 0, width: 640, height: 480),
            selectedTabIndex: 0, tabs: [.init(stateVersion: TerminalRestorableState.version, stateData: Data("{}".utf8))])
        let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
        let before = TerminalController.all.count
        #expect(throws: (any Error).self) { try TerminalController.reopen(invalid, ghostty: ghostty) }
        #expect(TerminalController.all.count == before)
    }

    @Test func failedTerminalInitializationCannotBeAcceptedAsRecovery() throws {
        let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
        let controller = TerminalController(ghostty)
        defer { controller.closeWindowImmediately() }
        let view = try #require(controller.surfaceTree.first)
        let context = ClosedWindowRecoveryContext()
        try context.validate(view)
        // This is the error state SurfaceView exposes when ghostty_surface_new fails.
        view.error = Ghostty.Error.apiFailed
        defer { view.error = nil }
        #expect(throws: (any Error).self) { try context.validate(view) }
    }
}
