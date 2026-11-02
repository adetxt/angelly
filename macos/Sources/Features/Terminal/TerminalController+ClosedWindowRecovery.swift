import AppKit

extension TerminalController {
    @MainActor
    static func reopen(
        _ snapshot: ClosedWindowSnapshot,
        ghostty: Ghostty.App
    ) throws -> (controllers: [TerminalController], usedDirectoryFallback: Bool) {
        try snapshot.validate()
        let context = ClosedWindowRecoveryContext()
        let decoder = JSONDecoder()
        decoder.userInfo[ClosedWindowRecoveryContext.decoderKey] = context
        let states = try snapshot.tabs.map { try decoder.decode(TerminalRestorableState.self, from: $0.stateData) }
        var controllers: [TerminalController] = []
        do {
            for state in states {
                let controller = TerminalController(ghostty, withSurfaceTree: state.surfaceTree)
                controllers.append(controller)
                guard let window = controller.window else { throw TerminalRestoreError.windowDidNotLoad }
                TerminalWindowRestoration.applyState(state, to: controller, recovery: context)
                // The saved window must not merge into another open window on presentation.
                window.tabbingMode = .disallowed
                guard controller.showWindowSafely(nil) else { throw TerminalRestoreError.windowDidNotLoad }
                if let group = window.tabGroup, group.windows.count > 1 { group.removeWindow(window) }
            }
            guard let first = controllers.first?.window else { throw TerminalRestoreError.windowDidNotLoad }
            first.setFrame(snapshot.frame, display: false)
            first.constrainToScreen()
            for controller in controllers.dropFirst() {
                guard let window = controller.window else { throw TerminalRestoreError.windowDidNotLoad }
                window.tabbingMode = .automatic
                let last = first.tabGroup?.windows.last ?? first
                guard last.addTabbedWindowSafely(window, ordered: .above) else {
                    throw TerminalRestoreError.windowDidNotLoad
                }
            }
            first.tabbingMode = .automatic
            let selected = controllers[snapshot.selectedTabIndex]
            selected.window?.makeKeyAndOrderFront(nil)
            if let mode = states[snapshot.selectedTabIndex].effectiveFullscreenMode {
                selected.toggleFullscreen(mode: mode)
            }
            NSApp.activate(ignoringOtherApps: true)
            return (controllers, context.usedDirectoryFallback)
        } catch {
            for controller in controllers { controller.closeWindowImmediately(origin: .recoveryCleanup) }
            throw error
        }
    }
}
