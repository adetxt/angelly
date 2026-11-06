import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// App-wide on/off state of the vertical tab sidebar. The sidebar replaces the native
/// tab bar in every terminal window at once, so the preference is global and persisted.
final class TabSidebarState: ObservableObject {
    static let shared = TabSidebarState()
    static let defaultWidth: CGFloat = 220
    static let widthRange: ClosedRange<CGFloat> = 160...480

    private static let defaultsKey = "TabSidebar"
    private static let widthKey = "TabSidebarWidth"

    @Published private(set) var isVisible: Bool
    private(set) var width: CGFloat
    private let widthSubject: CurrentValueSubject<CGFloat, Never>
    private let resizingSubject = CurrentValueSubject<Bool, Never>(false)

    var widthPublisher: AnyPublisher<CGFloat, Never> { widthSubject.eraseToAnyPublisher() }
    var resizingPublisher: AnyPublisher<Bool, Never> { resizingSubject.eraseToAnyPublisher() }
    var isResizing: Bool { resizingSubject.value }

    private init() {
        isVisible = UserDefaults.ghostty.object(forKey: Self.defaultsKey) as? Bool ?? true
        let stored = UserDefaults.ghostty.object(forKey: Self.widthKey) as? Double
        width = stored.map { Self.clamp(CGFloat($0)) } ?? Self.defaultWidth
        widthSubject = CurrentValueSubject(width)
    }

    func setWidth(_ newWidth: CGFloat) {
        resize(to: newWidth)
        UserDefaults.ghostty.set(Double(width), forKey: Self.widthKey)
    }

    func beginResize() { resizingSubject.send(true) }

    func resize(to newWidth: CGFloat) {
        let clamped = Self.clamp(newWidth)
        guard width != clamped else { return }
        width = clamped
        widthSubject.send(clamped)
    }

    func endResize() {
        guard isResizing else { return }
        UserDefaults.ghostty.set(Double(width), forKey: Self.widthKey)
        resizingSubject.send(false)
    }

    private static func clamp(_ width: CGFloat) -> CGFloat {
        min(max(width, widthRange.lowerBound), widthRange.upperBound)
    }

    func toggle() {
        isVisible.toggle()
        UserDefaults.ghostty.set(isVisible, forKey: Self.defaultsKey)
    }
}

/// The show/hide button next to the traffic lights. It stays put in both states, so a
/// collapsed sidebar can always be brought back.
///
/// AppKit asserts if a titlebar accessory view moves inside its container, which happens
/// on a display change when the view is shorter than the container. The `Spacer` makes the
/// hosting view fill the container's height, like the other titlebar accessories.
struct TabSidebarToggleButton: View {
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

struct TabSidebarTitlebarControls: View {
    @ObservedObject var model: TerminalTabModel
    @ObservedObject private var state = TabSidebarState.shared
    @AppStorage(HorizontalNewTabButtonPlacement.defaultsKey, store: UserDefaults.ghostty)
    private var newButtonPlacement = HorizontalNewTabButtonPlacement.defaultValue

