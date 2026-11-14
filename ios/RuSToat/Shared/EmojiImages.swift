import Observation
import SwiftUI
import UIKit

/// Custom emoji images, loaded once per URL and kept for the session, at the
/// sizes text draws them. Views that ask for an image are drawn again when
/// it arrives (through Observation).
@MainActor
@Observable
final class EmojiImages {
    static let shared = EmojiImages()

    /// Inline emoji, beside body text.
    static let inlineSize: CGFloat = 20
    /// Emoji in a message of only emoji.
    static let jumboSize: CGFloat = 48

    private var images: [String: UIImage] = [:]
    @ObservationIgnored private var loading: Set<String> = []
    @ObservationIgnored private let session: URLSession
    /// How many downloads were started (for tests).
    @ObservationIgnored private(set) var loadsStarted = 0

    init(session: URLSession = .shared) {
        self.session = session
    }

    /// The image at `url`, if loaded; otherwise starts loading it and
    /// returns `nil`.
    func image(_ url: String, size: CGFloat) -> UIImage? {
        let key = "\(size)|\(url)"
        if let image = images[key] {
            return image
        }
        load(url, size: size, key: key)
        return nil
    }

    private func load(_ url: String, size: CGFloat, key: String) {
        guard !loading.contains(key), let parsed = URL(string: url) else { return }
        loading.insert(key)
        loadsStarted += 1
        Task {
            defer { loading.remove(key) }
            guard let (data, _) = try? await session.data(from: parsed),
                  let image = UIImage(data: data) else { return }
            images[key] = Self.scaled(image, to: size)
        }
    }

    /// `image` fitted into a `size`-point square, keeping its shape.
    static func scaled(_ image: UIImage, to size: CGFloat) -> UIImage {
        let longest = max(image.size.width, image.size.height, 1)
        let scale = size / longest
        let target = CGSize(width: image.size.width * scale, height: image.size.height * scale)
        return UIGraphicsImageRenderer(size: target).image { _ in
            image.draw(in: CGRect(origin: .zero, size: target))
        }
    }
}
