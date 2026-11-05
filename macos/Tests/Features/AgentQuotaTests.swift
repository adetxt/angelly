import AppKit
import Foundation
import SwiftUI
import Testing
@testable import Ghostty

@Suite
struct AgentQuotaTests {
    @Test func codexClassifiesWindowsByDurationAndShowsRemaining() throws {
        let snapshot = try AgentQuotaSnapshot.decode(Data("""
        {"rate_limit":{"primary_window":{"used_percent":30,"limit_window_seconds":604800,"reset_at":1800000000},
        "secondary_window":{"used_percent":80,"limit_window_seconds":18000,"reset_at":1799990000}}}
        """.utf8), provider: .codex)
        #expect(snapshot.fiveHour?.remainingPercent == 20)
        #expect(snapshot.weekly?.remainingPercent == 70)
        #expect(snapshot.ringWindow?.kind == .fiveHour)
        #expect(snapshot.fiveHour?.resetsAt == Date(timeIntervalSince1970: 1799990000))
    }

    @Test func depletedFiveHourDoesNotFallBackToWeekly() throws {
        let snapshot = try AgentQuotaSnapshot.decode(Data("""
        {"five_hour":{"utilization":100,"resets_at":null},"seven_day":{"utilization":25}}
        """.utf8), provider: .claude)
        #expect(snapshot.ringWindow?.kind == .fiveHour)
        #expect(snapshot.ringWindow?.remainingPercent == 0)
    }

    @Test(arguments: ["null", "{}", "{\"utilization\":-1}", "{\"utilization\":\"bad\"}"])
    func absentOrInvalidFiveHourUsesWeekly(window: String) throws {
        let snapshot = try AgentQuotaSnapshot.decode(Data("""
        {"five_hour":\(window),"seven_day":{"utilization":12.5,"resets_at":"2026-11-10T12:30:00.123Z"}}
        """.utf8), provider: .claude)
        #expect(snapshot.fiveHour == nil)
        #expect(snapshot.ringWindow?.kind == .weekly)
        #expect(snapshot.ringWindow?.remainingPercent == 87.5)
        #expect(snapshot.weekly?.resetsAt != nil)
    }

    @Test func invalidResetDateDoesNotDiscardQuotaAndOverageIsEmpty() throws {
        let snapshot = try AgentQuotaSnapshot.decode(Data("""
        {"five_hour":{"utilization":125,"resets_at":"invalid"}}
        """.utf8), provider: .claude)
        #expect(snapshot.ringWindow?.remainingPercent == 0)
        #expect(snapshot.ringWindow?.resetsAt == nil)
    }

