import SwiftUI

/// A link preview or bot embed, as desktop draws it: a card with a colour
/// bar, the site's icon and name, a title linking out, the description (a
/// bot's Markdown drawn as rich text), and a small picture beside the text
/// or a large one under it. A video link's picture gets a play badge and
/// opens the link (in its app, when it has one). Pictures and GIFs alone
/// are drawn bare.
struct EmbedCardView: View {
    let embed: EmbedItem
    let actions: MessageActions
    /// Open a picture full screen (by its address, or a file's ID).
    let view: (String) -> Void

    @Environment(\.openURL) private var openURL
    @Environment(\.mediaMaxWidth) private var available

    /// No card is wider than this.
    static let maxWidth: CGFloat = 320

    var body: some View {
        switch embed {
        case .card(let card):
            cardView(card)
        case .image(let image):
            Button {
                view(image.url)
            } label: {
                picture(image, size: fit(image, within: available))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Image")
        case .gifPoster(let image, let link):
            Button {
                open(link)
            } label: {
                picture(image, size: fit(image, within: available))
                    .overlay(alignment: .bottomLeading) { gifBadge }
            }
            .buttonStyle(.plain)
            .accessibilityLabel("GIF")
        case .video(let url):
            FileCard(name: URL(string: url)?.host() ?? url, detail: "Video", icon: "play.rectangle") {
                open(url)
            }
        }
    }

    private var cardWidth: CGFloat { min(available, Self.maxWidth) }
    /// Inside the bar and the padding.
    private var innerWidth: CGFloat { cardWidth - 4 - 24 }

    private func fit(_ image: EmbedImageItem, within width: CGFloat) -> CGSize {
        MediaLayout.fit(width: image.width, height: image.height, available: width)
    }

    private func picture(_ image: EmbedImageItem, size: CGSize) -> some View {
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

    private var gifBadge: some View {
        Text("GIF")
            .font(.caption2.bold())
            .foregroundStyle(.white)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(.black.opacity(0.6), in: .rect(cornerRadius: 4))
            .padding(8)
    }

    private func cardView(_ card: EmbedCardItem) -> some View {
        HStack(spacing: 0) {
            Rectangle()
                .fill(card.colour.map(Color.init(rgba:)) ?? Color(uiColor: .separator))
                .frame(width: 4)
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .top, spacing: 10) {
                    text(card)
                    Spacer(minLength: 0)
                    if let thumbnail = card.thumbnail {
                        picture(thumbnail, size: CGSize(width: CGFloat(thumbnail.width), height: CGFloat(thumbnail.height)))
                    }
                }
                if let image = card.image {
                    largeImage(image, card: card)
                }
                if let media = card.media {
                    AttachmentView(attachment: media, actions: actions, view: view)
                        .environment(\.mediaMaxWidth, innerWidth)
                }
            }
            .padding(12)
        }
        .frame(maxWidth: cardWidth, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
        .background(.quaternary.opacity(0.5))
        .clipShape(.rect(cornerRadius: 12))
    }

    @ViewBuilder private func text(_ card: EmbedCardItem) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if card.icon != nil || card.siteName != nil {
                HStack(spacing: 6) {
                    if let icon = card.icon.flatMap(URL.init(string:)) {
                        RemoteImage(url: icon, size: CGSize(width: 16, height: 16))
                            .clipShape(.rect(cornerRadius: 3))
                    }
                    if let site = card.siteName {
                        Text(site)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
            }
            if let title = card.title {
                if let link = card.link {
                    Button {
                        open(link)
                    } label: {
                        Text(title)
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.tint)
                            .multilineTextAlignment(.leading)
                            .lineLimit(3)
                    }
                    .buttonStyle(.plain)
                } else {
                    Text(title)
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(3)
                }
            }
            if !card.richDescription.isEmpty {
                RichTextView(blocks: card.richDescription)
                    .font(.footnote)
            } else if let description = card.description {
                Text(description)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .lineLimit(5)
            }
        }
    }

    /// A video's picture (with a play badge) opens the video; any other
    /// opens the page.
    private func largeImage(_ image: EmbedImageItem, card: EmbedCardItem) -> some View {
        Button {
            open(card.link ?? image.source)
        } label: {
            picture(image, size: fit(image, within: innerWidth))
                .overlay {
                    if card.playable {
                        Image(systemName: "play.fill")
                            .font(.title2)
                            .foregroundStyle(.white)
                            .frame(width: 54, height: 54)
                            .background(.black.opacity(0.55), in: .circle)
                    }
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(card.playable ? "Play video" : "Open link")
    }

    private func open(_ url: String?) {
        if let url = url.flatMap(URL.init(string:)) {
            openURL(url)
        }
    }
}

extension Color {
    /// `0xRRGGBBAA`, as the core gives colours.
    init(rgba: UInt32) {
        self.init(
            .sRGB,
            red: Double(rgba >> 24 & 0xff) / 255,
            green: Double(rgba >> 16 & 0xff) / 255,
            blue: Double(rgba >> 8 & 0xff) / 255,
            opacity: Double(rgba & 0xff) / 255
        )
    }
}
