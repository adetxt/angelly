import Foundation

/// Reads and edits `key = value` lines of a Ghostty config file in place.
///
/// Edits only ever touch the one line for the key being changed, so comments,
/// ordering and every other key in the file are left exactly as written.
struct ConfigFile {
    let path: String

    /// Repeatable keys where the first line is the primary value and later
    /// lines are fallbacks. For every other key the last line wins.
    private static let firstLineWins: Set<String> = ["font-family"]

    /// The value of every key the file sets. Keys set to an empty value
    /// (which resets them to the default) are omitted.
    func values() -> [String: String] {
        let lines = readLines()
        let keys = Set(lines.compactMap { Self.parse($0)?.key })
        var result: [String: String] = [:]
        for key in keys {
            guard let index = Self.effectiveLine(for: key, in: lines),
                  let value = Self.parse(lines[index])?.value,
                  !value.isEmpty else { continue }
            result[key] = value
        }
        return result
    }

    /// Set a key to a value, or pass nil to remove the line so the key falls
    /// back to its default. The effective line is rewritten in place if it
    /// exists, otherwise a new line is appended.
    func set(_ key: String, to value: String?) throws {
        var lines = readLines()
        let newLine = value.map { "\(key) = \($0)" }

        switch (Self.effectiveLine(for: key, in: lines), newLine) {
        case let (index?, newLine?):
            lines[index] = newLine

        case let (index?, nil):
            lines.remove(at: index)

        case let (nil, newLine?):
            // Keep the file ending in a newline.
            if lines.last == "" { lines.removeLast() }
            lines.append(newLine)
            lines.append("")

        case (nil, nil):
            return
        }

        // Write through symlinks so dotfile setups keep their link.
        let url = URL(fileURLWithPath: path).resolvingSymlinksInPath()
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(),
            withIntermediateDirectories: true)
        try lines.joined(separator: "\n").write(to: url, atomically: true, encoding: .utf8)
    }

    private func readLines() -> [String] {
        guard let contents = try? String(contentsOfFile: path, encoding: .utf8) else { return [] }
        return contents.components(separatedBy: "\n")
    }

    /// The index of the line that determines the key's value, if any.
    private static func effectiveLine(for key: String, in lines: [String]) -> Int? {
        var result: Int?
        for (index, line) in lines.enumerated() {
            guard let entry = parse(line), entry.key == key else { continue }
            if firstLineWins.contains(key) {
                // An empty value resets the list, so the next line is primary.
                if entry.value.isEmpty {
                    result = nil
                } else if result == nil {
                    result = index
                }
            } else {
                result = index
            }
        }
        return result
    }

    /// Parse a line the same way Ghostty's config loader does: whole-line
    /// `#` comments, whitespace around `=`, and optional surrounding quotes.
    private static func parse(_ line: String) -> (key: String, value: String)? {
        let line = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !line.isEmpty, !line.hasPrefix("#"), let eq = line.firstIndex(of: "=") else { return nil }

        let key = line[..<eq].trimmingCharacters(in: .whitespaces)
        var value = line[line.index(after: eq)...].trimmingCharacters(in: .whitespaces)
        if value.count >= 2, value.hasPrefix("\""), value.hasSuffix("\"") {
            value = String(value.dropFirst().dropLast())
        }
        return (key, value)
    }
}
