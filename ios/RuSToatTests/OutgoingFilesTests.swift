import Foundation
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
