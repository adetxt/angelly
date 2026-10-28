import AppKit
import Testing
@testable import Ghostty

@Suite(.serialized)
@MainActor
struct TerminalControllerTests {
    @Test(arguments: [(true, false), (true, true), (false, true)])
    func newTabAppendsOutsideSidebarGroups(activeGrouped: Bool, lastGrouped: Bool) throws {
        let ghostty = try #require((NSApp.delegate as? AppDelegate)?.ghostty)
        try #require(ghostty.config.windowNewTabPosition == "current")
        let first = TerminalController(ghostty)
        var controllers = [first]
        defer {
            for controller in controllers.reversed() {
                controller.closeWindowImmediately()
            }
        }

        let firstWindow = try #require(first.window as? TerminalWindow)
        let second = try #require(TerminalController.newTab(ghostty, from: firstWindow))
        controllers.append(second)
        let secondWindow = try #require(second.window as? TerminalWindow)
        let third = try #require(TerminalController.newTab(ghostty, from: secondWindow))
        controllers.append(third)
        let thirdWindow = try #require(third.window as? TerminalWindow)

        let group = UUID()
        firstWindow.sidebarGroup = activeGrouped ? group : nil
        secondWindow.sidebarGroup = group
        thirdWindow.sidebarGroup = lastGrouped ? group : nil
        TabGroupStore.shared.restore(.init(
            id: group, info: .init(name: "Test", color: .blue, isCollapsed: false)))
        defer { TabGroupStore.shared.remove(group) }
        firstWindow.tabGroup?.selectedWindow = firstWindow

        let created = try #require(TerminalController.newTab(ghostty, from: firstWindow))
        controllers.append(created)
        let createdWindow = try #require(created.window as? TerminalWindow)

        #expect(firstWindow.tabGroup?.windows == [firstWindow, secondWindow, thirdWindow, createdWindow])
        #expect(firstWindow.tabGroup?.selectedWindow === createdWindow)
        #expect(createdWindow.sidebarGroup == nil)
        #expect(firstWindow.sidebarGroup == (activeGrouped ? group : nil))
        #expect(secondWindow.sidebarGroup == group)
        #expect(thirdWindow.sidebarGroup == (lastGrouped ? group : nil))
    }
}
