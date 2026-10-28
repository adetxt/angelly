import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// Native tracking keeps a button click and a tab drag in the same responder.
struct TerminalTabDragButton: NSViewRepresentable {
    let title: String
    let label: String
    let selected: Bool
    let click: () -> Void
    let beginDrag: () -> Void
    let endDrag: () -> Void

    func makeNSView(context: Context) -> DragButton { DragButton(frame: .zero) }

    func updateNSView(_ button: DragButton, context: Context) {
        button.title = title
        button.setAccessibilityLabel(label)
        button.selected = selected
        button.click = click
        button.onBeginDrag = beginDrag
        button.onEndDrag = endDrag
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: DragButton, context: Context) -> CGSize? {
        CGSize(width: proposal.width ?? 0, height: proposal.height ?? 32)
    }

    final class DragButton: NSButton, NSDraggingSource {
        var selected = false
        var click: (() -> Void)?
        var onBeginDrag: (() -> Void)?
        var onEndDrag: (() -> Void)?
        private var start: NSPoint?
        private var didDrag = false

        override init(frame frameRect: NSRect) {
            super.init(frame: frameRect)
            setButtonType(.momentaryChange)
            isBordered = false
            isTransparent = true
            target = self
            action = #selector(activate)
        }

        required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

        override var mouseDownCanMoveWindow: Bool { false }
        override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
        override func isAccessibilitySelected() -> Bool { selected }

        @objc private func activate() { click?() }

        override func mouseDown(with event: NSEvent) {
            start = event.locationInWindow
            didDrag = false
        }

        override func mouseUp(with event: NSEvent) {
            defer { start = nil }
            guard start != nil, !didDrag, bounds.contains(convert(event.locationInWindow, from: nil)) else { return }
            performClick(nil)
        }

        override func mouseDragged(with event: NSEvent) {
            guard let start, !didDrag,
                  hypot(event.locationInWindow.x - start.x, event.locationInWindow.y - start.y) >= 4 else { return }
            didDrag = true
            onBeginDrag?()
            let pasteboard = NSPasteboardItem()
            pasteboard.setData(Data(), forType: .init(UTType.ghosttyTerminalTabItem.identifier))
            let item = NSDraggingItem(pasteboardWriter: pasteboard)
            let size = NSSize(width: min(bounds.width, 200), height: 32)
            let image = NSImage(size: size, flipped: false) { [title] rect in
                NSColor.controlBackgroundColor.setFill()
                NSBezierPath(roundedRect: rect, xRadius: 6, yRadius: 6).fill()
                let paragraph = NSMutableParagraphStyle()
                paragraph.lineBreakMode = .byTruncatingMiddle
                (title as NSString).draw(in: rect.insetBy(dx: 8, dy: 8), withAttributes: [
                    .font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.labelColor,
                    .paragraphStyle: paragraph,
                ])
                return true
            }
            item.setDraggingFrame(NSRect(origin: .zero, size: size), contents: image)
            beginDraggingSession(with: [item], event: event, source: self)
                .animatesToStartingPositionsOnCancelOrFail = false
        }

        func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
            context == .withinApplication ? .move : []
        }

        func draggingSession(_ session: NSDraggingSession, endedAt point: NSPoint, operation: NSDragOperation) {
            start = nil
            onEndDrag?()
        }
    }
}
