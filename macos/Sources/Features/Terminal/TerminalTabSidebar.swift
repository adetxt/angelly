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
    }

    @Published private(set) var items: [Item] = []

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

        let selected = group?.selectedWindow ?? window
        items = (group?.windows ?? [window]).enumerated().map { index, tab in
            let shortcut = index < 9
                ? ghostty.config.keyboardShortcut(for: "goto_tab:\(index + 1)").map { "\($0)" }
                : nil
            return Item(
                id: ObjectIdentifier(tab),
                window: tab,
                title: tab.title.isEmpty ? "Terminal" : tab.title,
                color: (tab as? TerminalWindow)?.tabColor.displayColor.map(Color.init(nsColor:)),
                shortcut: shortcut,
                hasBell: (tab.windowController as? BaseTerminalController)?.bell ?? false,
                isSelected: tab === selected)
        }
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
                    ForEach(model.items) { item in
                        TabSidebarRow(model: model, item: item)
                            .transition(.opacity)
                    }
                }
                .padding(.horizontal, 8)
                .padding(.bottom, 8)
                .animation(.easeInOut(duration: 0.18), value: model.items.map(\.id))
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
            model: model, item: item, isTargeted: $isDropTarget))
        .contextMenu { contextMenu }
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

        Divider()

        Button("Close Tab") { close() }
        Button("Close Other Tabs") { controller?.closeOtherTabs(nil) }
        Button("Close Tabs to the Right") { controller?.closeTabsOnTheRight(nil) }
    }

    private func close() {
        model.controller(item.window)?.closeTab(nil)
    }
}

private struct TabSidebarDropDelegate: DropDelegate {
    let model: TabSidebarModel
    let item: TabSidebarModel.Item
    @Binding var isTargeted: Bool

    func validateDrop(info: DropInfo) -> Bool {
        model.dropEdge(on: item) != nil
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
        model.drop(on: item)
        return true
    }
}
