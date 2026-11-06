import Combine
import Foundation

struct AgentQuotaState {
    var snapshot: AgentQuotaSnapshot?
    var error: AgentQuotaError?
    var lastUpdated: Date?
    var isLoading = false

    var retryAfter: Date? {
        if case .rateLimited(let until) = error { return until }
        return nil
    }
}

@MainActor
final class AgentQuotaStore: ObservableObject {
    static let shared = AgentQuotaStore()

    @Published private(set) var states: [AgentQuotaProvider: AgentQuotaState] = [:]

    private struct Request {
        let id: UUID
        let task: Task<Void, Never>
    }

    private let fetch: @Sendable (AgentQuotaProvider, Bool) async throws -> AgentQuotaSnapshot
    private let now: @Sendable () -> Date
    private var observers: Set<UUID> = []
    private var requests: [AgentQuotaProvider: Request] = [:]
    private var lastAttempt: [AgentQuotaProvider: Date] = [:]
    private var polling: Task<Void, Never>?

    init(
        fetch: @escaping @Sendable (AgentQuotaProvider, Bool) async throws -> AgentQuotaSnapshot = {
            try await AgentQuotaClient().fetch(provider: $0, allowInteraction: $1)
        },
        now: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.fetch = fetch
        self.now = now
    }

    deinit {
        polling?.cancel()
        for request in requests.values { request.task.cancel() }
    }

    func state(for provider: AgentQuotaProvider) -> AgentQuotaState { states[provider] ?? AgentQuotaState() }

    func observe(_ id: UUID) {
        guard observers.insert(id).inserted, observers.count == 1 else { return }
        for provider in AgentQuotaProvider.allCases { refresh(provider) }
        polling = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(nanoseconds: 300_000_000_000) } catch { return }
                for provider in AgentQuotaProvider.allCases { self?.refresh(provider) }
            }
        }
    }

    func stopObserving(_ id: UUID) {
        guard observers.remove(id) != nil, observers.isEmpty else { return }
        polling?.cancel()
        polling = nil
        for (provider, request) in requests {
            request.task.cancel()
            states[provider, default: AgentQuotaState()].isLoading = false
        }
        // A request may complete after cancellation, including an in-progress Keychain read.
        requests.removeAll()
    }

    @discardableResult
    func refresh(_ provider: AgentQuotaProvider, allowInteraction: Bool = false, force: Bool = false) -> Task<Void, Never>? {
        guard !observers.isEmpty else { return nil }
        if let pending = requests[provider] { return pending.task }
        let state = state(for: provider)
        let now = now()
        if let retry = state.retryAfter, retry > now { return nil }
        if !force, state.snapshot != nil || state.error != nil,
           let attempt = lastAttempt[provider], now.timeIntervalSince(attempt) < 300 { return nil }

        let id = UUID()
        let fetch = fetch
        lastAttempt[provider] = now
        states[provider, default: AgentQuotaState()].isLoading = true
        states[provider, default: AgentQuotaState()].snapshot = nil
        states[provider, default: AgentQuotaState()].error = nil
        let task = Task { [weak self] in
            let result: Result<AgentQuotaSnapshot, any Error>
            do { result = .success(try await fetch(provider, allowInteraction)) } catch { result = .failure(error) }
            guard let self, self.requests[provider]?.id == id else { return }
            self.requests[provider] = nil
            self.states[provider, default: AgentQuotaState()].isLoading = false
            switch result {
            case .success(let snapshot):
                self.states[provider, default: AgentQuotaState()].snapshot = snapshot
                self.states[provider, default: AgentQuotaState()].lastUpdated = self.now()
            case .failure(let error):
                self.states[provider, default: AgentQuotaState()].error = error as? AgentQuotaError ?? .network
            }
        }
        requests[provider] = Request(id: id, task: task)
        return task
    }
}
