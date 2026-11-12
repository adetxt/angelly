import AppIntents
import AppKit

// App Intents for Shortcuts. Every intent asks permission first, then forwards to the Rust
// application through the same JSON requests AppleScript uses.

// MARK: Errors and permission

enum IntentError: Error, CustomLocalizedStringResourceConvertible {
    case appUnavailable
    case terminalNotFound
    case permissionDenied
    case failed(String)

    var localizedStringResource: LocalizedStringResource {
        switch self {
        case .appUnavailable: "The Angelly app isn't properly initialized."
        case .terminalNotFound: "The terminal no longer exists."
        case .permissionDenied: "Angelly doesn't allow Shortcuts."
        case .failed(let message): "\(message)"
        }
    }
}

/// Asks whether Shortcuts may interact with Angelly, following `macos-shortcuts`. An "Allow"
/// answer is remembered, a refusal is asked again next time.
@MainActor
func requestIntentPermission() -> Bool {
    switch angelly_shortcuts_policy() {
    case 1: return true
    case 2: return false
    default: break
    }
    if angelly_shortcuts_remembered() == 1 { return true }

    let alert = NSAlert()
    alert.messageText = "Allow Shortcuts to interact with Angelly?"
    alert.alertStyle = .informational
    alert.addButton(withTitle: "Allow")
    alert.addButton(withTitle: "Don't Allow")
    guard alert.runModal() == .alertFirstButtonReturn else { return false }
    angelly_shortcuts_remember(true)
    return true
}

/// Runs a request on the application and turns a failure into an intent error.
@MainActor
private func send(_ body: [String: Any]) throws -> Response {
    guard requestIntentPermission() else { throw IntentError.permissionDenied }
    let response = Rust.request(body)
    guard response.ok else {
        let message = response.error
        throw message.contains("no longer exists") ? IntentError.terminalNotFound : IntentError.failed(message)
    }
    return response
}

private func modifierList(_ mods: [KeyEventMods]) -> String {
    mods.map(\.rawValue).joined(separator: ",")
}

// MARK: Entities

struct TerminalEntity: AppEntity {
    let id: String

    @Property(title: "Title")
    var title: String

    @Property(title: "Working Directory")
    var workingDirectory: String?

    @Property(title: "PID")
    var pid: Int?

    @Property(title: "TTY")
    var tty: String?

    @Property(title: "Kind")
    var kind: Kind

    static var typeDisplayRepresentation: TypeDisplayRepresentation {
        TypeDisplayRepresentation(name: "Terminal")
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(title)")
    }

    static var defaultQuery = TerminalQuery()

    init(_ info: [String: Any]) {
        id = info["id"] as? String ?? ""
        title = info["title"] as? String ?? ""
        workingDirectory = info["working_directory"] as? String
        pid = info["pid"] as? Int
        tty = info["tty"] as? String
        kind = (info["kind"] as? String) == "quick" ? .quick : .normal
    }

    enum Kind: String, AppEnum {
        case normal
        case quick

        static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Terminal Kind")

        static var caseDisplayRepresentations: [Self: DisplayRepresentation] = [
            .normal: .init(title: "Normal"),
            .quick: .init(title: "Quick"),
        ]
    }
}

struct TerminalQuery: EntityStringQuery, EnumerableEntityQuery {
    @MainActor
    private var all: [TerminalEntity] {
        (Rust.snapshot()["terminals"] as? [[String: Any]] ?? []).map(TerminalEntity.init)
    }

    @MainActor
    func entities(for identifiers: [String]) async throws -> [TerminalEntity] {
        all.filter { identifiers.contains($0.id) }
    }

    @MainActor
    func entities(matching string: String) async throws -> [TerminalEntity] {
        all.filter { $0.title.localizedCaseInsensitiveContains(string) }
    }

    @MainActor
    func allEntities() async throws -> [TerminalEntity] { all }

    @MainActor
    func suggestedEntities() async throws -> [TerminalEntity] { all }
}

struct CommandEntity: AppEntity {
    let id: ID

