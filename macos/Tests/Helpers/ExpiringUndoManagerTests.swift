import Foundation
import Testing
@testable import Ghostty

@MainActor
struct ExpiringUndoManagerTests {
    @Test func clearingLiveActionsDoesNotReenterTheTargetSetDuringDeallocation() {
        let manager = ExpiringUndoManager()
        let target = NSObject()
        manager.registerUndo(withTarget: target, expiresAfter: .seconds(60)) { _ in }
        manager.removeAllActions()
        #expect(!manager.canUndo)
        #expect(!manager.canRedo)
    }
}
