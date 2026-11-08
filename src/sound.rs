//! The sound a message notice plays (macOS's built-in "Glass"), and a
//! voice call's sounds. Stoat's own sounds aren't licensed for other
//! clients, and system sounds need nothing bundled. Nothing plays on other
//! platforms.

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

/// Something in a voice call that plays a sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSound {
    /// The user joined a call.
    Join,
    /// The user left a call, or was disconnected.
    Leave,
    /// Someone joined the user's call.
    OtherJoin,
    /// Someone left the user's call.
    OtherLeave,
    /// The user muted or deafened.
    Mute,
    /// The user unmuted or undeafened.
    Unmute,
}

impl CallSound {
    /// The macOS system sound it plays.
    pub fn system_sound(self) -> &'static str {
        match self {
            CallSound::Join => "Pop",
            CallSound::Leave => "Bottle",
            CallSound::OtherJoin => "Tink",
            CallSound::OtherLeave => "Purr",
            CallSound::Mute => "Morse",
            CallSound::Unmute => "Tink",
        }
    }

    /// Others coming and going share a [`Throttle`], so a crowd joining
    /// at once plays once. The user's own sounds always play.
    fn throttled(self) -> bool {
        matches!(self, CallSound::OtherJoin | CallSound::OtherLeave)
    }
}

/// Whether `sound` plays at `now`: never with call sounds off, and others'
/// comings and goings at most once per [`MIN_GAP`] (counted in `others`).
pub fn should_play_call(
    sounds_on: bool,
    sound: CallSound,
    others: &mut Throttle,
    now: Instant,
) -> bool {
    sounds_on && (!sound.throttled() || others.allow(now))
}

/// Others joining and leaving, app-wide. Not the message sound's: a
/// message doesn't silence a join.
static OTHERS_THROTTLE: Mutex<Throttle> = Mutex::new(Throttle::new());

/// Play `sound` if [`should_play_call`] says so. AppKit's: call it on the
/// main thread (GPUI's view callbacks are).
pub fn play_call_sound(sound: CallSound, sounds_on: bool) {
    let allowed = OTHERS_THROTTLE
        .lock()
        .map(|mut others| should_play_call(sounds_on, sound, &mut others, Instant::now()))
        .unwrap_or(sounds_on);
    if allowed {
        play(sound.system_sound());
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
    fn each_call_sound_is_a_system_sound() {
        let system = ["Bottle", "Morse", "Pop", "Purr", "Tink"];
        for sound in [
            CallSound::Join,
            CallSound::Leave,
            CallSound::OtherJoin,
            CallSound::OtherLeave,
            CallSound::Mute,
            CallSound::Unmute,
        ] {
            assert!(
                system.contains(&sound.system_sound()),
                "{sound:?} plays {}",
                sound.system_sound()
            );
        }
        assert_ne!(
            CallSound::Join.system_sound(),
            CallSound::Leave.system_sound()
        );
    }

    #[test]
    fn others_coming_and_going_play_once_a_second() {
        let start = Instant::now();
        let mut others = Throttle::new();
        assert!(should_play_call(
            true,
            CallSound::OtherJoin,
            &mut others,
            start
        ));
        assert!(!should_play_call(
            true,
            CallSound::OtherJoin,
            &mut others,
            start
        ));
        assert!(!should_play_call(
            true,
            CallSound::OtherLeave,
            &mut others,
            start + Duration::from_millis(500)
        ));
        // The user's own sounds aren't held back.
        assert!(should_play_call(true, CallSound::Leave, &mut others, start));
        assert!(should_play_call(true, CallSound::Mute, &mut others, start));
        assert!(should_play_call(
            true,
            CallSound::OtherLeave,
            &mut others,
            start + MIN_GAP
        ));
    }

    #[test]
    fn call_sounds_off_plays_nothing() {
        let now = Instant::now();
        for sound in [CallSound::Join, CallSound::OtherJoin, CallSound::Unmute] {
            assert!(!should_play_call(false, sound, &mut Throttle::new(), now));
        }
    }

    #[test]
    fn a_clock_going_backwards_doesnt_let_a_burst_through() {
        let start = Instant::now() + Duration::from_secs(10);
        let mut throttle = Throttle::new();
        assert!(throttle.allow(start));
        assert!(!throttle.allow(start - Duration::from_millis(1)));
    }
}
