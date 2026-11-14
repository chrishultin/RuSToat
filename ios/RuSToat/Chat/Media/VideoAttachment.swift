import AVKit
import SwiftUI

/// A video attachment at its display size: its first frame with a play
/// button until tapped, then the system player inline (muted until the user
/// turns the sound on), with a button to go full screen. A spoiler is
/// covered until tapped, with no poster made meanwhile.
struct VideoAttachmentView: View {
    let video: VideoAttachment
    let uncover: (String) -> Void
    var loader: ImageLoader = .shared

    @Environment(\.mediaMaxWidth) private var available
    @Environment(\.displayScale) private var scale
    @State private var poster: UIImage?
    @State private var player: AVPlayer?
    /// Full screen is up: leaving the row then mustn't stop the video.
    @State private var fullScreen = false

    var body: some View {
        let size = MediaLayout.fit(width: video.width, height: video.height, available: available)
        Group {
            if video.spoiler {
                SpoilerCover(thumbhash: nil) { uncover(video.id) }
            } else if let player {
                VideoPlayer(player: player)
                    .overlay(alignment: .topTrailing) { fullScreenButton(player) }
            } else {
                posterButton(size)
            }
        }
        .frame(width: size.width, height: size.height)
        .background(Color(white: 0.1))
        .clipShape(.rect(cornerRadius: 12))
        .task(id: video.url) {
            guard !video.spoiler, poster == nil, let url = URL(string: video.url) else { return }
            poster = loader.cachedPoster(url, fitting: size, scale: scale)
            if poster == nil {
                let frame = await loader.poster(url, fitting: size, scale: scale)
                withAnimation(.easeOut(duration: 0.2)) { poster = frame }
            }
            #if DEBUG
            // Start playing at launch, for screenshots:
            // SIMCTL_CHILD_RUSTOAT_DEV_PLAY=<file ID> xcrun simctl launch …
            if ProcessInfo.processInfo.environment["RUSTOAT_DEV_PLAY"] == video.id, player == nil {
                play()
            }
            #endif
        }
        // A list never holds many players: one that scrolls away goes back
        // to its poster.
        .onDisappear {
            guard !fullScreen, let player else { return }
            player.pause()
            Playback.shared.stopped(video.id)
            self.player = nil
        }
    }

    private func posterButton(_ size: CGSize) -> some View {
        Button(action: play) {
            ZStack {
                if let poster {
                    Image(uiImage: poster)
                        .resizable()
                        .scaledToFill()
                        .transition(.opacity)
                }
                Image(systemName: "play.fill")
                    .font(.title2)
                    .foregroundStyle(.white)
                    .frame(width: 54, height: 54)
                    .background(.black.opacity(0.55), in: .circle)
            }
            .frame(width: size.width, height: size.height)
            .clipped()
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Play \(video.name)")
    }

    private func fullScreenButton(_ player: AVPlayer) -> some View {
        Button("Full Screen", systemImage: "arrow.up.left.and.arrow.down.right") {
            fullScreen = true
            player.isMuted = false
            FullScreenVideo.present(player) { fullScreen = false }
        }
        .labelStyle(.iconOnly)
        .font(.footnote.weight(.semibold))
        .foregroundStyle(.white)
        .frame(width: 32, height: 32)
        .background(.black.opacity(0.5), in: .circle)
        .padding(8)
    }

    private func play() {
        guard let url = URL(string: video.url) else { return }
        let player = AVPlayer(url: url)
        player.isMuted = true
        self.player = player
        Playback.shared.start(video.id, player: player)
        player.play()
    }
}

/// The system's full-screen player, presented over everything, sharing the
/// inline player so the video carries on where it was.
@MainActor
enum FullScreenVideo {
    static func present(_ player: AVPlayer, onClose: @escaping () -> Void) {
        let controller = ClosingPlayerController()
        controller.player = player
        controller.onClose = onClose
        controller.modalPresentationStyle = .fullScreen
        guard let top = topViewController() else {
            onClose()
            return
        }
        top.present(controller, animated: true) { player.play() }
    }

    private static func topViewController() -> UIViewController? {
        let window = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap { $0.windows }
            .first { $0.isKeyWindow }
        var top = window?.rootViewController
        while let presented = top?.presentedViewController {
            top = presented
        }
        return top
    }
}

/// An `AVPlayerViewController` that says when it's been closed.
final class ClosingPlayerController: AVPlayerViewController {
    var onClose: () -> Void = {}

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        if isBeingDismissed {
            onClose()
        }
    }
}
