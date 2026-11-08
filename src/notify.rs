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

use serde_json::{Map, Value};

use crate::api::models::{Channel, Message, MessageFlags, Presence, RelationshipStatus};
use crate::friends::FRIENDS;
use crate::markdown;
use crate::store::{ChatStore, channel_id, display_name};

/// The user setting's key.
pub const SETTINGS_KEY: &str = "notifications";
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

    /// The level as the setting writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::All => "all",
            Level::Mention => "mention",
            Level::None => "none",
        }
    }

    /// The level as the Notifications menus name it.
    pub fn label(self) -> &'static str {
        match self {
            Level::All => "All Messages",
            Level::Mention => "Mentions Only",
            Level::None => "None",
        }
    }
}

/// How long a mute lasts, as the Mute menus offer it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuteFor {
    Minutes15,
    Hour1,
    Hours3,
    Hours8,
    Hours24,
    /// Until the user unmutes.
    Forever,
}

impl MuteFor {
    /// The Mute menus' choices, in order.
    pub const ALL: [MuteFor; 6] = [
        MuteFor::Minutes15,
        MuteFor::Hour1,
        MuteFor::Hours3,
        MuteFor::Hours8,
        MuteFor::Hours24,
        MuteFor::Forever,
    ];

    /// When a mute started at `now_ms` ends; `None` is never.
    pub fn until(self, now_ms: i64) -> Option<i64> {
        const MINUTE: i64 = 60 * 1000;
        let minutes = match self {
            MuteFor::Minutes15 => 15,
            MuteFor::Hour1 => 60,
            MuteFor::Hours3 => 3 * 60,
            MuteFor::Hours8 => 8 * 60,
            MuteFor::Hours24 => 24 * 60,
            MuteFor::Forever => return None,
        };
        Some(now_ms + minutes * MINUTE)
    }

    pub fn label(self) -> &'static str {
        match self {
            MuteFor::Minutes15 => "For 15 minutes",
            MuteFor::Hour1 => "For 1 hour",
            MuteFor::Hours3 => "For 3 hours",
            MuteFor::Hours8 => "For 8 hours",
            MuteFor::Hours24 => "For 24 hours",
            MuteFor::Forever => "Until I turn it back on",
        }
    }
}

/// The setting's four maps: levels, then mutes, for servers and channels.
const SERVER: &str = "server";
const CHANNEL: &str = "channel";
const SERVER_MUTES: &str = "server_mutes";
const CHANNEL_MUTES: &str = "channel_mutes";

/// The user's notification preferences (the `notifications` user setting).
///
/// The setting is kept as parsed (`raw`), and changes edit that, so fields
/// this client doesn't know survive a write. The typed maps are a view of
/// it, rebuilt after each change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationSettings {
    server: HashMap<String, Level>,
    channel: HashMap<String, Level>,
    /// Mute expiry in ms since the epoch; `None` is "until unmuted".
    server_mutes: HashMap<String, Option<i64>>,
    channel_mutes: HashMap<String, Option<i64>>,
    raw: Map<String, Value>,
}

impl NotificationSettings {
    /// Parse the setting's JSON. Lenient like the web client's `clean()`:
    /// unknown levels are ignored, and the legacy level `"muted"` becomes an
    /// open-ended mute (moved into the mutes map, as a write would leave it).
    pub fn parse(json: &str) -> Self {
        let mut raw = match serde_json::from_str(json) {
            Ok(Value::Object(raw)) => raw,
            _ => Map::new(),
        };
        for (levels, mutes) in [(SERVER, SERVER_MUTES), (CHANNEL, CHANNEL_MUTES)] {
            let Some(levels) = raw.get_mut(levels).and_then(Value::as_object_mut) else {
                continue;
            };
            let muted: Vec<String> = levels
                .iter()
                .filter(|(_, level)| level.as_str() == Some("muted"))
                .map(|(id, _)| id.clone())
                .collect();
            if muted.is_empty() {
                continue;
            }
            for id in &muted {
                levels.remove(id);
            }
            // A mute already there (with an end) wins over the legacy one.
            let mutes = object_entry(&mut raw, mutes);
            for id in muted {
                mutes.entry(id).or_insert_with(|| Value::Object(Map::new()));
            }
        }
        Self::from_raw(raw)
    }

