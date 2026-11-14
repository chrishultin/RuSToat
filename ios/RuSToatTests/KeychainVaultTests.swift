import Testing
@testable import RuSToat

/// The Keychain vault against the simulator's real Keychain, under a
/// test-only service name.
struct KeychainVaultTests {
    let vault = KeychainVault(service: "com.chrishultin.rustoat.ios.tests")
    let key = "https://keychain-test.invalid#01TESTUSER"

    @Test func roundTrips() throws {
        try vault.clear(key: key)
        #expect(try vault.load(key: key) == nil)
        try vault.save(key: key, token: "first")
        #expect(try vault.load(key: key) == "first")
        try vault.save(key: key, token: "second")
        #expect(try vault.load(key: key) == "second", "saving again replaces the token")
        try vault.clear(key: key)
        #expect(try vault.load(key: key) == nil)
        try vault.clear(key: key)
    }
}
