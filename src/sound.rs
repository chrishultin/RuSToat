//! The sound a message notice plays: macOS's built-in "Glass". Stoat's own
//! sounds aren't licensed for other clients, and a system sound needs
//! nothing bundled. Nothing plays on other platforms.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The system sound a message notice plays.
pub const MESSAGE_SOUND: &str = "Glass";

/// At most one sound this often, so a burst of messages (a busy channel,
/// or the backlog after reconnecting) plays once.
pub const MIN_GAP: Duration = Duration::from_secs(1);

/// Whether a notice plays a sound: only a message's, and only with sounds
/// on (friend requests stay silent).
pub fn should_play(sounds_on: bool, is_message_notice: bool) -> bool {
    sounds_on && is_message_notice
}

/// Lets a sound play at most once per [`MIN_GAP`].
#[derive(Debug, Default)]
pub struct Throttle {
    last: Option<Instant>,
}

impl Throttle {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Whether a sound may play at `now`; if so, it counts as played.
    pub fn allow(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < MIN_GAP)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

/// App-wide, so two accounts notifying at once play once too.
static THROTTLE: Mutex<Throttle> = Mutex::new(Throttle::new());

/// Play the message sound, unless it played within the last [`MIN_GAP`].
/// AppKit's: call it on the main thread (GPUI's view callbacks are).
pub fn play_message_sound() {
    let allowed = THROTTLE
        .lock()
        .map(|mut throttle| throttle.allow(Instant::now()))
        .unwrap_or(true);
    if allowed {
        play(MESSAGE_SOUND);
    }
}

/// Play the message sound now, for Settings' preview.
pub fn preview_message_sound() {
    play(MESSAGE_SOUND);
}

#[cfg(target_os = "macos")]
fn play(name: &str) {
    use std::sync::Once;

    use objc2_app_kit::NSSound;
    use objc2_foundation::NSString;

    static MISSING: Once = Once::new();
    match NSSound::soundNamed(&NSString::from_str(name)) {
        Some(sound) => {
            sound.play();
        }
        None => MISSING.call_once(|| log::warn!("no system sound named {name}")),
    }
}

#[cfg(not(target_os = "macos"))]
fn play(_name: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_message_notices_play_and_only_with_sounds_on() {
        assert!(should_play(true, true));
        assert!(!should_play(false, true));
        assert!(!should_play(true, false));
        assert!(!should_play(false, false));
    }

    #[test]
    fn a_burst_plays_once_a_second() {
        let start = Instant::now();
        let mut throttle = Throttle::new();
        assert!(throttle.allow(start));
        assert!(!throttle.allow(start));
        assert!(!throttle.allow(start + Duration::from_millis(999)));
        assert!(throttle.allow(start + MIN_GAP));
        assert!(!throttle.allow(start + Duration::from_millis(1500)));
        assert!(throttle.allow(start + Duration::from_millis(2000)));
    }

    #[test]
    fn a_clock_going_backwards_doesnt_let_a_burst_through() {
        let start = Instant::now() + Duration::from_secs(10);
        let mut throttle = Throttle::new();
        assert!(throttle.allow(start));
        assert!(!throttle.allow(start - Duration::from_millis(1)));
    }
}
