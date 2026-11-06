import Foundation

struct AgentQuotaClient: Sendable {
    var loadCredentials: @Sendable (AgentQuotaProvider, Bool) async throws -> AgentQuotaCredentials = { provider, interaction in
        let task = Task.detached(priority: .utility) {
            try Task.checkCancellation()
            return try AgentQuotaCredentialReader().load(provider: provider, allowInteraction: interaction)
        }
        return try await withTaskCancellationHandler {
            try await task.value
        } onCancel: {
            task.cancel()
        }
    }
    var load: @Sendable (URLRequest) async throws -> (Data, URLResponse) = { try await session.data(for: $0) }
    var now: @Sendable () -> Date = { Date() }

    func fetch(provider: AgentQuotaProvider, allowInteraction: Bool = false) async throws -> AgentQuotaSnapshot {
        do {
            let credentials = try await loadCredentials(provider, allowInteraction)
            try Task.checkCancellation()
            var request = URLRequest(url: provider.endpoint, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10)
            request.httpMethod = "GET"
            request.setValue("Bearer \(credentials.accessToken)", forHTTPHeaderField: "Authorization")
            request.setValue("application/json", forHTTPHeaderField: "Accept")
            request.setValue("Angelly", forHTTPHeaderField: "User-Agent")
            switch provider {
            case .codex:
                if let account = credentials.accountID, !account.isEmpty {
                    request.setValue(account, forHTTPHeaderField: "ChatGPT-Account-Id")
                }
            case .claude:
                request.setValue("oauth-2025-04-20", forHTTPHeaderField: "anthropic-beta")
            }
            let (data, response) = try await load(request)
            try Task.checkCancellation()
            guard let response = response as? HTTPURLResponse else { throw AgentQuotaError.unavailable }
            switch response.statusCode {
            case 200:
                guard data.count <= 1_048_576 else { throw AgentQuotaError.unavailable }
                return try AgentQuotaSnapshot.decode(data, provider: provider)
            case 401: throw AgentQuotaError.expired
            case 403: throw AgentQuotaError.forbidden
            case 429: throw AgentQuotaError.rateLimited(until: retryDate(response.value(forHTTPHeaderField: "Retry-After")))
            default: throw AgentQuotaError.unavailable
            }
        } catch is CancellationError {
            throw CancellationError()
        } catch let error as AgentQuotaError {
            throw error
        } catch {
            if Task.isCancelled { throw CancellationError() }
            throw AgentQuotaError.network
        }
    }

    private func retryDate(_ header: String?) -> Date {
        let now = now()
        if let header {
            if let seconds = Double(header), seconds.isFinite, seconds >= 0 {
                return now.addingTimeInterval(max(1, seconds))
            }
            let formatter = DateFormatter()
            formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.timeZone = TimeZone(secondsFromGMT: 0)
            formatter.dateFormat = "EEE, dd MMM yyyy HH:mm:ss zzz"
            if let date = formatter.date(from: header), date > now { return date }
        }
        return now.addingTimeInterval(300)
    }

    private final class RedirectDelegate: NSObject, URLSessionTaskDelegate {
        func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                        newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
            // Bearer tokens stay on the fixed provider endpoint.
            completionHandler(nil)
        }
    }

    private static let session: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 10
        configuration.timeoutIntervalForResource = 15
        configuration.httpShouldSetCookies = false
        configuration.urlCredentialStorage = nil
        return URLSession(configuration: configuration, delegate: RedirectDelegate(), delegateQueue: nil)
    }()
}
