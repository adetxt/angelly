import AppKit

// Cocoa scripting is convention based: the selectors below are the ones Cocoa derives
// from Angelly.sdef. The object model is read from the Rust application's snapshot, so
// these classes only carry identifiers.

// MARK: - Helpers

@MainActor
private func fail(_ command: NSScriptCommand, _ number: Int, _ message: String) {
    command.scriptErrorNumber = number
    command.scriptErrorString = message
}

@MainActor
private func dispatch(_ command: NSScriptCommand, _ body: [String: Any]) -> Response? {
    let response = Rust.request(body)
    guard response.ok else {
        fail(command, errAEEventFailed, response.error)
        return nil
    }
    return response
}

@MainActor
private func configuration(from record: NSDictionary?) throws -> [String: Any] {
    guard let record else { return [:] }
    guard let raw = record as? [String: Any] else {
        throw RecordError.invalidType("configuration", "a surface configuration record")
    }
    var config: [String: Any] = [:]

    if let value = raw["fontSize"] {
        guard let number = value as? NSNumber else { throw RecordError.invalidType("font size", "a number") }
        let size = number.doubleValue
        guard size.isFinite else { throw RecordError.invalidValue("font size", "must be a finite number") }
        if size < 0 { throw RecordError.invalidValue("font size", "must be a positive number") }
        if size > 0 { config["fontSize"] = size }
    }
    for (key, name) in [("workingDirectory", "initial working directory"), ("command", "command"), ("initialInput", "initial input")] {
        guard let value = raw[key] else { continue }
        guard let text = value as? String else { throw RecordError.invalidType(name, "text") }
        if !text.isEmpty { config[key] = text }
    }
    if let value = raw["waitAfterCommand"] {
        guard let number = value as? NSNumber else { throw RecordError.invalidType("wait after command", "boolean") }
        config["waitAfterCommand"] = number.boolValue
    }
    if let assignments = raw["environmentVariables"] as? [String], !assignments.isEmpty {
        config["environment"] = try assignments.map { assignment -> [String] in
            guard let separator = assignment.firstIndex(of: "="), separator != assignment.startIndex else {
                throw RecordError.invalidValue("environment variable \"\(assignment)\"", "must be in KEY=VALUE format")
            }
            return [String(assignment[..<separator]), String(assignment[assignment.index(after: separator)...])]
        }
    }
    return config
}

private enum RecordError: LocalizedError {
    case invalidType(String, String)
    case invalidValue(String, String)

    var errorDescription: String? {
        switch self {
        case let .invalidType(parameter, expected): "\(parameter) must be \(expected)."
        case let .invalidValue(parameter, message): "\(parameter) \(message)."
        }
    }
}

/// The scripting description of the application class. `NSApp` is a subclass the sdef
/// does not mention, so the description is looked up on `NSApplication` itself.
@MainActor
private var applicationDescription: NSScriptClassDescription? {
    NSScriptClassDescription(for: NSApplication.self)
}

@MainActor
private func record(_ command: NSScriptCommand) -> [String: Any]? {
    do {
        return try configuration(from: command.evaluatedArguments?["configuration"] as? NSDictionary)
    } catch {
        fail(command, errAECoercionFail, error.localizedDescription)
        return nil
    }
}

private func press(from code: UInt32?) -> String {
    code == "GIrl".fourCharCode ? "release" : "press"
}

private func snapshotList(_ key: String) -> [[String: Any]] {
    Rust.snapshot()[key] as? [[String: Any]] ?? []
}

// MARK: - Application

@MainActor
extension NSApplication {
    var isAppleScriptEnabled: Bool { Rust.appleScriptEnabled }

    @discardableResult
    func validateScript(command: NSScriptCommand) -> Bool {
        guard isAppleScriptEnabled else {
            fail(command, errAEEventNotPermitted, "AppleScript is disabled by the macos-applescript configuration.")
            return false
        }
        return true
    }

    @objc(scriptWindows)
    var scriptWindows: [AngellyScriptWindow] {
        guard isAppleScriptEnabled else { return [] }
        return snapshotList("windows").compactMap { ($0["id"] as? String).map(AngellyScriptWindow.init) }
    }

    @objc(frontWindow)
    var frontWindow: AngellyScriptWindow? {
        guard isAppleScriptEnabled else { return nil }
        return scriptWindows.first
    }

