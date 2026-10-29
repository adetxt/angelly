import Cocoa
import SwiftUI
import Combine

/// The settings window. Opened from the app menu or the `open_config` action.
class SettingsController: NSWindowController, NSWindowDelegate {
    private let model: SettingsModel

    init(ghostty: Ghostty.App) {
        model = SettingsModel(ghostty: ghostty)

        let hosting = NSHostingController(rootView: SettingsView(model: model))
        hosting.sizingOptions = [.preferredContentSize]
        let window = NSWindow(contentViewController: hosting)
        window.title = "Settings"
        window.styleMask = [.titled, .closable]
        window.isReleasedWhenClosed = false
        window.center()

        super.init(window: window)
        window.delegate = self
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    // MARK: - NSWindowDelegate

    func windowDidBecomeKey(_ notification: Notification) {
        // Pick up hand edits made while the window was in the background.
        model.reload()
    }
}

/// Reflects the values in the user's config file and writes changes back to
/// it, one line at a time, reloading the config after every change.
class SettingsModel: ObservableObject {
    /// The values the config file currently sets, keyed by config key.
    /// Keys missing here use Ghostty's default.
    @Published private(set) var values: [String: String] = [:]

    /// The last error writing the config file, if any.
    @Published private(set) var writeError: String?

    let fontFamilies: [String]
    let themes: [String]

    private let ghostty: Ghostty.App
    private var configChange: AnyCancellable?

    var path: String { ghostty.configFilePath }
    private var file: ConfigFile { ConfigFile(path: path) }

    init(ghostty: Ghostty.App) {
        self.ghostty = ghostty
        self.fontFamilies = Self.monospaceFontFamilies()
        self.themes = Self.themeNames()

        // Any reload, whether from us, a keybind or the menu, may mean the
        // file changed underneath us.
        configChange = NotificationCenter.default
            .publisher(for: .ghosttyConfigDidChange)
            .filter { $0.object == nil }
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.reload() }

        reload()
    }

    func reload() {
        values = file.values()
    }

    /// Set a key, or pass nil to reset it to the default.
    func set(_ key: String, _ value: String?) {
        guard values[key] != value else { return }

        do {
            try file.set(key, to: value)
            writeError = nil
        } catch {
            writeError = "Couldn't save settings: \(error.localizedDescription)"
            return
        }

        reload()
        ghostty.reloadConfig()
    }

    func openConfigFile() {
        ghostty.openConfig()
    }

    func checkForUpdates() {
        (NSApp.delegate as? AppDelegate)?.checkForUpdates(nil)
    }

    private static func monospaceFontFamilies() -> [String] {
        let names = NSFontManager.shared.availableFontNames(with: .fixedPitchFontMask) ?? []
        let families = Set(names.compactMap { NSFont(name: $0, size: 12)?.familyName })
        return families.sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
    }

    /// Theme names from the same directories Ghostty searches: the user's
    /// themes directory and the themes bundled with the app.
    private static func themeNames() -> [String] {
        let configHome = ProcessInfo.processInfo.environment["XDG_CONFIG_HOME"]
            .map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".config")
        var dirs = [configHome.appendingPathComponent("ghostty/themes")]
        if let resources = Bundle.main.resourceURL {
            dirs.append(resources.appendingPathComponent("ghostty/themes"))
        }

        let names = dirs.flatMap { (try? FileManager.default.contentsOfDirectory(atPath: $0.path)) ?? [] }
        return Set(names.filter { !$0.hasPrefix(".") })
            .sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
    }
}
