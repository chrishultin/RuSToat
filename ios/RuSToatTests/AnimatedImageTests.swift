import Foundation
import ImageIO
import Testing
import UIKit
import UniformTypeIdentifiers
@testable import RuSToat

struct AnimatedImageTests {
    /// A GIF of `count` frames, 40×20 pixels, each showing for `delay`.
    static func gif(frames count: Int, delay: Double = 0.25) -> Data {
        let data = NSMutableData()
        let destination = CGImageDestinationCreateWithData(data, UTType.gif.identifier as CFString, count, nil)!
        let loop = [kCGImagePropertyGIFDictionary: [kCGImagePropertyGIFLoopCount: 0]] as CFDictionary
        CGImageDestinationSetProperties(destination, loop)
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        for index in 0..<count {
            let frame = UIGraphicsImageRenderer(size: CGSize(width: 40, height: 20), format: format).image { context in
                (index.isMultiple(of: 2) ? UIColor.red : UIColor.blue).setFill()
                context.fill(CGRect(x: 0, y: 0, width: 40, height: 20))
            }
            let properties = [kCGImagePropertyGIFDictionary: [kCGImagePropertyGIFDelayTime: delay]] as CFDictionary
            CGImageDestinationAddImage(destination, frame.cgImage!, properties)
        }
        CGImageDestinationFinalize(destination)
        return data as Data
    }

    @Test func framesAndDelaysOfATwoFrameGIF() throws {
        let data = Self.gif(frames: 2)
        #expect(AnimatedFrames.isGIF(data))
        #expect(AnimatedFrames.frameCount(data) == 2)
        let frames = try #require(AnimatedFrames.decode(data, maxPixels: 20, scale: 1))
        #expect(frames.frames.count == 2)
        #expect(frames.delays.count == 2)
        #expect(frames.delays.allSatisfy { abs($0 - 0.25) < 0.001 })
        #expect(frames.frames[0].cgImage?.width == 20, "decoded at the size drawn")
        #expect(abs(frames.duration - 0.5) < 0.001)
    }

    @Test func longGIFsArentAnimated() {
        #expect(AnimatedFrames.decode(Self.gif(frames: AnimatedFrames.maxFrames + 1), maxPixels: 20, scale: 1) == nil)
        #expect(AnimatedFrames.decode(Self.gif(frames: AnimatedFrames.maxFrames), maxPixels: 20, scale: 1) != nil)
    }

    @Test func stillsArentAnimated() {
        #expect(AnimatedFrames.decode(Self.gif(frames: 1), maxPixels: 20, scale: 1) == nil)
        let png = UIGraphicsImageRenderer(size: CGSize(width: 2, height: 2)).pngData { _ in }
        #expect(!AnimatedFrames.isGIF(png))
        #expect(AnimatedFrames.frameCount(png) == 0)
    }

    @Test func framesLoopByTheirDelays() {
        let delays = [0.1, 0.2, 0.3]
        #expect(AnimatedFrames.index(at: 0, delays: delays) == 0)
        #expect(AnimatedFrames.index(at: 0.15, delays: delays) == 1)
        #expect(AnimatedFrames.index(at: 0.45, delays: delays) == 2)
        #expect(AnimatedFrames.index(at: 0.65, delays: delays) == 0, "round again")
        #expect(AnimatedFrames.index(at: 1, delays: []) == 0)
    }

    @Test func bigGIFsAreDecodedSmallerToFitTheBudget() {
        // Within budget: as asked, but never bigger than the GIF.
        #expect(AnimatedFrames.side(maxPixels: 900, width: 480, height: 270, frames: 50) == 480)
        // 200 frames of 900×600 would be 432 MB.
        let side = AnimatedFrames.side(maxPixels: 900, width: 1800, height: 1200, frames: 200)
        #expect(side < 900)
        let bytes = 4 * side * (side * 2 / 3) * 200
        #expect(bytes <= AnimatedFrames.memoryBudget)
    }
}
