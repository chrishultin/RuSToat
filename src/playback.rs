//! What's playing (an audio attachment, or a video in its window), and the
//! maths its controls need. Plain data with no GPUI or AVFoundation, so the
//! rules are unit-testable; `src/ui/chat.rs` drives the player.
//!
//! One thing plays at a time: starting another stops it, and so does
//! switching account. Audio also stops when its channel is left or the
//! window is closed.

/// Shown for a time that isn't known yet.
pub const UNKNOWN_TIME: &str = "–:––";

/// What kind of file is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// In its message.
    Audio,
    /// In the player window.
    Video,
}

/// The file that's playing (or paused), and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    /// The channel its message is in.
    pub channel: String,
    pub file_id: String,
    pub kind: MediaKind,
}

/// Something that may stop what's playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<'a> {
    /// The user started a file.
    Play(&'a NowPlaying),
    /// This channel is now open (`None`: none is, as on Home's Friends page).
    OpenChannel(Option<&'a str>),
    /// Another account was put on screen, or this one signed out.
    SwitchAccount,
    /// The main window was closed (hidden, with Keep running on).
    HideWindow,
}

/// Whether `change` stops `current`.
///
/// - Starting another file stops it; starting the same one again resumes
///   it.
/// - Opening another channel, or closing the main window, stops audio. A
///   video plays in its own window, so it carries on.
/// - Switching account stops anything.
pub fn stops(current: &NowPlaying, change: Change<'_>) -> bool {
    let audio = current.kind == MediaKind::Audio;
    match change {
        Change::Play(next) => next != current,
        Change::OpenChannel(open) => audio && open != Some(current.channel.as_str()),
        Change::HideWindow => audio,
        Change::SwitchAccount => true,
    }
}

/// "0:42 / 3:15": how far through, of how long. Before the length is
/// known, "–:–– / –:––". The position never shows past the length.
pub fn time_label(position: f64, duration: Option<f64>) -> String {
    let Some(duration) = duration.filter(|d| d.is_finite() && *d >= 0.0) else {
        return format!("{UNKNOWN_TIME} / {UNKNOWN_TIME}");
    };
    let total = duration.round() as u64;
    let at = (finite(position).floor() as u64).min(total);
    format!("{} / {}", clock(at), clock(total))
}

/// "0:07", "3:15", "1:02:03".
pub fn clock(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Where a click or drag `fraction` of the way along the seek bar goes:
/// clamped to the start and end.
pub fn seek_target(fraction: f64, duration: f64) -> f64 {
    finite(fraction).clamp(0.0, 1.0) * finite(duration).max(0.0)
}

/// How much of the seek bar is filled, 0 to 1 (empty until the length is
/// known).
pub fn progress(position: f64, duration: Option<f64>) -> f32 {
    match duration {
        Some(duration) if duration.is_finite() && duration > 0.0 => {
            (finite(position) / duration).clamp(0.0, 1.0) as f32
        }
        _ => 0.0,
    }
}

/// How far along a bar from `start` that's `width` wide `x` is, 0 to 1.
pub fn fraction_along(x: f32, start: f32, width: f32) -> f64 {
    if width <= 0.0 {
        return 0.0;
    }
    f64::from(((x - start) / width).clamp(0.0, 1.0))
}

/// `value`, or 0 if it's NaN or infinite.
fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}
