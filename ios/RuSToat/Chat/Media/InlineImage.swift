import SwiftUI
import UIKit

extension EnvironmentValues {
    /// How wide a message's pictures may be: the row's text column, set by
    /// the chat from its own width.
    @Entry var mediaMaxWidth: CGFloat = 300
}

enum MediaLayout {
    /// No picture in a row is wider than this, even on a big phone…
    static let maxWidth: CGFloat = 320
    /// …or taller than this.
    static let maxHeight: CGFloat = 400

    /// The core's display size (`width` × `height`, already fitted into
    /// desktop's box), scaled down to fit `available` points across and the
    /// limits above, keeping its shape. Never scaled up. Known before the
    /// picture arrives, so a row never changes height when it does.
    static func fit(width: UInt32, height: UInt32, available: CGFloat) -> CGSize {
        let w = CGFloat(max(width, 1))
        let h = CGFloat(max(height, 1))
        let scale = min(1, min(maxWidth, max(available, 1)) / w, maxHeight / h)
        return CGSize(width: (w * scale).rounded(), height: (h * scale).rounded())
    }
}

/// ThumbHashes decoded (by the core) into small pictures, kept by hash.
@MainActor
enum ThumbHashes {
    private static let cache = NSCache<NSData, UIImage>()

    static func image(_ hash: Data) -> UIImage? {
        if let image = cache.object(forKey: hash as NSData) {
            return image
        }
        guard let thumb = thumbhashImage(hash: hash), let image = picture(thumb) else { return nil }
        cache.setObject(image, forKey: hash as NSData)
        return image
    }

    /// The core's RGBA pixels (not premultiplied) as a picture.
    nonisolated static func picture(_ thumb: ThumbImage) -> UIImage? {
        let width = Int(thumb.width), height = Int(thumb.height)
        guard width > 0, height > 0, thumb.rgba.count == width * height * 4,
              let provider = CGDataProvider(data: thumb.rgba as CFData),
              let image = CGImage(
                  width: width, height: height,
                  bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                  space: CGColorSpace(name: CGColorSpace.sRGB)!,
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: true,
                  intent: .defaultIntent
              ) else { return nil }
        return UIImage(cgImage: image)
    }
}

/// What a picture shows while it loads: its ThumbHash stretched to fill
/// (a blurred copy), or a plain box.
struct PicturePlaceholder: View {
    let thumbhash: Data?

    var body: some View {
        if let thumbhash, let image = ThumbHashes.image(thumbhash) {
            Image(uiImage: image)
                .resizable()
                .interpolation(.medium)
        } else {
            Rectangle().fill(.quaternary)
        }
    }
}

/// A remote picture drawn at exactly `size`, filling it: the placeholder
/// until it arrives, then the picture fading in over it. Already-loaded
/// pictures show at once.
struct RemoteImage: View {
    let url: URL
    let size: CGSize
    var thumbhash: Data?
    var loader: ImageLoader = .shared

    @Environment(\.displayScale) private var scale
    /// What arrived, and for which picture (rows are reused).
    @State private var loaded: (key: Key, image: UIImage?)?

    var body: some View {
        let key = Key(url: url, size: size, scale: scale)
        let image = loaded?.key == key ? loaded?.image : loader.cached(url, fitting: size, scale: scale)
        ZStack {
            if image == nil {
                PicturePlaceholder(thumbhash: thumbhash)
                    .transition(.opacity)
            }
            if let image {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFill()
                    .transition(.opacity)
            }
        }
        .frame(width: size.width, height: size.height)
        .clipped()
        .task(id: key) {
            if let cached = loader.cached(url, fitting: size, scale: scale) {
                loaded = (key, cached)
                return
            }
            let picture = await loader.image(url, fitting: size, scale: scale)
            withAnimation(.easeOut(duration: 0.2)) { loaded = (key, picture) }
        }
    }

    private struct Key: Equatable {
        let url: URL
        let size: CGSize
        let scale: CGFloat
    }
}

/// An image attachment inline: the picture at its display size, a "Show
/// image" card in its place while it's too big to load by itself, or a
/// cover while it's a spoiler.
struct InlineImage: View {
    let image: ImageAttachment
    /// "Show image" and uncovering a spoiler (by file ID).
    let reveal: (String) -> Void
    let uncover: (String) -> Void
    /// Tapping the picture.
    let open: () -> Void

    @Environment(\.mediaMaxWidth) private var available

    var body: some View {
        let size = MediaLayout.fit(width: image.width, height: image.height, available: available)
        Group {
            if image.tooBig {
                tooBig
            } else if image.spoiler {
                SpoilerCover(thumbhash: image.thumbhash) { uncover(image.id) }
            } else if let url = URL(string: image.url) {
                Button(action: open) {
                    RemoteImage(url: url, size: size, thumbhash: image.thumbhash)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(image.name)
            } else {
                PicturePlaceholder(thumbhash: image.thumbhash)
            }
        }
        .frame(width: size.width, height: size.height)
        .clipShape(.rect(cornerRadius: 12))
    }

    /// The blurred preview with the file's name and size, and "Show image".
    private var tooBig: some View {
        ZStack {
            PicturePlaceholder(thumbhash: image.thumbhash)
                .overlay(.black.opacity(0.35))
            VStack(spacing: 6) {
                Image(systemName: "photo")
                    .font(.title2)
                Text(image.name)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                Text(image.sizeLabel)
                    .font(.caption)
                Button("Show image") { reveal(image.id) }
                    .buttonStyle(.glass)
                    .controlSize(.small)
            }
            .foregroundStyle(.white)
            .padding(8)
        }
    }
}

/// A spoiler picture or video, covered at its own shape: its preview,
/// heavily blurred, with a "Spoiler" tag. Tapping uncovers it.
struct SpoilerCover: View {
    let thumbhash: Data?
    let uncover: () -> Void

    var body: some View {
        Button(action: uncover) {
            ZStack {
                PicturePlaceholder(thumbhash: thumbhash)
                    .blur(radius: 12, opaque: true)
                    .overlay(.black.opacity(0.3))
                Label("Spoiler", systemImage: "eye.slash")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .background(.black.opacity(0.5), in: .capsule)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Spoiler. Tap to show.")
    }
}