    var body: some View {
        let showsNewButton = newButtonPlacement.showsInTitlebar(
            sidebarVisible: state.isVisible, titlebarAvailable: model.showsTitlebarHeader)
        HStack(spacing: 0) {
            TabSidebarToggleButton(state: state)
            if showsNewButton {
                TerminalNewTabMenu(tabs: model).frame(width: 24, height: 24)
            }
        }
        .padding(.trailing, showsNewButton ? 10 : 0)
        .frame(width: showsNewButton ? 68 : 34, height: 32, alignment: .leading)
        .onChange(of: showsNewButton) { _ in
            DispatchQueue.main.async { model.refresh() }
        }
    }
}

// MARK: - Views

struct TabSidebarView: View {
    @ObservedObject var model: TerminalTabModel
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: 0) {
            if model.titlebarHeight > 0 {
                Color.clear.frame(height: max(32, model.titlebarHeight))
            } else if !model.showsTitlebarHeader {
                header
            }

            GeometryReader { geometry in
                ScrollView(showsIndicators: false) {
                    VStack(spacing: 2) {
                        ForEach(model.rows) { row in
                            Group {
                                switch row {
                                case .group(let header, let anchor):
                                    TabSidebarGroupHeader(model: model, header: header, anchor: anchor)
                                case .tab(let item):
                                    TabSidebarRow(model: model, item: item)
                                }
                            }
                            .transition(.opacity)
                        }
                        TerminalTabEndDropTarget(model: model, axis: .vertical)
                    }
                    .padding(.horizontal, 8)
                    .padding(.bottom, 8)
                    .frame(minHeight: geometry.size.height, alignment: .top)
                    .animation(reduceMotion ? nil : .spring(response: 0.25, dampingFraction: 1), value: model.rows.map(\.id))
                }
            }

            AgentQuotaFooter(store: .shared)
        }
        .frame(maxWidth: .infinity)
        .frame(maxHeight: .infinity)
        .background(Color.primary.opacity(0.045))
        .ignoresSafeArea(.container, edges: .top)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("Vertical Tab Sidebar")
        .overlay(alignment: .trailing) {
            Rectangle()
                .fill(Color.primary.opacity(0.1))
                .frame(width: 1)
                .ignoresSafeArea(.container, edges: .top)
        }
        .overlay(alignment: .trailing) {
            TabSidebarResizeHandle()
                .frame(width: 6)
                .frame(maxHeight: .infinity)
                .ignoresSafeArea(.container, edges: .top)
                .accessibilityHidden(true)
        }
    }

    private var header: some View {
        HStack(spacing: 2) {
            WindowDragRegion()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityHidden(true)
            TerminalNewTabMenu(tabs: model).frame(width: 24, height: 24)
        }
        .padding(.leading, 8)
        .padding(.trailing, 10)
        .frame(height: max(32, model.titlebarHeight))
    }
}

/// The titlebar sits above the sidebar content, so both layers need a resize target.
private struct TabSidebarResizeHandle: NSViewRepresentable {
    func makeNSView(context: Context) -> TabSidebarResizeView { TabSidebarResizeView() }
    func updateNSView(_ nsView: TabSidebarResizeView, context: Context) {}
}

final class TabSidebarResizeView: NSView {
    private var dragStart: (x: CGFloat, width: CGFloat)?
    private var pendingWidth: CGFloat?
    private var stopResizeUpdates: (() -> Void)?

    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .resizeLeftRight)
    }

    override func mouseDown(with event: NSEvent) {
        dragStart = (event.locationInWindow.x, TabSidebarState.shared.width)
        TabSidebarState.shared.beginResize()
        if #available(macOS 14, *) {
            let link = displayLink(target: self, selector: #selector(applyPendingWidth))
            link.add(to: .main, forMode: .common)
            stopResizeUpdates = { link.invalidate() }
        } else {
            let timer = Timer(timeInterval: 1.0 / Double(window?.screen?.maximumFramesPerSecond ?? 60), repeats: true) { [weak self] _ in
                self?.applyPendingWidth()
            }
            RunLoop.main.add(timer, forMode: .common)
            stopResizeUpdates = { timer.invalidate() }
        }
    }

    override func mouseDragged(with event: NSEvent) {
        guard let dragStart else { return }
        // Local coordinates move with the divider and would feed back into the resize.
        let scale = window?.backingScaleFactor ?? 1
        let width = dragStart.width + event.locationInWindow.x - dragStart.x
        pendingWidth = (width * scale).rounded() / scale
    }

    override func mouseUp(with event: NSEvent) {
        guard dragStart != nil else { return }
        mouseDragged(with: event)
        finishResize()
    }

    override func viewWillMove(toWindow newWindow: NSWindow?) {
        if newWindow !== window { finishResize() }
        super.viewWillMove(toWindow: newWindow)
    }

    @objc private func applyPendingWidth() {
        guard let width = pendingWidth else { return }
        pendingWidth = nil
        TabSidebarState.shared.resize(to: width)
    }

    private func finishResize() {
        guard dragStart != nil else { return }
        stopResizeUpdates?()
        stopResizeUpdates = nil
        applyPendingWidth()
        dragStart = nil
        TabSidebarState.shared.endResize()
    }
}

