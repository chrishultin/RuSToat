import AVFoundation
import ImageIO
import SwiftUI
import UIKit

/// Every remote picture the app draws: attachments, embeds, emoji. Each
/// download happens once however many views ask at the same time; the bytes
/// stay in a disk cache (`URLCache`), and decoded pictures in a memory cache,
/// so rows scrolled back into view draw at once. Pictures are decoded at the
/// size they're drawn (ImageIO's thumbnailing), not at their full size.
@MainActor
final class ImageLoader {
    static let shared = ImageLoader()

    private let session: URLSession
    /// Decoded pictures, by URL and pixel size. Costs are bytes.
    private let images = NSCache<NSString, UIImage>()
    private var decoding: [String: Task<UIImage?, Never>] = [:]
    private var downloads: [URL: Task<Data?, Never>] = [:]
    /// Pictures found to be animated GIFs: views play these.
    private var animated: Set<URL> = []
    /// How many downloads were started (for tests).
    private(set) var downloadsStarted = 0

    /// `session` defaults to one with a 200 MB disk cache that keeps
    /// pictures without asking again: Stoat's files never change once
    /// uploaded, and proxied pictures rarely do.
    init(session: URLSession? = nil) {
        self.session = session ?? Self.cachingSession()
        images.totalCostLimit = 150 * 1024 * 1024
        images.countLimit = 600
    }

    private static func cachingSession() -> URLSession {
        let configuration = URLSessionConfiguration.default
        let directory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appending(path: "images")
        configuration.urlCache = URLCache(
            memoryCapacity: 8 * 1024 * 1024,
            diskCapacity: 200 * 1024 * 1024,
            directory: directory
        )
        configuration.requestCachePolicy = .returnCacheDataElseLoad
        return URLSession(configuration: configuration)
    }

    private static func key(_ url: URL, _ pixels: Int) -> NSString {
        "\(pixels)|\(url.absoluteString)" as NSString
    }

    /// The longest side, in pixels, of a picture fitted into `size` points.
    nonisolated static func pixels(fitting size: CGSize, scale: CGFloat) -> Int {
        max(1, Int((max(size.width, size.height) * scale).rounded(.up)))
    }

    /// The picture at `url`, if it's already decoded at this size: what a
    /// view shows straight away, without waiting a frame.
    func cached(_ url: URL, fitting size: CGSize, scale: CGFloat) -> UIImage? {
        images.object(forKey: Self.key(url, Self.pixels(fitting: size, scale: scale)))
    }

    /// The picture at `url`, decoded to fit `size` points at `scale` (never
    /// bigger than it is). `nil` if it couldn't be downloaded or decoded.
    func image(_ url: URL, fitting size: CGSize, scale: CGFloat) async -> UIImage? {
        let pixels = Self.pixels(fitting: size, scale: scale)
        let key = Self.key(url, pixels)
        if let image = images.object(forKey: key) {
            return image
        }
        if let task = decoding[key as String] {
            return await task.value
        }
        let task = Task<UIImage?, Never> {
            guard let data = await data(url) else { return nil }
            if AnimatedFrames.frameCount(data) > 1 {
                animated.insert(url)
            }
            return await Task.detached(priority: .userInitiated) {
                Self.decode(data, maxPixels: pixels, scale: scale)
            }.value
        }
        decoding[key as String] = task
        let image = await task.value
        decoding[key as String] = nil
        if let image {
            images.setObject(image, forKey: key, cost: Self.cost(of: image))
        }
        return image
    }

    /// Whether the picture at `url`, once loaded, turned out to be an
    /// animated GIF (by its bytes, not its address or type).
    func isAnimated(_ url: URL) -> Bool {
        animated.contains(url)
    }

    /// The GIF at `url`'s frames, decoded to fit `size` points at `scale`.
    /// Not cached: they're big, and dropped when the GIF leaves the screen.
    /// The bytes come from the disk cache. `nil` if it isn't animated, or
    /// has too many frames.
    func frames(_ url: URL, fitting size: CGSize, scale: CGFloat) async -> AnimatedFrames? {
        guard let data = await data(url) else { return nil }
        let pixels = Self.pixels(fitting: size, scale: scale)
        return await Task.detached(priority: .utility) {
            AnimatedFrames.decode(data, maxPixels: pixels, scale: scale)
        }.value
    }

    /// The first frame of the video at `url`, upright, fitted to `size`
    /// points at `scale`, kept in the memory cache like a picture. Only the
    /// start of the file is fetched (AVFoundation asks for ranges).
    func poster(_ url: URL, fitting size: CGSize, scale: CGFloat) async -> UIImage? {
        let pixels = Self.pixels(fitting: size, scale: scale)
        let key = "poster|\(Self.key(url, pixels))" as NSString
        if let image = images.object(forKey: key) {
            return image
        }
        if let task = decoding[key as String] {
            return await task.value
        }
        let task = Task<UIImage?, Never> {
            let generator = AVAssetImageGenerator(asset: AVURLAsset(url: url))
            generator.appliesPreferredTrackTransform = true
            generator.maximumSize = CGSize(width: pixels, height: pixels)
            guard let frame = try? await generator.image(at: .zero).image else { return nil }
            return UIImage(cgImage: frame, scale: scale, orientation: .up)
        }
        decoding[key as String] = task
        let image = await task.value
        decoding[key as String] = nil
        if let image {
            images.setObject(image, forKey: key, cost: Self.cost(of: image))
        }
        return image
    }

    /// A poster already made for `url` at this size.
    func cachedPoster(_ url: URL, fitting size: CGSize, scale: CGFloat) -> UIImage? {
        images.object(forKey: "poster|\(Self.key(url, Self.pixels(fitting: size, scale: scale)))" as NSString)
    }

    /// The bytes at `url` (from the disk cache when they're there), with
    /// simultaneous requests for one URL sharing a download.
    func data(_ url: URL) async -> Data? {
        if let task = downloads[url] {
            return await task.value
        }
        downloadsStarted += 1
        let session = session
        let task = Task<Data?, Never> {
            guard let (data, response) = try? await session.data(from: url) else { return nil }
            if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                return nil
            }
            return data
        }
        downloads[url] = task
        let data = await task.value
        downloads[url] = nil
        return data
    }

    /// `data` decoded with its longest side at most `maxPixels`, upright
    /// (EXIF orientation applied), as a picture of `scale`.
    nonisolated static func decode(_ data: Data, maxPixels: Int, scale: CGFloat) -> UIImage? {
        let sourceOptions = [kCGImageSourceShouldCache: false] as CFDictionary
        guard let source = CGImageSourceCreateWithData(data as CFData, sourceOptions) else { return nil }
        // ImageIO would scale a small picture up to the size asked for.
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        let side = max(
            properties?[kCGImagePropertyPixelWidth] as? Int ?? 0,
            properties?[kCGImagePropertyPixelHeight] as? Int ?? 0
        )
        let options = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: side > 0 ? min(maxPixels, side) : maxPixels,
        ] as CFDictionary
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options) else { return nil }
        return UIImage(cgImage: image, scale: scale, orientation: .up)
    }

    private static func cost(of image: UIImage) -> Int {
        guard let cgImage = image.cgImage else { return 1 }
        return cgImage.bytesPerRow * cgImage.height
    }
}
