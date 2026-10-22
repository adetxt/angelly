import SwiftUI

struct SettingsView: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        Form {
            Section("Font") {
                ConfigPicker("Family", key: "font-family", options: model.fontFamilies, model: model)
                ConfigTextField("Size", key: "font-size", model: model)
            }

            Section("Appearance") {
                ConfigPicker("Theme", key: "theme", options: model.themes, model: model)
                ConfigPicker("Window theme", key: "window-theme",
                             options: ["auto", "system", "light", "dark", "ghostty"],
                             label: enumLabel, model: model)
                ConfigTextField("Background opacity", key: "background-opacity", model: model)
                ConfigPicker("Background blur", key: "background-blur",
                             options: ["true", "false"], label: enumLabel, model: model)
            }

            Section("Cursor") {
                ConfigPicker("Style", key: "cursor-style",
                             options: ["block", "bar", "underline", "block_hollow"],
                             label: enumLabel, model: model)
                ConfigPicker("Blink", key: "cursor-style-blink",
                             options: ["true", "false"], label: enumLabel, model: model)
            }

            Section("Window") {
                ConfigTextField("Horizontal padding", key: "window-padding-x", model: model)
                ConfigTextField("Vertical padding", key: "window-padding-y", model: model)
                ConfigPicker("Titlebar style", key: "macos-titlebar-style",
                             options: ["native", "transparent", "tabs", "hidden"],
                             label: enumLabel, model: model)
                ConfigPicker("Restore windows", key: "window-save-state",
                             options: ["always", "never", "default"],
                             label: { $0 == "default" ? "Follow macOS" : enumLabel($0) },
                             model: model)
            }

            Section {
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
            } footer: {
                Text("Other options can be set in the config file. Some changes apply only to new windows.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 500)
        .fixedSize(horizontal: false, vertical: true)
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
