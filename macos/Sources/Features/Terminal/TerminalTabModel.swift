import AppKit
import Combine
import GhosttyKit
import SwiftUI
import UniformTypeIdentifiers

/// Group metadata shared by both tab layouts. Groups are app-wide because
/// every window of a tab group renders its own bar; which tabs belong to a group is
/// `TerminalWindow.sidebarGroup`, so a group with no member tabs left is dropped by `prune`.
@MainActor
final class TabGroupStore: ObservableObject {
    static let shared = TabGroupStore()

    struct Info: Codable {
        var name: String
        var color: TerminalTabColor
        var isCollapsed = false
    }

    struct Snapshot: Codable {
        let id: UUID
        let info: Info
    }

    func snapshot(_ id: UUID?) -> Snapshot? {
        guard let id, let info = groups[id] else { return nil }
        return Snapshot(id: id, info: info)
    }

    func restore(_ snapshot: Snapshot) {
        if groups[snapshot.id] == nil { groups[snapshot.id] = snapshot.info }
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
        for case let window as TerminalWindow in NSApp.windows where window.sidebarGroup == id {
            window.invalidateRestorableState()
        }
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
    /// Marks a sidebar row drag. It carries no data: the dragged row is `TerminalTabModel.dragged`.
    static let ghosttyTerminalTabItem = UTType(exportedAs: "com.mitchellh.ghosttyTerminalTabItem")
}

/// Shared tab and group commands for both tab layouts. Native windows own order and selection.
@MainActor
final class TerminalTabModel: ObservableObject {
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
        let gitStatus: GitTabStatus?

        var detail: String {
            let heading = shortcut.map { "\(title) (\($0))" } ?? title
            return [heading, gitStatus?.detail].compactMap { $0 }.joined(separator: "\n")
        }
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
    @Published private(set) var titlebarHeight: CGFloat = 0
    @Published private(set) var titlebarLeadingInset: CGFloat = 0
    @Published private(set) var directoryContext: TerminalDirectoryContext?
    private(set) var titlebarContentWidth: CGFloat = 0
    @Published private(set) var showsTitlebarHeader = false

    var selectedRowID: RowID? {
        rows.first { row in
            switch row {
            case .tab(let item): item.isSelected
            case .group(let header, _): header.isCollapsed && header.containsSelected
            }
        }?.id
    }

    /// The group whose name is being edited in place.
    @Published var renaming: UUID?

    /// The row being dragged, shared by every window's sidebar so a stale drag from one
    /// window can never be mistaken for another's. Only a row of the same tab group can drop.
    private enum Dragged {
        case tab(Item.ID)
        case group(UUID, anchor: Item.ID)
    }
    private static var dragged: Dragged?

    private weak var window: NSWindow?
    private let ghostty: Ghostty.App
    private var cancellables = Set<AnyCancellable>()
    private weak var observedGroup: NSWindowTabGroup?
    private var groupObservations: [NSKeyValueObservation] = []
    private weak var observedSurface: Ghostty.SurfaceView?
    private var directoryObservation: AnyCancellable?
    private weak var sidebarTitlebarAccessory: NSTitlebarAccessoryViewController?
    private weak var directoryTitlebarAccessory: NSTitlebarAccessoryViewController?
    // Each native tab owns a model; snapshots must survive switching between those models.
    private static var gitStatuses: [URL: GitTabStatus] = [:]
    private var gitDirectories: Set<URL> = []
    private var lastGitRefresh = Date.distantPast
    private var gitStatusTask: Task<Void, Never>?
    private struct GitBranch: Hashable {
        let directory: URL
        let branch: String
    }
    private var pullRequestTasks: [GitBranch: Task<Void, Never>] = [:]

