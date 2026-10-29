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
    @Published private(set) var width: CGFloat

    private init() {
        isVisible = UserDefaults.ghostty.object(forKey: Self.defaultsKey) as? Bool ?? true
        let stored = UserDefaults.ghostty.object(forKey: Self.widthKey) as? Double
        width = stored.map { Self.clamp(CGFloat($0)) } ?? Self.defaultWidth
    }

    func setWidth(_ newWidth: CGFloat) {
        width = Self.clamp(newWidth)
        UserDefaults.ghostty.set(Double(width), forKey: Self.widthKey)
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

// MARK: - Views

struct TabSidebarView: View {
    @ObservedObject var model: TerminalTabModel
    @ObservedObject private var state = TabSidebarState.shared
    @State private var dragStartWidth: CGFloat?

    var body: some View {
        VStack(spacing: 0) {
            header

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
                }
                .padding(.horizontal, 8)
                .padding(.bottom, 8)
                .animation(.easeInOut(duration: 0.18), value: model.rows.map(\.id))
            }
        }
        .frame(width: state.width)
        .frame(maxHeight: .infinity)
        .background(Color.primary.opacity(0.045))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("Vertical Tab Sidebar")
        .overlay(alignment: .trailing) {
            Rectangle()
                .fill(Color.primary.opacity(0.1))
                .frame(width: 1)
        }
        .overlay(alignment: .trailing) { resizeHandle }
    }

    /// Dragged in window coordinates: the handle moves with the width it is changing, so
    /// a local translation would feed back into itself.
    private var resizeHandle: some View {
        Color.clear
            .frame(width: 6)
            .contentShape(Rectangle())
            .onHover { inside in
                if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
            }
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { value in
                        let start = dragStartWidth ?? state.width
                        dragStartWidth = start
                        state.setWidth(start + value.translation.width)
                    }
                    .onEnded { _ in dragStartWidth = nil })
            .accessibilityHidden(true)
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
    @ObservedObject var model: TerminalTabModel
    let item: TerminalTabModel.Item

    @State private var hovering = false
    @State private var dropPosition: CGFloat?

    var body: some View {
        HStack(spacing: 8) {
            Capsule()
                .fill(item.color ?? (item.isSelected && item.group == nil ? Color.accentColor : .clear))
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
            axis: .vertical, position: $dropPosition, gap: dropPosition == nil ? nil : model.dropEdge(on: item),
            canDrop: { model.dropEdge(on: item) != nil },
            drop: { _ in model.drop(on: item) })
        .contextMenu { TerminalTabContextMenu(model: model, item: item) }
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
    private var gap: VerticalEdge? {
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
            axis: .vertical, position: $dropPosition, gap: gap,
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
