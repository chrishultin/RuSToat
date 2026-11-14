import Foundation
import Testing
import UIKit
@testable import RuSToat

/// Answers every request with a 64×64-pixel PNG, counting them.
final class StubImageProtocol: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var requests = 0
    static let png: Data = UIGraphicsImageRenderer(
        size: CGSize(width: 64, height: 64),
        format: { let format = UIGraphicsImageRendererFormat(); format.scale = 1; return format }()
    ).pngData { context in
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

extension ImageLoader {
    /// A loader whose downloads are answered by `StubImageProtocol`.
    static func stubbed() -> ImageLoader {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [StubImageProtocol.self]
        return ImageLoader(session: URLSession(configuration: configuration))
    }
}

@MainActor
struct EmojiImagesTests {
    @Test func eachURLLoadsOnce() async throws {
        let loader = ImageLoader.stubbed()
        let images = EmojiImages(loader: loader)
        let url = "https://cdn.example/emojis/01PARTY"
        #expect(images.image(url, size: 20) == nil, "not loaded yet")
        #expect(images.image(url, size: 20) == nil)
        for _ in 0..<100 where images.image(url, size: 20) == nil {
            try await Task.sleep(for: .milliseconds(10))
        }
        let image = try #require(images.image(url, size: 20))
        #expect(image.size == CGSize(width: 20, height: 20), "scaled to the inline size")
        #expect(loader.downloadsStarted == 1, "asking again while it loads doesn't load it again")
    }
}
