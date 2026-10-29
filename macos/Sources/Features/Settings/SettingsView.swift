import SwiftUI

private enum SettingsCategory: String, CaseIterable, Identifiable {
    case font = "Font"
    case appearance = "Appearance"
    case cursor = "Cursor"
    case window = "Window"
    case behavior = "Behavior"
    case keyboard = "Keyboard"
    case updates = "Updates"

    var id: Self { self }

    var icon: String {
        switch self {
        case .font: return "textformat"
        case .appearance: return "paintpalette"
        case .cursor: return "cursorarrow.rays"
        case .window: return "macwindow"
        case .behavior: return "slider.horizontal.3"
        case .keyboard: return "keyboard"
        case .updates: return "arrow.down.circle"
        }
    }
}

struct SettingsView: View {
    @ObservedObject var model: SettingsModel
    @State private var category: SettingsCategory? = .font

    var body: some View {
        HStack(spacing: 0) {
            List(SettingsCategory.allCases, selection: $category) { category in
                Label(category.rawValue, systemImage: category.icon)
            }
            .listStyle(.sidebar)
            .frame(width: 170)

            Divider()

            VStack(spacing: 0) {
                Form { content }
                    .formStyle(.grouped)

                Divider()
                footer.padding(12)
            }
        }
        .frame(width: 720, height: 420)
    }

    @ViewBuilder
    private var content: some View {
        switch category ?? .font {
        case .font:
            ConfigPicker("Family", key: "font-family", options: model.fontFamilies, model: model)
            ConfigTextField("Size", key: "font-size", model: model)

        case .appearance:
            ConfigPicker("Theme", key: "theme", options: model.themes, model: model)
            ConfigPicker("Window theme", key: "window-theme",
                         options: ["auto", "system", "light", "dark", "ghostty"],
                         label: enumLabel, model: model)
            ConfigTextField("Background opacity", key: "background-opacity", model: model)
            ConfigPicker("Background blur", key: "background-blur",
                         options: ["true", "false"], label: enumLabel, model: model)

        case .cursor:
            ConfigPicker("Style", key: "cursor-style",
                         options: ["block", "bar", "underline", "block_hollow"],
                         label: enumLabel, model: model)
            ConfigPicker("Blink", key: "cursor-style-blink",
                         options: ["true", "false"], label: enumLabel, model: model)

        case .window:
            ConfigTextField("Horizontal padding", key: "window-padding-x", model: model)
            ConfigTextField("Vertical padding", key: "window-padding-y", model: model)
            ConfigPicker("Titlebar style", key: "macos-titlebar-style",
                         options: ["native", "transparent", "tabs", "hidden"],
                         label: enumLabel, model: model)
            ConfigPicker("Restore windows", key: "window-save-state",
                         options: ["always", "never", "default"],
                         label: { $0 == "default" ? "Follow macOS" : enumLabel($0) },
                         model: model)

        case .behavior:
            ConfigPicker("Copy on select", key: "copy-on-select",
                         options: ["true", "false"], label: enumLabel, model: model)
            ConfigPicker("Confirm before closing", key: "confirm-close-surface",
                         options: ["true", "false", "always"],
                         label: { ["true": "If a process is running", "false": "Never", "always": "Always"][$0] ?? $0 },
                         model: model)
            ConfigPicker("New tab position", key: "window-new-tab-position",
                         options: ["current", "end"],
                         label: { $0 == "end" ? "At the end" : "Next to current tab" },
                         model: model)
            ConfigPicker("Quit after last window closes", key: "quit-after-last-window-closed",
                         options: ["true", "false"], label: enumLabel, model: model)
            ConfigPicker("Hide mouse while typing", key: "mouse-hide-while-typing",
                         options: ["true", "false"], label: enumLabel, model: model)
            ConfigPicker("Paste protection", key: "clipboard-paste-protection",
                         options: ["true", "false"], label: enumLabel, model: model)
            ConfigPicker("New windows keep working directory", key: "window-inherit-working-directory",
                         options: ["true", "false"], label: enumLabel, model: model)

        case .keyboard:
            ConfigPicker("Option as Alt", key: "macos-option-as-alt",
                         options: ["true", "false", "left", "right"],
                         label: { ["left": "Left Option only", "right": "Right Option only"][$0] ?? enumLabel($0) },
                         model: model)

        case .updates:
            ConfigPicker("Automatic updates", key: "auto-update",
                         options: ["off", "check", "download"],
                         label: { $0 == "download" ? "Check and download" : enumLabel($0) },
                         model: model)
            Button("Check for Updates Now") { model.checkForUpdates() }
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(model.path)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                Spacer()
                Button("Open Config File…") { model.openConfigFile() }
            }

            if let error = model.writeError {
                Text(error).foregroundStyle(.red)
            }

            Text("Other options can be set in the config file. Some changes apply only to new windows.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    /// Turns config enum values like `block_hollow` into labels like "Block hollow".
    private func enumLabel(_ value: String) -> String {
        switch value {
        case "true": return "On"
        case "false": return "Off"
        default:
            let words = value.replacingOccurrences(of: "_", with: " ")
            return words.prefix(1).uppercased() + words.dropFirst()
        }
    }
}

/// A picker for a config key. "Default" removes the key from the file. A
/// value in the file that isn't one of the options is kept as its own choice
/// so opening settings never changes it.
private struct ConfigPicker: View {
    let title: String
    let key: String
    let options: [String]
    let label: (String) -> String
    @ObservedObject var model: SettingsModel

    init(
        _ title: String,
        key: String,
        options: [String],
        label: @escaping (String) -> String = { $0 },
        model: SettingsModel
    ) {
        self.title = title
        self.key = key
        self.options = options
        self.label = label
        self.model = model
    }

    var body: some View {
        let current = model.values[key]
        let choices = current.map { options.contains($0) ? options : [$0] + options } ?? options

        Picker(title, selection: Binding(
            get: { current ?? "" },
            set: { model.set(key, $0.isEmpty ? nil : $0) }
        )) {
            Text("Default").tag("")
            Divider()
            ForEach(choices, id: \.self) { Text(label($0)).tag($0) }
        }
    }
}

/// A free-form text field for a config key. Changes are saved on Return or
/// when the field loses focus; an empty field removes the key from the file.
private struct ConfigTextField: View {
    let title: String
    let key: String
    @ObservedObject var model: SettingsModel

    @State private var draft = ""
    @FocusState private var focused: Bool

    init(_ title: String, key: String, model: SettingsModel) {
        self.title = title
        self.key = key
        self.model = model
    }

    var body: some View {
        TextField(title, text: $draft, prompt: Text("Default"))
            .multilineTextAlignment(.trailing)
            .focused($focused)
            .onSubmit(commit)
            .onChange(of: focused) { isFocused in
                if !isFocused { commit() }
            }
            .onReceive(model.$values) { values in
                if !focused { draft = values[key] ?? "" }
            }
    }

    private func commit() {
        let value = draft.trimmingCharacters(in: .whitespaces)
        model.set(key, value.isEmpty ? nil : value)
    }
}
