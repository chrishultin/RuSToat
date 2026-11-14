import Testing
@testable import RuSToat

/// Calls across the Swift → Rust bridge.
struct CoreBridgeTests {
    @Test func normalizesInstanceAddresses() throws {
        #expect(try normalizeInstance(input: "stoat.example.com") == "https://stoat.example.com")
    }

    @Test func blankAddressesThrowTheCoreMessage() {
        #expect(throws: CoreError.self) { try normalizeInstance(input: "  ") }
        do {
            _ = try normalizeInstance(input: "  ")
        } catch let CoreError.Failed(message) {
            #expect(message == "Enter the address of a Stoat server, like stoat.example.com.")
        } catch {
            Issue.record("unexpected error \(error)")
        }
    }

    /// The Rust side checks this against stoat-ffi's Cargo.toml.
    @Test func coreVersionIsTheCrateVersion() {
        #expect(coreVersion() == "0.1.0")
    }
}
