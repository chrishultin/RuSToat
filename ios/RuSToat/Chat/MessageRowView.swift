import SwiftUI

/// A message: avatar, name and time on the first of a group, then its
/// replies, text, attachments and embeds.
struct MessageRowView: View {
    let message: MessageItem

    var body: some View {
        if message.system {
            systemRow
        } else {
            HStack(alignment: .top, spacing: 12) {
                if message.continuation {
                    Color.clear.frame(width: 36, height: 1)
                } else {
                    AvatarView(url: message.avatarUrl, name: message.author, size: 36)
                }
                VStack(alignment: .leading, spacing: 4) {
                    if !message.continuation {
                        header
                    }
                    ForEach(message.replies, id: \.id) { reply in
                        ReplyPreviewView(reply: reply)
                    }
                    if let body = message.body {
                        MessageText(markdown: body, jumbo: message.jumbo)
                            .id(message.renderKey)
                    }
                    ForEach(Array(message.attachments.enumerated()), id: \.offset) { _, attachment in
                        AttachmentCard(attachment: attachment)
                    }
                    ForEach(Array(message.embeds.enumerated()), id: \.offset) { _, embed in
                        EmbedCard(embed: embed)
                    }
                    if case .failed(let reason) = message.pending {
                        Label(reason, systemImage: "exclamationmark.circle.fill")
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16)
            .padding(.top, message.continuation ? 2 : 10)
            .opacity(message.pending == .sending ? 0.5 : 1)
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(message.author)
                .font(.subheadline.weight(.semibold))
                .lineLimit(1)
            Text(message.time)
                .font(.caption)
                .foregroundStyle(.secondary)
            if message.edited {
                Text("(edited)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var systemRow: some View {
        HStack {
            Spacer()
            if let body = message.body {
                Text(MessageText.attributed(body))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 6)
    }
}

/// An attachment, compactly: a thumbnail for images, else a card. Opens in
/// Safari. (Full media comes in a later milestone.)
struct AttachmentCard: View {
    let attachment: AttachmentItem
    @Environment(\.openURL) private var openURL

    var body: some View {
        Button {
            if let url = attachment.url.flatMap(URL.init(string:)) {
                openURL(url)
            }
        } label: {
            if attachment.isImage, let url = attachment.url.flatMap(URL.init(string:)) {
                let size = displaySize
                AsyncImage(url: url) { image in
                    image.resizable().scaledToFill()
                } placeholder: {
                    Rectangle().fill(.quaternary)
                }
                .frame(width: size.width, height: size.height)
                .clipShape(.rect(cornerRadius: 12))
            } else {
                HStack(spacing: 10) {
                    Image(systemName: "doc")
                        .font(.title3)
                        .foregroundStyle(.tint)
                    VStack(alignment: .leading) {
                        Text(attachment.name).lineLimit(1)
                        Text(attachment.sizeLabel)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(10)
                .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 12))
            }
        }
        .buttonStyle(.plain)
        .disabled(attachment.url == nil)
    }

    /// The core's size, scaled to fit a phone (at most 240 points wide).
    private var displaySize: CGSize {
        let width = CGFloat(max(attachment.width, 1))
        let height = CGFloat(max(attachment.height, 1))
        let scale = min(1, 240 / width, 320 / height)
        return CGSize(width: width * scale, height: height * scale)
    }
}

/// A link preview or bot embed, compactly.
struct EmbedCard: View {
    let embed: EmbedItem
    @Environment(\.openURL) private var openURL

    var body: some View {
        Button {
            if let url = embed.link.flatMap(URL.init(string:)) {
                openURL(url)
            }
        } label: {
            HStack(alignment: .top, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    if let site = embed.siteName {
                        Text(site).font(.caption).foregroundStyle(.secondary)
                    }
                    if let title = embed.title {
                        Text(title).font(.subheadline.weight(.semibold)).lineLimit(2)
                    }
                    if let description = embed.description {
                        Text(description).font(.caption).foregroundStyle(.secondary).lineLimit(3)
                    }
                }
                Spacer(minLength: 0)
                if let image = embed.imageUrl.flatMap(URL.init(string:)) {
                    AsyncImage(url: image) { image in
                        image.resizable().scaledToFill()
                    } placeholder: {
                        Rectangle().fill(.quaternary)
                    }
                    .frame(width: 64, height: 64)
                    .clipShape(.rect(cornerRadius: 8))
                }
            }
            .padding(10)
            .frame(maxWidth: 320, alignment: .leading)
            .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 12))
        }
        .buttonStyle(.plain)
        .disabled(embed.link == nil)
    }
}
