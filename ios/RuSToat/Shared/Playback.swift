import AVFoundation
import Observation

/// Something that plays and can be paused: an `AVPlayer`, or a fake in tests.
@MainActor
protocol Pausable: AnyObject {
    func pause()
}

extension AVPlayer: Pausable {}

/// What's playing, app-wide. As on desktop, only one video or voice note
/// plays at a time: starting one pauses the last.
@MainActor
@Observable
final class Playback {
    static let shared = Playback()

    /// The attachment playing (by file ID).
    private(set) var current: String?
    @ObservationIgnored private var player: Pausable?
    @ObservationIgnored private let activateSession: @MainActor () -> Void
    @ObservationIgnored private var sessionActive = false

    /// `activateSession` runs before the first item plays. It's put off
    /// until then, because activating a `.playback` session stops other
    /// apps' audio.
    init(activateSession: @escaping @MainActor () -> Void = Playback.activatePlaybackSession) {
        self.activateSession = activateSession
    }

    /// Media the user chose to play: heard with the silent switch on, as in
    /// other apps.
    static func activatePlaybackSession() {
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playback, mode: .default)
        try? session.setActive(true)
    }

    /// `id` is starting, with `player`: whatever was playing is paused.
    func start(_ id: String, player: Pausable) {
        if !sessionActive {
            activateSession()
            sessionActive = true
        }
        if let previous = self.player, previous !== player {
            previous.pause()
        }
        current = id
        self.player = player
    }

    /// The user paused `id`.
    func pause(_ id: String) {
        guard current == id else { return }
        player?.pause()
        stopped(id)
    }

    /// `id` stopped by itself (it ended, or its row went away).
    func stopped(_ id: String) {
        guard current == id else { return }
        current = nil
        player = nil
    }

    /// "0:05", "12:34", or "1:02:03" for an hour or more. Unknown times
    /// (not yet loaded, or live) are "0:00".
    nonisolated static func timeLabel(_ seconds: Double) -> String {
        guard seconds.isFinite, seconds > 0 else { return "0:00" }
        let total = Int(seconds.rounded(.down))
        let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60)
        return hours > 0
            ? String(format: "%d:%02d:%02d", hours, minutes, secs)
            : String(format: "%d:%02d", minutes, secs)
    }
}