    @objc(valueInScriptWindowsWithUniqueID:)
    func valueInScriptWindows(uniqueID: String) -> AngellyScriptWindow? {
        guard isAppleScriptEnabled else { return nil }
        return scriptWindows.first { $0.stableID == uniqueID }
    }

    @objc(terminals)
    var terminals: [AngellyScriptTerminal] {
        guard isAppleScriptEnabled else { return [] }
        return snapshotList("terminals").compactMap { ($0["id"] as? String).map(AngellyScriptTerminal.init) }
    }

    @objc(valueInTerminalsWithUniqueID:)
    func valueInTerminals(uniqueID: String) -> AngellyScriptTerminal? {
        guard isAppleScriptEnabled else { return nil }
        return terminals.first { $0.stableID == uniqueID }
    }

    @objc(handlePerformActionScriptCommand:)
    func handlePerformActionScriptCommand(_ command: NSScriptCommand) -> NSNumber? {
        guard validateScript(command: command) else { return nil }
        guard let action = command.directParameter as? String else {
            fail(command, errAEParamMissed, "Missing action string.")
            return nil
        }
        guard let terminal = command.evaluatedArguments?["on"] as? AngellyScriptTerminal else {
            fail(command, errAEParamMissed, "Missing terminal target.")
            return nil
        }
        let response = Rust.request(["cmd": "perform_action", "terminal": terminal.stableID, "action": action])
        return NSNumber(value: response.ok && response.result)
    }

    @objc(handleNewSurfaceConfigurationScriptCommand:)
    func handleNewSurfaceConfigurationScriptCommand(_ command: NSScriptCommand) -> NSDictionary? {
        guard validateScript(command: command) else { return nil }
        guard let config = record(command) else { return nil }
        var result: [String: Any] = [
            "fontSize": 0, "workingDirectory": "", "command": "", "initialInput": "",
            "waitAfterCommand": false, "environmentVariables": [String](),
        ]
        if let size = config["fontSize"] as? Double { result["fontSize"] = NSNumber(value: size) }
        for key in ["workingDirectory", "command", "initialInput"] {
            if let text = config[key] as? String { result[key] = text }
        }
        if config["waitAfterCommand"] as? Bool == true { result["waitAfterCommand"] = true }
        if let pairs = config["environment"] as? [[String]] {
            result["environmentVariables"] = pairs.map { "\($0[0])=\($0[1])" }
        }
        return result as NSDictionary
    }

    @objc(handleNewWindowScriptCommand:)
    func handleNewWindowScriptCommand(_ command: NSScriptCommand) -> AngellyScriptWindow? {
        guard validateScript(command: command) else { return nil }
        guard let config = record(command) else { return nil }
        guard let response = dispatch(command, ["cmd": "new_window", "config": config]),
              let id = response.string("window")
        else { return nil }
        return AngellyScriptWindow(stableID: id)
    }

    @objc(handleNewTabScriptCommand:)
    func handleNewTabScriptCommand(_ command: NSScriptCommand) -> AngellyScriptTab? {
        guard validateScript(command: command) else { return nil }
        guard let config = record(command) else { return nil }
        var body: [String: Any] = ["cmd": "new_tab", "config": config]
        if let window = command.evaluatedArguments?["window"] as? AngellyScriptWindow {
            guard window.exists else {
                fail(command, errAEEventFailed, "Target window is no longer available.")
                return nil
            }
            body["window"] = window.stableID
        }
        guard let response = dispatch(command, body), let tab = response.string("tab") else {
            return nil
        }
        return AngellyScriptTab(stableID: tab)
    }

    @objc(handleQuitScriptCommand:)
    func handleQuitScriptCommand(_ command: NSScriptCommand) {
        guard validateScript(command: command) else { return }
        terminate(nil)
    }
}

// MARK: - Window

@MainActor
@objc(AngellyScriptWindow)
final class AngellyScriptWindow: NSObject {
    let stableID: String

    init(stableID: String) {
        self.stableID = stableID
    }

    private var info: [String: Any]? {
        snapshotList("windows").first { $0["id"] as? String == stableID }
    }

    var exists: Bool { info != nil }

    @objc(id) var idValue: String { NSApp.isAppleScriptEnabled ? stableID : "" }

