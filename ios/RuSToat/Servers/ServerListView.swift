import SwiftUI

/// Where the Servers tab can go: a server's channels, then a channel.
enum ServersRoute: Hashable {
    case server(String)
    case channel(ChannelRoute)
}

/// The user's servers, in their order.
struct ServerListView: View {
    let model: AccountModel
    @State private var path: [ServersRoute] = []
    /// "server ID" or "server ID/channel ID", to reopen at the next launch.
    @SceneStorage("serverPlace") private var savedPlace = ""
    @State private var restored = false

    var body: some View {
        NavigationStack(path: $path) {
            List(model.servers, id: \.id) { server in
                NavigationLink(value: ServersRoute.server(server.id)) {
                    HStack(spacing: 12) {
                        AvatarView(url: server.iconUrl, name: server.name, size: 40, rounded: true)
                        Text(server.name)
                            .font(server.unread ? .body.weight(.semibold) : .body)
                            .lineLimit(1)
                        Spacer(minLength: 8)
                        UnreadBadge(unread: server.unread, mentions: server.mentions)
                    }
                }
            }
            .overlay {
                if model.servers.isEmpty && model.readyCount > 0 {
                    ContentUnavailableView("No servers", systemImage: "square.stack.3d.up",
                                           description: Text("Servers you join appear here."))
                }
            }
            .navigationTitle("Servers")
            .navigationDestination(for: ServersRoute.self) { route in
                switch route {
                case .server(let id):
                    ServerChannelsView(model: model, serverId: id)
                case .channel(let channel):
                    ChatView(model: model, route: channel)
                }
            }
        }
        .onChange(of: path) {
            savedPlace = path.map { route in
                switch route {
                case .server(let id): id
                case .channel(let channel): channel.id
                }
            }.joined(separator: "/")
        }
        .onChange(of: model.readyCount, initial: true) {
            guard !restored, model.readyCount > 0 else { return }
            restored = true
            let parts = savedPlace.split(separator: "/").map(String.init)
            guard path.isEmpty, let server = parts.first, model.server(server) != nil else { return }
            var restoredPath: [ServersRoute] = [.server(server)]
            if parts.count > 1, let channel = model.channel(parts[1]) {
                restoredPath.append(.channel(ChannelRoute(id: channel.id, name: channel.name)))
            }
            path = restoredPath
        }
    }
}