    @Test(arguments: ["{}", "{\"five_hour\":null,\"seven_day\":null}"])
    func missingQuotaIsUnavailable(json: String) {
        #expect(throws: AgentQuotaError.unavailable) {
            try AgentQuotaSnapshot.decode(Data(json.utf8), provider: .claude)
        }
    }

    @Test func unrecognizedCodexDurationIsNotMislabeled() {
        #expect(throws: AgentQuotaError.unavailable) {
            try AgentQuotaSnapshot.decode(Data("""
            {"rate_limit":{"primary_window":{"used_percent":1,"limit_window_seconds":3600}}}
            """.utf8), provider: .codex)
        }
    }

    @Test func credentialsDecodeTokensAndRejectAPIKeysAndExpiry() throws {
        let codex = try AgentQuotaCredentials.decode(Data("""
        {"tokens":{"access_token":"codex-token","account_id":"account-123"}}
        """.utf8), provider: .codex)
        #expect(codex.accessToken == "codex-token")
        #expect(codex.accountID == "account-123")
        #expect(throws: AgentQuotaError.apiKeyOnly) {
            try AgentQuotaCredentials.decode(Data("{\"OPENAI_API_KEY\":\"key\"}".utf8), provider: .codex)
        }
        #expect(throws: AgentQuotaError.expired) {
            try AgentQuotaCredentials.decode(Data("""
            {"claudeAiOauth":{"accessToken":"claude-token","expiresAt":1000}}
            """.utf8), provider: .claude)
        }
        #expect(throws: AgentQuotaError.invalidCredentials) {
            try AgentQuotaCredentials.decode(Data("{\"tokens\":{}}".utf8), provider: .codex)
        }
    }

    @Test(arguments: ["{}", "{\"claudeAiOauth\":\"broken\"}"])
    func malformedClaudeCredentialsDoNotClaimAPIKeyAuthentication(json: String) {
        #expect(throws: AgentQuotaError.invalidCredentials) {
            try AgentQuotaCredentials.decode(Data(json.utf8), provider: .claude)
        }
    }

    @Test func configuredClaudeDirectoryNeverReadsDefaultKeychain() throws {
        let reader = AgentQuotaCredentialReader(environment: ["CLAUDE_CONFIG_DIR": "/profiles/work"],
                                               homeDirectory: URL(fileURLWithPath: "/test-home"), readFile: { url in
            #expect(url.path == "/profiles/work/.credentials.json")
            return nil
        }, readKeychain: { _, _, _ in
            Issue.record("Custom Claude profile must not select the default Keychain account")
            return nil
        })
        #expect(throws: AgentQuotaError.notSignedIn) { try reader.load(provider: .claude) }
    }

    @Test func malformedFileDoesNotFallThroughToAnotherCredentialStore() {
        let reader = AgentQuotaCredentialReader(environment: [:], homeDirectory: URL(fileURLWithPath: "/test-home"),
                                               readFile: { url in
            url.lastPathComponent == "auth.json" ? Data("broken".utf8) : nil
        }, readKeychain: { _, _, _ in
            Issue.record("Malformed native credentials must not substitute another source")
            return nil
        })
        #expect(throws: AgentQuotaError.invalidCredentials) { try reader.load(provider: .codex) }
    }

    @Test func codexUsesConfiguredHomeAndClaudeDecodesNativeKeychainPayload() throws {
        let reader = AgentQuotaCredentialReader(environment: ["CODEX_HOME": "/profiles/codex"],
                                               homeDirectory: URL(fileURLWithPath: "/test-home"), readFile: { url in
            if url.lastPathComponent == "config.toml" { return nil }
            #expect(url.path == "/profiles/codex/auth.json")
            return Data("{\"tokens\":{\"access_token\":\"local-token\"}}".utf8)
        }, readKeychain: { _, _, _ in
            Issue.record("File credentials should not query Keychain")
            return nil
        })
        #expect(try reader.load(provider: .codex).accessToken == "local-token")

        let claude = AgentQuotaCredentialReader(environment: [:], homeDirectory: URL(fileURLWithPath: "/test-home"),
                                               readFile: { _ in nil }, readKeychain: { service, account, interaction in
            #expect(service == "Claude Code-credentials")
            #expect(account != nil)
            #expect(!interaction)
            return Data("{\"claudeAiOauth\":{\"accessToken\":\"claude-token\",\"scopes\":[\"user:profile\"]}}".utf8)
        })
        #expect(try claude.load(provider: .claude).accessToken == "claude-token")
    }

    @Test(arguments: AgentQuotaProvider.allCases)
    func usageRequestsStayOnProviderEndpointWithRequiredHeaders(provider: AgentQuotaProvider) async throws {
        let client = AgentQuotaClient(loadCredentials: { requested, interaction in
            #expect(requested == provider)
            #expect(!interaction)
            return AgentQuotaCredentials(accessToken: "secret-fixture", accountID: "account-fixture")
        }, load: { request in
            #expect(request.httpMethod == "GET")
            #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer secret-fixture")
            #expect(request.cachePolicy == .reloadIgnoringLocalCacheData)
            let fixture: String
            if provider == .codex {
                #expect(request.url?.absoluteString == "https://chatgpt.com/backend-api/wham/usage")
                #expect(request.value(forHTTPHeaderField: "ChatGPT-Account-Id") == "account-fixture")
                fixture = "{\"rate_limit\":{\"primary_window\":{\"used_percent\":25,\"limit_window_seconds\":18000}}}"
            } else {
                #expect(request.url?.absoluteString == "https://api.anthropic.com/api/oauth/usage")
                #expect(request.value(forHTTPHeaderField: "anthropic-beta") == "oauth-2025-04-20")
                fixture = "{\"five_hour\":{\"utilization\":25}}"
            }
            let url = try #require(request.url)
            let response = try #require(HTTPURLResponse(url: url, statusCode: 200, httpVersion: nil, headerFields: nil))
            return (Data(fixture.utf8), response)
        })
        #expect(try await client.fetch(provider: provider).ringWindow?.remainingPercent == 75)
    }

    @Test(arguments: [401, 403, 429, 500, 302])
    func httpFailuresAreSanitizedAndRateLimitsRespectRetryAfter(status: Int) async throws {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let client = AgentQuotaClient(loadCredentials: { _, _ in
            AgentQuotaCredentials(accessToken: "secret-fixture", accountID: nil)
        }, load: { request in
            let url = try #require(request.url)
            let response = try #require(HTTPURLResponse(url: url, statusCode: status,
                                                       httpVersion: nil, headerFields: ["Retry-After": "600"]))
            return (Data("do not show this response or secret-fixture".utf8), response)
        }, now: { now })
        let expected: AgentQuotaError
        switch status {
        case 401: expected = .expired
        case 403: expected = .forbidden
        case 429: expected = .rateLimited(until: now.addingTimeInterval(600))
        default: expected = .unavailable
        }
        await #expect(throws: expected) { try await client.fetch(provider: .codex) }
    }

    @Test func credentialFilesAreReadFromScopedTemporaryDirectory() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try Data("{\"tokens\":{\"access_token\":\"file-fixture\"}}".utf8)
            .write(to: root.appendingPathComponent("auth.json"))
        let reader = AgentQuotaCredentialReader(environment: ["CODEX_HOME": root.path])
        #expect(try reader.load(provider: .codex).accessToken == "file-fixture")
    }

    @MainActor @Test func observersDeduplicateAndLastRemovalRejectsLateResults() async {
        let probe = QuotaFetchProbe()
        let store = AgentQuotaStore(fetch: { provider, _ in try await probe.fetch(provider) })
        let first = UUID()
        let second = UUID()
        store.observe(first)
        store.observe(second)
        let pending = store.refresh(.codex, force: true)
        await probe.waitForRequest(.codex)
        await probe.waitForRequest(.claude)
        #expect(await probe.count(.codex) == 1)
        store.stopObserving(first)
        #expect(store.state(for: .codex).isLoading)
        store.stopObserving(second)
        #expect(!store.state(for: .codex).isLoading)
        #expect(store.refresh(.codex, force: true) == nil)
        await probe.finish(.codex)
        await probe.finish(.claude)
        await pending?.value
        #expect(store.state(for: .codex).snapshot == nil)
    }

    @MainActor @Test func slowProviderDoesNotBlockOtherProvidersPublication() async {
        let probe = QuotaFetchProbe()
        let store = AgentQuotaStore(fetch: { provider, _ in try await probe.fetch(provider) })
        let observer = UUID()
        store.observe(observer)
        let codex = store.refresh(.codex)
        await probe.waitForRequest(.codex)
        await probe.waitForRequest(.claude)
        await probe.finish(.codex)
        await codex?.value
        #expect(store.state(for: .codex).snapshot?.ringWindow?.remainingPercent == 75)
        #expect(store.state(for: .claude).isLoading)
        store.stopObserving(observer)
        await probe.finish(.claude)
    }

    @MainActor @Test func manualRefreshCannotBypassProviderCooldown() async {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let retry = now.addingTimeInterval(600)
        let store = AgentQuotaStore(fetch: { provider, _ in
            if provider == .codex { throw AgentQuotaError.rateLimited(until: retry) }
            return AgentQuotaSnapshot(fiveHour: AgentQuotaWindow(kind: .fiveHour, remainingPercent: 75, resetsAt: nil), weekly: nil)
        }, now: { now })
        let observer = UUID()
        store.observe(observer)
        await store.refresh(.codex)?.value
        #expect(store.state(for: .codex).error == .rateLimited(until: retry))
        #expect(store.refresh(.codex, force: true) == nil)
        #expect(store.refresh(.codex, allowInteraction: true, force: true) == nil)
        store.stopObserving(observer)
        store.observe(observer)
        #expect(store.refresh(.codex, force: true) == nil)
        store.stopObserving(observer)
    }

    @MainActor @Test(arguments: [false, true])
    func footerFitsMinimumSidebarWidthInBothAppearances(dark: Bool) async throws {
        let store = AgentQuotaStore(fetch: { provider, _ in
            let window = AgentQuotaWindow(kind: provider == .codex ? .fiveHour : .weekly,
                                          remainingPercent: provider == .codex ? 75 : 25, resetsAt: nil)
            return AgentQuotaSnapshot(fiveHour: provider == .codex ? window : nil,
                                      weekly: provider == .claude ? window : nil)
        })
        let observer = UUID()
        store.observe(observer)
        await store.refresh(.codex)?.value
        await store.refresh(.claude)?.value
        defer { store.stopObserving(observer) }
        let view = NSHostingView(rootView: AgentQuotaFooter(store: store)
            .background(dark ? Color(white: 0.2) : .white)
            .environment(\.colorScheme, dark ? .dark : .light))
        view.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        #expect(view.fittingSize.width <= 160)
        view.frame = NSRect(x: 0, y: 0, width: 160, height: 36)
        view.layoutSubtreeIfNeeded()
        let bitmap = try #require(view.bitmapImageRepForCachingDisplay(in: view.bounds))
        view.cacheDisplay(in: view.bounds, to: bitmap)
        let png = try #require(bitmap.representation(using: .png, properties: [:]))
        try png.write(to: FileManager.default.temporaryDirectory
            .appendingPathComponent("angelly-quota-footer-\(dark ? "dark" : "light").png"))
    }
}

private actor QuotaFetchProbe {
    private var requests: [AgentQuotaProvider: CheckedContinuation<AgentQuotaSnapshot, any Error>] = [:]
    private var waiters: [AgentQuotaProvider: CheckedContinuation<Void, Never>] = [:]
    private var counts: [AgentQuotaProvider: Int] = [:]

    func fetch(_ provider: AgentQuotaProvider) async throws -> AgentQuotaSnapshot {
        counts[provider, default: 0] += 1
        return try await withCheckedThrowingContinuation { continuation in
            requests[provider] = continuation
            waiters.removeValue(forKey: provider)?.resume()
        }
    }

    func waitForRequest(_ provider: AgentQuotaProvider) async {
        if requests[provider] != nil { return }
        await withCheckedContinuation { waiters[provider] = $0 }
    }

    func count(_ provider: AgentQuotaProvider) -> Int { counts[provider, default: 0] }

    func finish(_ provider: AgentQuotaProvider) {
        requests.removeValue(forKey: provider)?.resume(returning: AgentQuotaSnapshot(
            fiveHour: AgentQuotaWindow(kind: .fiveHour, remainingPercent: 75, resetsAt: nil), weekly: nil))
    }
}
