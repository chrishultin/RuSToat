//! AVFoundation plays a file and reports its duration. Offline and silent:
//! a generated second of silence, muted.
//!
//! Not a normal test: a player lives on the main thread, and only updates
//! while the main run loop runs, so this runs as `main` (`harness = false`
//! in Cargo.toml) and runs the loop while it waits.

#[cfg(target_os = "macos")]
fn main() {
    use std::time::{Duration, Instant};

    use stoat_gpui::media_player::{Player, PlayerStatus, run_main_loop_for};

    // Off the main thread, there's no player.
    let elsewhere = std::thread::spawn(|| Player::open("file:///nothing.wav").is_err());
    assert!(
        elsewhere.join().unwrap(),
        "a player opened off the main thread"
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("second.wav");
    std::fs::write(&path, silent_wav(8000, 1.0)).unwrap();
    let url = url::Url::from_file_path(&path).unwrap();

    let player = Player::open(url.as_str()).expect("opens");
    player.set_muted(true);
    assert!(player.is_muted());

    let deadline = Instant::now() + Duration::from_secs(5);
    let snapshot = loop {
        run_main_loop_for(Duration::from_millis(50));
        let snapshot = player.snapshot();
        match snapshot.status {
            PlayerStatus::Ready if snapshot.duration.is_some() => break snapshot,
            PlayerStatus::Failed(error) => panic!("failed: {error}"),
            _ if Instant::now() > deadline => panic!("not ready in 5 s: {snapshot:?}"),
            _ => {}
        }
    };
    let duration = snapshot.duration.unwrap();
    assert!((duration - 1.0).abs() <= 0.05, "duration {duration}");
    assert!(!snapshot.playing);
    assert_eq!(snapshot.position, 0.0);

    // An unreadable file fails, with AVFoundation's error.
    let junk = dir.path().join("junk.wav");
    std::fs::write(&junk, b"not a wave file").unwrap();
    let junk = Player::open(url::Url::from_file_path(&junk).unwrap().as_str()).unwrap();
    junk.set_muted(true);
    let deadline = Instant::now() + Duration::from_secs(5);
    let error = loop {
        run_main_loop_for(Duration::from_millis(50));
        if let PlayerStatus::Failed(error) = junk.snapshot().status {
            break error;
        }
        assert!(Instant::now() < deadline, "junk didn't fail in 5 s");
    };
    assert!(error.contains("AVFoundationErrorDomain"), "{error}");
    assert!(!junk.snapshot().playing);

    println!("media_player: ok ({duration:.3} s; junk: {error})");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("media_player: skipped (playback needs macOS)");
}

/// A mono 16-bit PCM WAV of `seconds` of silence at `rate` Hz.
#[cfg(target_os = "macos")]
fn silent_wav(rate: u32, seconds: f64) -> Vec<u8> {
    let samples = (f64::from(rate) * seconds) as u32;
    let data_len = samples * 2;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes()); // bytes per second
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.resize(44 + data_len as usize, 0);
    wav
}