    @objc(title) var title: String { NSApp.isAppleScriptEnabled ? info?["title"] as? String ?? "" : "" }

    @objc(tabs) var tabs: [AngellyScriptTab] {
        guard NSApp.isAppleScriptEnabled else { return [] }
        let tabs = info?["tabs"] as? [[String: Any]] ?? []
        return tabs.compactMap { ($0["id"] as? String).map(AngellyScriptTab.init) }
    }

    @objc(selectedTab) var selectedTab: AngellyScriptTab? {
        guard NSApp.isAppleScriptEnabled, let id = info?["selected_tab"] as? String else { return nil }
        return AngellyScriptTab(stableID: id)
    }

    @objc(valueInTabsWithUniqueID:)
    func valueInTabs(uniqueID: String) -> AngellyScriptTab? {
        tabs.first { $0.stableID == uniqueID }
    }

    @objc(terminals) var terminals: [AngellyScriptTerminal] {
        guard NSApp.isAppleScriptEnabled else { return [] }
        return snapshotList("terminals")
            .filter { $0["window"] as? String == stableID }
            .compactMap { ($0["id"] as? String).map(AngellyScriptTerminal.init) }
    }

    @objc(valueInTerminalsWithUniqueID:)
    func valueInTerminals(uniqueID: String) -> AngellyScriptTerminal? {
        terminals.first { $0.stableID == uniqueID }
    }

    @objc(handleActivateWindowCommand:)
    func handleActivateWindow(_ command: NSScriptCommand) -> Any? {
        guard NSApp.validateScript(command: command) else { return nil }
        _ = dispatch(command, ["cmd": "activate_window", "window": stableID])
        return nil
    }

    @objc(handleCloseWindowCommand:)
    func handleCloseWindow(_ command: NSScriptCommand) -> Any? {
        guard NSApp.validateScript(command: command) else { return nil }
        _ = dispatch(command, ["cmd": "close_window", "window": stableID])
        return nil
    }

    override var objectSpecifier: NSScriptObjectSpecifier? {
        guard NSApp.isAppleScriptEnabled, let appDescription = applicationDescription else { return nil }
        return NSUniqueIDSpecifier(
            containerClassDescription: appDescription, containerSpecifier: nil,
            key: "scriptWindows", uniqueID: stableID)
    }
}

// MARK: - Tab

@MainActor
@objc(AngellyScriptTab)
final class AngellyScriptTab: NSObject {
    let stableID: String

    init(stableID: String) {
        self.stableID = stableID
    }

    private var windowInfo: [String: Any]? {
        snapshotList("windows").first { window in
            (window["tabs"] as? [[String: Any]] ?? []).contains { $0["id"] as? String == stableID }
        }
    }

    private var info: [String: Any]? {
        (windowInfo?["tabs"] as? [[String: Any]])?.first { $0["id"] as? String == stableID }
    }

    @objc(id) var idValue: String { NSApp.isAppleScriptEnabled ? stableID : "" }

    @objc(title) var title: String { NSApp.isAppleScriptEnabled ? info?["title"] as? String ?? "" : "" }

    @objc(titleOverride) var titleOverride: String {
        get { NSApp.isAppleScriptEnabled ? info?["title_override"] as? String ?? "" : "" }
        set {
            guard NSApp.isAppleScriptEnabled else { return }
            _ = Rust.request(["cmd": "set_title_override", "tab": stableID, "title": newValue])
        }
    }

    @objc(index) var index: Int { info?["index"] as? Int ?? 0 }

    @objc(selected) var selected: Bool { info?["selected"] as? Bool ?? false }

    @objc(focusedTerminal) var focusedTerminal: AngellyScriptTerminal? {
        guard NSApp.isAppleScriptEnabled, let id = info?["focused_terminal"] as? String else { return nil }
        return AngellyScriptTerminal(stableID: id)
    }

    @objc(terminals) var terminals: [AngellyScriptTerminal] {
        guard NSApp.isAppleScriptEnabled else { return [] }
        return (info?["terminals"] as? [String] ?? []).map(AngellyScriptTerminal.init)
    }

    @objc(valueInTerminalsWithUniqueID:)
    func valueInTerminals(uniqueID: String) -> AngellyScriptTerminal? {
        terminals.first { $0.stableID == uniqueID }
    }

