import AppKit

struct ClosedWindowSnapshot: Codable, Identifiable {
    var id: UUID
    let closedAt: Date
    let title: String
    let frame: NSRect
    let selectedTabIndex: Int
    let tabs: [Tab]

    struct Tab: Codable {
        let stateVersion: Int
        let stateData: Data
    }

    enum RecoveryError: LocalizedError {
        case unavailable, invalidHistory, oversized, disabled

        var errorDescription: String? {
            switch self {
            case .unavailable: return "This window contains a terminal that cannot be saved for recovery."
            case .invalidHistory: return "The saved window history is damaged or uses an unsupported format."
            case .oversized: return "This window exceeds the recovery history size limit."
            case .disabled: return "Window recovery is disabled."
            }
        }
    }

    @MainActor
    static func capture(from controller: TerminalController) throws -> Self {
        guard let window = controller.window else { throw RecoveryError.unavailable }
        let windows = window.tabGroup?.windows ?? [window]
        let tabs = try windows.map { window -> Tab in
            guard let controller = window.windowController as? TerminalController,
                  controller.supportsClosedWindowRecovery else { throw RecoveryError.unavailable }
            return try Tab(
                stateVersion: TerminalRestorableState.version,
                stateData: JSONEncoder().encode(TerminalRestorableState(from: controller)))
        }
        let selected = window.tabGroup?.selectedWindow ?? window
        return Self(
            id: UUID(), closedAt: Date(),
            title: String((controller.titleOverride ?? window.title).prefix(120)),
            frame: window.frame,
            selectedTabIndex: windows.firstIndex(of: selected) ?? 0, tabs: tabs)
    }

    @MainActor
    func validate() throws {
        guard !tabs.isEmpty, tabs.count <= 256, tabs.indices.contains(selectedTabIndex),
              closedAt.timeIntervalSince1970.isFinite, title.count <= 120,
              [frame.origin.x, frame.origin.y, frame.width, frame.height].allSatisfy({ $0.isFinite }),
              frame.width > 0, frame.height > 0,
              tabs.reduce(0, { $0 + $1.stateData.count }) <= ClosedWindowStore.maximumBytes else {
            throw RecoveryError.invalidHistory
        }
        var ids = Set<UUID>()
        for tab in tabs {
            guard (TerminalRestorableState.minimumVersion...TerminalRestorableState.version)
                .contains(tab.stateVersion) else { throw RecoveryError.invalidHistory }
            // Validate the existing codec without decoding SurfaceView, which starts a shell.
            let state = try JSONDecoder().decode(
                TerminalRestorableState.InternalState<ArchivedSurface>.self, from: tab.stateData)
            guard let root = state.surfaceTree.root else { throw RecoveryError.invalidHistory }
            try Self.validate(node: root, depth: 0, ids: &ids)
            if let focused = state.focusedSurface {
                guard state.surfaceTree.contains(where: { $0.id.uuidString == focused }) else {
                    throw RecoveryError.invalidHistory
                }
            }
        }
    }

    private static func validate(node: SplitTree<ArchivedSurface>.Node, depth: Int, ids: inout Set<UUID>) throws {
        guard depth < 64, ids.count < 1024 else { throw RecoveryError.invalidHistory }
        switch node {
        case .leaf(let view):
            guard ids.insert(view.id).inserted else { throw RecoveryError.invalidHistory }
        case .split(let split):
            guard split.ratio.isFinite, split.ratio > 0, split.ratio < 1 else { throw RecoveryError.invalidHistory }
            try validate(node: split.left, depth: depth + 1, ids: &ids)
            try validate(node: split.right, depth: depth + 1, ids: &ids)
        }
    }

    private final class ArchivedSurface: NSView, Codable, Identifiable {
        let id: UUID
        let pwd: String?
        let title: String?
        let isUserSetTitle: Bool?

        enum CodingKeys: CodingKey { case uuid, pwd, title, isUserSetTitle }

        required init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            id = try container.decode(UUID.self, forKey: .uuid)
            pwd = try container.decode(String?.self, forKey: .pwd)
            title = try container.decodeIfPresent(String.self, forKey: .title)
            isUserSetTitle = try container.decodeIfPresent(Bool.self, forKey: .isUserSetTitle)
            super.init(frame: .zero)
        }

        required init?(coder: NSCoder) { nil }

        func encode(to encoder: Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(id, forKey: .uuid)
            try container.encode(pwd, forKey: .pwd)
            try container.encode(title, forKey: .title)
            try container.encode(isUserSetTitle, forKey: .isUserSetTitle)
        }
    }
}

/// Only explicit recovery remaps identities; AppKit launch restoration preserves them.
final class ClosedWindowRecoveryContext {
    static let decoderKey = CodingUserInfoKey(rawValue: "ClosedWindowRecovery")!
    private var ids: [UUID: UUID] = [:]
    var usedDirectoryFallback = false

    func validate(_ view: Ghostty.SurfaceView) throws {
        guard view.surface != nil, view.error == nil else { throw TerminalRestoreError.stateDecodeFailed }
    }

    func replacement(for id: UUID) -> UUID {
        if let replacement = ids[id] { return replacement }
        let replacement = UUID()
        ids[id] = replacement
        return replacement
    }
}
