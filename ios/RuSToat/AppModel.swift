import Foundation
import Observation

/// The app's sign-in state, driving which screen is shown.
@MainActor
@Observable
final class AppModel {
    enum Phase: Equatable {
        /// Checking the saved session at launch.
        case restoring
        /// A saved session that couldn't be checked; `message` says why.
        case offline(account: Account, message: String)
        /// Nothing saved, the session was rejected, or the user logged out.
        case signedOut
        case signedIn(account: Account)
    }

    private(set) var phase: Phase = .restoring
    let core: Core

    init(core: Core) {
        self.core = core
    }

    /// The real core: tokens in the Keychain, the account list in
    /// Application Support.
    static func live() -> AppModel {
        let support = URL.applicationSupportDirectory
        try? FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        return AppModel(core: Core(vault: KeychainVault(), dataDir: support.path(percentEncoded: false)))
    }

    /// Sign in to the saved account, if there is one.
    func restore() async {
        phase = .restoring
        switch await core.restore() {
        case .signedIn(let account):
            phase = .signedIn(account: account)
        case .needsLogin:
            phase = .signedOut
        case .offline(let account, let message):
            phase = .offline(account: account, message: message)
        }
    }

    /// A login finished (from `LoginView`).
    func loggedIn(_ account: Account) {
        phase = .signedIn(account: account)
    }

    /// Go to the login screen, keeping the saved session (from "offline").
    func showLogin() {
        phase = .signedOut
    }

    func logOut() async {
        await core.logOut()
        phase = .signedOut
    }
}
