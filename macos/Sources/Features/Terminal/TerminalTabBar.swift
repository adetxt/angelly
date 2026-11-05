import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct TerminalTabBar: View {
    static let height: CGFloat = 40

    @ObservedObject var model: TerminalTabModel
    @ObservedObject private var sidebarState = TabSidebarState.shared
    @AppStorage(HorizontalNewTabButtonPlacement.defaultsKey, store: UserDefaults.ghostty)
    private var newButtonPlacement = HorizontalNewTabButtonPlacement.defaultValue
    @State private var endDropPosition: CGFloat?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var scrollTarget: TerminalTabModel.RowID? {
        if let renaming = model.renaming {
            return model.rows.first {
                if case .group(let header, _) = $0 { return header.id == renaming }
                return false
            }?.id
        }
        return model.selectedRowID
    }

    var body: some View {
        HStack(spacing: 0) {
            ScrollViewReader { proxy in
                GeometryReader { geometry in
                    ScrollView(.horizontal, showsIndicators: true) {
                        HStack(spacing: 3) {
                            ForEach(model.rows) { row in
                                switch row {
                                case .tab(let item):
                                    TerminalTabButton(model: model, item: item).id(row.id)
                                case .group(let header, let anchor):
                                    TerminalTabGroupButton(model: model, header: header, anchor: anchor).id(row.id)
                                }
                            }
                            TerminalTabEndDropTarget(model: model, axis: .horizontal)
                        }
                        .padding(.horizontal, 6)
                        .padding(.vertical, 4)
                        .frame(minWidth: geometry.size.width, alignment: .leading)
                        .animation(reduceMotion ? nil : .spring(response: 0.25, dampingFraction: 1), value: model.rows.map(\.id))
                    }
                    .onChange(of: geometry.size.width) { _ in scrollToTarget(proxy) }
                }
                .onAppear { scrollToTarget(proxy) }
                .onChange(of: sidebarState.isVisible) { visible in
                    if !visible { scrollToTarget(proxy) }
                }
                .onChange(of: scrollTarget) { _ in scrollToTarget(proxy) }
            }

            if !newButtonPlacement.showsInTitlebar(sidebarVisible: sidebarState.isVisible, titlebarAvailable: model.showsTitlebarHeader) {
                TerminalNewTabMenu(tabs: model)
                    .frame(width: 36, height: Self.height)
                    .background(endDropPosition == nil ? Color.clear : Color.accentColor.opacity(0.15))
                    .tabDropTarget(
                        axis: .horizontal, position: $endDropPosition, edge: nil,
                        canDrop: { model.canDropAtEnd }, drop: { _ in model.dropAtEnd() })
            }
        }
        .frame(height: Self.height)
        .background(Color.primary.opacity(0.045))
        .overlay(alignment: .bottom) { Divider() }
        .accessibilityIdentifier("Horizontal Tab Bar")
    }

    private func scrollToTarget(_ proxy: ScrollViewProxy) {
        guard let target = scrollTarget else { return }
        DispatchQueue.main.async { proxy.scrollTo(target, anchor: .center) }
    }
}

private struct TerminalTabButton: View {
    @ObservedObject var model: TerminalTabModel
    let item: TerminalTabModel.Item
    @State private var dropPosition: CGFloat?

