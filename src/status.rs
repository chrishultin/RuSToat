//! The user's own presence and custom status: what the status menu offers,
//! and the rules for the custom status text. Plain data with no GPUI, so
//! it's unit-testable.

use crate::api::models::Presence;
use crate::store::PresenceKind;

/// The longest custom status Stoat accepts, in characters.
pub const STATUS_TEXT_MAX: usize = 128;

/// The status menu's presences, in order.
pub const PRESENCES: [Presence; 5] = [
    Presence::Online,
    Presence::Idle,
    Presence::Focus,
    Presence::Busy,
    Presence::Invisible,
];

/// A presence as the user's own menu and status line name it (Busy is
/// "Do Not Disturb", as in the web client).
pub fn presence_label(presence: &Presence) -> &'static str {
    match presence {
        Presence::Online => "Online",
        Presence::Idle => "Idle",
        Presence::Focus => "Focus",
        Presence::Busy => "Do Not Disturb",
        Presence::Invisible => "Invisible",
    }
}

/// What a presence does to notifications, under its name in the menu.
pub fn presence_hint(presence: &Presence) -> Option<&'static str> {
    match presence {
        Presence::Focus => Some("Only mentions notify you"),
        Presence::Busy => Some("You won't get any notifications"),
        Presence::Online | Presence::Idle | Presence::Invisible => None,
    }
}

/// The dot a presence is drawn with. Invisible is the offline dot.
pub fn presence_dot(presence: &Presence) -> PresenceKind {
    match presence {
        Presence::Online => PresenceKind::Online,
        Presence::Idle => PresenceKind::Idle,
        Presence::Focus => PresenceKind::Focus,
        Presence::Busy => PresenceKind::Busy,
        Presence::Invisible => PresenceKind::Offline,
    }
}

/// What saving the custom status editor would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCheck<'a> {
    /// Blank: clear the status.
    Clear,
    /// Over [`STATUS_TEXT_MAX`] characters (this many).
    TooLong(usize),
    /// Set it to this, trimmed.
    Ok(&'a str),
}

/// Check what's typed in the custom status editor. Length counts
/// characters, not bytes, after trimming.
pub fn check_status_text(text: &str) -> StatusCheck<'_> {
    let text = text.trim();
    if text.is_empty() {
        return StatusCheck::Clear;
    }
    match text.chars().count() {
        n if n > STATUS_TEXT_MAX => StatusCheck::TooLong(n),
        _ => StatusCheck::Ok(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_or_whitespace_clears() {
        assert_eq!(check_status_text(""), StatusCheck::Clear);
        assert_eq!(check_status_text("  \n\t "), StatusCheck::Clear);
    }

    #[test]
    fn text_is_trimmed() {
        assert_eq!(check_status_text("  brb \n"), StatusCheck::Ok("brb"));
    }

    #[test]
    fn the_limit_is_128_characters() {
        let at = "a".repeat(128);
        assert_eq!(check_status_text(&at), StatusCheck::Ok(at.as_str()));
        assert_eq!(
            check_status_text(&"a".repeat(129)),
            StatusCheck::TooLong(129)
        );
    }

    #[test]
    fn characters_are_counted_not_bytes() {
        // 128 four-byte characters: 512 bytes, but within the limit.
        let emoji = "🎉".repeat(128);
        assert_eq!(check_status_text(&emoji), StatusCheck::Ok(emoji.as_str()));
        assert_eq!(
            check_status_text(&"é".repeat(129)),
            StatusCheck::TooLong(129)
        );
    }

    #[test]
    fn surrounding_spaces_dont_count() {
        let text = format!("  {}  ", "a".repeat(128));
        assert_eq!(check_status_text(&text), StatusCheck::Ok(&text[2..130]));
    }

    #[test]
    fn busy_is_do_not_disturb_and_invisible_draws_offline() {
        assert_eq!(presence_label(&Presence::Busy), "Do Not Disturb");
        assert_eq!(presence_dot(&Presence::Invisible), PresenceKind::Offline);
        assert_eq!(
            presence_hint(&Presence::Focus),
            Some("Only mentions notify you")
        );
    }
}
