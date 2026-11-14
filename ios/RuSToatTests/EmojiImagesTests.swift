import Foundation
import Testing
import UIKit
@testable import RuSToat

/// Answers every request with a 64×64 PNG, counting them.
final class StubImageProtocol: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var requests = 0
    static let png: Data = UIGraphicsImageRenderer(size: CGSize(width: 64, height: 64)).pngData { context in
        UIColor.systemPink.setFill()
        context.fill(CGRect(x: 0, y: 0, width: 64, height: 64))
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.requests += 1
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil,
                                       headerFields: ["Content-Type": "image/png"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Self.png)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

@MainActor
struct EmojiImagesTests {
    private func stubbed() -> EmojiImages {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [StubImageProtocol.self]
        return EmojiImages(session: URLSession(configuration: configuration))
    }

    @Test func eachURLLoadsOnce() async throws {
        let images = stubbed()
        let url = "https://cdn.example/emojis/01PARTY"
        #expect(images.image(url, size: 20) == nil, "not loaded yet")
        #expect(images.image(url, size: 20) == nil)
        #expect(images.loadsStarted == 1, "asking again while it loads doesn't load it again")
        for _ in 0..<100 where images.image(url, size: 20) == nil {
            try await Task.sleep(for: .milliseconds(10))
        }
        let image = try #require(images.image(url, size: 20))
        #expect(image.size == CGSize(width: 20, height: 20), "scaled to the inline size")
        #expect(images.loadsStarted == 1)
    }

    @Test func scalingKeepsTheShape() {
        let wide = UIGraphicsImageRenderer(size: CGSize(width: 100, height: 50)).image { _ in }
        #expect(EmojiImages.scaled(wide, to: 20).size == CGSize(width: 20, height: 10))
    }
}
