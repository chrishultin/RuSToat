import Testing
@testable import RuSToat

@MainActor
final class FakePlayer: Pausable {
    var paused = false
    func pause() { paused = true }
}

@MainActor
struct PlaybackTests {
    @Test func startingASecondItemPausesTheFirst() {
        var activations = 0
        let playback = Playback { activations += 1 }
        let (first, second) = (FakePlayer(), FakePlayer())
        #expect(activations == 0, "the audio session waits until something plays")
        playback.start("a", player: first)
        #expect(playback.current == "a")
        playback.start("b", player: second)
        #expect(first.paused)
        #expect(!second.paused)
        #expect(playback.current == "b")
        #expect(activations == 1)
    }

    @Test func pausingAndStopping() {
        let playback = Playback {}
        let player = FakePlayer()
        playback.start("a", player: player)
        playback.pause("other")
        #expect(!player.paused, "only what's playing")
        playback.pause("a")
        #expect(player.paused)
        #expect(playback.current == nil)

        playback.start("b", player: FakePlayer())
        playback.stopped("a")
        #expect(playback.current == "b", "an old item ending doesn't stop the new one")
        playback.stopped("b")
        #expect(playback.current == nil)
    }

    @Test func restartingTheSameItemDoesntPauseIt() {
        let playback = Playback {}
        let player = FakePlayer()
        playback.start("a", player: player)
        playback.start("a", player: player)
        #expect(!player.paused)
    }

    @Test func timeLabels() {
        #expect(Playback.timeLabel(0) == "0:00")
        #expect(Playback.timeLabel(5.9) == "0:05")
        #expect(Playback.timeLabel(754) == "12:34")
        #expect(Playback.timeLabel(3723) == "1:02:03")
        #expect(Playback.timeLabel(.nan) == "0:00")
        #expect(Playback.timeLabel(.infinity) == "0:00")
    }
}
