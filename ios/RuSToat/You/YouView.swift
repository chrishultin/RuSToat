import SwiftUI

/// The signed-in user: who they are, which server, and Log Out.
struct YouView: View {
    let account: Account
    let accountModel: AccountModel
    let onLogOut: () async -> Void
    @State private var confirming = false
    @State private var loggingOut = false

    var body: some View {
        NavigationStack {
            List {
                Section {
                    HStack(spacing: 14) {
                        AvatarView(url: accountModel.me?.avatarUrl, name: name, size: 64)
                        VStack(alignment: .leading, spacing: 3) {
                            Text(name).font(.title3.bold())
                            if let me = accountModel.me {
                                Text(me.username).font(.subheadline).foregroundStyle(.secondary)
                                Label(me.status ?? me.presence, systemImage: "circle.fill")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .symbolRenderingMode(.palette)
                            }
                        }
                    }
                    .padding(.vertical, 6)
                }
                Section("Server") {
                    LabeledContent("Address", value: account.instance)
                    LabeledContent("Connection", value: connection)
                }
                Section {
                    Button("Log Out", role: .destructive) { confirming = true }
                        .disabled(loggingOut)
                } footer: {
                    Text("RuSToat \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "") · core \(coreVersion())")
                }
            }
            .navigationTitle("You")
            .confirmationDialog("Log out of \(name)?", isPresented: $confirming, titleVisibility: .visible) {
                Button("Log Out", role: .destructive) {
                    loggingOut = true
                    Task { await onLogOut() }
                }
            } message: {
                Text("This ends the session on this iPhone.")
            }
        }
    }

    private var name: String {
        accountModel.me?.name ?? account.name
    }

    private var connection: String {
        switch accountModel.status {
        case .connected: "Connected"
        case .connecting(let attempt): attempt <= 1 ? "Connecting…" : "Reconnecting…"
        case .disconnected: "Offline"
        case .paused: "Paused"
        case .sessionRejected: "Session ended"
        }
    }
}
