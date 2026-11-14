import Foundation
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
