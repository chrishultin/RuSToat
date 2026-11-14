import AVFoundation
import Foundation
import ImageIO
import Synchronization
import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// A file waiting in a channel's tray, under the message box: RuSToat's own
/// copy in the temporary folder.
struct TrayFile: Identifiable, Equatable {
    let id = UUID()
    let url: URL
    /// As the user's file is named.
    let name: String
    let size: Int64
    var spoiler = false

    var sizeLabel: String {
        ByteCountFormatter.string(fromByteCount: size, countStyle: .file)
    }

    var isImage: Bool {
        UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) ?? false
    }

    /// What the core sends.
    var outgoing: OutgoingFile {
        OutgoingFile(path: url.path(percentEncoded: false), name: name, spoiler: spoiler)
    }
}

/// Files being sent live in `tmp/outgoing/<uuid>/<name>` (each in its own
/// folder, so they keep their names) until their message is sent or
/// discarded: the core hands them back through `released_files()`, and
/// they're deleted. Anything left from a previous run is cleared at launch.
enum OutgoingFiles {
    static var folder: URL {
        FileManager.default.temporaryDirectory.appending(path: "outgoing", directoryHint: .isDirectory)
    }

    /// A fresh folder for one file.
    static func newFolder() throws -> URL {
        let folder = folder.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    /// Copy a file the user picked in Files (a security-scoped URL, which
    /// is only readable while access is held) into the outgoing folder.
    static func copy(picked url: URL) throws -> TrayFile {
        let scoped = url.startAccessingSecurityScopedResource()
        defer {
            if scoped { url.stopAccessingSecurityScopedResource() }
        }
        let name = safeName(url.lastPathComponent)
        let copy = try newFolder().appending(path: name)
        try FileManager.default.copyItem(at: url, to: copy)
        return try tray(copy, name: name)
    }

    /// `data` as a file called `name` in the outgoing folder.
    static func write(_ data: Data, named name: String) throws -> TrayFile {
        let name = safeName(name)
        let file = try newFolder().appending(path: name)
        try data.write(to: file, options: .atomic)
        return try tray(file, name: name)
    }

    /// A photo (any format ImageIO reads) re-encoded as a JPEG in the
    /// outgoing folder (see `PhotoEncoding`).
    static func photo(_ data: Data, named name: String = photoName()) async throws -> TrayFile {
        let jpeg = await Task.detached(priority: .userInitiated) { PhotoEncoding.jpeg(data) }.value
        guard let jpeg else { throw AttachFailure("Couldn't read that photo.") }
        return try write(jpeg, named: name)
    }

    /// A camera shot, upright, as a JPEG in the outgoing folder.
    @MainActor
    static func photo(_ image: UIImage) async throws -> TrayFile {
        guard let jpeg = PhotoEncoding.jpeg(image) else { throw AttachFailure("Couldn't save that photo.") }
        return try write(jpeg, named: photoName())
    }

    /// What the user picked in Photos: a video copied as it is (as MP4 if
    /// it isn't one), or a photo re-encoded as a JPEG.
    static func file(from item: PhotosPickerItem) async throws -> TrayFile {
        if item.supportedContentTypes.contains(where: { $0.conforms(to: .movie) }) {
            guard let movie = try await item.loadTransferable(type: PickedMovie.self) else {
                throw AttachFailure("Couldn't read that video.")
            }
            let url = await mp4(movie.url)
            return try tray(url, name: url.lastPathComponent)
        }
        guard let data = try await item.loadTransferable(type: Data.self) else {
            throw AttachFailure("Couldn't read that photo.")
        }
        return try await photo(data)
    }

    /// `url` as an MP4, exported beside it (the original deleted), unless
    /// it is one already. Copied as it is when it can be (`Passthrough`),
    /// re-encoded when not; sent as it was if neither works.
    static func mp4(_ url: URL) async -> URL {
        guard url.pathExtension.lowercased() != "mp4" else { return url }
        let output = url.deletingPathExtension().appendingPathExtension("mp4")
        let asset = AVURLAsset(url: url)
        for preset in [AVAssetExportPresetPassthrough, AVAssetExportPresetHighestQuality] {
            guard let session = AVAssetExportSession(asset: asset, presetName: preset) else { continue }
            do {
                try await session.export(to: output, as: .mp4)
                try? FileManager.default.removeItem(at: url)
                return output
            } catch {
                try? FileManager.default.removeItem(at: output)
            }
        }
        return url
    }

    /// "IMG_20261114_114805_3.jpg": photos have no name of their own.
    static func photoName(_ date: Date = .now) -> String {
        let n = photoCount.withLock { count in
            count += 1
            return count
        }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyyMMdd_HHmmss"
        return "IMG_\(formatter.string(from: date))_\(n).jpg"
    }

    /// Photos named so far, so two in one second differ.
    private static let photoCount = Mutex(0)

    private static func tray(_ url: URL, name: String) throws -> TrayFile {
        let size = try FileManager.default.attributesOfItem(atPath: url.path(percentEncoded: false))[.size] as? Int64 ?? 0
        return TrayFile(url: url, name: name, size: size)
    }

    /// A name that's safe as one path component.
    static func safeName(_ name: String) -> String {
        let cleaned = name
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: ":", with: "_")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return cleaned.isEmpty || cleaned == "." || cleaned == ".." ? "file" : cleaned
    }

