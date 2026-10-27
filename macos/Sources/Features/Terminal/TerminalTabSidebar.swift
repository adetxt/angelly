import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// App-wide on/off state of the vertical tab sidebar. The sidebar replaces the native
/// tab bar in every terminal window at once, so the preference is global and persisted.
final class TabSidebarState: ObservableObject {
    static let shared = TabSidebarState()
    static let width: CGFloat = 220

    private static let defaultsKey = "TabSidebar"

    @Published private(set) var isVisible: Bool

    private init() {
        isVisible = UserDefaults.ghostty.object(forKey: Self.defaultsKey) as? Bool ?? true
    }

    func toggle() {
        isVisible.toggle()
        UserDefaults.ghostty.set(isVisible, forKey: Self.defaultsKey)
    }
}

/// Name, color and collapsed state of the sidebar's tab groups. Groups are app-wide because
/// every window of a tab group renders its own sidebar; which tabs belong to a group is
/// `TerminalWindow.sidebarGroup`, so a group with no member tabs left is dropped by `prune`.
@MainActor
final class TabGroupStore: ObservableObject {
    static let shared = TabGroupStore()

    struct Info {
        var name: String
        var color: TerminalTabColor
        var isCollapsed = false
    }

    @Published private(set) var groups: [UUID: Info] = [:]

    private static let palette: [TerminalTabColor] = [
        .blue, .purple, .pink, .red, .orange, .yellow, .green, .teal,
    ]

    /// Adds a group, coloring it with the next palette color so neighbors look different.
    func create() -> UUID {
        let id = UUID()
        groups[id] = Info(name: "Group", color: Self.palette[groups.count % Self.palette.count])
        return id
    }

    func update(_ id: UUID, _ change: (inout Info) -> Void) {
        guard var info = groups[id] else { return }
        change(&info)
        groups[id] = info
    }

    func remove(_ id: UUID) {
        groups[id] = nil
    }

    func prune() {
        let live = Set(NSApp.windows.compactMap { ($0 as? TerminalWindow)?.sidebarGroup })
        let stale = groups.keys.filter { !live.contains($0) }
        for id in stale { groups[id] = nil }
    }
}

extension UTType {
    /// Marks a sidebar row drag. It carries no data: the dragged row is `TabSidebarModel.dragged`.
    static let ghosttyTabSidebarItem = UTType(exportedAs: "com.mitchellh.ghosttyTabSidebarItem")
}

/// Mirrors the native tab group of one terminal window so the sidebar can render it.
/// The native tab group stays the source of truth (selection, ordering, restoration,
/// keybinds); the sidebar only reads it and forwards actions to the tab controllers.
@MainActor
final class TabSidebarModel: ObservableObject {
    struct Item: Identifiable {
        let id: ObjectIdentifier
        weak var window: NSWindow?
        let title: String
        let color: Color?
        let shortcut: String?
        let hasBell: Bool
        let isSelected: Bool
        /// Nil when the tab is ungrouped.
        let group: UUID?
        let groupColor: Color?
    }

    /// A run of consecutive tabs of one group, shown as a collapsible header above them.
    struct GroupHeader: Identifiable {
        let id: UUID
        let name: String
        let color: Color?
        let isCollapsed: Bool
        let count: Int
        let hasBell: Bool
        let containsSelected: Bool
    }

    enum RowID: Hashable {
        case tab(ObjectIdentifier)
        /// Anchored on its first tab so a group split apart by a tab move keeps unique rows.
        case group(UUID, anchor: ObjectIdentifier)
    }

    enum Row: Identifiable {
        case group(GroupHeader, anchor: ObjectIdentifier)
        case tab(Item)

        var id: RowID {
            switch self {
            case .group(let header, let anchor): .group(header.id, anchor: anchor)
            case .tab(let item): .tab(item.id)
            }
        }
    }

    @Published private(set) var items: [Item] = []

    /// What the sidebar lists: `items` with group headers, minus the tabs of collapsed groups.
    @Published private(set) var rows: [Row] = []

    /// The group whose name is being edited in place.
    @Published var renaming: UUID?

    /// The row being dragged, shared by every window's sidebar so a stale drag from one
    /// window can never be mistaken for another's. Only a row of the same tab group can drop.
    private(set) static var dragged: Item.ID?

