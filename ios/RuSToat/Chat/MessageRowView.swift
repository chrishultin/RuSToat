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
    /// The message's text as typed, for Copy Text.
    let rawText: (String) -> String?
    /// A link to the message on the web client, if the instance has one.
    let link: (String) -> String?
    /// The user may pin and unpin here.
    let canPin: Bool
    let setPinned: (String, Bool) -> Void
    /// Join or leave a reaction (a message ID and the emoji's key).
    let react: (String, String) -> Void
    /// Open the reaction picker for a message.
    let pickReaction: (MessageItem) -> Void
    /// Edit or delete one of the user's own messages.
    let edit: (MessageItem) -> Void
    let delete: (MessageItem) -> Void
    /// Load a too-big image (by file ID), or uncover a spoiler attachment.
    let revealImage: (String) -> Void
    let revealAttachment: (String) -> Void
    /// Open a message's picture (a file ID or an embed's address) full screen.
    let viewImage: (MessageItem, String) -> Void
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
            Button("Add Reaction", systemImage: "face.smiling") { actions.pickReaction(message) }
        }
        if message.body != nil {
            Button("Copy Text", systemImage: "doc.on.doc") {
                // As typed; a pending message's is its own text.
                UIPasteboard.general.string = actions.rawText(message.id) ?? message.body
            }
        }
        if message.pending == nil, let link = actions.link(message.id) {
            Button("Copy Link", systemImage: "link") {
                UIPasteboard.general.string = link
            }
        }
        if message.pending == nil, actions.canPin {
            if message.pinned {
                Button("Unpin", systemImage: "pin.slash") { actions.setPinned(message.id, false) }
            } else {
                Button("Pin", systemImage: "pin") { actions.setPinned(message.id, true) }
            }
        }
        if message.mine, message.pending == nil, !message.system {
            Divider()
            Button("Edit", systemImage: "pencil") { actions.edit(message) }
            Button("Delete Message", systemImage: "trash", role: .destructive) { actions.delete(message) }
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
                        AttachmentView(attachment: attachment, actions: actions) { id in
                            actions.viewImage(message, id)
                        }
                    }
                    ForEach(Array(message.embeds.enumerated()), id: \.offset) { _, embed in
                        EmbedCard(embed: embed, actions: actions) { id in
                            actions.viewImage(message, id)
                        }
                    }
                    if !message.reactions.isEmpty {
                        ReactionsView(
                            reactions: message.reactions,
                            toggle: { key in actions.react(message.id, key) },
                            add: { actions.pickReaction(message) }
                        )
                        .padding(.top, 2)
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
            if message.pinned {
                Image(systemName: "pin.fill")
                    .font(.caption2)
                    .foregroundStyle(.orange)
                    .accessibilityLabel("Pinned")
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

/// An attachment: images and videos inline, audio with a player, and
/// other files as a card that opens in Safari.
struct AttachmentView: View {
    let attachment: AttachmentItem
    let actions: MessageActions
    /// Open a picture full screen (by its file ID).
    let view: (String) -> Void
    @Environment(\.openURL) private var openURL

    var body: some View {
        switch attachment {
        case .image(let image):
            InlineImage(
                image: image,
                reveal: actions.revealImage,
                uncover: actions.revealAttachment,
                open: { view(image.id) }
            )
        case .file(let file):
            FileCard(name: file.name, detail: file.sizeLabel, icon: file.kind.symbol, spoiler: file.spoiler) {
                open(file.url)
            }
            .disabled(file.url == nil)
        case .audio(let audio):
            AudioAttachmentView(audio: audio)
        case .video(let video):
            VideoAttachmentView(video: video, uncover: actions.revealAttachment)
        case .local(let local):
            FileCard(name: local.name, detail: local.sizeLabel, icon: local.kind.symbol, spoiler: local.spoiler) {}
                .disabled(true)
        }
    }

    private func open(_ url: String?) {
        if let url = url.flatMap(URL.init(string:)) {
            openURL(url)
        }
    }
}

extension FileKindItem {
    /// The card's icon.
    var symbol: String {
        switch self {
        case .image: "photo"
        case .video: "film"
        case .audio: "waveform"
        case .text: "doc.text"
        case .file: "doc"
        }
    }
}

/// A file's card: its icon, name and size, with a tag when it's a spoiler.
struct FileCard: View {
    let name: String
    let detail: String
    let icon: String
    var spoiler = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: icon)
                    .font(.title3)
                    .foregroundStyle(.tint)
                    .frame(width: 28)
                VStack(alignment: .leading) {
                    Text(name).lineLimit(1)
                    HStack(spacing: 6) {
                        Text(detail)
                        if spoiler {
                            Text("Spoiler").fontWeight(.semibold)
                        }
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
            }
            .padding(10)
            .frame(maxWidth: 280, alignment: .leading)
            .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 12))
        }
        .buttonStyle(.plain)
    }
}