    /// The typed view of `raw`.
    fn from_raw(raw: Map<String, Value>) -> Self {
        let levels = |key: &str| -> HashMap<String, Level> {
            raw.get(key)
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(id, level)| Some((id.clone(), Level::parse(level.as_str()?)?)))
                .collect()
        };
        let mutes = |key: &str| -> HashMap<String, Option<i64>> {
            raw.get(key)
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter(|(_, mute)| mute.is_object())
                .map(|(id, mute)| (id.clone(), mute.get("until").and_then(Value::as_i64)))
                .collect()
        };
        NotificationSettings {
            server: levels(SERVER),
            channel: levels(CHANNEL),
            server_mutes: mutes(SERVER_MUTES),
            channel_mutes: mutes(CHANNEL_MUTES),
            raw,
        }
    }

    /// Change `raw`'s `key` map with `change`, then rebuild the typed view.
    fn edit(&mut self, key: &str, change: impl FnOnce(&mut Map<String, Value>)) {
        let mut raw = std::mem::take(&mut self.raw);
        change(object_entry(&mut raw, key));
        *self = Self::from_raw(raw);
    }

    /// The level for a channel: its own setting, else its server's (default
    /// `mention`), else the channel type's default (`all` for DMs, groups and
    /// Saved Notes).
    pub fn level(&self, channel: &str, server: Option<&str>) -> Level {
        if let Some(level) = self.channel.get(channel) {
            return *level;
        }
        match server {
            Some(server) => self.server_level(server),
            None => Level::All,
        }
    }

    /// A server's level: its own setting, else `mention`.
    pub fn server_level(&self, server: &str) -> Level {
        self.server.get(server).copied().unwrap_or(Level::Mention)
    }

    /// A channel's own level, if it overrides its server's (or its type's
    /// default).
    pub fn channel_override(&self, channel: &str) -> Option<Level> {
        self.channel.get(channel).copied()
    }

    /// Muted if the channel or its server has a mute that hasn't expired.
    pub fn is_muted(&self, channel: &str, server: Option<&str>, now_ms: i64) -> bool {
        server.is_some_and(|s| self.server_mute(s, now_ms).is_some())
            || self.channel_mute(channel, now_ms).is_some()
    }

    /// A server's mute, if it hasn't expired: `Some(Some(until))`, or
    /// `Some(None)` until unmuted.
    pub fn server_mute(&self, server: &str, now_ms: i64) -> Option<Option<i64>> {
        active_mute(&self.server_mutes, server, now_ms)
    }

    /// A channel's own mute, if it hasn't expired (as [`Self::server_mute`]).
    pub fn channel_mute(&self, channel: &str, now_ms: i64) -> Option<Option<i64>> {
        active_mute(&self.channel_mutes, channel, now_ms)
    }

    /// When the next timed mute after `now_ms` ends, if any.
    pub fn next_mute_expiry(&self, now_ms: i64) -> Option<i64> {
        self.server_mutes
            .values()
            .chain(self.channel_mutes.values())
            .filter_map(|until| *until)
            .filter(|until| *until > now_ms)
            .min()
    }

    pub fn set_server_level(&mut self, server: &str, level: Level) {
        self.edit(SERVER, |levels| {
            levels.insert(server.to_string(), level.as_str().into());
        });
    }

    /// Set a channel's own level, or with `None` return it to its server's
    /// (or its type's default).
    pub fn set_channel_level(&mut self, channel: &str, level: Option<Level>) {
        self.edit(CHANNEL, |levels| match level {
            Some(level) => {
                levels.insert(channel.to_string(), level.as_str().into());
            }
            None => {
                levels.remove(channel);
            }
        });
    }

    /// Mute a server until `until` (ms since the epoch), or until unmuted.
    pub fn mute_server(&mut self, server: &str, until: Option<i64>) {
        self.edit(SERVER_MUTES, |mutes| {
            mutes.insert(server.to_string(), mute_value(until));
        });
    }

    /// Mute a channel until `until` (ms since the epoch), or until unmuted.
    pub fn mute_channel(&mut self, channel: &str, until: Option<i64>) {
        self.edit(CHANNEL_MUTES, |mutes| {
            mutes.insert(channel.to_string(), mute_value(until));
        });
    }

    pub fn unmute_server(&mut self, server: &str) {
        self.edit(SERVER_MUTES, |mutes| {
            mutes.remove(server);
        });
    }

    pub fn unmute_channel(&mut self, channel: &str) {
        self.edit(CHANNEL_MUTES, |mutes| {
            mutes.remove(channel);
        });
    }

    /// The setting's JSON, to write back whole: as parsed (unknown fields
    /// kept), with this client's changes, legacy `"muted"` levels moved
    /// into the mutes maps, and mutes that ended by `now_ms` left out.
    pub fn to_json(&self, now_ms: i64) -> String {
        let mut raw = self.raw.clone();
        for key in [SERVER_MUTES, CHANNEL_MUTES] {
            if let Some(mutes) = raw.get_mut(key).and_then(Value::as_object_mut) {
                mutes.retain(|_, mute| {
                    mute.get("until")
                        .and_then(Value::as_i64)
                        .is_none_or(|until| until > now_ms)
                });
            }
        }
        Value::Object(raw).to_string()
    }
}

