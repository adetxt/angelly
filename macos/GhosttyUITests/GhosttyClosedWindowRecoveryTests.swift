import XCTest

final class GhosttyClosedWindowRecoveryTests: GhosttyCustomConfigCase {
    override static var defaultTestSuite: XCTestSuite { XCTestSuite(forTestCaseClass: Self.self) }

    @MainActor
    func testRedButtonHistorySurvivesRestartWithUndoDisabled() throws {
        try updateConfig("window-save-state = always\nundo-timeout = 0\nquit-after-last-window-closed = false\n")
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let app = try ghosttyApplication(defaultsSuite: "GHOSTTY_RECOVERY_TESTS_\(UUID().uuidString)")
        app.launchEnvironment["GHOSTTY_RECOVERY_DIRECTORY"] = directory.path
        app.launch()
        defer { app.terminate() }
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 5))
        app.groups["Terminal pane"].typeKey("t", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: \.tabs.count, toEqual: 2, timeout: 3))
        app.groups["Terminal pane"].typeKey("n", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: \.windows.count, toEqual: 2, timeout: 3))

        // Close A (two tabs), then B (one tab), using their red traffic lights.
        let first = app.windows.allElementsBoundByIndex.first { $0.tabs.count == 2 }
        XCTAssertNotNil(first)
        first?.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: \.windows.count, toEqual: 1, timeout: 3))
        app.windows.firstMatch.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.wait(for: \.windows.count, toEqual: 0, timeout: 3))
        app.menuBars.menuBarItems["File"].click()
        XCTAssertTrue(app.menuItems["Reopen Closed Window"].isEnabled)
        app.typeKey(.escape, modifierFlags: [])
        app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 5))

        app.launch()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 5))
        app.menuBars.menuBarItems["File"].click()
        app.menuItems["Recently Closed Windows"].hover()
        let saved = app.menuItems.matching(NSPredicate(format: "title CONTAINS %@", "2 tabs")).firstMatch
        XCTAssertTrue(saved.waitForExistence(timeout: 3))
        saved.click()
        XCTAssertTrue(app.wait(for: \.tabs.count, toEqual: 2, timeout: 5))
        app.menuBars.menuBarItems["File"].click()
        app.menuItems["Recently Closed Windows"].hover()
        XCTAssertTrue(app.menuItems.matching(NSPredicate(format: "title CONTAINS %@", "1 tab")).firstMatch.exists)
        app.typeKey(.escape, modifierFlags: [])
    }

    @MainActor
    func testFailedSaveDefaultsToKeepingWindowOpen() throws {
        try updateConfig("window-save-state = always\nquit-after-last-window-closed = false\n")
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let app = try ghosttyApplication(defaultsSuite: "GHOSTTY_RECOVERY_TESTS_\(UUID().uuidString)")
        app.launchEnvironment["GHOSTTY_RECOVERY_DIRECTORY"] = directory.path
        app.launch()
        defer { app.terminate() }
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 5))
        // A directory in place of the file makes atomic history replacement fail.
        try FileManager.default.createDirectory(
            at: directory.appendingPathComponent("recently-closed-windows.json"), withIntermediateDirectories: false)
        app.windows.firstMatch.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.sheets.buttons["Cancel"].waitForExistence(timeout: 3))
        app.sheets.buttons["Cancel"].click()
        XCTAssertEqual(app.windows.count, 1)
        app.windows.firstMatch.buttons[XCUIIdentifierCloseWindow].click()
        XCTAssertTrue(app.sheets.buttons["Close Without Recovery"].waitForExistence(timeout: 3))
        app.sheets.buttons["Close Without Recovery"].click()
        XCTAssertTrue(app.wait(for: \.windows.count, toEqual: 0, timeout: 3))
    }
}