/// A link preview or bot embed.
struct EmbedCard: View {
    let embed: EmbedItem
    let actions: MessageActions
    /// Open a picture full screen (by its address, or a file's ID).
    let view: (String) -> Void
    @Environment(\.openURL) private var openURL
    @Environment(\.mediaMaxWidth) private var available

    var body: some View {
        switch embed {
        case .card(let card):
            cardView(card)
        case .image(let image):
            Button {
                view(image.url)
            } label: {
                remote(image, size: size(image))
            }
            .buttonStyle(.plain)
        case .gifPoster(let image, let link):
            picture(image, opening: link)
                .overlay(alignment: .bottomLeading) {
                    Text("GIF")
                        .font(.caption2.bold())
                        .foregroundStyle(.white)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(.black.opacity(0.6), in: .rect(cornerRadius: 4))
                        .padding(8)
                }
        case .video(let url):
            FileCard(name: URL(string: url)?.host() ?? url, detail: "Video", icon: "play.rectangle") {
                open(url)
            }
        }
    }

    private func size(_ image: EmbedImageItem) -> CGSize {
        MediaLayout.fit(width: image.width, height: image.height, available: available)
    }

    private func picture(_ image: EmbedImageItem, opening link: String) -> some View {
        Button {
            open(link)
        } label: {
            remote(image, size: size(image))
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder private func remote(_ image: EmbedImageItem, size: CGSize) -> some View {
        Group {
            if let url = URL(string: image.url) {
                RemoteImage(url: url, size: size)
            } else {
                Rectangle().fill(.quaternary)
            }
        }
        .frame(width: size.width, height: size.height)
        .clipShape(.rect(cornerRadius: 8))
    }

    private func cardView(_ card: EmbedCardItem) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    if let site = card.siteName {
                        Text(site).font(.caption).foregroundStyle(.secondary)
                    }
                    if let title = card.title {
                        Text(title).font(.subheadline.weight(.semibold)).lineLimit(2)
                    }
                    if !card.richDescription.isEmpty {
                        RichTextView(blocks: card.richDescription)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(6)
                    } else if let description = card.description {
                        Text(description).font(.caption).foregroundStyle(.secondary).lineLimit(3)
                    }
                }
                Spacer(minLength: 0)
                if let thumbnail = card.thumbnail {
                    remote(thumbnail, size: CGSize(width: CGFloat(thumbnail.width), height: CGFloat(thumbnail.height)))
                }
            }
            if let image = card.image {
                // Inside the card's padding.
                remote(image, size: MediaLayout.fit(
                    width: image.width, height: image.height, available: min(available, 320) - 20
                ))
            }
            if let media = card.media {
                AttachmentView(attachment: media, actions: actions, view: view)
                    .environment(\.mediaMaxWidth, min(available, 320) - 20)
            }
        }
        .padding(10)
        .frame(maxWidth: 320, alignment: .leading)
        .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 12))
        .contentShape(.rect)
        .onTapGesture { open(card.link) }
    }

    private func open(_ url: String?) {
        if let url = url.flatMap(URL.init(string:)) {
            openURL(url)
        }
    }
}
