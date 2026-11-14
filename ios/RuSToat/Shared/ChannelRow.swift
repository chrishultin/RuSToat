import SwiftUI

/// Where a channel row leads.
struct ChannelRoute: Hashable {
    let id: String
    let name: String
}

/// A channel or conversation in a list: its avatar or symbol, its name, and
/// its unread state. Muted ones are dimmed.
struct ChannelRow: View {
    let channel: ChannelItem

    var body: some View {
        HStack(spacing: 12) {
            leading
            Text(channel.name)
                .font(channel.unread && !channel.muted ? .body.weight(.semibold) : .body)
                .lineLimit(1)
            Spacer(minLength: 8)
            UnreadBadge(unread: channel.unread, mentions: channel.mentions)
        }
        .opacity(channel.muted ? 0.5 : 1)
    }

    @ViewBuilder private var leading: some View {
        switch channel.kind {
        case .text:
            symbol("number")
        case .voice:
            symbol("speaker.wave.2")
        case .savedNotes:
            symbol("note.text")
        case .directMessage, .group:
            AvatarView(url: channel.avatarUrl, name: channel.name, size: 36)
        }
    }

    private func symbol(_ name: String) -> some View {
        Image(systemName: name)
            .foregroundStyle(.secondary)
            .frame(width: 24)
    }
}
