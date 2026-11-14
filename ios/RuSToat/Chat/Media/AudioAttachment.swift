import AVFoundation
import Observation
import SwiftUI

/// An audio attachment: its card, with a player under it (play/pause, a
/// scrubber, and the time). Starting it pauses anything else playing.
struct AudioAttachmentView: View {
    let audio: AudioAttachment
    @State private var model: AudioPlayerModel

    init(audio: AudioAttachment) {
        self.audio = audio
        _model = State(initialValue: AudioPlayerModel(id: audio.id, url: URL(string: audio.url)))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                Image(systemName: "waveform")
                    .font(.title3)
                    .foregroundStyle(.tint)
                    .frame(width: 28)
                VStack(alignment: .leading) {
                    Text(audio.name).lineLimit(1)
                    HStack(spacing: 6) {
                        Text(audio.sizeLabel)
                        if audio.spoiler {
                            Text("Spoiler").fontWeight(.semibold)
                        }
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
            }
            HStack(spacing: 10) {
                Button(model.playing ? "Pause" : "Play",
                       systemImage: model.playing ? "pause.fill" : "play.fill",
                       action: model.toggle)
                    .labelStyle(.iconOnly)
                    .font(.title3)
                    .frame(width: 28)
                    .contentTransition(.symbolEffect(.replace))
                Slider(
                    value: Binding(get: { model.elapsed }, set: { model.elapsed = $0 }),
                    in: 0...max(model.duration, 0.01)
                ) { editing in
                    model.scrub(editing: editing)
                }
                .disabled(model.duration <= 0)
                // The length to the nearest second (3.99 s is "0:04"); the time
                // played so far counts whole seconds.
                Text("\(Playback.timeLabel(model.elapsed)) / \(Playback.timeLabel(model.duration.rounded()))")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                    .fixedSize()
            }
        }
        .padding(10)
        .frame(maxWidth: 320, alignment: .leading)
        .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 12))
        .task {
            await model.loadDuration()
            #if DEBUG
            // As for videos: SIMCTL_CHILD_RUSTOAT_DEV_PLAY=<file ID>.
            if ProcessInfo.processInfo.environment["RUSTOAT_DEV_PLAY"] == audio.id, !model.playing {
                model.toggle()
            }
            #endif
        }
        .onDisappear(perform: model.release)
    }
}

/// One audio attachment's player, made on first play and let go when its
/// row leaves the screen.
@MainActor
@Observable
final class AudioPlayerModel {
    let id: String
    let url: URL?
    var elapsed: Double = 0
    private(set) var duration: Double = 0
    @ObservationIgnored private var player: AVPlayer?
    @ObservationIgnored private var timeObserver: Any?
    @ObservationIgnored private var endObserver: NSObjectProtocol?
    @ObservationIgnored private var scrubbing = false

    init(id: String, url: URL?) {
        self.id = id
        self.url = url
    }

    var playing: Bool { Playback.shared.current == id }

    /// The length, from the file's header, before anything plays.
    func loadDuration() async {
        guard duration == 0, let url,
              let time = try? await AVURLAsset(url: url).load(.duration),
              time.seconds.isFinite else { return }
        duration = time.seconds
    }

    func toggle() {
        if playing {
            Playback.shared.pause(id)
        } else {
            play()
        }
    }

    private func play() {
        guard let player = player ?? makePlayer() else { return }
        Playback.shared.start(id, player: player)
        player.play()
    }

    private func makePlayer() -> AVPlayer? {
        guard let url else { return nil }
        let player = AVPlayer(url: url)
        if elapsed > 0 {
            player.seek(to: CMTime(seconds: elapsed, preferredTimescale: 600))
        }
        timeObserver = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 0.25, preferredTimescale: 600),
            queue: .main
        ) { [weak self] time in
            MainActor.assumeIsolated {
                guard let self, !self.scrubbing, time.seconds.isFinite else { return }
                self.elapsed = time.seconds
            }
        }
        endObserver = NotificationCenter.default.addObserver(
            forName: AVPlayerItem.didPlayToEndTimeNotification,
            object: player.currentItem,
            queue: .main
        ) { [weak self] _ in
            // Finished: back to the start, ready to play again.
            MainActor.assumeIsolated {
                guard let self else { return }
                self.seek(to: 0)
                Playback.shared.stopped(self.id)
            }
        }
        self.player = player
        return player
    }

    /// The scrubber was picked up (`editing`) or let go: seek there.
    func scrub(editing: Bool) {
        scrubbing = editing
        if !editing {
            seek(to: elapsed)
        }
    }

    private func seek(to seconds: Double) {
        elapsed = seconds
        player?.seek(to: CMTime(seconds: seconds, preferredTimescale: 600))
    }

    func release() {
        if playing {
            Playback.shared.pause(id)
        }
        if let timeObserver {
            player?.removeTimeObserver(timeObserver)
        }
        if let endObserver {
            NotificationCenter.default.removeObserver(endObserver)
        }
        timeObserver = nil
        endObserver = nil
        player = nil
    }
}
