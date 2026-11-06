//! Which messages notify, following Stoat's web client (`stoatchat/for-web`,
//! read 2026-11-04):
//!
//! - Settings: `components/state/stores/NotificationOptions.ts`, synced as the
//!   user setting `notifications`.
//! - Decision: `components/client/NotificationsWorker.tsx`.
//!
//! Plain logic over [`ChatStore`]; the UI decides how to show a [`Notice`].

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use crate::api::models::{Channel, Message, MessageFlags, Presence, RelationshipStatus};
use crate::friends::FRIENDS;
use crate::markdown;
use crate::store::{ChatStore, channel_id, display_name};

/// The author ID Stoat uses for system messages.
const SYSTEM_AUTHOR: &str = "00000000000000000000000000";
/// Notification bodies are cut to this many characters.
pub const BODY_MAX_CHARS: usize = 200;

/// A per-server or per-channel notification level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    All,
    Mention,
    None,
}

impl Level {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Level::All),
            "mention" => Some(Level::Mention),
            "none" => Some(Level::None),
            _ => None,
        }
    }
}

/// The user's notification preferences (the `notifications` user setting).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationSettings {
    server: HashMap<String, Level>,
    channel: HashMap<String, Level>,
    /// Mute expiry in ms since the epoch; `None` is "until unmuted".
    server_mutes: HashMap<String, Option<i64>>,
    channel_mutes: HashMap<String, Option<i64>>,
}

impl NotificationSettings {
    /// Parse the setting's JSON. Lenient like the web client's `clean()`:
    /// unknown levels are ignored, and the legacy level `"muted"` becomes an
    /// open-ended mute.
    pub fn parse(json: &str) -> Self {
        #[derive(Deserialize, Default)]
        struct Raw {
            #[serde(default)]
            server: HashMap<String, Value>,
            #[serde(default)]
            channel: HashMap<String, Value>,
            #[serde(default)]
            server_mutes: HashMap<String, Value>,
            #[serde(default)]
            channel_mutes: HashMap<String, Value>,
        }
        let raw: Raw = serde_json::from_str(json).unwrap_or_default();

        let mut settings = NotificationSettings::default();
        let levels = |map: HashMap<String, Value>,
                      levels: &mut HashMap<String, Level>,
                      mutes: &mut HashMap<String, Option<i64>>| {
            for (id, value) in map {
                match value.as_str() {
                    Some("muted") => {
                        mutes.insert(id, None);
                    }
                    Some(level) => {
                        if let Some(level) = Level::parse(level) {
                            levels.insert(id, level);
                        }
                    }
                    None => {}
                }
            }
        };
        levels(raw.server, &mut settings.server, &mut settings.server_mutes);
        levels(
            raw.channel,
            &mut settings.channel,
            &mut settings.channel_mutes,
        );
        let mutes = |map: HashMap<String, Value>, mutes: &mut HashMap<String, Option<i64>>| {
            for (id, value) in map {
                if value.is_object() {
                    mutes.insert(id, value.get("until").and_then(Value::as_i64));
                }
            }
        };
        mutes(raw.server_mutes, &mut settings.server_mutes);
        mutes(raw.channel_mutes, &mut settings.channel_mutes);
        settings
    }

    /// The level for a channel: its own setting, else its server's (default
    /// `mention`), else the channel type's default (`all` for DMs, groups and
    /// Saved Notes).
    pub fn level(&self, channel: &str, server: Option<&str>) -> Level {
        if let Some(level) = self.channel.get(channel) {
            return *level;
        }
        match server {
            Some(server) => self.server.get(server).copied().unwrap_or(Level::Mention),
            None => Level::All,
        }
    }

    /// Muted if the channel or its server has a mute that hasn't expired.
    pub fn is_muted(&self, channel: &str, server: Option<&str>, now_ms: i64) -> bool {
        let active = |until: &Option<i64>| until.is_none_or(|until| until > now_ms);
        server.is_some_and(|s| self.server_mutes.get(s).is_some_and(active))
            || self.channel_mutes.get(channel).is_some_and(active)
    }
}

/// Milliseconds since the Unix epoch, now.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// What the user is looking at when a message arrives.
#[derive(Clone, Copy, Debug)]
pub struct Viewing<'a> {
    /// The open channel, if any.
    pub channel: Option<&'a str>,
    pub window_active: bool,
}

/// A notification to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// One per channel: a newer notice for the same channel replaces the older.
    pub tag: String,
    pub channel: String,
    pub title: String,
    pub body: String,
}

/// Where a notice is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// A macOS banner (Notification Center).
    Banner,
    /// An in-app toast.
    Toast,
}

/// A macOS banner when RuSToat is in the background and banners can be shown
/// (bundled, not denied); an in-app toast otherwise. A toast is right while
/// the window is in front, and it's the fallback that keeps notices from
/// being lost when banners aren't possible.
pub fn delivery(window_active: bool, banners_available: bool) -> Delivery {
    if !window_active && banners_available {
        Delivery::Banner
    } else {
        Delivery::Toast
    }
}

/// The tag for a channel's banner and toast in one account: the channel,
/// then the account's ID. Channel IDs are ULIDs and account IDs are URLs, so
/// neither has spaces. A newer notice with the same tag replaces the older.
pub fn banner_tag(account: &str, channel: &str) -> String {
    format!("{channel} {account}")
}

/// Split a tag from [`banner_tag`] into (account, channel). A tag without an
/// account is a banner from before multiple accounts, which was just the
/// channel.
pub fn parse_banner_tag(tag: &str) -> (Option<&str>, &str) {
    match tag.split_once(' ') {
        Some((channel, account)) => (Some(account), channel),
        None => (None, tag),
    }
}