    private weak var window: NSWindow?
    private let ghostty: Ghostty.App
    private var cancellables = Set<AnyCancellable>()
    private weak var observedGroup: NSWindowTabGroup?
    private var groupObservations: [NSKeyValueObservation] = []

    init(window: NSWindow, ghostty: Ghostty.App) {
        self.window = window
        self.ghostty = ghostty

        // Deferred one turn so a closing window has already left its tab group.
        let center = NotificationCenter.default
        Publishers.MergeMany(
            center.publisher(for: NSWindow.didBecomeMainNotification),
            center.publisher(for: NSWindow.willCloseNotification),
            center.publisher(for: .terminalWindowBellDidChangeNotification),
            center.publisher(for: .terminalWindowTabDidChangeNotification)
        )
        .receive(on: DispatchQueue.main)
        .sink { [weak self] _ in self?.refresh() }
        .store(in: &cancellables)

        // Only one tab bar at a time: the native one yields to the sidebar.
        (window as? TerminalWindow)?.tabBarIsHidden = TabSidebarState.shared.isVisible
        TabSidebarState.shared.$isVisible
            .dropFirst()
            .receive(on: DispatchQueue.main)
            .sink { [weak self] visible in
                (self?.window as? TerminalWindow)?.tabBarIsHidden = visible
                self?.refresh()
            }
            .store(in: &cancellables)

        TabGroupStore.shared.$groups
            .dropFirst()
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.refresh() }
            .store(in: &cancellables)

        DispatchQueue.main.async { [weak self] in self?.refresh() }

