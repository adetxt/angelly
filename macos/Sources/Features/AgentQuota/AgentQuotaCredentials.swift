import CryptoKit
import Foundation
import LocalAuthentication
import Security

struct AgentQuotaCredentials: Sendable {
    let accessToken: String
    let accountID: String?

    static func decode(_ data: Data, provider: AgentQuotaProvider, now: Date = Date()) throws -> Self {
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AgentQuotaError.invalidCredentials
        }
        let token: String?
        var accountID: String?
        switch provider {
        case .codex:
            if root["auth_mode"] as? String == "apikey" { throw AgentQuotaError.apiKeyOnly }
            let tokens = root["tokens"] as? [String: Any] ?? [:]
            token = tokens["access_token"] as? String
            if token == nil, root["OPENAI_API_KEY"] is String { throw AgentQuotaError.apiKeyOnly }
            let claims = token.flatMap(jwtClaims)
            if let expiry = claims?["exp"] as? Double, expiry <= now.timeIntervalSince1970 {
                throw AgentQuotaError.expired
            }
            let identity = (tokens["id_token"] as? String).flatMap(jwtClaims) ?? claims
            let auth = identity?["https://api.openai.com/auth"] as? [String: Any]
            accountID = tokens["account_id"] as? String ?? auth?["chatgpt_account_id"] as? String
        case .claude:
            guard let oauth = root["claudeAiOauth"] as? [String: Any] else {
                if let apiKey = root["apiKey"] as? String, !apiKey.isEmpty { throw AgentQuotaError.apiKeyOnly }
                throw AgentQuotaError.invalidCredentials
            }
            token = oauth["accessToken"] as? String
            if let expiry = oauth["expiresAt"] as? Double, expiry / 1000 <= now.timeIntervalSince1970 {
                throw AgentQuotaError.expired
            }
        }
        guard let token, !token.isEmpty, !token.contains(where: { $0.isWhitespace || $0.isNewline }),
              accountID?.contains(where: { $0.isWhitespace || $0.isNewline }) != true else {
            throw AgentQuotaError.invalidCredentials
        }
        return Self(accessToken: token, accountID: accountID)
    }

    private static func jwtClaims(_ token: String) -> [String: Any]? {
        let parts = token.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 3 else { return nil }
        var base64 = parts[1].replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        base64 += String(repeating: "=", count: (4 - base64.count % 4) % 4)
        guard let data = Data(base64Encoded: base64) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }
}

struct AgentQuotaCredentialReader: Sendable {
    var environment = ProcessInfo.processInfo.environment
    var homeDirectory = FileManager.default.homeDirectoryForCurrentUser
    var readFile: @Sendable (URL) throws -> Data? = { try Self.fileData($0) }
    var readKeychain: @Sendable (String, String?, Bool) throws -> Data? = {
        try Self.keychainData(service: $0, account: $1, allowInteraction: $2)
    }

    func load(provider: AgentQuotaProvider, allowInteraction: Bool = false) throws -> AgentQuotaCredentials {
        let data: Data?
        switch provider {
        case .codex:
            let root = directory(environment["CODEX_HOME"], fallback: homeDirectory.appendingPathComponent(".codex"))
            let mode = try codexStorage(root)
            if mode == "file" {
                data = try readFile(root.appendingPathComponent("auth.json"))
            } else {
                let canonical = root.resolvingSymlinksInPath().path
                let digest = SHA256.hash(data: Data(canonical.utf8)).map { String(format: "%02x", $0) }.joined()
                let keychain = try readKeychain("Codex Auth", "cli|\(digest.prefix(16))", allowInteraction)
                if keychain == nil, mode == "auto" {
                    data = try readFile(root.appendingPathComponent("auth.json"))
                } else {
                    data = keychain
                }
            }
        case .claude:
            let defaultRoot = homeDirectory.appendingPathComponent(".claude")
            let configRoot = directory(environment["CLAUDE_CONFIG_DIR"], fallback: defaultRoot)
            let root = directory(environment["CLAUDE_SECURESTORAGE_CONFIG_DIR"], fallback: configRoot)
            // Claude's shared Keychain service does not identify a custom config directory.
            if root.standardizedFileURL == defaultRoot.standardizedFileURL,
               let native = try readKeychain("Claude Code-credentials", NSUserName(), allowInteraction) {
                data = native
            } else {
                data = try readFile(root.appendingPathComponent(".credentials.json"))
            }
        }
        guard let data else { throw AgentQuotaError.notSignedIn }
        return try AgentQuotaCredentials.decode(data, provider: provider)
    }

    private func directory(_ path: String?, fallback: URL) -> URL {
        guard let path, !path.isEmpty else { return fallback }
        return URL(fileURLWithPath: path, isDirectory: true).standardizedFileURL
    }

    private func codexStorage(_ root: URL) throws -> String {
        guard let data = try readFile(root.appendingPathComponent("config.toml")),
              let text = String(data: data, encoding: .utf8) else { return "file" }
        for line in text.components(separatedBy: .newlines) {
            let content = line.trimmingCharacters(in: .whitespaces)
            if content.hasPrefix("[") { break }
            let parts = content.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            guard parts.count == 2, parts[0] == "cli_auth_credentials_store" else { continue }
            let value = parts[1].split(separator: "#", maxSplits: 1).first.map(String.init) ?? ""
            let mode = value.trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))
            guard ["file", "keyring", "auto"].contains(mode) else { throw AgentQuotaError.invalidCredentials }
            return mode
        }
        return "file"
    }

    private static func fileData(_ url: URL) throws -> Data? {
        do { return try Data(contentsOf: url) } catch CocoaError.fileReadNoSuchFile { return nil } catch {
            throw AgentQuotaError.invalidCredentials
        }
    }

    private static let claudeKeychainService = "Claude Code-credentials"

    /// Claude Code writes its item with `/usr/bin/security`, so that tool is already trusted by the
    /// item's access list. Reading through it avoids the Keychain password prompt a direct
    /// `SecItemCopyMatching` from this app triggers.
    private static func securityToolData(service: String, account: String?) throws -> Data? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        process.arguments = ["find-generic-password", "-s", service] + (account.map { ["-a", $0] } ?? []) + ["-w"]
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do { try process.run() } catch { throw AgentQuotaError.invalidCredentials }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        switch process.terminationStatus {
        case 0: return data
        case 44: return nil // errSecItemNotFound
        default: throw AgentQuotaError.permissionRequired
        }
    }

    private static func keychainData(service: String, account: String?, allowInteraction: Bool) throws -> Data? {
        if service == claudeKeychainService { return try securityToolData(service: service, account: account) }
        let context = LAContext()
        context.interactionNotAllowed = !allowInteraction
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
            kSecUseAuthenticationContext as String: context,
        ]
        if let account { query[kSecAttrAccount as String] = account }
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            guard let data = result as? Data else { throw AgentQuotaError.invalidCredentials }
            return data
        case errSecItemNotFound: return nil
        case errSecInteractionNotAllowed, errSecAuthFailed, errSecUserCanceled: throw AgentQuotaError.permissionRequired
        default: throw AgentQuotaError.invalidCredentials
        }
    }
}
