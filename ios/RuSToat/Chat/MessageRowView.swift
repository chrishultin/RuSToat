import SwiftUI

/// What can be done to a message from its row.
struct MessageActions {
    let reply: (MessageItem) -> Void
    /// By the pending message's key.
    let retry: (String) -> Void
    let discard: (String) -> Void
    /// Reveal spoiler `index` of a message (by its ID).
    let revealSpoiler: (String, UInt32) -> Void
    /// A link whose text isn't its address: ask before opening `url`.
    let openGuarded: (URL, String) -> Void
}

/// What tapping a `stoat:` link in a message does.
enum InternalLinkTap: Equatable {
    case spoiler(index: UInt32)
    case guarded(url: URL, text: String)
    /// Mentions, timestamps and the rest: nothing yet.
    case ignore

    init(_ url: URL) {
        let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        let path = (components?.path ?? "").split(separator: "/").map(String.init)
        switch path.first {
        case "spoiler" where path.count > 1:
            self = UInt32(path[1]).map { .spoiler(index: $0) } ?? .ignore
        case "link":
            // Form-encoded (`+` for a space), as the core writes it.
            let query = components?.percentEncodedQueryItems ?? []
            let value = { (name: String) in
                query.first { $0.name == name }?.value?
                    .replacingOccurrences(of: "+", with: " ")
                    .removingPercentEncoding
            }
            if let target = value("url").flatMap(URL.init(string:)) {
                self = .guarded(url: target, text: value("text") ?? "")
            } else {
                self = .ignore
            }
        default:
            self = .ignore
        }
    }
}

/// A message: avatar, name and time on the first of a group, then its
/// replies, text, attachments and embeds. Long-press for its menu, or swipe
/// right to reply.
struct MessageRowView: View {
    let message: MessageItem
    let actions: MessageActions
    @State private var swipe: CGFloat = 0

    var body: some View {
        if message.system {
            systemRow
        } else {
            row
                .environment(\.openURL, OpenURLAction(handler: open))
                .offset(x: swipe)
                .contentShape(.rect)
                .contextMenu { menu }
                .simultaneousGesture(swipeToReply)
        }
    }

    private func open(_ url: URL) -> OpenURLAction.Result {
        guard url.scheme == "stoat" else { return .systemAction }
        switch InternalLinkTap(url) {
        case .spoiler(let index):
            actions.revealSpoiler(message.id, index)
        case .guarded(let target, let text):
            actions.openGuarded(target, text)
        case .ignore:
            break
        }
        return .handled
    }

    @ViewBuilder private var menu: some View {
        if message.pending == nil {
            Button("Reply", systemImage: "arrowshape.turn.up.left") { actions.reply(message) }
        }
        if let body = message.body {
            Button("Copy Text", systemImage: "doc.on.doc") {
                UIPasteboard.general.string = String(MessageText.attributed(body).characters)
            }
        }
        if case .failed = message.pending {
            Button("Retry", systemImage: "arrow.clockwise") { actions.retry(message.key) }
            Button("Discard", systemImage: "trash", role: .destructive) { actions.discard(message.key) }
        }
    }

    /// A short swipe right replies, as in Messages.
    private var swipeToReply: some Gesture {
        DragGesture(minimumDistance: 20)
            .onChanged { value in
                guard message.pending == nil,
                      value.translation.width > 0,
                      abs(value.translation.height) < abs(value.translation.width) else { return }
                swipe = min(value.translation.width, 70) * 0.6
            }
            .onEnded { value in
                if message.pending == nil, value.translation.width > 60,
                   abs(value.translation.height) < 40 {
                    actions.reply(message)
                }
                withAnimation(.spring) { swipe = 0 }
            }
    }

    private var row: some View {
        Group {
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
                    if !message.rich.isEmpty {
                        RichTextView(blocks: message.rich, jumbo: message.jumbo)
                            .id(message.renderKey)
                    }
                    ForEach(Array(message.attachments.enumerated()), id: \.offset) { _, attachment in
                        AttachmentCard(attachment: attachment)
                    }
                    ForEach(Array(message.embeds.enumerated()), id: \.offset) { _, embed in
                        EmbedCard(embed: embed)
                    }
                    if case .failed(let reason) = message.pending {
                        Button {
                            actions.retry(message.key)
                        } label: {
                            Label("\(reason) Tap to retry.", systemImage: "exclamationmark.arrow.circlepath")
                                .font(.caption)
                                .foregroundStyle(.red)
                        }
                        .buttonStyle(.plain)
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
                    if !embed.richDescription.isEmpty {
                        RichTextView(blocks: embed.richDescription)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(6)
                    } else if let description = embed.description {
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