    var body: some View {
        HStack(spacing: 0) {
            ZStack {
                TerminalTabDragButton(
                    title: item.title, label: item.detail, selected: item.isSelected,
                    click: { model.select(item) }, beginDrag: { model.beginDrag(item) }, endDrag: model.endDrag)
                HStack(spacing: 6) {
                    if let color = item.color {
                        Circle().fill(color).frame(width: 6, height: 6)
                    }
                    TerminalTabTitle(item: item, horizontal: true)
                    Spacer(minLength: 0)
                    if item.hasBell {
                        Image(systemName: "bell.fill").font(.system(size: 10))
                    }
                }
                .padding(.leading, 10)
                .frame(maxWidth: .infinity, minHeight: 32)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            }

            Button { model.controller(item.window)?.closeTab(nil) } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 9, weight: .semibold))
                    .frame(width: 26, height: 32)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Close \(item.title)")
            .help("Close Tab")
        }
        .frame(width: 168, height: 32)
        .foregroundStyle(.primary)
        .background(RoundedRectangle(cornerRadius: 6).fill(Color.primary.opacity(item.isSelected ? 0.12 : 0.035)))
        .overlay(alignment: .bottom) {
            if let color = item.groupColor {
                Rectangle().fill(color).frame(height: 3)
            }
        }
        .help(item.detail)
        .tabDropTarget(
            axis: .horizontal, position: $dropPosition, edge: dropPosition.map(horizontalEdge(at:)),
            canDrop: { model.dropEdge(on: item) != nil },
            drop: { model.drop(on: item, edge: horizontalEdge(at: $0)) })
        .contextMenu { TerminalTabContextMenu(model: model, item: item) }
    }
}

private struct TerminalTabGroupButton: View {
    @ObservedObject var model: TerminalTabModel
    let header: TerminalTabModel.GroupHeader
    let anchor: TerminalTabModel.Item.ID
    @State private var dropPosition: CGFloat?

    private var isRenaming: Bool { model.renaming == header.id }
    private var isGroupDrag: Bool { model.groupDropEdge(anchor: anchor) != nil }

    /// A dragged group lands before or after this one by which half it hovers; a dragged tab by `landing`.
    private var dropEdge: VerticalEdge? {
        guard let position = dropPosition else { return nil }
        if isGroupDrag { return horizontalEdge(at: position) }
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
        Group {
            if isRenaming {
                TerminalTabGroupName(model: model, header: header).frame(width: 120)
            } else {
                ZStack {
                    TerminalTabDragButton(
                        title: header.name,
                        label: "\(header.name), \(header.count) tabs, \(header.isCollapsed ? "collapsed" : "expanded")",
                        selected: header.isCollapsed && header.containsSelected,
                        click: { model.toggleCollapsed(header.id) },
                        beginDrag: { model.beginDrag(header.id, anchor: anchor) }, endDrag: model.endDrag)
                    HStack(spacing: 6) {
                        Image(systemName: header.isCollapsed ? "chevron.right" : "chevron.down")
                            .font(.system(size: 9, weight: .bold))
                        TerminalTabGroupName(model: model, header: header)
                        if header.isCollapsed {
                            if header.hasBell { Image(systemName: "bell.fill").font(.system(size: 10)) }
                            Text("\(header.count)").font(.system(size: 11))
                        }
                    }
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                }
                .help(header.isCollapsed ? "Expand Group" : "Collapse Group")
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 32)
        .background(RoundedRectangle(cornerRadius: 6).fill(
            (header.color ?? .secondary).opacity(isJoining ? 0.25 : 0.12)))
        .overlay(alignment: .bottom) {
            Rectangle().fill(header.color ?? .secondary).frame(height: 3)
        }
        .overlay {
            if header.isCollapsed && header.containsSelected {
                RoundedRectangle(cornerRadius: 6).strokeBorder(Color.primary.opacity(0.6), lineWidth: 1)
            }
        }
        .tabDropTarget(
            axis: .horizontal, position: $dropPosition, edge: dropEdge,
            canDrop: { isGroupDrag || model.draggedItem != nil },
            drop: { position in
                if isGroupDrag {
                    model.dropGroup(anchor: anchor, edge: horizontalEdge(at: position))
                } else {
                    model.drop(onGroup: header, at: position)
                }
            })
        .contextMenu { TerminalTabGroupMenu(model: model, header: header) }
    }
}

private func horizontalEdge(at position: CGFloat) -> VerticalEdge { position < 0.5 ? .top : .bottom }
