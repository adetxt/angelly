import SwiftUI

extension View {
    /// Makes the view a drop target for dragged tabs and groups in either tab layout. `position` reports
    /// where the drag hovers as a 0...1 fraction along `axis`, nil when it is not over the view. Opening
    /// `gap` (.top before the view, .bottom after it) slides the neighbors apart where the drag would land.
    func tabDropTarget(
        axis: Axis,
        position: Binding<CGFloat?>,
        gap: VerticalEdge?,
        canDrop: @escaping () -> Bool,
        drop: @escaping (CGFloat) -> Void
    ) -> some View {
        modifier(TerminalTabDropTarget(axis: axis, position: position, gap: gap, canDrop: canDrop, drop: drop))
    }
}

private struct TerminalTabDropTarget: ViewModifier {
    static let gapSize: CGFloat = 24

    let axis: Axis
    @Binding var position: CGFloat?
    let gap: VerticalEdge?
    let canDrop: () -> Bool
    let drop: (CGFloat) -> Void

    @State private var extent: CGFloat = 1
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        // The gap lives inside the drop area, so opening it never moves the pointer out of the view.
        content
            .background(GeometryReader { geometry in
                Color.clear
                    .onAppear { extent = length(of: geometry.size) }
                    .onChange(of: length(of: geometry.size)) { extent = $0 }
            })
            .padding(axis == .horizontal ? .leading : .top, gap == .top ? Self.gapSize : 0)
            .padding(axis == .horizontal ? .trailing : .bottom, gap == .bottom ? Self.gapSize : 0)
            .contentShape(Rectangle())
            .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: gap)
            .onDrop(of: [.ghosttyTerminalTabItem], delegate: TerminalTabDropDelegate(
                position: $position, axis: axis, extent: extent,
                inset: gap == .top ? Self.gapSize : 0, canDrop: canDrop, drop: drop))
    }

    private func length(of size: CGSize) -> CGFloat {
        axis == .horizontal ? size.width : size.height
    }
}

private struct TerminalTabDropDelegate: DropDelegate {
    @Binding var position: CGFloat?
    let axis: Axis
    /// The view's own length along `axis`, without any gap.
    let extent: CGFloat
    /// Length of the open gap before the view, which pushes it away from the drop area's origin.
    let inset: CGFloat
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

    /// Measured against the view itself rather than the gap-padded drop area, so the answer does not
    /// change when the gap opens under a stationary pointer.
    private func fraction(of info: DropInfo) -> CGFloat {
        let along = axis == .horizontal ? info.location.x : info.location.y
        return (along - inset) / max(extent, 1)
    }
}
