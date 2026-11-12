import AppKit

/// Thin typed layer over the C functions the Rust application exports.
enum Rust {
    static func snapshot() -> [String: Any] {
        guard let raw = angelly_automation_snapshot() else { return [:] }
        defer { angelly_string_free(raw) }
        return decode(String(cString: raw))
    }

    static func request(_ body: [String: Any]) -> Response {
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let json = String(data: data, encoding: .utf8),
              let raw = angelly_automation_request(json)
        else {
            return Response(fields: ["ok": false, "error": "Angelly is unavailable."])
        }
        defer { angelly_string_free(raw) }
        return Response(fields: decode(String(cString: raw)))
    }

    private static func decode(_ json: String) -> [String: Any] {
        guard let data = json.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return [:] }
        return object
    }

    static var appleScriptEnabled: Bool { angelly_applescript_enabled() }
}

struct Response {
    let fields: [String: Any]

    var ok: Bool { fields["ok"] as? Bool ?? false }
    var error: String { fields["error"] as? String ?? "The request failed." }

    func string(_ key: String) -> String? { fields[key] as? String }
    var result: Bool { fields["result"] as? Bool ?? false }
}

extension String {
    /// The four-character code Apple events use for enumerators, as an integer.
    var fourCharCode: UInt32 {
        utf8.reduce(0) { ($0 << 8) | UInt32($1) }
    }
}

@_cdecl("angelly_shim_install")
public func angellyShimInstall() {
    // Cocoa scripting resolves its classes by name when the first Apple event
    // arrives; touching them here keeps the linker from stripping them.
    _ = [
        AngellyScriptWindow.self, AngellyScriptTab.self, AngellyScriptTerminal.self,
        AngellyScriptInputTextCommand.self, AngellyScriptKeyEventCommand.self,
        AngellyScriptMouseButtonCommand.self, AngellyScriptMousePosCommand.self,
        AngellyScriptMouseScrollCommand.self,
    ] as [AnyClass]
}
