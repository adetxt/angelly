import Foundation
import Testing
@testable import Ghostty

@Suite
struct ConfigFileTests {
    private let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("ConfigFileTests-\(UUID().uuidString)")

    private func file(_ contents: String? = nil) throws -> ConfigFile {
        let path = dir.appendingPathComponent("config.ghostty").path
        if let contents {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try contents.write(toFile: path, atomically: true, encoding: .utf8)
        }
        return ConfigFile(path: path)
    }

    private func contents(_ file: ConfigFile) throws -> String {
        try String(contentsOfFile: file.path, encoding: .utf8)
    }

    @Test func readsLastValueIgnoringCommentsAndQuotes() throws {
        let file = try file("""
        # font-size = 99
        font-size = 12
          font-size   =   14
        theme = "Builtin Dark"
        cursor-style =
        """)

        #expect(file.values() == ["font-size": "14", "theme": "Builtin Dark"])
    }

    @Test func fontFamilyUsesFirstLineAfterReset() throws {
        let file = try file("""
        font-family = Old
        font-family =
        font-family = Primary
        font-family = Fallback
        """)

        #expect(file.values()["font-family"] == "Primary")

        try file.set("font-family", to: "New")
        #expect(try contents(file) == """
        font-family = Old
        font-family =
        font-family = New
        font-family = Fallback
        """)
    }

    @Test func rewritesEffectiveLineInPlace() throws {
        let file = try file("""
        # my config
        font-size = 12
        theme = Dark

        font-size = 14

        """)

        try file.set("font-size", to: "16")
        #expect(try contents(file) == """
        # my config
        font-size = 12
        theme = Dark

        font-size = 16

        """)
    }

    @Test func appendsMissingKeyAndRemovesOnReset() throws {
        let file = try file("theme = Dark")

        try file.set("font-size", to: "13")
        #expect(try contents(file) == "theme = Dark\nfont-size = 13\n")

        try file.set("theme", to: nil)
        #expect(try contents(file) == "font-size = 13\n")
    }

    @Test func createsMissingFile() throws {
        let file = try file()

        try file.set("font-size", to: "13")
        #expect(try contents(file) == "font-size = 13\n")
    }

    @Test func writesThroughSymlink() throws {
        let target = try file("theme = Dark\n")
        let link = dir.appendingPathComponent("link.ghostty").path
        try FileManager.default.createSymbolicLink(atPath: link, withDestinationPath: target.path)

        try ConfigFile(path: link).set("theme", to: "Light")
        #expect(try FileManager.default.destinationOfSymbolicLink(atPath: link) == target.path)
        #expect(try contents(target) == "theme = Light\n")
    }
}