        // The sidebar's own controls vanish with it, so the toggle lives in the titlebar.
        if window.styleMask.contains(.titled) {
            let accessory = NSTitlebarAccessoryViewController()
            accessory.layoutAttribute = .left
            // Non-draggable so a slightly moving click isn't taken as a titlebar window drag.
            accessory.view = NonDraggableHostingView(rootView: TabSidebarToggleButton(state: .shared))
            window.addTitlebarAccessoryViewController(accessory)
            // Same as the other titlebar accessories: adding resets this, so it comes after.
            accessory.view.translatesAutoresizingMaskIntoConstraints = false
        }
    }

    func refresh() {
        // Touching `tabGroup` is expensive, so windows stay untouched until the sidebar is on.
        guard TabSidebarState.shared.isVisible, let window else { return }

        let group = window.tabGroup
        observe(group)

        // Publishes only when a group lost its last tab, so this can't loop back here.
        let store = TabGroupStore.shared
        store.prune()
        let groups = store.groups

        let selected = group?.selectedWindow ?? window
        items = (group?.windows ?? [window]).enumerated().map { index, tab in
            let shortcut = index < 9
                ? ghostty.config.keyboardShortcut(for: "goto_tab:\(index + 1)").map { "\($0)" }
                : nil
            let info = (tab as? TerminalWindow)?.sidebarGroup.flatMap { groups[$0] }
            return Item(
                id: ObjectIdentifier(tab),
                window: tab,
                title: tab.title.isEmpty ? "Terminal" : tab.title,
                color: (tab as? TerminalWindow)?.tabColor.displayColor.map(Color.init(nsColor:)),
                shortcut: shortcut,
                hasBell: (tab.windowController as? BaseTerminalController)?.bell ?? false,
                isSelected: tab === selected,
                group: info == nil ? nil : (tab as? TerminalWindow)?.sidebarGroup,
                groupColor: info?.color.displayColor.map(Color.init(nsColor:)))
        }
        rows = Self.rows(for: items, groups: groups)
    }

    private static func rows(for items: [Item], groups: [UUID: TabGroupStore.Info]) -> [Row] {
        var rows: [Row] = []
        var index = 0
        while index < items.count {
            guard let id = items[index].group, let info = groups[id] else {
                rows.append(.tab(items[index]))
                index += 1
                continue
            }

            let run = items[index...].prefix { $0.group == id }
            rows.append(.group(
                GroupHeader(
                    id: id,
                    name: info.name,
                    color: run.first?.groupColor,
                    isCollapsed: info.isCollapsed,
                    count: run.count,
                    hasBell: run.contains { $0.hasBell },
                    containsSelected: run.contains { $0.isSelected }),
                anchor: items[index].id))
            if !info.isCollapsed { rows += run.map(Row.tab) }
            index += run.count
        }
        return rows
    }

    private func observe(_ group: NSWindowTabGroup?) {
        guard group !== observedGroup else { return }
        observedGroup = group
        groupObservations = []
        guard let group else { return }

        groupObservations = [
            group.observe(\.windows) { [weak self] _, _ in
                Task { @MainActor in self?.refresh() }
            },
            group.observe(\.selectedWindow) { [weak self] _, _ in
                Task { @MainActor in self?.refresh() }
            },
        ]
    }

    // MARK: Actions

    func select(_ item: Item) {
        item.window?.makeKeyAndOrderFront(nil)
    }

    func beginDrag(_ item: Item) {
        Self.dragged = item.id
    }

    /// The side of `item` where the dragged row would land, or nil if it can't drop there.
    func dropEdge(on item: Item) -> VerticalEdge? {
        guard let from = items.firstIndex(where: { $0.id == Self.dragged }),
              let to = items.firstIndex(where: { $0.id == item.id }),
              from != to else { return nil }
        return from < to ? .bottom : .top
    }

    func drop(on item: Item) {
        guard let edge = dropEdge(on: item),
              let dragged = items.first(where: { $0.id == Self.dragged })?.window,
              let target = item.window else { return }
        TerminalController.moveTab(dragged, relativeTo: target, ordered: edge == .bottom ? .above : .below)
        // Landing next to a tab means joining its group (or leaving the old one).
        (dragged as? TerminalWindow)?.sidebarGroup = item.group
    }

    var draggedItem: Item? {
        items.first { $0.id == Self.dragged }
    }

    /// Dropping on a group header appends the dragged tab to that group.
    func canDrop(onGroup id: UUID) -> Bool {
        draggedItem.map { $0.group != id } ?? false
    }

    func drop(onGroup id: UUID) {
        guard let dragged = draggedItem else { return }
        add(dragged, to: id)
    }

    // MARK: Groups

    var groupHeaders: [GroupHeader] {
        var seen = Set<UUID>()
        return rows.compactMap {
            guard case .group(let header, _) = $0, seen.insert(header.id).inserted else { return nil }
            return header
        }
    }

    func newGroup(with item: Item) {
        guard let tab = item.window as? TerminalWindow else { return }
        let id = TabGroupStore.shared.create()
        tab.sidebarGroup = id
        renaming = id
    }

    /// Moves the tab behind the group's last tab so a group stays one run of tabs.
    func add(_ item: Item, to group: UUID) {
        guard item.group != group,
              let tab = item.window as? TerminalWindow,
              let last = items.last(where: { $0.group == group && $0.id != item.id })?.window
        else { return }
        TerminalController.moveTab(tab, relativeTo: last, ordered: .above)
        tab.sidebarGroup = group
    }

    func leaveGroup(_ item: Item) {
        guard let tab = item.window as? TerminalWindow, let group = item.group else { return }
        // A tab leaving from the middle steps out behind the group instead of splitting it.
        if let last = items.last(where: { $0.group == group }), last.id != item.id, let target = last.window {
            TerminalController.moveTab(tab, relativeTo: target, ordered: .above)
        }
        tab.sidebarGroup = nil
    }

    func ungroup(_ group: UUID) {
        for case let tab as TerminalWindow in items.filter({ $0.group == group }).compactMap(\.window) {
            tab.sidebarGroup = nil
        }
        TabGroupStore.shared.remove(group)
    }

    func toggleCollapsed(_ group: UUID) {
        TabGroupStore.shared.update(group) { $0.isCollapsed.toggle() }
    }

    func setColor(_ color: TerminalTabColor, of group: UUID) {
        TabGroupStore.shared.update(group) { $0.color = color }
    }

    func rename(_ group: UUID, to name: String) {
        // Submitting and losing focus both end an edit; only the first counts.
        guard renaming == group else { return }
        defer {
            renaming = nil
            // The field held keyboard focus; hand it back to the terminal.
            if let surface = (window?.windowController as? BaseTerminalController)?.focusedSurface {
                window?.makeFirstResponder(surface)
            }
        }
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        TabGroupStore.shared.update(group) { $0.name = name }
    }

    func newTab() {
        controller(window)?.newTab(nil)
    }

    func controller(_ window: NSWindow?) -> TerminalController? {
        window?.windowController as? TerminalController
    }
}

/// The show/hide button next to the traffic lights. It stays put in both states, so a
/// collapsed sidebar can always be brought back.
///
/// AppKit asserts if a titlebar accessory view moves inside its container, which happens
/// on a display change when the view is shorter than the container. The `Spacer` makes the
/// hosting view fill the container's height, like the other titlebar accessories.
private struct TabSidebarToggleButton: View {
    @ObservedObject var state: TabSidebarState

