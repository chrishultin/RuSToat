import Foundation
import Testing
import UIKit
@testable import RuSToat

@MainActor
struct ImageLoaderTests {
    private let url = URL(string: "https://cdn.example/attachments/f-cat")!

    @Test func oneURLAskedTwiceLoadsOnce() async throws {
        let loader = ImageLoader.stubbed()
        let size = CGSize(width: 32, height: 32)
        async let first = loader.image(url, fitting: size, scale: 1)
        async let second = loader.image(url, fitting: size, scale: 1)
        let (a, b) = await (first, second)
        #expect(a != nil && b != nil)
        #expect(loader.downloadsStarted == 1)
        // Now it's in memory: no waiting, and no download.
        #expect(loader.cached(url, fitting: size, scale: 1) != nil)
        _ = await loader.image(url, fitting: size, scale: 1)
        #expect(loader.downloadsStarted == 1)
    }

    @Test func picturesAreDecodedAtTheSizeTheyreDrawn() async throws {
        let loader = ImageLoader.stubbed()
        // The stub's picture is 64×64 pixels.
        let image = try #require(await loader.image(url, fitting: CGSize(width: 16, height: 10), scale: 2))
        #expect(image.cgImage?.width == 32, "the long side at 16 pt × 2")
        #expect(image.size == CGSize(width: 16, height: 16))
        // Never bigger than it is.
        let big = try #require(await loader.image(url, fitting: CGSize(width: 500, height: 500), scale: 3))
        #expect(big.cgImage?.width == 64)
    }

    @Test func pixelSizes() {
        #expect(ImageLoader.pixels(fitting: CGSize(width: 300, height: 200), scale: 3) == 900)
        #expect(ImageLoader.pixels(fitting: CGSize(width: 0.1, height: 0.1), scale: 1) == 1)
    }

    @Test func undecodableDataIsNil() {
        #expect(ImageLoader.decode(Data("not a picture".utf8), maxPixels: 100, scale: 1) == nil)
    }
}
