import SwiftUI

extension View {
    /// Makes the view a drop target for dragged tabs and groups in either tab layout. `position` reports
    /// where the drag hovers as a 0...1 fraction along `axis`, nil when it is not over the view.
    /// `edge` (.top before the view, .bottom after it) marks where the drag would land without moving the target.
    func tabDropTarget(
        axis: Axis,
        position: Binding<CGFloat?>,
        edge: VerticalEdge?,
        canDrop: @escaping () -> Bool,
        drop: @escaping (CGFloat) -> Void
    ) -> some View {
        modifier(TerminalTabDropTarget(axis: axis, position: position, edge: edge, canDrop: canDrop, drop: drop))
    }
}

private struct TerminalTabDropTarget: ViewModifier {
    let axis: Axis
    @Binding var position: CGFloat?
    let edge: VerticalEdge?
    let canDrop: () -> Bool
    let drop: (CGFloat) -> Void

    @State private var extent: CGFloat = 1
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content
            .background(GeometryReader { geometry in
                Color.clear
                    .onAppear { extent = length(of: geometry.size) }
                    .onChange(of: length(of: geometry.size)) { extent = $0 }
            })
            .overlay(alignment: axis == .horizontal
                ? (edge == .top ? .leading : .trailing)
                : (edge == .top ? .top : .bottom)) {
                if edge != nil {
                    RoundedRectangle(cornerRadius: 1)
                        .fill(Color.accentColor)
                        .frame(width: axis == .horizontal ? 2 : nil, height: axis == .vertical ? 2 : nil)
                        .padding(axis == .horizontal ? .vertical : .horizontal, 4)
                        .allowsHitTesting(false)
                        .transition(.opacity)
                }
            }
            .contentShape(Rectangle())
            .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: edge)
            .onDrop(of: [.ghosttyTerminalTabItem], delegate: TerminalTabDropDelegate(
                position: $position, axis: axis, extent: extent,
                canDrop: canDrop, drop: drop))
    }

    private func length(of size: CGSize) -> CGFloat {
        axis == .horizontal ? size.width : size.height
    }
}

/// Fills the space after the last row so the whole empty area accepts an append.
struct TerminalTabEndDropTarget: View {
    @ObservedObject var model: TerminalTabModel
    let axis: Axis
    @State private var position: CGFloat?

    var body: some View {
        Color.clear
            .frame(minWidth: axis == .horizontal ? 32 : nil, minHeight: axis == .vertical ? 32 : nil)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .tabDropTarget(
                axis: axis, position: $position, edge: position == nil ? nil : .top,
                canDrop: { model.canDropAtEnd }, drop: { _ in model.dropAtEnd() })
            .accessibilityHidden(true)
    }
}

private struct TerminalTabDropDelegate: DropDelegate {
    @Binding var position: CGFloat?
    let axis: Axis
    /// The view's length along `axis`.
    let extent: CGFloat
    let canDrop: () -> Bool
    let drop: (CGFloat) -> Void

    func validateDrop(info: DropInfo) -> Bool { canDrop() }

    func dropEntered(info: DropInfo) { updatePosition(info) }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        updatePosition(info)
        return DropProposal(operation: canDrop() ? .move : .forbidden)
    }

    func dropExited(info: DropInfo) { position = nil }

    func performDrop(info: DropInfo) -> Bool {
        defer { position = nil }
        guard canDrop() else { return false }
        drop(fraction(of: info))
        return true
    }

    private func updatePosition(_ info: DropInfo) {
        position = canDrop() ? fraction(of: info) : nil
    }

    private func fraction(of info: DropInfo) -> CGFloat {
        let along = axis == .horizontal ? info.location.x : info.location.y
        return along / max(extent, 1)
    }
}
