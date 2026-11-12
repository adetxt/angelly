import AppIntents
import AppKit

/// Test hook behind `ANGELLY_SELFTEST_INTENTS`: runs the intents the way Shortcuts would
/// and prints one line per intent. The quick terminal intent is left out because it puts a
/// window on screen.
@_cdecl("angelly_shim_selftest_intents")
public func selftestIntents() {
    setvbuf(stdout, nil, _IOLBF, 0)
    Task { @MainActor in
        func report(_ name: String, _ body: () async throws -> String) async {
            do {
                print("intents selftest: \(name): ok \(try await body())")
            } catch {
                print("intents selftest: \(name): error \(error)")
            }
        }
        func terminals() -> [TerminalEntity] {
            (Rust.snapshot()["terminals"] as? [[String: Any]] ?? []).map(TerminalEntity.init)
        }

        guard let first = terminals().first else {
            print("intents selftest: no terminal")
            return
        }

        for detail in [TerminalDetail.title, .workingDirectory, .visibleText] {
            await report("details \(detail.rawValue)") {
                var intent = GetTerminalDetailsIntent()
                intent.detail = detail
                intent.terminal = first
                let result = try await intent.perform()
                return "\(String(describing: result.value ?? nil).prefix(40))"
            }
        }
        await report("input text") {
            var intent = InputTextIntent()
            intent.text = "echo intents-text-ok\n"
            intent.terminal = first
            _ = try await intent.perform()
            return ""
        }
        await report("key event") {
            var intent = KeyEventIntent()
            intent.key = .a
            intent.mods = [.control]
            intent.action = .press
            intent.terminal = first
            _ = try await intent.perform()
            return ""
        }
        await report("mouse position") {
            var intent = MousePosIntent()
            intent.x = 10
            intent.y = 10
            intent.mods = []
            intent.terminal = first
            _ = try await intent.perform()
            return ""
        }
        await report("mouse button") {
            var intent = MouseButtonIntent()
            intent.button = .left
            intent.action = .press
            intent.mods = []
            intent.terminal = first
            _ = try await intent.perform()
            return ""
        }
        await report("mouse scroll") {
            var intent = MouseScrollIntent()
            intent.x = 0
            intent.y = 3
            intent.precision = false
            intent.momentum = IntentMomentum.none
            intent.terminal = first
            _ = try await intent.perform()
            return ""
        }
        await report("keybind") {
            var intent = KeybindIntent()
            intent.terminal = first
            intent.action = "new_split:right"
            let result = try await intent.perform()
            return "performed=\(result.value ?? false) terminals=\(terminals().count)"
        }
        await report("command palette") {
            var intent = CommandPaletteIntent()
            intent.terminal = first
            let commands = Rust.request(["cmd": "commands"]).fields["commands"] as? [[String: Any]] ?? []
            guard let entry = commands.first(where: { ($0["action"] as? String)?.hasPrefix("new_split") == true }) else {
                return "no command"
            }
            intent.command = CommandEntity(entry, terminalID: first.id)
            let result = try await intent.perform()
            return "action=\(entry["action"] ?? "") performed=\(result.value ?? false) terminals=\(terminals().count)"
        }
        if #available(macOS 15.0, *) {
            var created: TerminalEntity?
            await report("new terminal (tab)") {
                var intent = NewTerminalIntent()
                intent.location = .tab
                intent.command = "echo intents-new-ok"
                intent.env = ["INTENT_TEST=1", "malformed"]
                intent.parent = first
                let result = try await intent.perform()
                return "id=\((result.value ?? nil)?.id ?? "nil") terminals=\(terminals().count)"
            }
            await report("new terminal (split)") {
                var intent = NewTerminalIntent()
                intent.location = .splitDown
                intent.env = []
                intent.parent = first
                let result = try await intent.perform()
                created = result.value ?? nil
                return "id=\(created?.id ?? "nil") terminals=\(terminals().count)"
            }
            await report("new terminal (window)") {
                var intent = NewTerminalIntent()
                intent.location = .window
                intent.env = []
                let result = try await intent.perform()
                return "id=\((result.value ?? nil)?.id ?? "nil") windows=\((Rust.snapshot()["windows"] as? [Any])?.count ?? 0)"
            }
            if let created {
                await report("focus") {
                    var intent = FocusTerminalIntent()
                    intent.terminal = created
                    _ = try await intent.perform()
                    return ""
                }
                await report("close") {
                    var intent = CloseTerminalIntent()
                    intent.terminal = created
                    _ = try await intent.perform()
                    return "terminals=\(terminals().count)"
                }
                await report("close again") {
                    var intent = CloseTerminalIntent()
                    intent.terminal = created
                    _ = try await intent.perform()
                    return ""
                }
            }
        }
    }
}