    @Property(title: "Title")
    var title: String

    @Property(title: "Description")
    var commandDescription: String

    @Property(title: "Action")
    var action: String

    /// A command identifier is a composite key of the terminal and the action.
    struct ID: Hashable {
        let terminalID: String
        let action: String
    }

    static var typeDisplayRepresentation: TypeDisplayRepresentation {
        TypeDisplayRepresentation(name: "Command Palette Command")
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(
            title: LocalizedStringResource(stringLiteral: title),
            subtitle: LocalizedStringResource(stringLiteral: commandDescription))
    }

    static var defaultQuery = CommandQuery()

    init(_ info: [String: Any], terminalID: String) {
        let action = info["action"] as? String ?? ""
        id = ID(terminalID: terminalID, action: action)
        title = info["title"] as? String ?? ""
        commandDescription = info["description"] as? String ?? ""
        self.action = action
    }
}

extension CommandEntity.ID: RawRepresentable {
    var rawValue: String { "\(terminalID):\(action)" }

    init?(rawValue: String) {
        let parts = rawValue.split(separator: ":", maxSplits: 1)
        guard parts.count == 2 else { return nil }
        self.init(terminalID: String(parts[0]), action: String(parts[1]))
    }
}

extension CommandEntity.ID: EntityIdentifierConvertible {
    static func entityIdentifier(for entityIdentifierString: String) -> CommandEntity.ID? {
        .init(rawValue: entityIdentifierString)
    }

    var entityIdentifierString: String { rawValue }
}

struct CommandQuery: EntityQuery {
    @IntentParameterDependency<CommandPaletteIntent>(\.$terminal)
    var commandPaletteIntent

    @MainActor
    private func commands(for terminalID: String) -> [CommandEntity] {
        let response = Rust.request(["cmd": "commands"])
        return (response.fields["commands"] as? [[String: Any]] ?? []).map {
            CommandEntity($0, terminalID: terminalID)
        }
    }

    @MainActor
    func entities(for identifiers: [CommandEntity.ID]) async throws -> [CommandEntity] {
        identifiers.compactMap { id in
            commands(for: id.terminalID).first { $0.action == id.action }
        }
    }

    @MainActor
    func suggestedEntities() async throws -> [CommandEntity] {
        guard let terminal = commandPaletteIntent?.terminal else { return [] }
        return commands(for: terminal.id)
    }
}

// MARK: Terminals

enum NewTerminalLocation: String, AppEnum {
    case tab
    case window
    case splitLeft = "split:left"
    case splitRight = "split:right"
    case splitUp = "split:up"
    case splitDown = "split:down"

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Terminal Location")

    static var caseDisplayRepresentations: [Self: DisplayRepresentation] = [
        .tab: .init(title: "Tab"),
        .window: .init(title: "Window"),
        .splitLeft: .init(title: "Split Left"),
        .splitRight: .init(title: "Split Right"),
        .splitUp: .init(title: "Split Up"),
        .splitDown: .init(title: "Split Down"),
    ]

    var splitDirection: String? {
        rawValue.hasPrefix("split:") ? String(rawValue.dropFirst("split:".count)) : nil
    }
}

@available(macOS 15.0, *)
struct NewTerminalIntent: AppIntent {
    static var title: LocalizedStringResource = "New Terminal"
    static var description = IntentDescription("Create a new terminal.")

    @Parameter(
        title: "Location",
        description: "The location that the terminal should be created.",
        default: .window)
    var location: NewTerminalLocation

    @Parameter(
        title: "Command",
        description: "Command to execute within your configured shell.")
    var command: String?

    @Parameter(
        title: "Working Directory",
        description: "The working directory to open in the terminal.",
        supportedContentTypes: [.folder])
    var workingDirectory: IntentFile?

    @Parameter(
        title: "Environment Variables",
        description: "Environment variables in `KEY=VALUE` format.",
        default: [])
    var env: [String]

    @Parameter(
        title: "Parent Terminal",
        description: "The terminal to inherit the base configuration from.")
    var parent: TerminalEntity?