    init(window: NSWindow, ghostty: Ghostty.App) {
        self.window = window
        self.ghostty = ghostty
        if ghostty.config.macosTitlebarStyle == .transparent, window.styleMask.contains(.titled) {
            (window as? TransparentTitlebarTerminalWindow)?.enableFullSizeTabContent()
        }

        // Deferred one turn so a closing window has already left its tab group.
        let center = NotificationCenter.default
        Publishers.MergeMany(
            center.publisher(for: NSWindow.didBecomeMainNotification),
            center.publisher(for: NSWindow.didResizeNotification),
            center.publisher(for: NSWindow.didEnterFullScreenNotification),
            center.publisher(for: NSWindow.didExitFullScreenNotification),
            center.publisher(for: .fullscreenDidEnter),
            center.publisher(for: .fullscreenDidExit),
            center.publisher(for: NSApplication.didBecomeActiveNotification),
            center.publisher(for: NSWindow.willCloseNotification),
            center.publisher(for: .terminalWindowBellDidChangeNotification),
            center.publisher(for: .terminalWindowTabDidChangeNotification)
        )
        .receive(on: DispatchQueue.main)
        .sink { [weak self] _ in self?.refresh() }
        .store(in: &cancellables)

        Timer.publish(every: 10, on: .main, in: .common)
            .autoconnect()
            .sink { [weak self] _ in
                if NSApp.isActive { self?.refresh() }
            }
            .store(in: &cancellables)

        // Both custom layouts replace the native tab bar.
        (window as? TerminalWindow)?.tabBarIsHidden = true
        if #available(macOS 26, *), window is TitlebarTabsTahoeTerminalWindow {
            window.toolbar = nil
        }
        TabSidebarState.shared.$isVisible.combineLatest(TabSidebarState.shared.widthPublisher)
            .dropFirst()
            .sink { [weak self] visible, width in
                guard let self, let window = self.window else { return }
                if TabSidebarState.shared.isResizing {
                    guard window.isVisible, !window.isMiniaturized,
                          window.tabGroup?.selectedWindow == nil || window.tabGroup?.selectedWindow === window else { return }
                }
                self.refreshTitlebarInsets(window, sidebarVisible: visible, sidebarWidth: width)
            }
            .store(in: &cancellables)

        TabSidebarState.shared.resizingPublisher
            .dropFirst()
            .filter { !$0 }
            .sink { [weak self] _ in self?.refresh() }
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
            accessory.view = NonDraggableHostingView(rootView: TabSidebarTitlebarControls(model: self))
            window.addTitlebarAccessoryViewController(accessory)
            sidebarTitlebarAccessory = accessory
            // Same as the other titlebar accessories: adding resets this, so it comes after.
            accessory.view.translatesAutoresizingMaskIntoConstraints = false