/// Lets the empty portion of the sidebar's top row drag the window.
struct WindowDragRegion: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { DragView() }
    func updateNSView(_ nsView: NSView, context: Context) {}

    private final class DragView: NSView {
        override func mouseDown(with event: NSEvent) { window?.performDrag(with: event) }
    }
}

private struct TabSidebarRow: View {
    @ObservedObject var model: TerminalTabModel
    let item: TerminalTabModel.Item

    @State private var hovering = false
    @State private var dropPosition: CGFloat?

    var body: some View {
        HStack(spacing: 8) {
            Capsule()
                .fill(item.color ?? (item.isSelected && item.group == nil ? Color.accentColor : .clear))
                .frame(width: 3, height: 14)

            TerminalTabTitle(item: item)
                .foregroundStyle(item.isSelected ? .primary : .secondary)

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
        .onTapGesture { model.select(item) }
        .onDrag {
            model.beginDrag(item)
            let provider = NSItemProvider()
            provider.registerDataRepresentation(
                for: .ghosttyTerminalTabItem, visibility: .ownProcess
            ) { completion in
                completion(Data(), nil)
                return nil
            }
            return provider
        }
        .tabDropTarget(
            axis: .vertical, position: $dropPosition, edge: dropPosition == nil ? nil : model.dropEdge(on: item),
            canDrop: { model.dropEdge(on: item) != nil },
            drop: { _ in model.drop(on: item) })
        .contextMenu { TerminalTabContextMenu(model: model, item: item) }
        .help(item.detail)
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
        .accessibilityLabel(item.detail)
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
        }
    }

    private func close() {
        model.controller(item.window)?.closeTab(nil)
    }
}

private struct TabSidebarGroupHeader: View {
    @ObservedObject var model: TerminalTabModel
    let header: TerminalTabModel.GroupHeader
    let anchor: TerminalTabModel.Item.ID

    @State private var hovering = false
    @State private var dropPosition: CGFloat?

    private var isRenaming: Bool { model.renaming == header.id }
    private var isGroupDrag: Bool { model.groupDropEdge(anchor: anchor) != nil }

    /// A dragged group lands on the side it is dragged toward; a dragged tab by `landing`.
    private var dropEdge: VerticalEdge? {
        guard let position = dropPosition else { return nil }
        if isGroupDrag { return model.groupDropEdge(anchor: anchor) }
        switch model.landing(onGroup: header, at: position) {
        case .before: return .top
        case .after: return .bottom
        case .inside: return nil
        }
    }

    private var isJoining: Bool {
        guard let position = dropPosition, !isGroupDrag else { return false }
        return model.landing(onGroup: header, at: position) == .inside && model.canDrop(onGroup: header.id)
    }

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

            TerminalTabGroupName(model: model, header: header)

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
                .fill(isJoining
                    ? Color.accentColor.opacity(0.25)
                    : Color.primary.opacity(hovering ? 0.06 : header.isCollapsed && header.containsSelected ? 0.12 : 0)))
        .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .animation(.easeOut(duration: 0.12), value: hovering)
        .onHover { hovering = $0 }
        .onTapGesture { if !isRenaming { model.toggleCollapsed(header.id) } }
        .onDrag {
            model.beginDrag(header.id, anchor: anchor)
            let provider = NSItemProvider()
            provider.registerDataRepresentation(for: .ghosttyTerminalTabItem, visibility: .ownProcess) { completion in
                completion(Data(), nil)
                return nil
            }
            return provider
        }
        .tabDropTarget(
            axis: .vertical, position: $dropPosition, edge: dropEdge,
            canDrop: { isGroupDrag || model.draggedItem != nil },
            drop: { position in
                if isGroupDrag { model.dropGroup(anchor: anchor) } else { model.drop(onGroup: header, at: position) }
            })
        .contextMenu { TerminalTabGroupMenu(model: model, header: header) }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel("\(header.name), \(header.count) tabs, \(header.isCollapsed ? "collapsed" : "expanded")")
        .accessibilityAction(named: header.isCollapsed ? "Expand Group" : "Collapse Group") {
            model.toggleCollapsed(header.id)
        }
    }

}
