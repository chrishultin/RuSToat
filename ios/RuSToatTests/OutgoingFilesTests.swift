import Foundation
import ImageIO
import UIKit
import UniformTypeIdentifiers
import Testing
@testable import RuSToat

// Serialized: they share the outgoing folder.
@Suite(.serialized)
struct OutgoingFilesTests {
    @Test func namesAreSafePathComponents() {
        #expect(OutgoingFiles.safeName("minutes.pdf") == "minutes.pdf")
        #expect(OutgoingFiles.safeName("a/b:c.txt") == "a_b_c.txt")
        #expect(OutgoingFiles.safeName("  ") == "file")
        #expect(OutgoingFiles.safeName("..") == "file")
    }

    @Test func copiesKeepTheirNamesInFoldersOfTheirOwn() throws {
        let first = try OutgoingFiles.write(Data("one".utf8), named: "notes.txt")
        let second = try OutgoingFiles.write(Data("two!".utf8), named: "notes.txt")
        #expect(first.url.lastPathComponent == "notes.txt")
        #expect(first.url != second.url)
        #expect(second.size == 4)
        #expect(first.outgoing.name == "notes.txt")
        #expect(first.outgoing.path == first.url.path(percentEncoded: false))
        #expect(!first.isImage)

        OutgoingFiles.remove(first.url.path(percentEncoded: false))
        #expect(!FileManager.default.fileExists(atPath: first.url.path(percentEncoded: false)))
        #expect(!FileManager.default.fileExists(atPath: first.url.deletingLastPathComponent().path(percentEncoded: false)))
        #expect(FileManager.default.fileExists(atPath: second.url.path(percentEncoded: false)))
        OutgoingFiles.remove(second.url.path(percentEncoded: false))
    }

    @Test func onlyOurOwnCopiesAreDeleted() throws {
        let elsewhere = FileManager.default.temporaryDirectory.appending(path: "keep-\(UUID().uuidString).txt")
        try Data("mine".utf8).write(to: elsewhere)
        OutgoingFiles.remove(elsewhere.path(percentEncoded: false))
        #expect(FileManager.default.fileExists(atPath: elsewhere.path(percentEncoded: false)))
        try FileManager.default.removeItem(at: elsewhere)
    }

    @Test func pickedFilesAreCopiedOut() throws {
        let picked = FileManager.default.temporaryDirectory.appending(path: "picked-\(UUID().uuidString).jpg")
        try Data([0xFF, 0xD8, 0xFF]).write(to: picked)
        let file = try OutgoingFiles.copy(picked: picked)
        #expect(file.name == picked.lastPathComponent)
        #expect(file.isImage)
        #expect(file.url.path(percentEncoded: false).hasPrefix(OutgoingFiles.folder.path(percentEncoded: false)))
        #expect(FileManager.default.fileExists(atPath: picked.path(percentEncoded: false)), "the user's file is left alone")
        OutgoingFiles.remove(file.url.path(percentEncoded: false))
        try FileManager.default.removeItem(at: picked)
    }
}

struct PhotoEncodingTests {
    /// A `width` × `height` picture (opaque unless `alpha`), as `type`, with
    /// `properties` (orientation, GPS…) written into it.
    static func picture(
        width: Int, height: Int, type: UTType = .jpeg, alpha: Bool = false,
        properties: [CFString: Any] = [:]
    ) -> Data {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.opaque = !alpha
        let image = UIGraphicsImageRenderer(size: CGSize(width: width, height: height), format: format).image { context in
            UIColor.systemTeal.setFill()
            // Transparent pictures keep their top-left corner clear.
            context.fill(CGRect(x: alpha ? width / 2 : 0, y: 0, width: alpha ? width / 2 : width, height: height))
        }
        let data = NSMutableData()
        let destination = CGImageDestinationCreateWithData(data, type.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, image.cgImage!, properties as CFDictionary)
        CGImageDestinationFinalize(destination)
        return data as Data
    }

