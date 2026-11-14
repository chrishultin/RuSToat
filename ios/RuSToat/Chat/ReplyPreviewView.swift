import SwiftUI

/// The "↱ Name: snippet" line above a reply.
struct ReplyPreviewView: View {
    let reply: ReplyItem

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "arrow.turn.up.right")
                .font(.caption2)
            switch reply.state {
            case .loaded:
                Text(reply.author).fontWeight(.semibold)
                Text(reply.snippet).lineLimit(1)
            case .loading:
                Text("Loading…")
            case .deleted:
                Text("Original message was deleted").italic()
            case .unavailable:
                Text("Couldn't load the original message").italic()
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }
}