    @objc(handleSelectTabCommand:)
    func handleSelectTab(_ command: NSScriptCommand) -> Any? {
        guard NSApp.validateScript(command: command) else { return nil }
        _ = dispatch(command, ["cmd": "select_tab", "tab": stableID])
        return nil
    }

    @objc(handleCloseTabCommand:)
    func handleCloseTab(_ command: NSScriptCommand) -> Any? {
        guard NSApp.validateScript(command: command) else { return nil }
        _ = dispatch(command, ["cmd": "close_tab", "tab": stableID])
        return nil
    }

    override var objectSpecifier: NSScriptObjectSpecifier? {
        guard NSApp.isAppleScriptEnabled, let windowID = windowInfo?["id"] as? String,
              let windowDescription = NSScriptClassDescription(for: AngellyScriptWindow.self)
        else { return nil }
        return NSUniqueIDSpecifier(
            containerClassDescription: windowDescription,
            containerSpecifier: AngellyScriptWindow(stableID: windowID).objectSpecifier,
            key: "tabs", uniqueID: stableID)
    }
}

// MARK: - Terminal

@MainActor
@objc(AngellyScriptTerminal)
final class AngellyScriptTerminal: NSObject {
    let stableID: String

    init(stableID: String) {
        self.stableID = stableID
    }

    private var info: [String: Any]? {
        snapshotList("terminals").first { $0["id"] as? String == stableID }
    }

    @objc(id) var idValue: String { NSApp.isAppleScriptEnabled ? stableID : "" }

    @objc(title) var title: String { NSApp.isAppleScriptEnabled ? info?["title"] as? String ?? "" : "" }

    @objc(titleOverride) var titleOverride: String {
        get { NSApp.isAppleScriptEnabled ? info?["title_override"] as? String ?? "" : "" }
        set {
            guard NSApp.isAppleScriptEnabled else { return }
            _ = Rust.request(["cmd": "set_title_override", "terminal": stableID, "title": newValue])
        }
    }

    @objc(workingDirectory) var workingDirectory: String {
        NSApp.isAppleScriptEnabled ? info?["working_directory"] as? String ?? "" : ""
    }

    @objc(pid) var pid: Int { NSApp.isAppleScriptEnabled ? info?["pid"] as? Int ?? 0 : 0 }

    @objc(tty) var tty: String { NSApp.isAppleScriptEnabled ? info?["tty"] as? String ?? "" : "" }

    @objc(handleSplitCommand:)
    func handleSplit(_ command: NSScriptCommand) -> Any? {
        guard NSApp.validateScript(command: command) else { return nil }
        let directions: [UInt32: String] = [
            "GSrt".fourCharCode: "right", "GSlf".fourCharCode: "left",
            "GSdn".fourCharCode: "down", "GSup".fourCharCode: "up",
        ]
        guard let code = command.evaluatedArguments?["direction"] as? UInt32, let direction = directions[code] else {
            fail(command, errAEParamMissed, "Missing or unknown split direction.")
            return nil
        }
        guard let config = record(command) else { return nil }
        guard let response = dispatch(command, [
            "cmd": "split", "terminal": stableID, "direction": direction, "config": config,
        ]), let id = response.string("terminal") else { return nil }
        return AngellyScriptTerminal(stableID: id)
    }

    @objc(handleFocusCommand:)
    func handleFocus(_ command: NSScriptCommand) -> Any? {
        guard NSApp.validateScript(command: command) else { return nil }
        _ = dispatch(command, ["cmd": "focus", "terminal": stableID])
        return nil
    }

    @objc(handleCloseCommand:)
    func handleClose(_ command: NSScriptCommand) -> Any? {
        guard NSApp.validateScript(command: command) else { return nil }
        _ = dispatch(command, ["cmd": "close", "terminal": stableID])
        return nil
    }

    override var objectSpecifier: NSScriptObjectSpecifier? {
        guard NSApp.isAppleScriptEnabled, let appDescription = applicationDescription else { return nil }
        return NSUniqueIDSpecifier(
            containerClassDescription: appDescription, containerSpecifier: nil,
            key: "terminals", uniqueID: stableID)
    }
}

// MARK: - Input commands

@MainActor
private func target(of command: NSScriptCommand) -> AngellyScriptTerminal? {
    guard NSApp.validateScript(command: command) else { return nil }
    guard let terminal = command.evaluatedArguments?["terminal"] as? AngellyScriptTerminal else {
        fail(command, errAEParamMissed, "Missing terminal target.")
        return nil
    }
    return terminal
}

