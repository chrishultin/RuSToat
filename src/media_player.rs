//! Playing audio and video attachments, and their poster frames, through
//! AVFoundation. No GPUI: the chat view drives a [`Player`] and reads its
//! [`PlayerSnapshot`] on a timer, rather than observing it.
//!
//! A [`Player`] streams the file server's URL directly (it's public, with no
//! token). It's AppKit's main-thread kind: make and use it on the main
//! thread (GPUI's view callbacks are). [`poster`] blocks while it reads part
//! of the video: call it on a background thread.
//!
//! On other platforms nothing plays: [`Player::open`] and [`poster`] fail,
//! and callers fall back to the file's card.

/// How far a player has got with its file.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerStatus {
    /// Still reading the file's header: whether it can play isn't known yet.
    Loading,
    Ready,
    /// It can't play: AVFoundation's error (domain, code, description).
    Failed(String),
}

/// A player's state at one moment, read by polling.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshot {
    pub status: PlayerStatus,
    /// Seconds from the start.
    pub position: f64,
    /// Seconds, once the file's header has been read.
    pub duration: Option<f64>,
    /// Playing, or waiting for data to play (not paused).
    pub playing: bool,
    /// Stopped at the end.
    pub ended: bool,
}

impl PlayerSnapshot {
    /// Before anything is known.
    pub const LOADING: Self = Self {
        status: PlayerStatus::Loading,
        position: 0.0,
        duration: None,
        playing: false,
        ended: false,
    };
}

/// A player stopped within this of the end has ended (AVFoundation can stop
/// a frame short).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const END_SLACK: f64 = 0.05;

/// Whether a player that isn't `playing`, at `position` of `duration`, has
/// played to the end. (Only the macOS player needs it.)
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn at_end(playing: bool, position: f64, duration: Option<f64>) -> bool {
    !playing && duration.is_some_and(|duration| position >= duration - END_SLACK)
}

#[cfg(target_os = "macos")]
pub use mac::{Player, poster, run_main_loop_for};

#[cfg(not(target_os = "macos"))]
pub use other::{Player, poster, run_main_loop_for};

#[cfg(target_os = "macos")]
mod mac {
    use std::time::Duration;

