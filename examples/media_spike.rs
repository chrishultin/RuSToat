//! Milestone 31's spike: does AVFoundation play a file (a Stoat attachment's
//! link, or a `file://` URL), and make a poster frame for it?
//!
//! ```sh
//! cargo run --example media_spike -- [--audible] [--sleep] <url> [poster.png]
//! ```
//!
//! Muted unless `--audible` is given: the player runs, but makes no sound.
//! Plays the URL for 5 s, printing a snapshot every 250 ms, then seeks to
//! the middle and plays 2 s more. With a second argument, writes the
//! poster frame (at most 800 × 600) to that PNG first. The main run loop
//! runs between polls, as GPUI's does; with `--sleep`, the thread sleeps
//! instead, to see whether the player updates without it.

use std::time::{Duration, Instant};

use stoat_gpui::media_player::{Player, PlayerStatus, poster, run_main_loop_for};

const POLL: Duration = Duration::from_millis(250);

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let audible = args.next_if(|arg| arg == "--audible").is_some();
    let sleep = args.next_if(|arg| arg == "--sleep").is_some();
    let Some(url) = args.next() else {
        eprintln!("usage: media_spike [--audible] [--sleep] <url> [poster.png]");
        std::process::exit(2);
    };

    if let Some(path) = args.next() {
        let start = Instant::now();
        match poster(&url, (800, 600)) {
            Ok(image) => {
                let (width, height) = image.dimensions();
                match image.save(&path) {
                    Ok(()) => println!(
                        "poster: {width}×{height} in {:?}, written to {path}",
                        start.elapsed()
                    ),
                    Err(error) => println!("poster: {width}×{height}, not written: {error}"),
                }
            }
            Err(error) => println!("poster: failed in {:?}: {error}", start.elapsed()),
        }
    }

    let player = match Player::open(&url) {
        Ok(player) => player,
        Err(error) => {
            println!("open: {error}");
            std::process::exit(1);
        }
    };
    player.set_muted(!audible);
    if player.is_muted() == audible {
        println!("couldn't set muted to {}: not playing", !audible);
        std::process::exit(1);
    }
    let start = Instant::now();
    player.play();
    let mut ready_at = None;
    let duration = play_for(&player, start, Duration::from_secs(5), sleep, &mut ready_at);

    match duration {
        Some(duration) => {
            println!("seeking to {:.2} s", duration / 2.0);
            player.seek(duration / 2.0);
            player.play();
            play_for(&player, start, Duration::from_secs(2), sleep, &mut ready_at);
        }
        None => println!("no duration: not seeking"),
    }
    match ready_at {
        Some(at) => println!("ready after {at:?}"),
        None => println!("never ready"),
    }
}

/// Poll the player for `length`, printing each snapshot, and return the
/// last duration it reported. Stops early if it fails. `sleep`: sleep
/// between polls rather than running the run loop.
fn play_for(
    player: &Player,
    start: Instant,
    length: Duration,
    sleep: bool,
    ready_at: &mut Option<Duration>,
) -> Option<f64> {
    let until = Instant::now() + length;
    let mut duration = None;
    while Instant::now() < until {
        if sleep {
            std::thread::sleep(POLL);
        } else {
            run_main_loop_for(POLL);
        }
        let snapshot = player.snapshot();
        if snapshot.status == PlayerStatus::Ready && ready_at.is_none() {
            *ready_at = Some(start.elapsed());
        }
        println!(
            "{:>6.2}s  {:?}  {:.2} / {}  playing={} ended={}",
            start.elapsed().as_secs_f64(),
            snapshot.status,
            snapshot.position,
            snapshot
                .duration
                .map_or("?".to_string(), |d| format!("{d:.2}")),
            snapshot.playing,
            snapshot.ended,
        );
        duration = snapshot.duration.or(duration);
        if matches!(snapshot.status, PlayerStatus::Failed(_)) {
            break;
        }
    }
    duration
}
