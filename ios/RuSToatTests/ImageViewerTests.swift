import CoreGraphics
import Foundation
import Testing
import UIKit
@testable import RuSToat

struct ImageViewerTests {
    @Test func pagingStaysWithinTheMessagesImages() {
        #expect(ViewerMath.page(0, by: 1, count: 3) == 1)
        #expect(ViewerMath.page(2, by: 1, count: 3) == 2)
        #expect(ViewerMath.page(0, by: -1, count: 3) == 0)
        #expect(ViewerMath.page(0, by: 1, count: 0) == 0)
    }

    @Test func zoomIsClamped() {
        #expect(ViewerMath.clampZoom(0.5) == 1)
        #expect(ViewerMath.clampZoom(2) == 2)
        #expect(ViewerMath.clampZoom(10) == ViewerMath.maxZoom)
    }

    @Test func doubleTapZoomsInThenOut() {
        #expect(ViewerMath.doubleTapTarget(from: 1) == ViewerMath.doubleTapZoom)
        #expect(ViewerMath.doubleTapTarget(from: 1.5) == 1)
        #expect(ViewerMath.doubleTapTarget(from: 4) == 1)
    }

    @Test func picturesFitTheScreenKeepingTheirShape() {
        let screen = CGSize(width: 400, height: 800)
        let wide = ViewerMath.fitted(CGSize(width: 300, height: 200), in: screen)
        #expect(wide.width == 400 && abs(wide.height - 800.0 / 3) < 0.001)
        #expect(ViewerMath.fitted(CGSize(width: 100, height: 400), in: screen) == CGSize(width: 200, height: 800))
        #expect(ViewerMath.fitted(.zero, in: screen) == screen)
    }

    @Test func zoomingCentresTheTappedPoint() {
        let rect = ViewerMath.zoomRect(around: CGPoint(x: 100, y: 200), scale: 2, bounds: CGSize(width: 400, height: 800))
        #expect(rect == CGRect(x: 0, y: 0, width: 200, height: 400))
    }

    @Test func farOrFastDragsDownClose() {
        #expect(ViewerMath.dismisses(translation: 150, velocity: 0))
        #expect(ViewerMath.dismisses(translation: 40, velocity: 1200))
        #expect(!ViewerMath.dismisses(translation: 60, velocity: 200))
        #expect(!ViewerMath.dismisses(translation: 10, velocity: 2000), "a flick that barely moved")
    }

    @Test func aMessagesShownPicturesAreOfferedInOrder() throws {
        func image(_ id: String, tooBig: Bool = false, spoiler: Bool = false) -> AttachmentItem {
            .image(image: ImageAttachment(
                id: id, url: "https://cdn.example/attachments/\(id)", name: "\(id).jpg", sizeLabel: "1 KB",
                width: 400, height: 300, originalWidth: 800, originalHeight: 600,
                thumbhash: nil, tooBig: tooBig, spoiler: spoiler
            ))
        }
        let embed = EmbedImageItem(
            url: "https://proxy.example/proxy?url=x", source: "https://pics.example/tower.png",
            width: 200, height: 300, originalWidth: 200, originalHeight: 300
        )
        let message = MessageItem(
            id: "m", key: "m", author: "Ada", authorId: nil, avatarUrl: nil, time: "", body: nil,
            rich: [], relativeTime: false, renderKey: "", system: false, edited: false, pinned: false,
            continuation: false, mine: false, jumbo: false, pending: nil, replies: [],
            attachments: [image("a"), image("big", tooBig: true), image("hidden", spoiler: true), image("b")],
            embeds: [.image(image: embed), .video(url: "https://v.example/v.mp4")],
            reactions: []
        )
        let images = ViewerImage.images(of: message, available: 300)
        #expect(images.map(\.id) == ["a", "b", embed.url], "not the too-big one or the covered spoiler")
        #expect(images[0].inlineSize == CGSize(width: 300, height: 225))
        #expect(images[2].name == "tower.png")
        let request = try #require(ViewerRequest(images: images, showing: "b"))
        #expect(request.start == 1)
        #expect(ViewerRequest(images: images, showing: "big") == nil)
    }

    @Test func sharedFilesAreNamedForTheirType() {
        let png = UIGraphicsImageRenderer(size: CGSize(width: 2, height: 2)).pngData { _ in }
        #expect(ViewerFiles.fileName("photo.jpg", for: png) == "photo.jpg")
        #expect(ViewerFiles.fileName("proxy", for: png) == "proxy.png")
        #expect(ViewerFiles.fileName("a/b", for: Data()) == "a_b")
    }
}
