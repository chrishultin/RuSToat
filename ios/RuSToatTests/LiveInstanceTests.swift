import Foundation
import Testing
@testable import RuSToat

/// Talks to the real api.stoat.chat, so it's opt-in:
///   TEST_RUNNER_RUSTOAT_LIVE=1 xcodebuild … test
/// Checks that HTTPS works from iOS (rustls with the platform verifier) and
/// that Stoat's answers parse, using an address that can't have an account.
@Suite(.enabled(if: ProcessInfo.processInfo.environment["RUSTOAT_LIVE"] == "1"))
struct LiveInstanceTests {
    @Test func aBogusLoginIsRefusedWithDesktopsText() async throws {
        let core = Core(vault: KeychainVault(service: "com.chrishultin.rustoat.ios.tests"),
                        dataDir: FileManager.default.temporaryDirectory.path(percentEncoded: false))
        do {
            _ = try await core.login(server: "api.stoat.chat",
                                     email: "nobody@rustoat-test.invalid",
                                     password: "not-a-real-password")
            Issue.record("a bogus login succeeded")
        } catch let CoreError.Failed(message) {
            #expect(message == "Incorrect email or password.")
        }
    }
}
