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
    /// The signed-in account's live state.
    private(set) var accountModel: AccountModel?
    /// Shown on the login screen (e.g. why the user was signed out).
    private(set) var loginNotice: String?
    let core: Core
    @ObservationIgnored private var active = true

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
            signIn(account)
        case .needsLogin:
            if await devLogin() { return }
            phase = .signedOut
        case .offline(let account, let message):
            phase = .offline(account: account, message: message)
        }
    }

    /// Debug builds only: log in from the launch environment, for trying
    /// the app against `cargo run -p stoat-ffi --example fake_stoat`:
    ///   SIMCTL_CHILD_RUSTOAT_DEV_LOGIN='http://127.0.0.1:PORT' xcrun simctl launch …
    private func devLogin() async -> Bool {
        #if DEBUG
        guard let server = ProcessInfo.processInfo.environment["RUSTOAT_DEV_LOGIN"] else { return false }
        if case .loggedIn(let account)? = try? await core.login(server: server, email: "dev@example.com", password: "dev") {
            signIn(account)
            return true
        }
        #endif
        return false
    }

    /// A login finished (from `LoginView`).
    func loggedIn(_ account: Account) {
        loginNotice = nil
        signIn(account)
    }

    private func signIn(_ account: Account) {
        do {
            let model = try AccountModel(core: core, account: account)
            model.onSessionRejected = { [weak self] in
                Task { await self?.sessionEnded() }
            }
            accountModel = model
            if active {
                model.connect()
            }
            phase = .signedIn(account: account)
        } catch {
            loginNotice = "Couldn't start the session. Please log in again."
            phase = .signedOut
        }
    }

    /// Go to the login screen, keeping the saved session (from "offline").
    func showLogin() {
        phase = .signedOut
    }

    func logOut() async {
        accountModel?.disconnect()
        accountModel = nil
        await core.logOut()
        phase = .signedOut
    }

    /// Stoat ended the session (revoked elsewhere, or expired).
    private func sessionEnded() async {
        accountModel?.disconnect()
        accountModel = nil
        await core.forgetSession()
        loginNotice = "Your session ended. Please log in again."
        phase = .signedOut
    }

    /// The app came to the front (`true`) or went to the background:
    /// connect, or disconnect until it's back.
    func setActive(_ active: Bool) {
        self.active = active
        if active {
            accountModel?.connect()
        } else {
            accountModel?.disconnect()
        }
    }
}
