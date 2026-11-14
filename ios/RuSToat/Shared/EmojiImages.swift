import Observation
import SwiftUI
import UIKit

/// Custom emoji images at the sizes text draws them, loaded through
/// `ImageLoader`. Text is built synchronously, so this keeps what's arrived
/// where views can observe it: views that ask for an image are drawn again
/// when it does.
@MainActor
@Observable
final class EmojiImages {
    static let shared = EmojiImages()

    /// Inline emoji, beside body text.
    static let inlineSize: CGFloat = 20
    /// Emoji in a message of only emoji.
    static let jumboSize: CGFloat = 48
    /// Emoji are drawn into text at this scale (an iPhone's).
    static let scale: CGFloat = 3

    private var images: [String: UIImage] = [:]
    @ObservationIgnored private var loading: Set<String> = []
    @ObservationIgnored private let loader: ImageLoader

    init(loader: ImageLoader = .shared) {
        self.loader = loader
    }

    /// The image at `url` fitted into a `size`-point square, if loaded;
    /// otherwise starts loading it and returns `nil`.
    func image(_ url: String, size: CGFloat) -> UIImage? {
        let key = "\(size)|\(url)"
        if let image = images[key] {
            return image
        }
        guard !loading.contains(key), let parsed = URL(string: url) else { return nil }
        loading.insert(key)
        Task {
            defer { loading.remove(key) }
            let square = CGSize(width: size, height: size)
            if let image = await loader.image(parsed, fitting: square, scale: Self.scale) {
                images[key] = image
            }
        }
        return nil
    }
}
