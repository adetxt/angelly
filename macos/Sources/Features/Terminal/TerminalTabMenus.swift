import AppKit
import SwiftUI

struct TerminalTabGroupName: View {
    @ObservedObject var model: TerminalTabModel
    let header: TerminalTabModel.GroupHeader
    @State private var draft = ""
    @FocusState private var fieldFocused: Bool
    @State private var editShortcutMonitor: Any?

    private var isRenaming: Bool { model.renaming == header.id }

    var body: some View {
        Group {
            if isRenaming {
                TextField("Group name", text: $draft)
                    .textFieldStyle(.plain)
                    .focused($fieldFocused)
                    .onSubmit { model.rename(header.id, to: draft) }
                    .onExitCommand { model.rename(header.id, to: "") }
                    .onAppear {
                        draft = header.name
                        DispatchQueue.main.async { fieldFocused = true }
                    }
                    .onChange(of: fieldFocused) { focused in
                        if focused {
                            editShortcutMonitor = TitleEditShortcuts.monitor { _ in fieldFocused }
                        } else {
                            stopMonitoring()
                            model.rename(header.id, to: draft)
                        }
                    }
            } else {
                Text(header.name)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .help(header.name)
            }
        }
        .font(.system(size: 12, weight: .semibold))
        .foregroundStyle(.primary)
        .onDisappear { stopMonitoring() }
    }

    private func stopMonitoring() {
        if let editShortcutMonitor {
            NSEvent.removeMonitor(editShortcutMonitor)
            self.editShortcutMonitor = nil
        }
    }
}

struct TerminalTabContextMenu: View {
    @ObservedObject var model: TerminalTabModel
    let item: TerminalTabModel.Item

    var body: some View {
        let controller = model.controller(item.window)

        Button("Rename Tab...") { controller?.promptTabTitle() }

        Menu("Tab Color") {
            ForEach(TerminalTabColor.allCases, id: \.self) { color in
                Button(color.localizedName) {
                    (item.window as? TerminalWindow)?.tabColor = color
                }
            }
        }

        Menu("Add to Group") {
            Button("New Group") { model.newGroup(with: item) }

            let others = model.groupHeaders.filter { $0.id != item.group }
            if !others.isEmpty {
                Divider()
                ForEach(others) { group in
                    Button(group.name) { model.add(item, to: group.id) }
                }
            }
        }
        if item.group != nil {
            Button("Remove from Group") { model.leaveGroup(item) }
        }

        Divider()

        Button("Close Tab") { close() }
        Button("Close Other Tabs") { controller?.closeOtherTabs(nil) }
        Button("Close Tabs to the Right") { controller?.closeTabsOnTheRight(nil) }
    }

    private func close() {
        model.controller(item.window)?.closeTab(nil)
    }
}

struct TerminalTabGroupMenu: View {
    @ObservedObject var model: TerminalTabModel
    let header: TerminalTabModel.GroupHeader

    var body: some View {
        Button("Rename Group...") { model.renaming = header.id }
        Menu("Group Color") {
            ForEach(TerminalTabColor.allCases, id: \.self) { color in
                Button(color.localizedName) { model.setColor(color, of: header.id) }
            }
        }
        Divider()
        Button(header.isCollapsed ? "Expand Group" : "Collapse Group") {
            model.toggleCollapsed(header.id)
        }
        Button("Ungroup") { model.ungroup(header.id) }
        Button("Close Group") { model.closeGroup(header.id) }
    }
}
