import XCTest

final class GhosttyHorizontalTabDragTests: GhosttyCustomConfigCase {
    override static var defaultTestSuite: XCTestSuite {
        XCTestSuite(forTestCaseClass: Self.self)
    }

    override func setUp() async throws {
        try await super.setUp()
        try updateConfig(
            """
            macos-titlebar-style = transparent
            window-save-state = never
            """
        )
    }

    @MainActor
    func testDragsActiveTab() throws {
        let app = try ghosttyApplication(defaultsSuite: "GHOSTTY_TAB_DRAG_TESTS_\(UUID().uuidString)")
        app.launch()
        defer { app.terminate() }
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 5))
        if !app.scrollViews["Horizontal Tab Bar"].firstMatch.exists { app.buttons["sidebar.left"].click() }
        app.groups["Terminal pane"].typeKey("t", modifierFlags: .command)
        app.groups["Terminal pane"].typeKey("t", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: \.tabs.count, toEqual: 3, timeout: 2))
        let buttons = app.scrollViews["Horizontal Tab Bar"].buttons.matching(
            NSPredicate(format: "NOT (label BEGINSWITH %@)", "Close "))
        XCTAssertTrue(buttons.element(boundBy: 2).isSelected)
        drag(buttons.element(boundBy: 2), after: buttons.element(boundBy: 0))
        XCTAssertTrue(buttons.element(boundBy: 1).isSelected)
    }

    @MainActor
    func testDragsTabUsingItsTitle() throws {
        let app = try launchTabs()
        defer { app.terminate() }
        let bar = app.scrollViews["Horizontal Tab Bar"].firstMatch
        drag(bar.buttons["First"], after: bar.buttons["Third"])

        XCTAssertTrue(app.wait(for: \.tabs.count, toEqual: 3, timeout: 2))
        XCTAssertTrue(bar.buttons["First"].waitForExistence(timeout: 2))
        XCTAssertLessThan(bar.buttons["Second"].frame.minX, bar.buttons["Third"].frame.minX)
        XCTAssertLessThan(bar.buttons["Third"].frame.minX, bar.buttons["First"].frame.minX)
        bar.buttons["Second"].click()
        XCTAssertEqual(app.windows.firstMatch.title, "Second")
    }

    @MainActor
    func testDragsWholeGroupUsingItsLabel() throws {
        let app = try launchTabs()
        defer { app.terminate() }
        let bar = app.scrollViews["Horizontal Tab Bar"].firstMatch
        bar.buttons["First"].rightClick()
        app.menuItems["Add to Group"].click()
        app.menuItems["New Group"].click()
        let name = app.textFields.firstMatch
        XCTAssertTrue(name.waitForExistence(timeout: 2))
        name.typeKey("a", modifierFlags: .command)
        name.typeText("Work")
        name.typeKey(.return, modifierFlags: [])

        bar.buttons["Second"].rightClick()
        app.menuItems["Add to Group"].click()
        app.menuItems["Work"].click()

        let group = bar.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Work, 2 tabs")).firstMatch
        XCTAssertTrue(group.waitForExistence(timeout: 2))
        drag(group, after: bar.buttons["Third"])

        XCTAssertTrue(group.waitForExistence(timeout: 2))
        XCTAssertLessThan(bar.buttons["Third"].frame.minX, group.frame.minX)
        XCTAssertLessThan(group.frame.minX, bar.buttons["First"].frame.minX)
        XCTAssertLessThan(bar.buttons["First"].frame.minX, bar.buttons["Second"].frame.minX)
        group.click()
        XCTAssertTrue(bar.buttons["First"].waitForNonExistence(timeout: 2))
        group.click()
        XCTAssertTrue(bar.buttons["First"].waitForExistence(timeout: 2))
    }

    @MainActor
    func testDragsTabBesideCollapsedGroups() throws {
        let app = try launchTabs()
        defer { app.terminate() }
        let bar = app.scrollViews["Horizontal Tab Bar"].firstMatch
        let first = collapsedGroup(named: "A", around: "First", in: bar, app: app)
        let third = collapsedGroup(named: "B", around: "Third", in: bar, app: app)
        let second = bar.buttons["Second"]

        drag(second, to: first, atX: 0.1)
        XCTAssertTrue(second.waitForExistence(timeout: 2))
        XCTAssertLessThan(second.frame.minX, first.frame.minX)
        XCTAssertLessThan(first.frame.minX, third.frame.minX)

        drag(second, to: third, atX: 0.9)
        XCTAssertTrue(second.waitForExistence(timeout: 2))
        XCTAssertLessThan(first.frame.minX, third.frame.minX)
        XCTAssertLessThan(third.frame.minX, second.frame.minX)
    }

    @MainActor
    func testDragsTabBesideCollapsedGroupsInSidebar() throws {
        let app = try launchTabs(sidebar: true)
        defer { app.terminate() }
        let sidebar = app.groups["Vertical Tab Sidebar"].firstMatch
        let first = collapsedGroup(named: "A", around: "First", in: sidebar, app: app)
        let third = collapsedGroup(named: "B", around: "Third", in: sidebar, app: app)
        let second = tab("Second", in: sidebar)

        drag(second, to: first, at: CGVector(dx: 0.5, dy: 0.1))
        XCTAssertTrue(second.waitForExistence(timeout: 2))
        XCTAssertLessThan(second.frame.minY, first.frame.minY)
        XCTAssertLessThan(first.frame.minY, third.frame.minY)
        // The gap that opens as a drag enters a header slides it away from the pointer, so a blind
        // drag cannot reach a vertical header's far zone; that zone is covered by the model tests.
    }

    /// Titles are matched by prefix because the sidebar appends each tab's shortcut to its label.
    private func tab(_ title: String, in container: XCUIElement) -> XCUIElement {
        container.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch
    }

    @MainActor
    private func collapsedGroup(
        named name: String, around title: String, in container: XCUIElement, app: XCUIApplication
    ) -> XCUIElement {
        tab(title, in: container).rightClick()
        app.menuItems["Add to Group"].click()
        app.menuItems["New Group"].click()
        // The sidebar header combines its children, hiding the name field, which already has focus.
        let field = app.textFields.firstMatch
        let input: XCUIElement = field.waitForExistence(timeout: 1) ? field : app
        input.typeKey("a", modifierFlags: .command)
        input.typeText(name)
        input.typeKey(.return, modifierFlags: [])
        let header = tab("\(name), 1 tabs", in: container)
        XCTAssertTrue(header.waitForExistence(timeout: 2))
        header.click()
        XCTAssertTrue(tab(title, in: container).waitForNonExistence(timeout: 2))
        return header
    }

    @MainActor
    private func launchTabs(sidebar: Bool = false) throws -> XCUIApplication {
        let app = try ghosttyApplication(defaultsSuite: "GHOSTTY_TAB_DRAG_TESTS_\(UUID().uuidString)")
        app.launch()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 5))
        if app.scrollViews["Horizontal Tab Bar"].firstMatch.exists == sidebar {
            app.buttons["sidebar.left"].click()
        }
        let container = sidebar ? app.groups["Vertical Tab Sidebar"].firstMatch : app.scrollViews["Horizontal Tab Bar"].firstMatch
        let rows = container.buttons.matching(NSPredicate(
            format: "NOT (label BEGINSWITH %@) AND NOT (label BEGINSWITH %@)", "Close ", "New Tab"))
        for (index, title) in ["First", "Second", "Third"].enumerated() {
            if index > 0 { app.groups["Terminal pane"].typeKey("t", modifierFlags: .command) }
            rows.element(boundBy: index).rightClick()
            app.menuItems["Rename Tab..."].click()
            let field = app.sheets.textFields.firstMatch
            XCTAssertTrue(field.waitForExistence(timeout: 3))
            field.typeKey("a", modifierFlags: .command)
            field.typeText(title)
            app.sheets.buttons["OK"].click()
            XCTAssertTrue(tab(title, in: container).waitForExistence(timeout: 2))
        }
        return app
    }

    @MainActor
    private func drag(_ source: XCUIElement, after target: XCUIElement) {
        drag(source, to: target, atX: 1)
    }

    @MainActor
    private func drag(_ source: XCUIElement, to target: XCUIElement, atX x: CGFloat) {
        drag(source, to: target, at: CGVector(dx: x, dy: 0.5))
    }

    @MainActor
    private func drag(_ source: XCUIElement, to target: XCUIElement, at offset: CGVector) {
        source.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click(
            forDuration: 0.2, thenDragTo: target.coordinate(withNormalizedOffset: offset))
    }
}
