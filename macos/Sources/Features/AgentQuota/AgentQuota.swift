import Foundation

enum AgentQuotaPreferences {
    static let visibilityKey = "ShowAIQuotaTracker"
}

enum AgentQuotaProvider: String, CaseIterable, Identifiable, Sendable {
    case codex, claude

    var id: Self { self }
    var name: String { self == .codex ? "Codex" : "Claude Code" }
    var logo: String { self == .codex ? "AgentQuotaCodex" : "AgentQuotaClaude" }
    var loginCommand: String { self == .codex ? "codex login" : "claude auth login" }
    var endpoint: URL {
        switch self {
        case .codex: URL(string: "https://chatgpt.com/backend-api/wham/usage")!
        case .claude: URL(string: "https://api.anthropic.com/api/oauth/usage")!
        }
    }
}

enum AgentQuotaError: Error, Equatable, Sendable {
    case notSignedIn, invalidCredentials, apiKeyOnly, expired, permissionRequired, forbidden, unavailable, network
    case rateLimited(until: Date)

    func message(for provider: AgentQuotaProvider) -> String {
        switch self {
        case .notSignedIn: "No local login found. Run \(provider.loginCommand) in your terminal, then refresh."
        case .invalidCredentials: "Couldn't read the local login. Sign in with \(provider.loginCommand), then refresh."
        case .apiKeyOnly: "Subscription quota isn't available for API-key logins."
        case .expired: "The local login has expired. Open \(provider.name) to renew it, then refresh."
        case .permissionRequired: "Allow Keychain access to read your existing \(provider.name) login."
        case .forbidden: "This login can't access subscription quota. Check your subscription and sign in again."
        case .unavailable: "Quota isn't available in this response. Try refreshing later."
        case .network: "Couldn't reach \(provider.name). Check your connection, then refresh."
        case .rateLimited: "Too many quota requests. Refresh will be available after the cooldown."
        }
    }
}

struct AgentQuotaWindow: Equatable, Sendable {
    enum Kind: Sendable {
        case fiveHour, weekly
        var label: String { self == .fiveHour ? "5-hour" : "Weekly" }
    }

    let kind: Kind
    let remainingPercent: Double
    let resetsAt: Date?
}

struct AgentQuotaSnapshot: Equatable, Sendable {
    let fiveHour: AgentQuotaWindow?
    let weekly: AgentQuotaWindow?
    var ringWindow: AgentQuotaWindow? { fiveHour ?? weekly }

    static func decode(_ data: Data, provider: AgentQuotaProvider) throws -> Self {
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AgentQuotaError.unavailable
        }
        var fiveHour: AgentQuotaWindow?
        var weekly: AgentQuotaWindow?
        switch provider {
        case .codex:
            let limits = root["rate_limit"] as? [String: Any] ?? [:]
            for key in ["primary_window", "secondary_window"] {
                guard let raw = limits[key] as? [String: Any],
                      let duration = number(raw["limit_window_seconds"]),
                      let used = number(raw["used_percent"]), used >= 0 else { continue }
                let reset = number(raw["reset_at"]).flatMap { $0 > 0 ? Date(timeIntervalSince1970: $0) : nil }
                if duration == 18_000 {
                    fiveHour = AgentQuotaWindow(kind: .fiveHour, remainingPercent: max(0, 100 - used), resetsAt: reset)
                } else if duration == 604_800 {
                    weekly = AgentQuotaWindow(kind: .weekly, remainingPercent: max(0, 100 - used), resetsAt: reset)
                }
            }
        case .claude:
            func window(_ key: String, kind: AgentQuotaWindow.Kind) -> AgentQuotaWindow? {
                guard let raw = root[key] as? [String: Any],
                      let used = number(raw["utilization"]), used >= 0 else { return nil }
                let reset = (raw["resets_at"] as? String).flatMap(parseDate)
                return AgentQuotaWindow(kind: kind, remainingPercent: max(0, 100 - used), resetsAt: reset)
            }
            fiveHour = window("five_hour", kind: .fiveHour)
            weekly = window("seven_day", kind: .weekly)
        }
        guard fiveHour != nil || weekly != nil else { throw AgentQuotaError.unavailable }
        return Self(fiveHour: fiveHour, weekly: weekly)
    }

    private static func number(_ value: Any?) -> Double? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.isFinite else { return nil }
        return number.doubleValue
    }

    private static func parseDate(_ text: String) -> Date? {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = formatter.date(from: text) { return date }
        formatter.formatOptions = [.withInternetDateTime]
        return formatter.date(from: text)
    }
}