    /// Delete one of our copies, with its folder. Paths outside the outgoing
    /// folder are left alone.
    static func remove(_ path: String) {
        let file = URL(filePath: path).standardizedFileURL
        let root = folder.standardizedFileURL.path(percentEncoded: false)
        guard file.path(percentEncoded: false).hasPrefix(root) else { return }
        try? FileManager.default.removeItem(at: file.deletingLastPathComponent())
    }

    /// Everything left from before (the app was quit with files in a tray,
    /// or mid-send).
    static func removeAll() {
        try? FileManager.default.removeItem(at: folder)
    }
}

/// Why a file couldn't be attached, for the user.
struct AttachFailure: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

/// A video from Photos, copied into the outgoing folder as it arrives.
struct PickedMovie: Transferable {
    let url: URL

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(contentType: .movie) { movie in
            SentTransferredFile(movie.url)
        } importing: { received in
            let name = OutgoingFiles.safeName(received.file.lastPathComponent)
            let copy = try OutgoingFiles.newFolder().appending(path: name)
            try FileManager.default.copyItem(at: received.file, to: copy)
            return PickedMovie(url: copy)
        }
    }
}

/// How photos are sent: as JPEG (Stoat's web client can't show HEIC), at
/// most 4096 px on the long side, quality 0.85, upright, and with no
/// metadata, so no location goes with them. Transparency is flattened onto
/// white.
enum PhotoEncoding {
    static let maxPixels = 4096
    static let quality = 0.85

    /// `data` (any format ImageIO reads) as such a JPEG.
    static func jpeg(_ data: Data) -> Data? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(source) > 0 else { return nil }
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        let side = max(
            properties?[kCGImagePropertyPixelWidth] as? Int ?? 0,
            properties?[kCGImagePropertyPixelHeight] as? Int ?? 0,
            1
        )
        let options = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            // EXIF orientation applied to the pixels: upright everywhere.
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: min(maxPixels, side),
        ] as CFDictionary
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options) else { return nil }
        return jpeg(image)
    }

    /// A camera shot (already oriented as `UIImage`s are) as such a JPEG.
    @MainActor
    static func jpeg(_ image: UIImage) -> Data? {
        let pixels = CGSize(width: image.size.width * image.scale, height: image.size.height * image.scale)
        let scale = min(1, CGFloat(maxPixels) / max(pixels.width, pixels.height, 1))
        let size = CGSize(width: (pixels.width * scale).rounded(), height: (pixels.height * scale).rounded())
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.opaque = true
        let upright = UIGraphicsImageRenderer(size: size, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: size))
        }
        return upright.cgImage.flatMap(jpeg)
    }

    /// `image` as a JPEG, written with no metadata at all.
    static func jpeg(_ image: CGImage) -> Data? {
        guard let opaque = flattened(image) else { return nil }
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, UTType.jpeg.identifier as CFString, 1, nil)
        else { return nil }
        let options = [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary
        CGImageDestinationAddImage(destination, opaque, options)
        return CGImageDestinationFinalize(destination) ? data as Data : nil
    }

    /// `image` on white, if it has transparency (JPEG hasn't).
    static func flattened(_ image: CGImage) -> CGImage? {
        switch image.alphaInfo {
        case .none, .noneSkipFirst, .noneSkipLast:
            return image
        default:
            let rect = CGRect(x: 0, y: 0, width: image.width, height: image.height)
            guard let context = CGContext(
                data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: 0,
                space: image.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
            ) else { return nil }
            context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
            context.fill(rect)
            context.draw(image, in: rect)
            return context.makeImage()
        }
    }
}

/// The camera (the photo library on a simulator, which has none): hands
/// back the shot.
struct CameraPicker: UIViewControllerRepresentable {
    let taken: (UIImage) -> Void
    @Environment(\.dismiss) private var dismiss

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = UIImagePickerController.isSourceTypeAvailable(.camera) ? .camera : .photoLibrary
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ picker: UIImagePickerController, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let parent: CameraPicker

        init(_ parent: CameraPicker) {
            self.parent = parent
        }

        func imagePickerController(
            _ picker: UIImagePickerController,
            didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]
        ) {
            if let image = info[.originalImage] as? UIImage {
                parent.taken(image)
            }
            parent.dismiss()
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
            parent.dismiss()
        }
    }
}
