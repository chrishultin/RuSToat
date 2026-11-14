import SwiftUI

/// Saved Notes, then DMs and groups, most recent first.
struct HomeView: View {
    let model: AccountModel

    var body: some View {
        NavigationStack {
            List(model.home, id: \.id) { channel in
                NavigationLink(value: ChannelRoute(id: channel.id, name: channel.name)) {
                    ChannelRow(channel: channel)
                }
            }
            .overlay {
                if model.home.isEmpty && model.readyCount > 0 {
                    ContentUnavailableView("No conversations", systemImage: "bubble.left.and.bubble.right",
                                           description: Text("Your DMs and groups appear here."))
                }
            }
            .navigationTitle("Home")
            .navigationDestination(for: ChannelRoute.self) { route in
                ChatView(model: model, route: route)
            }
        }
    }
}
