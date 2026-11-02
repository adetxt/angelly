import AppKit
import SwiftUI
import Testing
import UniformTypeIdentifiers
@testable import Ghostty

@Suite(.serialized)
@MainActor
struct TerminalTabModelTests {
    @Test(arguments: [Axis.horizontal, .vertical], [VerticalEdge.top, .bottom])
    func insertionMarkerKeepsDropTargetSize(axis: Axis, edge: VerticalEdge) {
        let view = NSHostingView(rootView: Color.clear
            .frame(width: 168, height: 32)
            .tabDropTarget(
                axis: axis, position: .constant(0.1), edge: edge,
                canDrop: { true }, drop: { _ in }))
        #expect(view.fittingSize == NSSize(width: 168, height: 32))
    }

    /// SwiftUI only matches the tab drag payload when its type is declared by the app bundle.
    @Test func declaresTheTabDragType() throws {
        let declarations = try #require(
            Bundle(for: TerminalTabModel.self).object(forInfoDictionaryKey: "UTExportedTypeDeclarations")
                as? [[String: Any]])
        let identifiers = declarations.compactMap { $0["UTTypeIdentifier"] as? String }
        #expect(identifiers.contains(UTType.ghosttyTerminalTabItem.identifier))
    }

    @Test func refreshesWhenSidebarIsHidden() throws {
        let fixture = try Fixture(sidebarVisible: false)
        defer { fixture.close() }
        fixture.model.refresh()
        #expect(fixture.model.items.count == 4)
        #expect(fixture.model.rows.count == 4)
        #expect(fixture.windows[0].tabBarIsHidden)
    }

    @Test func collapsedGroupKeepsSelectedTabInHeader() throws {
        let fixture = try Fixture()
        defer { fixture.close() }
        let group = fixture.group([0, 1])
        fixture.windows[0].tabGroup?.selectedWindow = fixture.windows[0]
        fixture.model.toggleCollapsed(group)
        fixture.model.refresh()
        #expect(fixture.model.rows.count == 3)
        let header = try #require(fixture.model.groupHeaders.first)
        #expect(header.id == group)
        #expect(header.count == 2)
        #expect(header.isCollapsed)
        #expect(header.containsSelected)
    }

    @Test func movesWholeGroupWithoutSplittingAnotherGroup() throws {
        let fixture = try Fixture()
        defer { fixture.close() }
        let first = fixture.group([0, 1])
        let second = fixture.group([2, 3])
        let selected = fixture.windows[0].tabGroup?.selectedWindow
        fixture.model.beginDrag(first, anchor: ObjectIdentifier(fixture.windows[0]))
        fixture.model.drop(on: fixture.model.items[2])
        fixture.model.refresh()
        #expect(fixture.windows[0].tabGroup?.windows == [
            fixture.windows[2], fixture.windows[3], fixture.windows[0], fixture.windows[1],
        ])
        #expect(fixture.model.items.map(\.group) == [second, second, first, first])
        #expect(fixture.windows[0].tabGroup?.selectedWindow === selected)
    }

    @Test(arguments: [0, 3], [false, true])
    func reorderingPreservesWindowFrame(dragged: Int, titlebarTabs: Bool) async throws {
        let fixture = try Fixture(titlebarTabs: titlebarTabs)
        defer { fixture.close() }
        let selected = fixture.windows[0]
        selected.makeKeyAndOrderFront(nil)
        selected.setFrame(NSRect(x: 140, y: 160, width: 720, height: 520), display: false)
        let frame = selected.frame
        fixture.model.beginDrag(fixture.model.items[dragged])
        fixture.model.drop(on: fixture.model.items[dragged == 0 ? 3 : 0])
        #expect(selected.frame == frame)
        #expect(selected.tabGroup?.selectedWindow === selected)
        let order = dragged == 0 ? [1, 2, 3, 0] : [3, 0, 1, 2]
        #expect(selected.tabGroup?.windows == order.map { fixture.windows[$0] })
        // Tahoe defers making an inactive tab key until the next turn.
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
        #expect(selected.frame == frame)
        #expect(selected.tabGroup?.selectedWindow === selected)
    }

    @Test func joinsAndLeavesGroupWithoutSplittingIt() throws {
        let fixture = try Fixture()
        defer { fixture.close() }
        let group = fixture.group([0, 1])
        fixture.model.add(fixture.model.items[3], to: group)
        fixture.model.refresh()
        #expect(fixture.windows[0].tabGroup?.windows == [
            fixture.windows[0], fixture.windows[1], fixture.windows[3], fixture.windows[2],
        ])
        fixture.model.leaveGroup(fixture.model.items[0])
        fixture.model.refresh()
        #expect(fixture.windows[0].tabGroup?.windows == [
            fixture.windows[1], fixture.windows[3], fixture.windows[0], fixture.windows[2],
        ])
        #expect(fixture.windows[0].sidebarGroup == nil)
        #expect(fixture.windows[1].sidebarGroup == group)
        #expect(fixture.windows[3].sidebarGroup == group)
    }

    @Test func droppingAtEndUngroupsTabAndFinishesDrag() throws {
        let fixture = try Fixture()
        defer { fixture.close() }
        _ = fixture.group([0, 1])
        fixture.model.beginDrag(fixture.model.items[0])
        fixture.model.dropAtEnd()
        fixture.model.refresh()
        #expect(fixture.windows[0].tabGroup?.windows == [
            fixture.windows[1], fixture.windows[2], fixture.windows[3], fixture.windows[0],
        ])
        #expect(fixture.windows[0].sidebarGroup == nil)
        #expect(fixture.model.draggedItem == nil)
    }

    @Test(arguments: [(VerticalEdge.top, 3, [0, 3, 1, 2]), (.bottom, 0, [1, 2, 0, 3])])
    func droppingBesideGroupMovesTabOutsideIt(edge: VerticalEdge, dragged: Int, order: [Int]) throws {
        let fixture = try Fixture()
        defer { fixture.close() }
        let group = fixture.group([1, 2])
        fixture.model.beginDrag(fixture.model.items[dragged])
        fixture.model.drop(besideGroup: group, edge: edge)
        fixture.model.refresh()
        #expect(fixture.windows[0].tabGroup?.windows == order.map { fixture.windows[$0] })
        #expect(fixture.windows[dragged].sidebarGroup == nil)
        #expect(fixture.windows[1].sidebarGroup == group)
        #expect(fixture.windows[2].sidebarGroup == group)
        #expect(fixture.model.draggedItem == nil)
    }

    @Test func tabLandsBesideGroupOnlyAtItsOuterQuarters() throws {
        let fixture = try Fixture()
        defer { fixture.close() }
        let group = fixture.group([1, 2])
        let expanded = try #require(fixture.model.groupHeaders.first)
        #expect(fixture.model.landing(onGroup: expanded, at: 0.1) == .before)
        #expect(fixture.model.landing(onGroup: expanded, at: 0.5) == .inside)
        #expect(fixture.model.landing(onGroup: expanded, at: 0.9) == .inside)

        fixture.model.toggleCollapsed(group)
        fixture.model.refresh()
        let collapsed = try #require(fixture.model.groupHeaders.first)
        #expect(fixture.model.landing(onGroup: collapsed, at: 0.5) == .inside)
        #expect(fixture.model.landing(onGroup: collapsed, at: 0.9) == .after)

        fixture.model.beginDrag(fixture.model.items[0])
        fixture.model.drop(onGroup: collapsed, at: 0.9)
        fixture.model.refresh()
        #expect(fixture.windows[0].tabGroup?.windows == [
            fixture.windows[1], fixture.windows[2], fixture.windows[0], fixture.windows[3],
        ])
        #expect(fixture.windows[0].sidebarGroup == nil)
    }

    @Test func droppingMemberBesideItsGroupLeavesTheGroup() throws {
        let fixture = try Fixture()
        defer { fixture.close() }
        let group = fixture.group([1, 2])
        fixture.model.beginDrag(fixture.model.items[1])
        fixture.model.drop(besideGroup: group, edge: .bottom)
        fixture.model.refresh()
        #expect(fixture.windows[0].tabGroup?.windows == [
            fixture.windows[0], fixture.windows[2], fixture.windows[1], fixture.windows[3],
        ])
        #expect(fixture.windows[1].sidebarGroup == nil)
        #expect(fixture.windows[2].sidebarGroup == group)
    }

    @Test func ignoresDragFromAnotherWindow() throws {
        let source = try Fixture()
        let destination = try Fixture()
        defer {
            destination.close()
            source.close()
        }
        let original = destination.windows[0].tabGroup?.windows
        source.model.beginDrag(source.model.items[0])
        destination.model.drop(on: destination.model.items[0], edge: .bottom)
        #expect(destination.windows[0].tabGroup?.windows == original)
        #expect(source.windows[0].tabGroup !== destination.windows[0].tabGroup)
    }

    @Test func closesOnlyMembersOfGroup() throws {
        let fixture = try Fixture()
        defer { fixture.close() }
        let group = fixture.group([0, 1])
        fixture.model.closeGroup(group)
        #expect(fixture.windows[2].tabGroup?.windows == [fixture.windows[2], fixture.windows[3]])
    }

    @Test func undoRestoresEntireGroupInOriginalOrder() async throws {
        var pendingFixture: Fixture? = try Fixture(withSurfaces: true)
        try await pendingFixture?.waitForProcessExit()
        let (group, undoManager, surfaceIDs, tabbingIdentifier) = try autoreleasepool {
            let fixture = try #require(pendingFixture)
            defer {
                fixture.close()
                pendingFixture = nil
            }
            let group = fixture.group([0, 1, 2, 3])
            let undoManager = try #require(fixture.windows[0].windowController?.undoManager)
            let surfaceIDs = fixture.windows.compactMap {
                ($0.windowController as? TerminalController)?.surfaceTree.first?.id
            }
            undoManager.beginUndoGrouping()
            fixture.model.closeGroup(group)
            undoManager.endUndoGrouping()
            return (group, undoManager, surfaceIDs, fixture.windows[0].tabbingIdentifier)
        }
        await Task.yield()
        undoManager.undo()
        // The window restore path finishes its initial presentation on the next turn.
        await Task.yield()
        let restored = NSApp.windows.compactMap { $0 as? TerminalWindow }.filter { $0.sidebarGroup == group }
        defer { restored.forEach { $0.close() } }
        #expect(restored.count == 4)
        #expect(restored.allSatisfy { $0.tabbingIdentifier == tabbingIdentifier })
        let windows = try #require(restored.first?.tabGroup?.windows)
        #expect(windows.count == 4)
        #expect(windows.compactMap {
            ($0.windowController as? TerminalController)?.surfaceTree.first?.id
        } == surfaceIDs)
    }

    @Test func reservesSpaceForOnlyTheVisibleTabLayout() throws {
        let fixture = try Fixture(sidebarVisible: false)
        defer { fixture.close() }
        let originalWidth = TabSidebarState.shared.width
        TabSidebarState.shared.setWidth(240)
        defer { TabSidebarState.shared.setWidth(originalWidth) }
        let container = TerminalViewContainer { EmptyView() }
        container.initialContentSize = NSSize(width: 640, height: 480)
        container.installTabViews(fixture.model)
        #expect(container.intrinsicContentSize == NSSize(width: 640, height: 520))
        TabSidebarState.shared.toggle()
        #expect(container.intrinsicContentSize == NSSize(width: 880, height: 480))
        TabSidebarState.shared.toggle()
        #expect(container.intrinsicContentSize == NSSize(width: 640, height: 520))
    }

    @MainActor
    private final class Fixture {
        let windows: [TerminalWindow]
        let model: TerminalTabModel
        private let wasVisible: Bool
        private var groups: [UUID] = []
        private var controllers: [TerminalController] = []

        init(sidebarVisible: Bool = true, withSurfaces: Bool = false, titlebarTabs: Bool = false) throws {
            let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
            wasVisible = TabSidebarState.shared.isVisible
            if wasVisible != sidebarVisible { TabSidebarState.shared.toggle() }
            let identifier = UUID().uuidString
            windows = (0..<4).map { index in
                let rect = NSRect(x: 0, y: 0, width: 640, height: 480)
                let style: NSWindow.StyleMask = [.titled, .closable, .resizable]
                let window: TerminalWindow
                if titlebarTabs, #available(macOS 26, *) {
                    window = TitlebarTabsTahoeTerminalWindow(
                        contentRect: rect, styleMask: style, backing: .buffered, defer: false)
                } else {
                    window = TerminalWindow(contentRect: rect, styleMask: style, backing: .buffered, defer: false)
                }
                window.isReleasedWhenClosed = false
                window.tabbingIdentifier = identifier
                window.title = "Tab \(index + 1)"
                return window
            }
            for index in 1..<windows.count {
                windows[index - 1].addTabbedWindow(windows[index], ordered: .above)
            }
            controllers = windows.map { window in
                let tree: SplitTree<Ghostty.SurfaceView>
                if withSurfaces, let app = ghostty.app {
                    var config = Ghostty.SurfaceConfiguration()
                    config.command = "/usr/bin/true"
                    config.waitAfterCommand = true
                    tree = .init(view: Ghostty.SurfaceView(app, baseConfig: config))
                } else {
                    tree = .init()
                }
                let controller = TerminalController(ghostty, withSurfaceTree: tree)
                controller.window = window
                return controller
            }
            model = TerminalTabModel(window: windows[0], ghostty: ghostty)
            model.refresh()
        }

        func group(_ indexes: [Int]) -> UUID {
            let id = UUID()
            for index in indexes { windows[index].sidebarGroup = id }
            TabGroupStore.shared.restore(.init(
                id: id, info: .init(name: "Test", color: .blue, isCollapsed: false)))
            groups.append(id)
            model.refresh()
            return id
        }

        func waitForProcessExit() async throws {
            for _ in 0..<100 {
                if controllers.allSatisfy({ $0.surfaceTree.allSatisfy(\.processExited) }) { return }
                try await Task.sleep(for: .milliseconds(10))
            }
            try #require(controllers.allSatisfy({ $0.surfaceTree.allSatisfy(\.processExited) }))
        }

        func close() {
            for window in windows {
                window.sidebarGroup = nil
                window.close()
            }
            for group in groups { TabGroupStore.shared.remove(group) }
            if TabSidebarState.shared.isVisible != wasVisible { TabSidebarState.shared.toggle() }
        }
    }
}
