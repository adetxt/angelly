import AppKit
import Testing
@testable import Ghostty

@MainActor
struct TerminalApplicationDiscoveryTests {
    @Test func discoversDefaultFolderHandlerAndDeduplicatesApplications() throws {
        let directory = FileManager.default.temporaryDirectory
        let expected = try #require(NSWorkspace.shared.urlForApplication(toOpen: directory))
        let applications = TerminalExternalApplication.available(for: directory, selectedPath: expected.path)
        #expect(applications.first?.url == expected.standardizedFileURL)
        #expect(applications.filter { $0.url == expected.standardizedFileURL }.count == 1)
        #expect(Set(applications.map(\.id)).count == applications.count)
    }

    @Test func remembersApplicationsOutsideTheSystemFolderHandlers() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let application = root.appendingPathComponent("Custom Editor.app")
        let contents = application.appendingPathComponent("Contents")
        try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
        let info: [String: String] = ["CFBundleIdentifier": "test.custom-editor", "CFBundleName": "Custom Editor",
                                      "CFBundleDisplayName": "My Editor", "CFBundlePackageType": "APPL"]
        try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
            .write(to: contents.appendingPathComponent("Info.plist"))
        let applications = TerminalExternalApplication.available(for: root, selectedPath: application.path)
        let custom = try #require(applications.first { $0.id == "test.custom-editor" })
        #expect(custom.name == "My Editor")
        #expect(custom.url.path == application.path)
    }
}
