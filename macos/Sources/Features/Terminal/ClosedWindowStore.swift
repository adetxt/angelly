import Foundation

@MainActor
final class ClosedWindowStore {
    static let maximumBytes = 10 * 1024 * 1024
    private static let maximumWindows = 20

    final class CloseTicket {
        let id: UUID
        var isRestored = false

        init(id: UUID) { self.id = id }
    }

    private struct History: Codable {
        let version: Int
        let windows: [ClosedWindowSnapshot]
    }

    let fileURL: URL
    private(set) var isPersistenceEnabled = false
    private(set) var loadFailure: Error?
    private var snapshots: [ClosedWindowSnapshot] = []
    private var tickets: [UUID: CloseTicket] = [:]

    var hasHistory: Bool { !snapshots.isEmpty || loadFailure != nil }

    var entries: [ClosedWindowSnapshot] {
        guard isPersistenceEnabled, loadFailure == nil else { return [] }
        return snapshots.filter { tickets[$0.id]?.isRestored != true }
    }

    init(directoryURL: URL) {
        fileURL = directoryURL.appendingPathComponent("recently-closed-windows.json")
    }

    func setPersistenceEnabled(_ enabled: Bool) throws {
        if enabled {
            guard !isPersistenceEnabled else { return }
            isPersistenceEnabled = true
            try load()
        } else {
            isPersistenceEnabled = false
            try clear()
        }
    }

    func load() throws {
        guard isPersistenceEnabled else { return }
        do {
            guard FileManager.default.fileExists(atPath: fileURL.path) else {
                snapshots = []
                loadFailure = nil
                return
            }
            let attributes = try FileManager.default.attributesOfItem(atPath: fileURL.path)
            guard attributes[.type] as? FileAttributeType == .typeRegular,
                  let size = attributes[.size] as? NSNumber, size.intValue <= Self.maximumBytes else {
                throw ClosedWindowSnapshot.RecoveryError.invalidHistory
            }
            let data = try Data(contentsOf: fileURL)
            guard data.count <= Self.maximumBytes else { throw ClosedWindowSnapshot.RecoveryError.invalidHistory }
            let history = try JSONDecoder().decode(History.self, from: data)
            guard history.version == 1, history.windows.count <= Self.maximumWindows,
                  Set(history.windows.map(\.id)).count == history.windows.count else {
                throw ClosedWindowSnapshot.RecoveryError.invalidHistory
            }
            for snapshot in history.windows { try snapshot.validate() }
            snapshots = history.windows
            loadFailure = nil
        } catch {
            loadFailure = error
            throw error
        }
    }

    func append(_ snapshot: ClosedWindowSnapshot) throws -> CloseTicket {
        guard isPersistenceEnabled else { throw ClosedWindowSnapshot.RecoveryError.disabled }
        if let loadFailure { throw loadFailure }
        try snapshot.validate()
        guard try encoded([snapshot]).count <= Self.maximumBytes else {
            throw ClosedWindowSnapshot.RecoveryError.oversized
        }
        var next = [snapshot] + entries.filter { $0.id != snapshot.id }
        next = Array(next.prefix(Self.maximumWindows))
        while try encoded(next).count > Self.maximumBytes { next.removeLast() }
        try commit(next)
        let ticket = CloseTicket(id: snapshot.id)
        tickets = tickets.filter { id, _ in next.contains { $0.id == id } }
        tickets[snapshot.id] = ticket
        return ticket
    }

    func ticket(for id: UUID) -> CloseTicket {
        if let ticket = tickets[id] { return ticket }
        let ticket = CloseTicket(id: id)
        tickets[id] = ticket
        return ticket
    }

    func consume(id: UUID) throws {
        ticket(for: id).isRestored = true
        guard isPersistenceEnabled, snapshots.contains(where: { $0.id == id }) else { return }
        try commit(snapshots.filter { $0.id != id })
        tickets[id] = nil
    }

    func clear() throws {
        snapshots = []
        tickets = [:]
        loadFailure = nil
        if FileManager.default.fileExists(atPath: fileURL.path) {
            do { try FileManager.default.removeItem(at: fileURL) } catch {
                loadFailure = error
                throw error
            }
        }
    }

    private func encoded(_ windows: [ClosedWindowSnapshot]) throws -> Data {
        try JSONEncoder().encode(History(version: 1, windows: windows))
    }

    private func commit(_ windows: [ClosedWindowSnapshot]) throws {
        let manager = FileManager.default
        let directory = fileURL.deletingLastPathComponent()
        try manager.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
        let temporary = directory.appendingPathComponent(UUID().uuidString)
        defer { try? manager.removeItem(at: temporary) }
        guard manager.createFile(atPath: temporary.path, contents: try encoded(windows), attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteUnknown)
        }
        guard rename(temporary.path, fileURL.path) == 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        snapshots = windows
    }
}