    static var openAppWhenRun = false

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<TerminalEntity?> {
        var config: [String: Any] = [:]
        // The command runs through the login shell so PATH and friends are set up.
        if let command, !command.isEmpty { config["initialInput"] = "\(command); exit\n" }
        if let url = workingDirectory?.fileURL {
            let directory = url.hasDirectoryPath ? url : url.deletingLastPathComponent()
            config["workingDirectory"] = directory.path
        }
        config["environment"] = env.compactMap { assignment -> [String]? in
            guard let separator = assignment.firstIndex(of: "=") else { return nil }
            return [String(assignment[..<separator]), String(assignment[assignment.index(after: separator)...])]
        }

        var request: [String: Any]
        switch location {
        case .window:
            request = ["cmd": "new_window"]
        case .tab:
            request = ["cmd": "new_tab"]
            if let parent, let window = Self.window(of: parent.id) { request["window"] = window }
        case .splitLeft, .splitRight, .splitUp, .splitDown:
            guard let target = parent?.id ?? Self.focusedTerminal() else { throw IntentError.terminalNotFound }
            request = ["cmd": "split", "terminal": target, "direction": location.splitDirection ?? "right"]
        }
        request["config"] = config

        let response = try send(request)
        if !NSApp.isActive { NSApp.activate(ignoringOtherApps: true) }
        guard let id = response.string("terminal") else { return .result(value: nil) }
        let created = (Rust.snapshot()["terminals"] as? [[String: Any]] ?? []).first { $0["id"] as? String == id }
        return .result(value: created.map(TerminalEntity.init))
    }

    @MainActor
    private static func window(of terminal: String) -> String? {
        (Rust.snapshot()["terminals"] as? [[String: Any]])?.first { $0["id"] as? String == terminal }?["window"] as? String
    }

    @MainActor
    private static func focusedTerminal() -> String? {
        let windows = Rust.snapshot()["windows"] as? [[String: Any]] ?? []
        let tabs = windows.first?["tabs"] as? [[String: Any]] ?? []
        return tabs.first { $0["selected"] as? Bool == true }?["focused_terminal"] as? String
    }
}

struct CloseTerminalIntent: AppIntent {
    static var title: LocalizedStringResource = "Close Terminal"
    static var description = IntentDescription("Close an existing terminal.")

    @Parameter(title: "Terminal", description: "The terminal to close.")
    var terminal: TerminalEntity

    @MainActor
    func perform() async throws -> some IntentResult {
        _ = try send(["cmd": "close", "terminal": terminal.id])
        return .result()
    }
}

struct FocusTerminalIntent: AppIntent {
    static var title: LocalizedStringResource = "Focus Terminal"
    static var description = IntentDescription("Move focus to an existing terminal.")

    @Parameter(title: "Terminal", description: "The terminal to focus.")
    var terminal: TerminalEntity

    @MainActor
    func perform() async throws -> some IntentResult {
        _ = try send(["cmd": "focus", "terminal": terminal.id])
        return .result()
    }
}

struct QuickTerminalIntent: AppIntent {
    static var title: LocalizedStringResource = "Open the Quick Terminal"
    static var description = IntentDescription("Open the Quick Terminal. If it is already open, then do nothing.")

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<[TerminalEntity]> {
        let response = try send(["cmd": "quick_terminal"])
        guard let id = response.string("terminal") else { return .result(value: []) }
        let terminals = (Rust.snapshot()["terminals"] as? [[String: Any]] ?? []).filter { $0["id"] as? String == id }
        return .result(value: terminals.map(TerminalEntity.init))
    }
}

// MARK: Actions

struct CommandPaletteIntent: AppIntent {
    static var title: LocalizedStringResource = "Invoke Command Palette Action"

    @Parameter(title: "Terminal", description: "The terminal to base available commands from.")
    var terminal: TerminalEntity

