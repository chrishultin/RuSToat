import Foundation
import Security

/// Session tokens in the iOS Keychain: one generic-password item per account,
/// named by the account's ID (`{API URL}#{user ID}`). The Rust core calls this
/// from its own threads; Keychain Services is thread-safe.
final class KeychainVault: TokenVault {
    /// The Keychain service for this app's tokens (its bundle identifier).
    static let service = "com.chrishultin.rustoat.ios"

    let service: String

    init(service: String = KeychainVault.service) {
        self.service = service
    }

    private func query(_ key: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
    }

    func load(key: String) throws -> String? {
        var query = query(key)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            guard let data = result as? Data else { return nil }
            return String(data: data, encoding: .utf8)
        case errSecItemNotFound:
            return nil
        default:
            throw CoreError.Failed(message: Self.describe(status, "reading the session from the Keychain"))
        }
    }

    func save(key: String, token: String) throws {
        let data = Data(token.utf8)
        let update: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlock,
        ]
        var status = SecItemUpdate(query(key) as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            let add = query(key).merging(update) { _, new in new }
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else {
            throw CoreError.Failed(message: Self.describe(status, "saving the session to the Keychain"))
        }
    }

    func clear(key: String) throws {
        let status = SecItemDelete(query(key) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw CoreError.Failed(message: Self.describe(status, "deleting the session from the Keychain"))
        }
    }

    private static func describe(_ status: OSStatus, _ doing: String) -> String {
        let reason = SecCopyErrorMessageString(status, nil) as String? ?? "error \(status)"
        return "Failed \(doing): \(reason)"
    }
}
