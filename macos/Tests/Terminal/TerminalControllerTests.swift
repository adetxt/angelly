import AppKit
import Testing
@testable import Ghostty

@Suite(.serialized)
@MainActor
struct TerminalControllerTests {
    /// Four tabs: `[a, b, c, d]` with `b` and `c` in a sidebar group.
    @MainActor
    private struct Fixture {
        let ghostty: Ghostty.App
        let windows: [TerminalWindow]
        let group: UUID
        var controllers: [TerminalController]

        init() throws {
            ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
            try #require(ghostty.config.windowNewTabPosition == "current")

            var controllers = [TerminalController(ghostty)]
            var windows = [try #require(controllers[0].window as? TerminalWindow)]
            for _ in 0..<3 {
                let next = try #require(TerminalController.newTab(ghostty, from: windows.last))
                controllers.append(next)
                windows.append(try #require(next.window as? TerminalWindow))
            }

            group = UUID()
            windows[1].sidebarGroup = group
            windows[2].sidebarGroup = group
            TabGroupStore.shared.restore(.init(
                id: group, info: .init(name: "Test", color: .blue, isCollapsed: false)))
            self.windows = windows
            self.controllers = controllers
        }

        func tearDown() {
            TabGroupStore.shared.remove(group)
            for controller in controllers.reversed() {
                controller.closeWindowImmediately()
            }
        }
    }

    @Test(arguments: [("current", 2), ("end", 3)])
    func placementFromGroupJoinsTheGroup(position: String, anchor: Int) throws {
        let fixture = try Fixture()
        defer { fixture.tearDown() }

        let placement = TerminalController.newTabPlacement(
            parent: fixture.windows[1], position: position)

        #expect(placement.anchor === fixture.windows[anchor - 1])
        #expect(placement.group == fixture.group)
    }

    @Test(arguments: [("current", 0, 0), ("end", 0, 3), ("current", 3, 3), ("end", 3, 3)])
    func placementFromUngroupedTabStaysOutside(position: String, parent: Int, anchor: Int) throws {
        let fixture = try Fixture()
        defer { fixture.tearDown() }

        let placement = TerminalController.newTabPlacement(
            parent: fixture.windows[parent], position: position)

        #expect(placement.anchor === fixture.windows[anchor])
        #expect(placement.group == nil)
    }

    @Test func newTabFromGroupOpensBesideCurrentAndJoins() throws {
        var fixture = try Fixture()
        defer { fixture.tearDown() }
        let (a, b, c, d) = (fixture.windows[0], fixture.windows[1], fixture.windows[2], fixture.windows[3])
        b.tabGroup?.selectedWindow = b

        let created = try #require(TerminalController.newTab(fixture.ghostty, from: b))
        fixture.controllers.append(created)
        let new = try #require(created.window as? TerminalWindow)

        #expect(b.tabGroup?.windows == [a, b, new, c, d])
        #expect(b.tabGroup?.selectedWindow === new)
        #expect(new.sidebarGroup == fixture.group)
        #expect(a.sidebarGroup == nil)
        #expect(d.sidebarGroup == nil)
    }

    @Test func newTabFromUngroupedTabOpensOutsideTheGroup() throws {
        var fixture = try Fixture()
        defer { fixture.tearDown() }
        let (a, b, c, d) = (fixture.windows[0], fixture.windows[1], fixture.windows[2], fixture.windows[3])
        a.tabGroup?.selectedWindow = a

        let created = try #require(TerminalController.newTab(fixture.ghostty, from: a))
        fixture.controllers.append(created)
        let new = try #require(created.window as? TerminalWindow)

        #expect(a.tabGroup?.windows == [a, new, b, c, d])
        #expect(new.sidebarGroup == nil)
        #expect(b.sidebarGroup == fixture.group)
        #expect(c.sidebarGroup == fixture.group)
    }
}