            if !(window is HiddenTitlebarTerminalWindow) {
                let directoryAccessory = NSTitlebarAccessoryViewController()
                directoryAccessory.layoutAttribute = .right
                directoryAccessory.view = TerminalTitlebarContainer(model: self)
                window.addTitlebarAccessoryViewController(directoryAccessory)
                directoryTitlebarAccessory = directoryAccessory
                directoryAccessory.view.translatesAutoresizingMaskIntoConstraints = false
                window.titleVisibility = .hidden
            }
        }
    }

    deinit {
        gitStatusTask?.cancel()
        for task in pullRequestTasks.values { task.cancel() }
    }

    func refresh() {
        guard let window, !TabSidebarState.shared.isResizing else { return }

        let group = window.tabGroup
        observe(group)

        // Prepare each tab's first list while it is hidden, then update only the selected list.
        let selected = group?.selectedWindow ?? window
        guard selected === window || rows.isEmpty else { return }
        refreshTitlebarInsets(window)
        observeDirectory(of: (selected.windowController as? BaseTerminalController)?.focusedSurface)

        // Publishes only when a group lost its last tab, so this can't loop back here.
        let store = TabGroupStore.shared
        store.prune()
        let groups = store.groups

        let newItems: [Item] = (group?.windows ?? [window]).enumerated().map { index, tab in
            let shortcut = index < 9
                ? ghostty.config.keyboardShortcut(for: "goto_tab:\(index + 1)").map { "\($0)" }
                : nil
            let info = (tab as? TerminalWindow)?.sidebarGroup.flatMap { groups[$0] }
            let controller = tab.windowController as? BaseTerminalController
            let status = gitDirectory(for: tab).flatMap { Self.gitStatuses[$0] }
            controller?.titlebarBranch = status?.titlebarBranch
            let title = controller?.tabTitle ?? tab.title
            return Item(
                id: ObjectIdentifier(tab),
                window: tab,
                title: title.isEmpty ? "Terminal" : title,
                // A grouped tab shows its group's color instead of its own.
                color: info == nil ? (tab as? TerminalWindow)?.tabColor.displayColor.map(Color.init(nsColor:)) : nil,
                shortcut: shortcut,
                hasBell: (tab.windowController as? BaseTerminalController)?.bell ?? false,
                // Every window shows its own list only while it is the selected tab, so a list
                // prepared while hidden must already highlight its own window.
                isSelected: tab === window,
                group: info == nil ? nil : (tab as? TerminalWindow)?.sidebarGroup,
                groupColor: info?.color.displayColor.map(Color.init(nsColor:)),
                gitStatus: status)
        }
        let newRows = Self.rows(for: newItems, groups: groups)
        let context = TerminalDirectoryContext(
            workingDirectory: gitDirectory(for: window),
            gitStatus: newItems.first(where: \.isSelected)?.gitStatus)
        if directoryContext != context { directoryContext = context }

        // A list that has never shown anything appears at once; fading in every row would
        // flash an empty list when a new tab opens.
        if rows.isEmpty {
            var transaction = Transaction()
            transaction.disablesAnimations = true
            withTransaction(transaction) {
                items = newItems
                rows = newRows
            }
        } else {
            items = newItems
            rows = newRows
        }
        if selected === window { refreshGitStatuses(in: newItems) }
    }

    private func refreshTitlebarInsets(_ window: NSWindow, sidebarVisible: Bool? = nil, sidebarWidth: CGFloat? = nil) {
        let hasTitlebar = window.styleMask.contains(.titled) && !window.styleMask.contains(.fullScreen)
        let hasHeader = hasTitlebar && directoryTitlebarAccessory != nil
        let fullSize = (window as? TransparentTitlebarTerminalWindow)?.usesFullSizeTabContent == true
            && hasTitlebar
        let height = fullSize ? max(0, window.frame.height - window.contentLayoutRect.maxY) : 0
        var leading: CGFloat = 0
        if height > 0 || hasHeader {
            let buttons = [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton]
                .compactMap { window.standardWindowButton($0) }
            let controlsRight = buttons.filter { !$0.isHidden }
                .map { $0.convert($0.bounds, to: window.contentView).maxX }.max() ?? 0
            var accessoryRight: CGFloat = 0
            for accessory in window.titlebarAccessoryViewControllers where accessory.layoutAttribute == .left && !accessory.view.isHidden {
                let frame = accessory.view.convert(accessory.view.bounds, to: window.contentView)
                // The creation menu expands to the sidebar edge; only the fixed toggle reserves space in the tab bar.
                let right = accessory === sidebarTitlebarAccessory ? frame.minX + 34 : frame.maxX
                accessoryRight = max(accessoryRight, right)
            }
            leading = max(controlsRight, accessoryRight) + 8
            leading = max(120, leading)
        }
        if titlebarHeight != height { titlebarHeight = height }
        if titlebarLeadingInset != leading { titlebarLeadingInset = leading }
        if let accessory = directoryTitlebarAccessory {
            let controls = sidebarTitlebarAccessory?.view
            let controlsLeft = controls.map { $0.convert($0.bounds, to: window.contentView).minX } ?? 80
            let controlsRight = controlsLeft + (controls?.fittingSize.width ?? 34)
            let otherAccessories = window.titlebarAccessoryViewControllers.filter {
                $0 !== accessory && $0.layoutAttribute == .right && !$0.view.isHidden
            }.reduce(CGFloat(0)) { $0 + $1.view.frame.width }
            let width = max(0, window.frame.width - controlsRight - otherAccessories)
            if let header = accessory.view as? TerminalTitlebarContainer {
                header.updateGeometry(width: width, leading: controlsRight,
                                      sidebarWidth: sidebarWidth ?? TabSidebarState.shared.width,
                                      sidebarVisible: sidebarVisible ?? TabSidebarState.shared.isVisible)
                titlebarContentWidth = header.contextView.frame.width
            }
            if showsTitlebarHeader != hasHeader { showsTitlebarHeader = hasHeader }
            accessory.view.isHidden = !hasHeader
        }
    }

    private func gitDirectory(for window: NSWindow?) -> URL? {
        guard let path = (window?.windowController as? BaseTerminalController)?.focusedSurface?.pwd,
              path.hasPrefix("/") else { return nil }
        return URL(fileURLWithPath: path).standardizedFileURL
    }

    private func observeDirectory(of surface: Ghostty.SurfaceView?) {
        guard observedSurface !== surface else { return }
        observedSurface = surface
        directoryObservation = surface?.$pwd
            .removeDuplicates()
            .dropFirst()
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.refresh() }
    }

    private func refreshGitStatuses(in items: [Item]) {
        let directories = Set(items.compactMap { gitDirectory(for: $0.window) })
        guard gitStatusTask == nil,
              directories != gitDirectories || Date().timeIntervalSince(lastGitRefresh) >= 10 else { return }
        gitStatusTask = Task { [weak self] in
            var statuses: [URL: GitTabStatus] = [:]
            for directory in directories {
                guard !Task.isCancelled else { return }
                if let status = try? await GitWorktreeService.shared.tabStatus(from: directory) {
                    statuses[directory] = status
                }
            }
            guard !Task.isCancelled, let self else { return }
            for directory in directories {
                if var status = statuses[directory] {
                    status.preservePullRequest(from: Self.gitStatuses[directory])
                    Self.gitStatuses[directory] = status
                } else {
                    Self.gitStatuses[directory] = nil
                }
            }
            let liveDirectories = Set(NSApp.windows.compactMap { self.gitDirectory(for: $0) })
            Self.gitStatuses = Self.gitStatuses.filter { liveDirectories.contains($0.key) }
            self.gitDirectories = directories
            self.lastGitRefresh = Date()
            self.gitStatusTask = nil
            self.refresh()
            for directory in directories {
                if let status = Self.gitStatuses[directory] { self.refreshPullRequest(for: status) }
            }
        }
    }

    private func refreshPullRequest(for status: GitTabStatus) {
        guard let branch = status.branch, status.pullRequest != .notApplicable else { return }
        let key = GitBranch(directory: status.directory, branch: branch)
        guard pullRequestTasks[key] == nil else { return }
        pullRequestTasks[key] = Task { [weak self] in
            let request = await GitHubPullRequestService.shared.status(in: key.directory, branch: key.branch)
            guard !Task.isCancelled, let self else { return }
            self.pullRequestTasks[key] = nil
            for directory in Array(Self.gitStatuses.keys) {
                guard var current = Self.gitStatuses[directory], current.directory == key.directory,
                      current.branch == key.branch, current.pullRequest != .notApplicable else { continue }
                current.pullRequest = request
                Self.gitStatuses[directory] = current
            }
            self.refresh()
        }
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
        if let surface = controller(item.window)?.focusedSurface {
            item.window?.makeFirstResponder(surface)
        }
    }

    func beginDrag(_ item: Item) {
        Self.dragged = .tab(item.id)
    }

    func beginDrag(_ group: UUID, anchor: Item.ID) {
        Self.dragged = .group(group, anchor: anchor)
    }

    private var draggedRun: [Item] {
        guard case let .group(id, anchor) = Self.dragged,
              let start = items.firstIndex(where: { $0.id == anchor && $0.group == id }) else { return [] }
        return Array(items[start...].prefix { $0.group == id })
    }

    /// The side of `item` where the dragged row would land, or nil if it can't drop there.
    func dropEdge(on item: Item) -> VerticalEdge? {
        guard let to = items.firstIndex(where: { $0.id == item.id }) else { return nil }
        if case .group = Self.dragged {
            let run = draggedRun
            guard let first = run.first, !run.contains(where: { $0.id == item.id }),
                  let from = items.firstIndex(where: { $0.id == first.id }) else { return nil }
            return from < to ? .bottom : .top
        }
        guard let dragged = draggedItem,
              let from = items.firstIndex(where: { $0.id == dragged.id }), from != to else { return nil }
        return from < to ? .bottom : .top
    }

    func drop(on item: Item, edge: VerticalEdge? = nil) {
        defer { endDrag() }
        guard let inferredEdge = dropEdge(on: item) else { return }
        let edge = edge ?? inferredEdge
        if case .group = Self.dragged {
            moveDraggedGroup(relativeTo: item, edge: edge)
            return
        }
        guard let dragged = draggedItem?.window, let target = item.window else { return }
        TerminalController.moveTab(dragged, relativeTo: target, ordered: edge == .bottom ? .above : .below)
        // Landing next to a tab means joining its group (or leaving the old one).
        (dragged as? TerminalWindow)?.sidebarGroup = item.group
    }

    var draggedItem: Item? {
        guard case let .tab(id) = Self.dragged else { return nil }
        return items.first { $0.id == id }
    }

    private func moveDraggedGroup(relativeTo item: Item, edge: VerticalEdge) {
        let run = draggedRun
        let selected = window?.tabGroup?.selectedWindow
        guard !run.isEmpty, let index = items.firstIndex(where: { $0.id == item.id }) else { return }
        // A group lands outside the target's entire run, never splitting another group.
        var targetIndex = index
        if let group = item.group {
            if edge == .top {
                while targetIndex > 0 && items[targetIndex - 1].group == group { targetIndex -= 1 }
            } else {
                while targetIndex + 1 < items.count && items[targetIndex + 1].group == group { targetIndex += 1 }
            }
        }
        guard let target = items[targetIndex].window else { return }
        let moving = edge == .top ? run : Array(run.reversed())
        for item in moving {
            guard let tab = item.window else { continue }
            TerminalController.moveTab(tab, relativeTo: target, ordered: edge == .top ? .below : .above)
        }
        if let selected { window?.tabGroup?.selectedWindow = selected }
    }

    func groupDropEdge(anchor: Item.ID) -> VerticalEdge? {
        guard case .group = Self.dragged, let item = items.first(where: { $0.id == anchor }) else { return nil }
        return dropEdge(on: item)
    }

    func dropGroup(anchor: Item.ID, edge: VerticalEdge? = nil) {
        defer { endDrag() }
        guard let item = items.first(where: { $0.id == anchor }),
              let inferredEdge = groupDropEdge(anchor: anchor) else { return }
        moveDraggedGroup(relativeTo: item, edge: edge ?? inferredEdge)
    }

    /// Dropping on a group header appends the dragged tab to that group.
    func canDrop(onGroup id: UUID) -> Bool {
        draggedItem.map { $0.group != id } ?? false
    }

    func drop(onGroup id: UUID) {
        defer { endDrag() }
        guard let dragged = draggedItem else { return }
        add(dragged, to: id)
    }

    enum GroupLanding { case before, after, inside }

    /// Where a dragged tab lands on a group header, by how far along it the drag is (0...1): beside
    /// the group in the outer quarters, inside it in between. An expanded group's far side leads into
    /// its own tabs, so only a collapsed group accepts a tab after it.
    func landing(onGroup header: GroupHeader, at position: CGFloat) -> GroupLanding {
        if position < 0.25 { return .before }
        if position > 0.75 && header.isCollapsed { return .after }
        return .inside
    }

    func drop(onGroup header: GroupHeader, at position: CGFloat) {
        switch landing(onGroup: header, at: position) {
        case .before: drop(besideGroup: header.id, edge: .top)
        case .after: drop(besideGroup: header.id, edge: .bottom)
        case .inside: drop(onGroup: header.id)
        }
    }

    /// Dropping beside a group header lands the dragged tab just outside the group's run, ungrouped.
    func drop(besideGroup id: UUID, edge: VerticalEdge) {
        defer { endDrag() }
        guard let dragged = draggedItem, let tab = dragged.window else { return }
        let members = items.filter { $0.group == id && $0.id != dragged.id }
        if let anchor = (edge == .top ? members.first : members.last)?.window {
            TerminalController.moveTab(tab, relativeTo: anchor, ordered: edge == .top ? .below : .above)
        }
        (tab as? TerminalWindow)?.sidebarGroup = nil
    }

    var canDropAtEnd: Bool { draggedItem != nil || !draggedRun.isEmpty }

    func dropAtEnd() {
        defer { endDrag() }
        guard let last = items.last else { return }
        if let dragged = draggedItem, let tab = dragged.window, let target = last.window {
            if tab !== target { TerminalController.moveTab(tab, relativeTo: target, ordered: .above) }
            (tab as? TerminalWindow)?.sidebarGroup = nil
        } else if !draggedRun.contains(where: { $0.id == last.id }) {
            moveDraggedGroup(relativeTo: last, edge: .bottom)
        }
    }

    func endDrag() {
        Self.dragged = nil
        refresh()
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
        if item.group != nil { leaveGroup(item) }
        let id = TabGroupStore.shared.create()
        tab.sidebarGroup = id
        renaming = id
        refresh()
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

    func closeGroup(_ group: UUID) {
        controller(window)?.closeTabGroup(group)
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

    var workingDirectory: URL? {
        guard let path = controller(window)?.focusedSurface?.pwd, !path.isEmpty else { return nil }
        return URL(fileURLWithPath: path)
    }

    func openWorktree(at directory: URL) {
        var config = Ghostty.SurfaceConfiguration()
        config.workingDirectory = directory.path
        config.context = GHOSTTY_SURFACE_CONTEXT_TAB
        _ = TerminalController.newTab(ghostty, from: window, withBaseConfig: config)
    }

    func controller(_ window: NSWindow?) -> TerminalController? {
        window?.windowController as? TerminalController
    }
}