@MainActor
@objc(AngellyScriptInputTextCommand)
final class AngellyScriptInputTextCommand: NSScriptCommand {
    override func performDefaultImplementation() -> Any? {
        guard let terminal = target(of: self) else { return nil }
        guard let text = directParameter as? String else {
            fail(self, errAEParamMissed, "Missing text to input.")
            return nil
        }
        _ = dispatch(self, ["cmd": "input_text", "terminal": terminal.stableID, "text": text])
        return nil
    }
}

@MainActor
@objc(AngellyScriptKeyEventCommand)
final class AngellyScriptKeyEventCommand: NSScriptCommand {
    override func performDefaultImplementation() -> Any? {
        guard let terminal = target(of: self) else { return nil }
        guard let key = directParameter as? String else {
            fail(self, errAEParamMissed, "Missing key name.")
            return nil
        }
        _ = dispatch(self, [
            "cmd": "send_key", "terminal": terminal.stableID, "key": key,
            "action": press(from: evaluatedArguments?["action"] as? UInt32),
            "modifiers": evaluatedArguments?["modifiers"] as? String ?? "",
        ])
        return nil
    }
}

@MainActor
@objc(AngellyScriptMouseButtonCommand)
final class AngellyScriptMouseButtonCommand: NSScriptCommand {
    override func performDefaultImplementation() -> Any? {
        guard let terminal = target(of: self) else { return nil }
        let buttons: [UInt32: String] = [
            "GMlf".fourCharCode: "left", "GMrt".fourCharCode: "right", "GMmd".fourCharCode: "middle",
        ]
        guard let code = directParameter as? UInt32, let button = buttons[code] else {
            fail(self, errAEParamMissed, "Missing or unknown mouse button.")
            return nil
        }
        _ = dispatch(self, [
            "cmd": "send_mouse_button", "terminal": terminal.stableID, "button": button,
            "action": press(from: evaluatedArguments?["action"] as? UInt32),
            "modifiers": evaluatedArguments?["modifiers"] as? String ?? "",
        ])
        return nil
    }
}

@MainActor
@objc(AngellyScriptMousePosCommand)
final class AngellyScriptMousePosCommand: NSScriptCommand {
    override func performDefaultImplementation() -> Any? {
        guard let terminal = target(of: self) else { return nil }
        guard let x = evaluatedArguments?["x"] as? Double else {
            fail(self, errAEParamMissed, "Missing x position.")
            return nil
        }
        guard let y = evaluatedArguments?["y"] as? Double else {
            fail(self, errAEParamMissed, "Missing y position.")
            return nil
        }
        _ = dispatch(self, [
            "cmd": "send_mouse_position", "terminal": terminal.stableID, "x": x, "y": y,
            "modifiers": evaluatedArguments?["modifiers"] as? String ?? "",
        ])
        return nil
    }
}

@MainActor
@objc(AngellyScriptMouseScrollCommand)
final class AngellyScriptMouseScrollCommand: NSScriptCommand {
    override func performDefaultImplementation() -> Any? {
        guard let terminal = target(of: self) else { return nil }
        guard let x = evaluatedArguments?["x"] as? Double else {
            fail(self, errAEParamMissed, "Missing x scroll delta.")
            return nil
        }
        guard let y = evaluatedArguments?["y"] as? Double else {
            fail(self, errAEParamMissed, "Missing y scroll delta.")
            return nil
        }
        let momenta: [UInt32: String] = [
            "SMno".fourCharCode: "none", "SMbg".fourCharCode: "began", "SMch".fourCharCode: "changed",
            "SMen".fourCharCode: "ended", "SMcn".fourCharCode: "cancelled", "SMmb".fourCharCode: "may_begin",
            "SMst".fourCharCode: "stationary",
        ]
        let momentum = (evaluatedArguments?["momentum"] as? UInt32).flatMap { momenta[$0] } ?? "none"
        _ = dispatch(self, [
            "cmd": "send_mouse_scroll", "terminal": terminal.stableID, "x": x, "y": y,
            "precision": evaluatedArguments?["precision"] as? Bool ?? false, "momentum": momentum,
        ])
        return nil
    }
}