    use image::RgbaImage;
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, sel};
    use objc2_av_foundation::{
        AVAssetImageGenerator, AVPlayer, AVPlayerItemStatus, AVPlayerStatus,
        AVPlayerTimeControlStatus, AVURLAsset,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    };
    use objc2_core_media::{CMTime, CMTimeFlags};
    use objc2_foundation::{NSDate, NSError, NSObjectProtocol, NSRunLoop, NSString, NSURL};

    use super::{PlayerSnapshot, PlayerStatus, at_end};

    /// Times are made in milliseconds' precision.
    const TIMESCALE: i32 = 1000;

    /// Plays one file. Dropping it stops it.
    pub struct Player {
        player: Retained<AVPlayer>,
    }

    impl Player {
        /// A player for `url` (`https://…` or `file://…`), paused at the
        /// start. It starts reading the file at once. Fails off the main
        /// thread, or for a URL that doesn't parse.
        pub fn open(url: &str) -> Result<Self, String> {
            let mtm = MainThreadMarker::new().ok_or("players live on the main thread")?;
            let url = ns_url(url)?;
            // SAFETY: a plain constructor, on the main thread.
            let player = unsafe { AVPlayer::playerWithURL(&url, mtm) };
            Ok(Self { player })
        }

        pub fn play(&self) {
            // SAFETY: plain method calls on a live player.
            unsafe { self.player.play() };
        }

        pub fn pause(&self) {
            // SAFETY: as above.
            unsafe { self.player.pause() };
        }

        /// The AVFoundation player, for a view that shows it (the video
        /// window).
        pub(crate) fn av_player(&self) -> &AVPlayer {
            &self.player
        }

        /// Silence this player (not the device), for the spike and tests.
        pub fn set_muted(&self, muted: bool) {
            // SAFETY: as above.
            unsafe { self.player.setMuted(muted) };
        }

        pub fn is_muted(&self) -> bool {
            // SAFETY: as above.
            unsafe { self.player.isMuted() }
        }

        /// Jump to `seconds` from the start, exactly (not the nearest
        /// keyframe), so the time shown is the time asked for.
        pub fn seek(&self, seconds: f64) {
            // SAFETY: plain values; `kCMTimeZero` is a constant.
            unsafe {
                let zero = objc2_core_media::kCMTimeZero;
                self.player.seekToTime_toleranceBefore_toleranceAfter(
                    CMTime::with_seconds(seconds.max(0.0), TIMESCALE),
                    zero,
                    zero,
                );
            }
        }

        pub fn snapshot(&self) -> PlayerSnapshot {
            // SAFETY: plain getters on a live player and its item.
            unsafe {
                let item = self.player.currentItem();
                let failed = self
                    .player
                    .error()
                    .or_else(|| item.as_ref().and_then(|item| item.error()));
                let status = if let Some(error) = failed {
                    PlayerStatus::Failed(describe(&error))
                } else if self.player.status() == AVPlayerStatus::Failed {
                    PlayerStatus::Failed("the player failed".into())
                } else {
                    match item.as_ref().map(|item| item.status()) {
                        Some(AVPlayerItemStatus::ReadyToPlay) => PlayerStatus::Ready,
                        Some(AVPlayerItemStatus::Failed) => {
                            PlayerStatus::Failed("the file failed".into())
                        }
                        _ => PlayerStatus::Loading,
                    }
                };
                let duration = item.as_ref().and_then(|item| seconds(item.duration()));
                let position = seconds(self.player.currentTime()).unwrap_or(0.0);
                // A failed player still says it's waiting to play.
                let playing = !matches!(status, PlayerStatus::Failed(_))
                    && self.player.timeControlStatus() != AVPlayerTimeControlStatus::Paused;
                PlayerSnapshot {
                    status,
                    position,
                    duration,
                    playing,
                    ended: at_end(playing, position, duration),
                }
            }
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            // Releasing a playing player can leave its sound going until the
            // item is torn down; pausing first stops it at once.
            self.pause();
        }
    }

    /// A video's frame at 0.5 s (or its first, if it's shorter), scaled
    /// to fit within `max` pixels and turned upright, as RGBA. Blocks while
    /// it reads the video.
    pub fn poster(url: &str, max: (u32, u32)) -> Result<RgbaImage, String> {
        let url = ns_url(url)?;
        // SAFETY: plain constructors and setters; the generator and asset
        // aren't main-thread-only.
        let image = unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
            generator.setAppliesPreferredTrackTransform(true);
            generator.setMaximumSize(CGSize::new(f64::from(max.0), f64::from(max.1)));
            // The frame asked for, not the nearest keyframe (often the first,
            // which is often black).
            let zero = objc2_core_media::kCMTimeZero;
            generator.setRequestedTimeToleranceBefore(zero);
            generator.setRequestedTimeToleranceAfter(zero);
            #[allow(deprecated)]
            let frame = |at: f64| {
                generator.copyCGImageAtTime_actualTime_error(
                    CMTime::with_seconds(at, TIMESCALE),
                    std::ptr::null_mut(),
                )
            };
            frame(0.5)
                .or_else(|_| frame(0.0))
                .map_err(|error| describe(&error))?
        };
        rgba(&image)
    }

    /// Run the main thread's run loop for `duration`, for code that waits
    /// on AppKit outside an app (the spike, tests). GPUI runs it already.
    pub fn run_main_loop_for(duration: Duration) {
        let until = NSDate::dateWithTimeIntervalSinceNow(duration.as_secs_f64());
        NSRunLoop::currentRunLoop().runUntilDate(&until);
    }

    fn ns_url(url: &str) -> Result<Retained<NSURL>, String> {
        NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| format!("not a URL: {url}"))
    }

    /// A time in seconds, if it's a real one (not invalid or indefinite,
    /// as a live stream's duration is).
    fn seconds(time: CMTime) -> Option<f64> {
        let flags = time.flags;
        if !flags.contains(CMTimeFlags::Valid) || flags.contains(CMTimeFlags::Indefinite) {
            return None;
        }
        // SAFETY: a plain conversion of a value.
        let seconds = unsafe { time.seconds() };
        seconds.is_finite().then_some(seconds.max(0.0))
    }

    /// An error as "domain code: description", with what it wraps, for
    /// the log.
    fn describe(error: &NSError) -> String {
        let mut text = format!(
            "{} {}: {}",
            error.domain(),
            error.code(),
            error.localizedDescription()
        );
        if let Some(reason) = error.localizedFailureReason() {
            text.push_str(&format!(" ({reason})"));
        }
        // `underlyingErrors` is macOS 11.3+; the app runs on 11.0.
        if error.respondsToSelector(sel!(underlyingErrors)) {
            for inner in error.underlyingErrors() {
                text.push_str(&format!(" <- {}", describe(&inner)));
            }
        }
        text
    }

    /// A CGImage's pixels, drawn into an RGBA bitmap of the same size.
    fn rgba(image: &CGImage) -> Result<RgbaImage, String> {
        let width = CGImage::width(Some(image));
        let height = CGImage::height(Some(image));
        if width == 0 || height == 0 {
            return Err("the frame is empty".into());
        }
        let mut pixels = vec![0u8; width * height * 4];
        let space = CGColorSpace::new_device_rgb().ok_or("no RGB colour space")?;
        // SAFETY: `pixels` is `height` rows of `width * 4` bytes, and
        // outlives the context, which is dropped at the end of this block.
        unsafe {
            let context = CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                Some(&space),
                CGImageAlphaInfo::PremultipliedLast.0,
            )
            .ok_or("couldn't make a bitmap")?;
            let rect = CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(width as f64, height as f64),
            );
            CGContext::draw_image(Some(&context), rect, Some(image));
        }
        RgbaImage::from_raw(width as u32, height as u32, pixels).ok_or("bad bitmap size".into())
    }
}

#[cfg(not(target_os = "macos"))]
mod other {
    use std::time::Duration;

    use image::RgbaImage;

    use super::PlayerSnapshot;

    const UNSUPPORTED: &str = "playback needs macOS";

    /// Never made: nothing plays here.
    pub struct Player(());

    impl Player {
        pub fn open(_url: &str) -> Result<Self, String> {
            Err(UNSUPPORTED.into())
        }

        pub fn play(&self) {}

        pub fn pause(&self) {}

        pub fn set_muted(&self, _muted: bool) {}

        pub fn is_muted(&self) -> bool {
            true
        }

        pub fn seek(&self, _seconds: f64) {}

        pub fn snapshot(&self) -> PlayerSnapshot {
            PlayerSnapshot::LOADING
        }
    }

    pub fn poster(_url: &str, _max: (u32, u32)) -> Result<RgbaImage, String> {
        Err(UNSUPPORTED.into())
    }

    pub fn run_main_loop_for(duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paused_player_at_the_end_has_ended() {
        assert!(at_end(false, 3.0, Some(3.0)));
        assert!(at_end(false, 2.97, Some(3.0)));
        assert!(!at_end(true, 3.0, Some(3.0)));
        assert!(!at_end(false, 1.0, Some(3.0)));
        assert!(!at_end(false, 0.0, None));
    }
}
