import AppKit

@MainActor
extension AppDelegate: NSMenuDelegate {
    @IBAction func saveWorkspace(_ sender: Any?) {
        guard let controller = NSApp.keyWindow?.windowController as? TerminalController else {
            showWindowRecoveryMessage("Couldn’t Save Workspace", detail: "Select a terminal window first.")
            return
        }
        do {
            let snapshot = try ClosedWindowSnapshot.capture(from: controller)
            try snapshot.validate()
            let data = try JSONEncoder().encode(snapshot)
            guard data.count <= ClosedWindowStore.maximumBytes else { throw ClosedWindowSnapshot.RecoveryError.oversized }
            let panel = NSSavePanel()
            panel.nameFieldStringValue = "Workspace.angellyworkspace"
            panel.message = "Saves tabs, splits, titles and working directories. Running processes and commands are not saved."
            guard panel.runModal() == .OK, let url = panel.url else { return }
            try data.write(to: url, options: .atomic)
        } catch {
            showWindowRecoveryMessage("Couldn’t Save Workspace", detail: error.localizedDescription)
        }
    }

    @IBAction func openWorkspace(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.message = "Open a saved Angelly workspace. New terminal sessions will be created."
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            guard let size = attributes[.size] as? NSNumber, size.intValue <= ClosedWindowStore.maximumBytes else {
                throw ClosedWindowSnapshot.RecoveryError.oversized
            }
            let data = try Data(contentsOf: url)
            guard data.count <= ClosedWindowStore.maximumBytes else { throw ClosedWindowSnapshot.RecoveryError.oversized }
            let snapshot = try JSONDecoder().decode(ClosedWindowSnapshot.self, from: data)
            let result = try TerminalController.reopen(snapshot, ghostty: ghostty)
            if result.usedDirectoryFallback {
                showWindowRecoveryMessage("Some Working Directories Are Unavailable", detail: "Those terminals opened in the configured startup directory instead.")
            }
        } catch {
            showWindowRecoveryMessage("Couldn’t Open Workspace", detail: error.localizedDescription)
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        guard menu.identifier?.rawValue == "recentlyClosedWindows" else { return }
        menu.removeAllItems()
        let store = closedWindowStore
        if store.entries.isEmpty {
            let text: String
            if !store.isPersistenceEnabled {
                text = "Window Recovery Is Disabled"
            } else if store.loadFailure != nil {
                text = "Window Recovery Is Unavailable"
            } else {
                text = "No Recently Closed Windows"
            }
            let item = menu.addItem(withTitle: text, action: nil, keyEquivalent: "")
            item.isEnabled = false
        } else {
            for snapshot in store.entries {
                let date = DateFormatter.localizedString(from: snapshot.closedAt, dateStyle: .short, timeStyle: .short)
                let title = snapshot.title.components(separatedBy: .newlines).joined(separator: " ")
                let tabs = snapshot.tabs.count == 1 ? "1 tab" : "\(snapshot.tabs.count) tabs"
                let item = menu.addItem(
                    withTitle: "\(title.isEmpty ? "Terminal" : title) · \(tabs) · \(date)",
                    action: #selector(reopenRecentWindow(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = snapshot.id
                item.toolTip = "Reopens the layout in new terminal sessions."
            }
        }
        menu.addItem(.separator())
        let clear = menu.addItem(
            withTitle: "Clear Recently Closed Windows", action: #selector(clearRecentlyClosedWindows(_:)), keyEquivalent: "")
        clear.target = self
    }

    @IBAction func reopenClosedWindow(_ sender: Any?) {
        guard let id = closedWindowStore.entries.first?.id else { return }
        reopenClosedWindow(id: id)
    }

    @IBAction func reopenRecentWindow(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? UUID else { return }
        reopenClosedWindow(id: id)
    }

    private func reopenClosedWindow(id: UUID) {
        guard let snapshot = closedWindowStore.entries.first(where: { $0.id == id }) else { return }
        do {
            let result = try TerminalController.reopen(snapshot, ghostty: ghostty)
            consumeClosedWindow(closedWindowStore.ticket(for: id))
            if result.usedDirectoryFallback {
                showWindowRecoveryMessage(
                    "Some Working Directories Are Unavailable",
                    detail: "Those terminals opened in the configured startup directory instead.")
            }
        } catch {
            showWindowRecoveryMessage(
                "Couldn’t Reopen Window",
                detail: "The saved layout could not be restored. It remains in Recently Closed Windows so you can try again.")
        }
    }

    @IBAction func clearRecentlyClosedWindows(_ sender: Any?) {
        do { try closedWindowStore.clear() } catch {
            showWindowRecoveryMessage(
                "Couldn’t Clear Window History",
                detail: "The saved history could not be deleted. Check the available disk space and file permissions, then try again.")
        }
    }

    func consumeClosedWindow(_ ticket: ClosedWindowStore.CloseTicket) {
        ticket.isRestored = true
        do { try closedWindowStore.consume(id: ticket.id) } catch {
            let alert = NSAlert()
            alert.messageText = "Window Reopened, but History Couldn’t Be Updated"
            alert.informativeText = "Your terminals are open. This history entry may reappear after restarting Angelly until the saved history is updated."
            alert.addButton(withTitle: "OK")
            alert.addButton(withTitle: "Retry")
            presentWindowRecoveryAlert(alert) { [weak self] response in
                if response == .alertSecondButtonReturn { self?.consumeClosedWindow(ticket) }
            }
        }
    }

    func showWindowRecoveryMessage(_ title: String, detail: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = detail
        alert.addButton(withTitle: "OK")
        presentWindowRecoveryAlert(alert) { _ in }
    }

    private func presentWindowRecoveryAlert(_ alert: NSAlert, completion: @escaping (NSApplication.ModalResponse) -> Void) {
        if let window = NSApp.keyWindow, window.attachedSheet == nil {
            alert.beginSheetModal(for: window, completionHandler: completion)
        } else {
            completion(alert.runModal())
        }
    }
}
