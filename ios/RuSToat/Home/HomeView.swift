import SwiftUI

/// Saved Notes, then DMs and groups, most recent first.
struct HomeView: View {
    let model: AccountModel
    @State private var path: [ChannelRoute] = []
    /// The open conversation's ID, to reopen at the next launch.
    @SceneStorage("homeChannel") private var savedChannel = ""
    @State private var restored = false

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
        .onChange(of: path) { savedChannel = path.last?.id ?? "" }
        // Once the lists are in, go back to where the user was.
        .onChange(of: model.readyCount, initial: true) {
            guard !restored, model.readyCount > 0 else { return }
            restored = true
            var id = savedChannel
            #if DEBUG
            // Or open one at launch, for screenshots:
            // SIMCTL_CHILD_RUSTOAT_DEV_OPEN=<channel ID> xcrun simctl launch …
            id = ProcessInfo.processInfo.environment["RUSTOAT_DEV_OPEN"] ?? id
            #endif
            if path.isEmpty, !id.isEmpty, let channel = model.channel(id) {
                path = [ChannelRoute(id: channel.id, name: channel.name)]
            }
        }
    }
}
