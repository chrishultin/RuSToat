import SwiftUI

/// The user's servers, in their order.
struct ServerListView: View {
    let model: AccountModel

    var body: some View {
        NavigationStack {
            List(model.servers, id: \.id) { server in
                NavigationLink(value: server.id) {
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
            .navigationDestination(for: String.self) { serverId in
                ServerChannelsView(model: model, serverId: serverId)
            }
            .navigationDestination(for: ChannelRoute.self) { route in
                ChatView(model: model, route: route)
            }
        }
    }
}