/// Whether `message` should notify, and what the notice says. Applies the
/// web client's checks in its order.
pub fn should_notify(
    store: &ChatStore,
    message: &Message,
    viewing: Viewing<'_>,
    now_ms: i64,
) -> Option<Notice> {
    // Looking right at it.
    if viewing.window_active && viewing.channel == Some(message.channel.as_str()) {
        return None;
    }
    // Our own messages.
    if message.author == store.me() {
        return None;
    }
    // Blocked authors.
    if store
        .user(&message.author)
        .is_some_and(|u| matches!(u.relationship, RelationshipStatus::Blocked))
    {
        return None;
    }
    // Sent with notifications suppressed (bit position 1, like the other flags).
    if message.flags & (1 << MessageFlags::SuppressNotifications as u32) != 0 {
        return None;
    }
    let channel = store.channel(&message.channel)?;
    let server = match channel {
        Channel::TextChannel { server, .. } => Some(server.as_str()),
        _ => None,
    };
    let settings = store.notification_settings();
    if settings.is_muted(&message.channel, server, now_ms) {
        return None;
    }
    let mentioned = store.is_mentioned(message);
    match settings.level(&message.channel, server) {
        Level::None => return None,
        Level::Mention if !mentioned => return None,
        _ => {}
    }
    // Busy: nothing. Focus: mentions only.
    match store.my_presence() {
        Some(Presence::Busy) => return None,
        Some(Presence::Focus) if !mentioned => return None,
        _ => {}
    }

    let author = store.author_name(message);
    let title = match channel {
        Channel::SavedMessages { .. } => return None,
        Channel::DirectMessage { .. } => format!("@{author}"),
        Channel::Group { name, .. } if message.author == SYSTEM_AUTHOR => name.clone(),
        Channel::Group { name, .. } => format!("@{author} - {name}"),
        Channel::TextChannel { name, server, .. } => {
            let server = store.server(server).map_or("", |s| s.name.as_str());
            format!("@{author} (#{name}, {server})")
        }
    };
    let body = match (&message.system, &message.content) {
        (Some(system), _) => store.system_text(system),
        (None, Some(content)) => markdown::plain_text(content, store),
        (None, None) => String::new(),
    };

    Some(Notice {
        tag: channel_id(channel).to_string(),
        channel: message.channel.clone(),
        title,
        body: truncate(&body, BODY_MAX_CHARS),
    })
}

/// Whether `user`'s new relationship, after `was`, is a new friend request
/// to the user that should notify, and what the notice says. Only a change
/// to Incoming from anything else notifies (a request already pending at
/// `Ready` never comes through here). Busy silences it; Focus doesn't, as a
/// request is aimed at the user, like a mention. Nor does it notify while the
/// user is looking at the Friends page's Pending tab (`viewing_pending`) with
/// the window in front.
///
/// Every request shares one tag (and the Friends page as its "channel"), so
/// a newer one replaces the older, and a click opens the page.
pub fn friend_request_notice(
    store: &ChatStore,
    user: &str,
    was: &RelationshipStatus,
    viewing_pending: bool,
    window_active: bool,
) -> Option<Notice> {
    let person = store.user(user).filter(|_| user != store.me())?;
    if person.relationship != RelationshipStatus::Incoming || *was == RelationshipStatus::Incoming {
        return None;
    }
    if window_active && viewing_pending {
        return None;
    }
    if store.my_presence() == Some(Presence::Busy) {
        return None;
    }
    let name = display_name(person);
    Some(Notice {
        tag: FRIENDS.to_string(),
        channel: FRIENDS.to_string(),
        title: "Friend request".to_string(),
        body: truncate(
            &format!("{name} sent you a friend request."),
            BODY_MAX_CHARS,
        ),
    })
}

fn truncate(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max_chars) {
        Some((cut, _)) => format!("{}…", text[..cut].trim_end()),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_is_on_characters() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("🦦🦦🦦", 2), "🦦🦦…");
        assert_eq!(truncate("  padded  ", 10), "padded");
    }

    #[test]
    fn settings_parse_levels_mutes_and_legacy() {
        let settings = NotificationSettings::parse(
            r#"{
                "server": { "s-all": "all", "s-old": "muted", "s-bad": "loud" },
                "channel": { "c-none": "none" },
                "server_mutes": { "s-timed": { "until": 2000 } },
                "channel_mutes": { "c-forever": {}, "c-junk": 5 },
                "unknown_key": true
            }"#,
        );
        assert_eq!(settings.level("x", Some("s-all")), Level::All);
        assert_eq!(
            settings.level("x", Some("s-bad")),
            Level::Mention,
            "unknown level ignored"
        );
        assert_eq!(
            settings.level("c-none", Some("s-all")),
            Level::None,
            "channel beats server"
        );
        assert_eq!(settings.level("dm", None), Level::All);
        assert!(settings.is_muted("x", Some("s-old"), 0), "legacy muted");
        assert!(settings.is_muted("x", Some("s-timed"), 1000));
        assert!(!settings.is_muted("x", Some("s-timed"), 3000), "expired");
        assert!(settings.is_muted("c-forever", None, i64::MAX));
        assert!(
            !settings.is_muted("c-junk", None, 0),
            "non-object mute ignored"
        );
    }

    #[test]
    fn garbage_settings_mean_defaults() {
        let settings = NotificationSettings::parse("not json");
        assert_eq!(settings, NotificationSettings::default());
        assert_eq!(settings.level("c", Some("s")), Level::Mention);
    }
}
