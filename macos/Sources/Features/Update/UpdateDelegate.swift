import Sparkle
import Cocoa

extension UpdateDriver: SPUUpdaterDelegate {
    func feedURLString(for updater: SPUUpdater) -> String? {
        // Angelly has no appcast of its own yet. Never fall back to Ghostty's
        // feeds: their releases are signed with Ghostty's key and would
        // replace this app. Add our own URL here once we publish updates.
        return nil
    }

    /// Called when an update is scheduled to install silently,
    /// which occurs when `auto-update = download`.
    ///
    /// When `auto-update = check`, Sparkle will call the corresponding
    /// delegate method on the responsible driver instead.
    func updater(_ updater: SPUUpdater, willInstallUpdateOnQuit item: SUAppcastItem, immediateInstallationBlock immediateInstallHandler: @escaping () -> Void) -> Bool {
        viewModel.state = .installing(.init(
            appcastItem: item,
            retryTerminatingApplication: immediateInstallHandler
        ))
        AppDelegate.logger.info("Version: \(item.displayVersionString) installed silently, waiting for relaunch...")
        // Even when hasUnobtrusiveTarget is false, we don't show the alert immediately.
        // We wait until the user manually checks for updates or relaunches.
        return true
    }
}