    var body: some View {
        VStack {
            Button(action: state.toggle) {
                Image(systemName: "sidebar.left")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .help(state.isVisible ? "Hide Tab Sidebar" : "Show Tab Sidebar")
            .frame(width: 20, height: 20)
            Spacer()
        }
        .padding(.top, 5)
        .padding(.horizontal, 7)
    }
}

// MARK: - Views

struct TabSidebarView: View {
    @ObservedObject var model: TabSidebarModel

    var body: some View {
        VStack(spacing: 0) {
            header

            ScrollView(showsIndicators: false) {
                VStack(spacing: 2) {
                    ForEach(model.rows) { row in
                        Group {
                            switch row {
                            case .group(let header, _):
                                TabSidebarGroupHeader(model: model, header: header)
                            case .tab(let item):
                                TabSidebarRow(model: model, item: item)
                            }
                        }
                        .transition(.opacity)
                    }
                }
                .padding(.horizontal, 8)
                .padding(.bottom, 8)
                .animation(.easeInOut(duration: 0.18), value: model.rows.map(\.id))
            }
        }
        .frame(width: TabSidebarState.width)
        .frame(maxHeight: .infinity)
        .background(Color.primary.opacity(0.045))
        .overlay(alignment: .trailing) {
            Rectangle()
                .fill(Color.primary.opacity(0.1))
                .frame(width: 1)
        }
    }

    private var header: some View {
        HStack(spacing: 2) {
            Text("TABS")
                .font(.system(size: 11, weight: .semibold))
                .tracking(0.9)
                .foregroundStyle(.secondary)

            Spacer()

            TabSidebarIconButton(symbol: "plus", help: "New Tab") { model.newTab() }
        }
        .padding(.leading, 16)
        .padding(.trailing, 10)
        .frame(height: 38)
    }
}

private struct TabSidebarIconButton: View {
    let symbol: String
    let help: String
    let action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(hovering ? .primary : .secondary)
                .frame(width: 24, height: 24)
                .background(
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(Color.primary.opacity(hovering ? 0.1 : 0)))
        }
        .buttonStyle(.plain)
        .help(help)
        .accessibilityLabel(help)
        .onHover { hovering = $0 }
    }
}

private struct TabSidebarRow: View {
    @ObservedObject var model: TabSidebarModel
    let item: TabSidebarModel.Item

    @State private var hovering = false
    @State private var isDropTarget = false