    @Parameter(
        title: "Command",
        description: "The command to invoke.",
        optionsProvider: CommandQuery())
    var command: CommandEntity

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<Bool> {
        let response = try send(["cmd": "perform_action", "terminal": terminal.id, "action": command.action])
        return .result(value: response.result)
    }
}

struct KeybindIntent: AppIntent {
    static var title: LocalizedStringResource = "Invoke a Keybind Action"

    @Parameter(title: "Terminal", description: "The terminal to invoke the action on.")
    var terminal: TerminalEntity

    @Parameter(
        title: "Action",
        description: "The keybind action to invoke. This can be any valid keybind action you could put in a configuration file.")
    var action: String

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<Bool> {
        let response = try send(["cmd": "perform_action", "terminal": terminal.id, "action": action])
        return .result(value: response.result)
    }
}

enum TerminalDetail: String, AppEnum {
    case title
    case workingDirectory
    case allContents
    case selectedText
    case visibleText

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Terminal Detail")

    static var caseDisplayRepresentations: [Self: DisplayRepresentation] = [
        .title: .init(title: "Title"),
        .workingDirectory: .init(title: "Working Directory"),
        .allContents: .init(title: "Full Contents"),
        .selectedText: .init(title: "Selected Text"),
        .visibleText: .init(title: "Visible Text"),
    ]
}

struct GetTerminalDetailsIntent: AppIntent {
    static var title: LocalizedStringResource = "Get Details of Terminal"

    @Parameter(title: "Detail", description: "The detail to extract about a terminal.")
    var detail: TerminalDetail

    @Parameter(title: "Terminal", description: "The terminal to extract information about.")
    var terminal: TerminalEntity

    static var parameterSummary: some ParameterSummary {
        Summary("Get \(\.$detail) from \(\.$terminal)")
    }

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<String?> {
        let response = try send(["cmd": "details", "terminal": terminal.id, "detail": detail.rawValue])
        return .result(value: response.string("text"))
    }
}

// MARK: Input

enum KeyEventMods: String, AppEnum, CaseIterable {
    case shift
    case control
    case option
    case command

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Modifier Key")

    static var caseDisplayRepresentations: [KeyEventMods: DisplayRepresentation] = [
        .shift: "Shift",
        .control: "Control",
        .option: "Option",
        .command: "Command",
    ]
}

enum IntentPress: String, AppEnum {
    case press
    case release

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Event Type")

    static var caseDisplayRepresentations: [IntentPress: DisplayRepresentation] = [
        .press: "Press",
        .release: "Release",
    ]
}

enum IntentMouseButton: String, AppEnum {
    case left
    case right
    case middle

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Mouse Button")

    static var caseDisplayRepresentations: [IntentMouseButton: DisplayRepresentation] = [
        .left: "Left",
        .right: "Right",
        .middle: "Middle",
    ]
}

enum IntentMomentum: String, AppEnum {
    case none
    case began
    case changed
    case ended
    case cancelled
    case mayBegin = "may_begin"
    case stationary

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Scroll Momentum")

    static var caseDisplayRepresentations: [IntentMomentum: DisplayRepresentation] = [
        .none: "None",
        .began: "Began",
        .changed: "Changed",
        .ended: "Ended",
        .cancelled: "Cancelled",
        .mayBegin: "May Begin",
        .stationary: "Stationary",
    ]
}

struct InputTextIntent: AppIntent {
    static var title: LocalizedStringResource = "Input Text to Terminal"

    @Parameter(
        title: "Text",
        description: "The text to input to the terminal. The text will be inputted as if it was pasted.",
        inputOptions: String.IntentInputOptions(
            capitalizationType: .none, multiline: true, autocorrect: false, smartQuotes: false, smartDashes: false))
    var text: String

    @Parameter(title: "Terminal", description: "The terminal to scope this action to.")
    var terminal: TerminalEntity

    @MainActor
    func perform() async throws -> some IntentResult {
        _ = try send(["cmd": "input_text", "terminal": terminal.id, "text": text])
        return .result()
    }
}

