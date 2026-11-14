import SwiftUI

/// Saved Notes, then DMs and groups, most recent first.
struct HomeView: View {
    let model: AccountModel
    @State private var path: [ChannelRoute] = []

    var body: some View {
        NavigationStack(path: $path) {
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
        #if DEBUG
        // Open a conversation at launch, for screenshots:
        // SIMCTL_CHILD_RUSTOAT_DEV_OPEN=<channel ID> xcrun simctl launch …
        .onChange(of: model.readyCount, initial: true) {
            if path.isEmpty, let id = ProcessInfo.processInfo.environment["RUSTOAT_DEV_OPEN"],
               let channel = model.channel(id) {
                path = [ChannelRoute(id: channel.id, name: channel.name)]
            }
        }
        #endif
    }
}