    var body: some View {
        HStack(spacing: 8) {
            Capsule()
                .fill(item.color ?? (item.isSelected ? Color.accentColor : .clear))
                .frame(width: 3, height: 14)

            Text(item.title)
                .font(.system(size: 13, weight: item.isSelected ? .medium : .regular))
                .foregroundStyle(item.isSelected ? .primary : .secondary)
                .lineLimit(1)
                .truncationMode(.middle)

            Spacer(minLength: 4)

            trailing
        }
        .padding(.leading, 6)
        .padding(.trailing, 8)
        .frame(height: 32)
        .background(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(Color.primary.opacity(item.isSelected ? 0.12 : hovering ? 0.06 : 0)))
        .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .animation(.easeOut(duration: 0.12), value: hovering)
        .animation(.easeOut(duration: 0.12), value: item.isSelected)
        .onHover { hovering = $0 }
        .overlay(alignment: model.dropEdge(on: item) == .top ? .top : .bottom) {
            if isDropTarget {
                Capsule()
                    .fill(Color.accentColor)
                    .frame(height: 2)
                    .padding(.horizontal, 4)
            }
        }
        .onTapGesture { model.select(item) }
        .onDrag {
            model.beginDrag(item)
            let provider = NSItemProvider()
            provider.registerDataRepresentation(
                for: .ghosttyTabSidebarItem, visibility: .ownProcess
            ) { completion in
                completion(Data(), nil)
                return nil
            }
            return provider
        }
        .onDrop(of: [.ghosttyTabSidebarItem], delegate: TabSidebarDropDelegate(
            isTargeted: $isDropTarget,
            canDrop: { model.dropEdge(on: item) != nil },
            drop: { model.drop(on: item) }))
        .contextMenu { contextMenu }
        // Grouped tabs sit under their header, joined to it by a line in the group's color.
        .padding(.leading, item.group == nil ? 0 : 12)
        .overlay(alignment: .leading) {
            if item.group != nil {
                Capsule()
                    .fill(item.groupColor ?? Color.secondary.opacity(0.4))
                    .frame(width: 2)
                    .padding(.leading, 6)
                    .padding(.vertical, -1)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(item.isSelected ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction(named: "Close Tab") { close() }
    }

    @ViewBuilder
    private var trailing: some View {
        if hovering {
            Button(action: close) {
                Image(systemName: "xmark")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(.secondary)
                    .frame(width: 18, height: 18)
                    .background(Circle().fill(Color.primary.opacity(0.1)))
            }
            .buttonStyle(.plain)
            .help("Close Tab")
            .accessibilityLabel("Close Tab")
        } else if item.hasBell {
            Image(systemName: "bell.fill")
                .font(.system(size: 10))
                .foregroundStyle(.orange)
        } else if let shortcut = item.shortcut {
            Text(shortcut)
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)
        }
    }

    @ViewBuilder
    private var contextMenu: some View {
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

private struct TabSidebarGroupHeader: View {
    @ObservedObject var model: TabSidebarModel
    let header: TabSidebarModel.GroupHeader

    @State private var hovering = false
    @State private var isDropTarget = false
    @State private var draft = ""
    @FocusState private var fieldFocused: Bool

    private var isRenaming: Bool { model.renaming == header.id }

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "chevron.right")
                .font(.system(size: 9, weight: .bold))
                .foregroundStyle(.tertiary)
                .rotationEffect(.degrees(header.isCollapsed ? 0 : 90))
                .frame(width: 10)

            Circle()
                .fill(header.color ?? Color.secondary.opacity(0.4))
                .frame(width: 8, height: 8)

            if isRenaming {
                TextField("Group name", text: $draft)
                    .textFieldStyle(.plain)
                    .font(.system(size: 12, weight: .semibold))
                    .focused($fieldFocused)
                    .onSubmit { model.rename(header.id, to: draft) }
                    .onExitCommand { model.rename(header.id, to: "") }
                    .onChange(of: fieldFocused) { focused in
                        if !focused { model.rename(header.id, to: draft) }
                    }
            } else {
                Text(header.name)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(header.containsSelected ? .primary : .secondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }

            Spacer(minLength: 4)

            // Collapsed, the header stands in for its tabs.
            if header.isCollapsed {
                if header.hasBell {
                    Image(systemName: "bell.fill")
                        .font(.system(size: 10))
                        .foregroundStyle(.orange)
                }
                Text("\(header.count)")
                    .font(.system(size: 11))
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.horizontal, 8)
        .frame(height: 28)
        .background(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(isDropTarget
                    ? Color.accentColor.opacity(0.25)
                    : Color.primary.opacity(hovering ? 0.06 : header.isCollapsed && header.containsSelected ? 0.12 : 0)))
        .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .animation(.easeOut(duration: 0.12), value: hovering)
        .onHover { hovering = $0 }
        .onTapGesture { if !isRenaming { model.toggleCollapsed(header.id) } }
        .onDrop(of: [.ghosttyTabSidebarItem], delegate: TabSidebarDropDelegate(
            isTargeted: $isDropTarget,
            canDrop: { model.canDrop(onGroup: header.id) },
            drop: { model.drop(onGroup: header.id) }))
        .contextMenu {
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
        }
        .onChange(of: isRenaming) { renaming in
            guard renaming else { return }
            draft = header.name
            // The field only exists once this update lands.
            DispatchQueue.main.async { fieldFocused = true }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel("\(header.name), \(header.count) tabs, \(header.isCollapsed ? "collapsed" : "expanded")")
        .accessibilityAction(named: header.isCollapsed ? "Expand Group" : "Collapse Group") {
            model.toggleCollapsed(header.id)
        }
    }
}

private struct TabSidebarDropDelegate: DropDelegate {
    @Binding var isTargeted: Bool
    let canDrop: () -> Bool
    let drop: () -> Void

    func validateDrop(info: DropInfo) -> Bool {
        canDrop()
    }

    func dropEntered(info: DropInfo) {
        isTargeted = true
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: .move)
    }

    func dropExited(info: DropInfo) {
        isTargeted = false
    }

    func performDrop(info: DropInfo) -> Bool {
        isTargeted = false
        drop()
        return true
    }
}
