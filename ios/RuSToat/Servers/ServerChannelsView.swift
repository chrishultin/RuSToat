import SwiftUI

/// One server's channels, under their category headings.
struct ServerChannelsView: View {
    let model: AccountModel
    let serverId: String

    var body: some View {
        let server = model.server(serverId)
        List {
            ForEach(model.channels(of: serverId), id: \.id) { section in
                Section {
                    ForEach(section.channels, id: \.id) { channel in
                        NavigationLink(value: ChannelRoute(id: channel.id, name: channel.name)) {
                            ChannelRow(channel: channel)
                        }
                    }
                } header: {
                    if let title = section.title {
                        Text(title)
                    }
                }
            }
        }
        .overlay {
            if server == nil {
                ContentUnavailableView("Server unavailable", systemImage: "square.slash",
                                       description: Text("You're no longer in this server."))
            }
        }
        .navigationTitle(server?.name ?? "")
        .navigationBarTitleDisplayMode(.inline)
    }
}