/// `raw[key]` as an object, made one (replacing whatever was there) if it
/// isn't.
fn object_entry<'a>(raw: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let entry = raw.entry(key).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().expect("just made an object")
}

/// A mute as the setting writes it: `{ "until": ms }`, or `{}` for until
/// unmuted.
fn mute_value(until: Option<i64>) -> Value {
    match until {
        Some(until) => serde_json::json!({ "until": until }),
        None => Value::Object(Map::new()),
    }
}

fn active_mute(mutes: &HashMap<String, Option<i64>>, id: &str, now_ms: i64) -> Option<Option<i64>> {
    mutes
        .get(id)
        .copied()
        .filter(|until| until.is_none_or(|until| until > now_ms))
}

/// How a mute is described: "Muted", or "Muted until 14:05" (local time)
/// for one that ends.
pub fn mute_label(until: Option<i64>) -> String {
    match until {
        Some(until) => format!("Muted until {}", crate::time::millis_until_label(until)),
        None => "Muted".to_string(),
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

    fn json(settings: &NotificationSettings, now_ms: i64) -> Value {
        serde_json::from_str(&settings.to_json(now_ms)).unwrap()
    }

    #[test]
    fn mutators_change_levels_and_mutes() {
        let mut settings = NotificationSettings::default();
        settings.set_server_level("s", Level::All);
        assert_eq!(settings.server_level("s"), Level::All);
        assert_eq!(settings.level("c", Some("s")), Level::All);

        settings.set_channel_level("c", Some(Level::None));
        assert_eq!(settings.channel_override("c"), Some(Level::None));
        assert_eq!(settings.level("c", Some("s")), Level::None);
        settings.set_channel_level("c", None);
        assert_eq!(settings.channel_override("c"), None);
        assert_eq!(
            settings.level("c", Some("s")),
            Level::All,
            "back to the server's"
        );

        settings.mute_server("s", Some(2000));
        assert_eq!(settings.server_mute("s", 1000), Some(Some(2000)));
        assert!(settings.is_muted("c", Some("s"), 1000));
        assert_eq!(settings.server_mute("s", 2000), None, "ended");
        settings.unmute_server("s");
        assert!(!settings.is_muted("c", Some("s"), 1000));

        settings.mute_channel("c", None);
        assert_eq!(settings.channel_mute("c", i64::MAX), Some(None));
        assert!(settings.is_muted("c", Some("s"), i64::MAX));
        settings.unmute_channel("c");
        assert!(!settings.is_muted("c", Some("s"), 0));
    }

    #[test]
    fn to_json_round_trips_through_parse() {
        let mut settings = NotificationSettings::default();
        settings.set_server_level("s", Level::None);
        settings.set_channel_level("c", Some(Level::Mention));
        settings.mute_server("s2", Some(5000));
        settings.mute_channel("c2", None);
        let written = settings.to_json(1000);
        assert_eq!(NotificationSettings::parse(&written), settings);
        assert_eq!(
            json(&settings, 1000),
            serde_json::json!({
                "server": { "s": "none" },
                "channel": { "c": "mention" },
                "server_mutes": { "s2": { "until": 5000 } },
                "channel_mutes": { "c2": {} }
            })
        );
    }

    #[test]
    fn unknown_fields_survive_a_write() {
        let mut settings = NotificationSettings::parse(
            r#"{
                "server": { "s-loud": "loud" },
                "server_mutes": { "s-timed": { "until": 9000, "why": "meetings" } },
                "channel_mutes": { "c-junk": 5 },
                "future_key": { "a": [1, 2] }
            }"#,
        );
        settings.set_server_level("s", Level::All);
        settings.mute_channel("c", Some(8000));
        let written = json(&settings, 1000);
        assert_eq!(written["future_key"], serde_json::json!({ "a": [1, 2] }));
        assert_eq!(written["server"]["s-loud"], "loud");
        assert_eq!(written["server"]["s"], "all");
        assert_eq!(
            written["server_mutes"]["s-timed"],
            serde_json::json!({ "until": 9000, "why": "meetings" })
        );
        assert_eq!(written["channel_mutes"]["c-junk"], 5);
        assert_eq!(
            written["channel_mutes"]["c"],
            serde_json::json!({ "until": 8000 })
        );
    }

    #[test]
    fn legacy_muted_levels_are_written_as_mutes() {
        let settings = NotificationSettings::parse(
            r#"{
                "server": { "s-old": "muted", "s-all": "all" },
                "channel": { "c-old": "muted", "c-both": "muted" },
                "channel_mutes": { "c-both": { "until": 5000 } }
            }"#,
        );
        let written = json(&settings, 1000);
        assert_eq!(written["server"], serde_json::json!({ "s-all": "all" }));
        assert_eq!(written["server_mutes"], serde_json::json!({ "s-old": {} }));
        assert_eq!(written["channel"], serde_json::json!({}));
        assert_eq!(
            written["channel_mutes"],
            serde_json::json!({ "c-old": {}, "c-both": { "until": 5000 } }),
            "a mute with an end wins over the legacy level"
        );
    }

    #[test]
    fn ended_mutes_are_left_out_of_a_write() {
        let mut settings = NotificationSettings::default();
        settings.mute_server("s-ended", Some(1000));
        settings.mute_server("s-now", Some(2000));
        settings.mute_channel("c-later", Some(3000));
        settings.mute_channel("c-forever", None);
        let written = json(&settings, 2000);
        assert_eq!(written["server_mutes"], serde_json::json!({}));
        assert_eq!(
            written["channel_mutes"],
            serde_json::json!({ "c-later": { "until": 3000 }, "c-forever": {} })
        );
    }

    #[test]
    fn mute_durations() {
        let now = 1_000_000;
        assert_eq!(MuteFor::Minutes15.until(now), Some(now + 15 * 60 * 1000));
        assert_eq!(MuteFor::Hour1.until(now), Some(now + 3_600_000));
        assert_eq!(MuteFor::Hours3.until(now), Some(now + 3 * 3_600_000));
        assert_eq!(MuteFor::Hours8.until(now), Some(now + 8 * 3_600_000));
        assert_eq!(MuteFor::Hours24.until(now), Some(now + 24 * 3_600_000));
        assert_eq!(MuteFor::Forever.until(now), None);
        assert_eq!(MuteFor::Forever.label(), "Until I turn it back on");
    }

    #[test]
    fn next_mute_expiry_is_the_soonest_future_end() {
        let mut settings = NotificationSettings::default();
        assert_eq!(settings.next_mute_expiry(0), None);
        settings.mute_server("s-forever", None);
        assert_eq!(
            settings.next_mute_expiry(0),
            None,
            "open-ended mutes don't end"
        );
        settings.mute_server("s", Some(5000));
        settings.mute_channel("c", Some(3000));
        settings.mute_channel("c-ended", Some(500));
        assert_eq!(settings.next_mute_expiry(1000), Some(3000));
        assert_eq!(settings.next_mute_expiry(3000), Some(5000));
        assert_eq!(settings.next_mute_expiry(5000), None);
    }
}
