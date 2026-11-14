import Foundation
import Observation

/// The signed-in account's live state, for SwiftUI. The Rust controller
/// holds the store; this keeps the snapshots the screens draw, read again
/// whenever the controller says something changed.
@MainActor
@Observable
final class AccountModel {
    let account: Account
    private(set) var status: ConnectionStatus = .connecting(attempt: 1)
    private(set) var servers: [ServerItem] = []
    private(set) var home: [ChannelItem] = []
    private(set) var summary = Summary(homeUnread: false, homeMentions: 0, serversUnread: false, serversMentions: 0)
    /// Bumped when the lists change, so views that read the controller
    /// directly (one server's channels) are drawn again.
    private(set) var listsVersion = 0
    /// Bumped per channel when its loaded messages change.
    private(set) var timelineVersions: [String: Int] = [:]
    /// A `Ready` replaced everything (on a reconnect, loaded messages too).
    private(set) var readyCount = 0

    @ObservationIgnored private var controller: AccountController?
    /// Called when Stoat ends the session.
    @ObservationIgnored var onSessionRejected: (() -> Void)?

    init(core: Core, account: Account) throws {
        self.account = account
        let listener = ListenerBridge(
            onChange: { [weak self] change in
                Task { @MainActor in self?.apply(change) }
            },
            onStatus: { [weak self] status in
                Task { @MainActor in self?.apply(status) }
            }
        )
        controller = try AccountController(core: core, listener: listener)
    }

    /// The controller, for the chat screens.
    var core: AccountController { controller! }

    func connect() {
        controller?.connect()
    }

    func disconnect() {
        controller?.disconnect()
    }

    func channels(of server: String) -> [ChannelSectionItem] {
        _ = listsVersion
        return controller?.serverChannels(serverId: server) ?? []
    }

    func server(_ id: String) -> ServerItem? {
        _ = listsVersion
        return controller?.server(serverId: id)
    }

    func channel(_ id: String) -> ChannelItem? {
        _ = listsVersion
        return controller?.channel(channelId: id)
    }

    private func apply(_ change: Change) {
        if change.ready {
            readyCount += 1
        }
        if change.lists || change.ready {
            refreshLists()
        }
        for channel in change.timelines {
            timelineVersions[channel, default: 0] += 1
        }
    }

    private func apply(_ status: ConnectionStatus) {
        self.status = status
        if status == .sessionRejected {
            onSessionRejected?()
        }
    }

    private func refreshLists() {
        guard let controller else { return }
        servers = controller.servers()
        home = controller.homeChannels()
        summary = controller.summary()
        listsVersion += 1
    }
}

/// The controller's listener: hands each call to the model on the main actor.
private final class ListenerBridge: ControllerListener {
    let onChange: @Sendable (Change) -> Void
    let onStatus: @Sendable (ConnectionStatus) -> Void

    init(onChange: @escaping @Sendable (Change) -> Void, onStatus: @escaping @Sendable (ConnectionStatus) -> Void) {
        self.onChange = onChange
        self.onStatus = onStatus
    }

    func changed(change: Change) { onChange(change) }
    func status(status: ConnectionStatus) { onStatus(status) }
}