struct KeyEventIntent: AppIntent {
    static var title: LocalizedStringResource = "Send Keyboard Event to Terminal"
    static var description = IntentDescription(
        "Simulate a keyboard event. This will not handle text encoding; use the 'Input Text' action for that.")

    @Parameter(title: "Key", description: "The key to send to the terminal.", default: .enter)
    var key: IntentKey

    @Parameter(title: "Modifier(s)", description: "The modifiers to send with the key event.", default: [])
    var mods: [KeyEventMods]

    @Parameter(title: "Event Type", description: "A key press or release.", default: .press)
    var action: IntentPress

    @Parameter(title: "Terminal", description: "The terminal to scope this action to.")
    var terminal: TerminalEntity

    @MainActor
    func perform() async throws -> some IntentResult {
        _ = try send([
            "cmd": "send_key", "terminal": terminal.id, "key": key.rawValue,
            "action": action.rawValue, "modifiers": modifierList(mods),
        ])
        return .result()
    }
}

struct MouseButtonIntent: AppIntent {
    static var title: LocalizedStringResource = "Send Mouse Button Event to Terminal"

    @Parameter(title: "Button", description: "The mouse button to press or release.", default: .left)
    var button: IntentMouseButton

    @Parameter(title: "Action", description: "Whether to press or release the button.", default: .press)
    var action: IntentPress

    @Parameter(title: "Modifier(s)", description: "The modifiers to send with the mouse event.", default: [])
    var mods: [KeyEventMods]

    @Parameter(title: "Terminal", description: "The terminal to scope this action to.")
    var terminal: TerminalEntity

    @MainActor
    func perform() async throws -> some IntentResult {
        _ = try send([
            "cmd": "send_mouse_button", "terminal": terminal.id, "button": button.rawValue,
            "action": action.rawValue, "modifiers": modifierList(mods),
        ])
        return .result()
    }
}

struct MousePosIntent: AppIntent {
    static var title: LocalizedStringResource = "Send Mouse Position Event to Terminal"
    static var description = IntentDescription(
        "Send a mouse position event to the terminal. This reports the cursor position for mouse tracking.")

    @Parameter(title: "X Position", description: "The horizontal position of the mouse cursor in pixels.", default: 0)
    var x: Double

    @Parameter(title: "Y Position", description: "The vertical position of the mouse cursor in pixels.", default: 0)
    var y: Double

    @Parameter(title: "Modifier(s)", description: "The modifiers to send with the mouse position event.", default: [])
    var mods: [KeyEventMods]

    @Parameter(title: "Terminal", description: "The terminal to scope this action to.")
    var terminal: TerminalEntity

    @MainActor
    func perform() async throws -> some IntentResult {
        _ = try send([
            "cmd": "send_mouse_position", "terminal": terminal.id, "x": x, "y": y,
            "modifiers": modifierList(mods),
        ])
        return .result()
    }
}

struct MouseScrollIntent: AppIntent {
    static var title: LocalizedStringResource = "Send Mouse Scroll Event to Terminal"
    static var description = IntentDescription(
        "Send a mouse scroll event to the terminal with configurable precision and momentum.")

    @Parameter(title: "X Scroll Delta", description: "The horizontal scroll amount.", default: 0)
    var x: Double

    @Parameter(title: "Y Scroll Delta", description: "The vertical scroll amount.", default: 0)
    var y: Double

    @Parameter(
        title: "High Precision",
        description: "Whether this is a high-precision scroll event (e.g., from trackpad).",
        default: false)
    var precision: Bool

    @Parameter(
        title: "Momentum Phase",
        description: "The momentum phase for inertial scrolling.",
        default: IntentMomentum.none)
    var momentum: IntentMomentum

    @Parameter(title: "Terminal", description: "The terminal to scope this action to.")
    var terminal: TerminalEntity

    @MainActor
    func perform() async throws -> some IntentResult {
        _ = try send([
            "cmd": "send_mouse_scroll", "terminal": terminal.id, "x": x, "y": y,
            "precision": precision, "momentum": momentum.rawValue,
        ])
        return .result()
    }
}
