import SwiftUI

/// A channel's messages. (Placeholder until reading lands.)
struct ChatView: View {
    let model: AccountModel
    let route: ChannelRoute

    var body: some View {
        ContentUnavailableView(route.name, systemImage: "text.bubble",
                               description: Text("Messages arrive in the next milestone chunk."))
            .navigationTitle(route.name)
            .navigationBarTitleDisplayMode(.inline)
    }
}