    static func properties(_ data: Data) -> [CFString: Any] {
        let source = CGImageSourceCreateWithData(data as CFData, nil)!
        return CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any] ?? [:]
    }

    static func size(_ data: Data) -> (Int, Int) {
        let properties = properties(data)
        return (properties[kCGImagePropertyPixelWidth] as? Int ?? 0, properties[kCGImagePropertyPixelHeight] as? Int ?? 0)
    }

    @Test func bigPhotosComeOutAt4096OnTheLongSide() throws {
        let jpeg = try #require(PhotoEncoding.jpeg(Self.picture(width: 6000, height: 4000)))
        #expect(Self.size(jpeg) == (4096, 2731))
        let small = try #require(PhotoEncoding.jpeg(Self.picture(width: 300, height: 200)))
        #expect(Self.size(small) == (300, 200), "never scaled up")
    }

    @Test func orientationIsAppliedToThePixels() throws {
        // A landscape sensor image the camera says to turn 90° clockwise.
        let rotated = Self.picture(width: 40, height: 20, properties: [kCGImagePropertyOrientation: 6])
        let jpeg = try #require(PhotoEncoding.jpeg(rotated))
        #expect(Self.size(jpeg) == (20, 40))
        let orientation = Self.properties(jpeg)[kCGImagePropertyOrientation] as? Int
        #expect(orientation == nil || orientation == 1, "upright with no turning left to do")
    }

    @Test func locationIsntSent() throws {
        let gps: [CFString: Any] = [
            kCGImagePropertyGPSLatitude: 51.5, kCGImagePropertyGPSLatitudeRef: "N",
            kCGImagePropertyGPSLongitude: 0.12, kCGImagePropertyGPSLongitudeRef: "W",
        ]
        let photo = Self.picture(width: 40, height: 20, properties: [kCGImagePropertyGPSDictionary: gps])
        #expect(Self.properties(photo)[kCGImagePropertyGPSDictionary] != nil, "the test photo has a location")
        let jpeg = try #require(PhotoEncoding.jpeg(photo))
        #expect(Self.properties(jpeg)[kCGImagePropertyGPSDictionary] == nil)
    }

    @Test func transparentPNGsAreSentAsJPEGOnWhite() throws {
        let png = Self.picture(width: 40, height: 20, type: .png, alpha: true)
        let jpeg = try #require(PhotoEncoding.jpeg(png))
        let source = try #require(CGImageSourceCreateWithData(jpeg as CFData, nil))
        #expect(CGImageSourceGetType(source) as String? == UTType.jpeg.identifier)
        // The clear corner is white, not black.
        let image = try #require(UIImage(data: jpeg)?.cgImage)
        var pixel = [UInt8](repeating: 0, count: 4)
        let context = CGContext(
            data: &pixel, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
        )!
        context.draw(image, in: CGRect(x: 0, y: -19, width: 40, height: 20))
        #expect(pixel[0] > 240 && pixel[1] > 240 && pixel[2] > 240, "\(pixel)")
    }

    @Test @MainActor func cameraShotsAreUprightJPEGs() throws {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let sensor = UIGraphicsImageRenderer(size: CGSize(width: 40, height: 20), format: format).image { _ in }
        // As the camera hands it over: pixels sideways, orientation `.right`.
        let shot = UIImage(cgImage: sensor.cgImage!, scale: 1, orientation: .right)
        let jpeg = try #require(PhotoEncoding.jpeg(shot))
        #expect(Self.size(jpeg) == (20, 40))
    }

    @Test func photosHaveNamesOfTheirOwn() {
        let date = Date(timeIntervalSince1970: 1_800_000_000)
        let first = OutgoingFiles.photoName(date)
        let second = OutgoingFiles.photoName(date)
        #expect(first.hasPrefix("IMG_2027"))
        #expect(first.hasSuffix(".jpg"))
        #expect(first != second)
    }
}
