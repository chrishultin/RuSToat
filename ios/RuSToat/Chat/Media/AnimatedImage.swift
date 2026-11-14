import ImageIO
import SwiftUI
import UIKit

/// An animated GIF's frames, decoded at the size they're drawn, with how
/// long each shows.
struct AnimatedFrames: Sendable {
    /// GIFs with more frames than this aren't animated: they show their
    /// first frame, and open in the viewer.
    static let maxFrames = 200
    /// The most one GIF's decoded frames may take: bigger ones are decoded
    /// smaller.
    static let memoryBudget = 48 * 1024 * 1024

    let frames: [UIImage]
    /// Seconds each frame shows.
    let delays: [Double]

    var duration: Double { delays.reduce(0, +) }

    /// Whether `data` is a GIF, by its signature: Stoat's proxy doesn't
    /// always say so in its headers, and a link needn't end in `.gif`.
    static func isGIF(_ data: Data) -> Bool {
        data.starts(with: Array("GIF8".utf8))
    }

    /// How many frames `data` has, if it's a GIF.
    static func frameCount(_ data: Data) -> Int {
        guard isGIF(data), let source = CGImageSourceCreateWithData(data as CFData, nil) else { return 0 }
        return CGImageSourceGetCount(source)
    }

    /// `data`'s frames with their longest side at most `maxPixels` (less if
    /// they'd go over the memory budget), at `scale`. `nil` unless it's a
    /// GIF of 2 to `maxFrames` frames.
    static func decode(_ data: Data, maxPixels: Int, scale: CGFloat, cap: Int = maxFrames) -> AnimatedFrames? {
        guard isGIF(data), let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let count = CGImageSourceGetCount(source)
        guard count > 1, count <= cap else { return nil }
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        let width = properties?[kCGImagePropertyPixelWidth] as? Int ?? maxPixels
        let height = properties?[kCGImagePropertyPixelHeight] as? Int ?? maxPixels
        let pixels = side(maxPixels: maxPixels, width: width, height: height, frames: count)
        let options = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: pixels,
        ] as CFDictionary
        var frames: [UIImage] = []
        var delays: [Double] = []
        for index in 0..<count {
            guard let image = CGImageSourceCreateThumbnailAtIndex(source, index, options) else { continue }
            frames.append(UIImage(cgImage: image, scale: scale, orientation: .up))
            delays.append(delay(source, index))
        }
        return frames.count > 1 ? AnimatedFrames(frames: frames, delays: delays) : nil
    }

    /// The longest side to decode `frames` frames of `width` × `height` at:
    /// what was asked, but never bigger than the GIF, and small enough that
    /// they all fit in `memoryBudget`.
    static func side(maxPixels: Int, width: Int, height: Int, frames: Int) -> Int {
        let long = max(width, height, 1)
        let short = max(min(width, height), 1)
        var side = min(maxPixels, long)
        // Bytes for all frames at `side`: 4 per pixel.
        let bytes = { (side: Int) in 4.0 * Double(side) * Double(side) * Double(short) / Double(long) * Double(frames) }
        if bytes(side) > Double(memoryBudget) {
            side = Int((Double(memoryBudget) / bytes(1)).squareRoot())
        }
        return max(side, 1)
    }

    /// A frame's delay as browsers treat it: very short ones (most often 0)
    /// show for a tenth of a second.
    private static func delay(_ source: CGImageSource, _ index: Int) -> Double {
        let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any]
        let gif = properties?[kCGImagePropertyGIFDictionary] as? [CFString: Any]
        let delay = gif?[kCGImagePropertyGIFUnclampedDelayTime] as? Double
            ?? gif?[kCGImagePropertyGIFDelayTime] as? Double
            ?? 0.1
        return delay < 0.02 ? 0.1 : delay
    }

    /// The frame showing `elapsed` seconds after the GIF started, looping.
    func index(at elapsed: Double) -> Int {
        Self.index(at: elapsed, delays: delays)
    }

    static func index(at elapsed: Double, delays: [Double]) -> Int {
        let total = delays.reduce(0, +)
        guard total > 0, elapsed.isFinite else { return 0 }
        var time = elapsed.truncatingRemainder(dividingBy: total)
        if time < 0 { time += total }
        for (index, delay) in delays.enumerated() {
            if time < delay { return index }
            time -= delay
        }
        return delays.count - 1
    }
}

/// An animated GIF at exactly `size`: `still` (its first frame) until the
/// frames are decoded, then playing on a loop. Frames are decoded only
/// while it's on screen, and dropped when it leaves.
struct AnimatedImage: View {
    let url: URL
    let size: CGSize
    let still: UIImage
    var loader: ImageLoader = .shared

    @Environment(\.displayScale) private var scale
    @State private var frames: AnimatedFrames?
    @State private var start = Date.now

    var body: some View {
        TimelineView(.animation(paused: frames == nil)) { context in
            let image = frames.map { $0.frames[$0.index(at: context.date.timeIntervalSince(start))] } ?? still
            Image(uiImage: image)
                .resizable()
                .scaledToFill()
        }
        .frame(width: size.width, height: size.height)
        .clipped()
        .task(id: url) {
            frames = await loader.frames(url, fitting: size, scale: scale)
            start = .now
        }
        .onDisappear { frames = nil }
    }
}
