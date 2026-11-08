//! The user's notification settings in the store: which copy wins, and
//! what a mute does to the sidebar.

use stoat_gpui::api::events::{Ready, ServerEvent, parse_frame};
use stoat_gpui::notify::{Level, NotificationSettings, now_ms};
use stoat_gpui::store::ChatStore;

fn ready() -> Ready {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let Some(ServerEvent::Ready(ready)) =
        parse_frame(&std::fs::read_to_string(path).unwrap()).pop()
    else {
        panic!("ready fixture");
    };
    *ready
}

/// A store whose notification settings were written at 1000.
fn store() -> ChatStore {
    let mut ready = ready();
    ready.notifications = Some((1000, NotificationSettings::parse("{}")));
    let mut store = ChatStore::new("u-me");
    store.apply_ready(ready);
    store
}

fn with_level(level: Level) -> NotificationSettings {
    let mut settings = NotificationSettings::default();
    settings.set_server_level("srv-alpha", level);
    settings
}

fn alpha_level(store: &ChatStore) -> Level {
    store.notification_settings().server_level("srv-alpha")
}

#[test]
fn ready_brings_the_settings_and_their_time() {
    let store = store();
    assert_eq!(store.notifications_updated(), 1000);
    assert_eq!(alpha_level(&store), Level::Mention);
}

#[test]
fn a_newer_copy_applies_and_an_older_one_is_ignored() {
    let mut store = store();
    assert!(store.set_notification_settings(2000, with_level(Level::All)));
    assert_eq!(alpha_level(&store), Level::All);
    assert_eq!(store.notifications_updated(), 2000);

    // The echo of an earlier change arrives after a later one.
    assert!(!store.set_notification_settings(1500, with_level(Level::None)));
    assert_eq!(alpha_level(&store), Level::All);
    assert_eq!(store.notifications_updated(), 2000);

    // The same time again (the echo of the change itself) applies.
    assert!(store.set_notification_settings(2000, with_level(Level::All)));
}

#[test]
fn an_echo_stamped_earlier_by_stoats_clock_moves_the_time_back() {
    let mut store = store();
    // A change made here, with this Mac's clock running ahead.
    store.set_notification_settings(5000, with_level(Level::All));
    // Stoat clamped it to its own time: same content, earlier time.
    assert!(!store.set_notification_settings(4000, with_level(Level::All)));
    assert_eq!(store.notifications_updated(), 4000);
    // So another client's change, stamped by Stoat after that, applies.
    assert!(store.set_notification_settings(4500, with_level(Level::None)));
    assert_eq!(alpha_level(&store), Level::None);
}

#[test]
fn muting_a_server_hides_its_unread_dot() {
    let mut store = store();
    let message = serde_json::from_value(serde_json::json!({
        "_id": "01JC00000000000000000000ZZ", "channel": "ch-random",
        "author": "u-ada", "content": "hi"
    }))
    .unwrap();
    store.apply_message(message);
    let alpha_unread = |store: &ChatStore| {
        store
            .servers()
            .into_iter()
            .find(|s| s.id == "srv-alpha")
            .unwrap()
            .unread
    };
    assert!(alpha_unread(&store));
    assert_eq!(store.server_mute_until("srv-alpha"), None);

    let until = now_ms() + 60_000;
    let mut settings = store.notification_settings().clone();
    settings.mute_server("srv-alpha", Some(until));
    store.set_notification_settings(2000, settings);
    assert!(!alpha_unread(&store));
    assert!(store.is_muted("ch-random"));
    assert_eq!(store.server_mute_until("srv-alpha"), Some(Some(until)));

    let mut settings = store.notification_settings().clone();
    settings.mute_server("srv-alpha", None);
    store.set_notification_settings(3000, settings);
    assert_eq!(store.server_mute_until("srv-alpha"), Some(None));

    // A mute that has ended doesn't count.
    let mut settings = store.notification_settings().clone();
    settings.mute_server("srv-alpha", Some(now_ms() - 1));
    store.set_notification_settings(4000, settings);
    assert_eq!(store.server_mute_until("srv-alpha"), None);
    assert!(alpha_unread(&store));
}

/// Change the store's settings with `change`, as a newer copy.
fn change(store: &mut ChatStore, change: impl FnOnce(&mut NotificationSettings)) {
    let mut settings = store.notification_settings().clone();
    change(&mut settings);
    let updated = store.notifications_updated() + 1;
    assert!(store.set_notification_settings(updated, settings));
}

fn entry(store: &ChatStore, channel: &str) -> stoat_gpui::store::ChannelEntry {
    store.channel_entry(channel).unwrap()
}

#[test]
fn a_muted_channel_is_muted_and_shows_no_unread_dot() {
    let mut store = store();
    // dm-bob has messages the user hasn't read (no read position).
    assert!(entry(&store, "dm-bob").unread);
    assert!(!entry(&store, "dm-bob").muted);

    let until = now_ms() + 60_000;
    change(&mut store, |s| s.mute_channel("dm-bob", Some(until)));
    let bob = entry(&store, "dm-bob");
    assert!(bob.muted);
    assert!(!bob.unread);
    assert_eq!(store.channel_mute_until("dm-bob"), Some(Some(until)));

    change(&mut store, |s| s.unmute_channel("dm-bob"));
    assert!(!entry(&store, "dm-bob").muted);
    assert_eq!(store.channel_mute_until("dm-bob"), None);
}

#[test]
fn a_ready_without_the_settings_keeps_the_mutes_held() {
    let mut store = store();
    change(&mut store, |s| {
        s.mute_channel("dm-bob", None);
        s.set_server_level("srv-alpha", Level::All);
    });
    let updated = store.notifications_updated();

    // Stoat sends `Ready` without the user settings, even when asked.
    let mut reconnect = ready();
    reconnect.notifications = None;
    store.apply_ready(reconnect);
    assert!(entry(&store, "dm-bob").muted);
    assert_eq!(alpha_level(&store), Level::All);
    assert_eq!(store.notifications_updated(), updated);

    // A `Ready` that does bring them is taken.
    let mut reconnect = ready();
    reconnect.notifications = Some((updated + 1, NotificationSettings::parse("{}")));
    store.apply_ready(reconnect);
    assert!(!entry(&store, "dm-bob").muted);
}

#[test]
fn a_channel_in_a_muted_server_is_muted() {
    let mut store = store();
    change(&mut store, |s| s.mute_server("srv-alpha", None));
    assert!(entry(&store, "ch-general").muted);
    assert!(entry(&store, "ch-random").muted);
    // The channel itself isn't: its menu still offers Mute Channel.
    assert_eq!(store.channel_mute_until("ch-general"), None);
    assert!(!entry(&store, "dm-ada").muted);
}

#[test]
fn a_mute_that_has_ended_isnt_muted() {
    let mut store = store();
    change(&mut store, |s| {
        s.mute_channel("grp-weekend", Some(now_ms() - 1));
        s.mute_server("srv-alpha", Some(now_ms() - 1));
    });
    assert!(!entry(&store, "grp-weekend").muted);
    assert!(!entry(&store, "ch-general").muted);
    assert_eq!(store.channel_mute_until("grp-weekend"), None);
}

#[test]
fn the_level_source_is_the_servers_or_all_outside_servers() {
    let mut store = store();
    assert_eq!(store.level_source("ch-general"), Level::Mention);
    assert_eq!(store.level_source("dm-ada"), Level::All);
    assert_eq!(store.level_source("grp-weekend"), Level::All);
    assert_eq!(store.level_source("ch-saved"), Level::All);

    change(&mut store, |s| s.set_server_level("srv-alpha", Level::None));
    assert_eq!(store.level_source("ch-general"), Level::None);
}

#[test]
fn clearing_a_channels_level_returns_it_to_the_servers() {
    let mut store = store();
    change(&mut store, |s| {
        s.set_server_level("srv-alpha", Level::All);
        s.set_channel_level("ch-general", Some(Level::None));
    });
    let level = |store: &ChatStore| {
        store
            .notification_settings()
            .level("ch-general", Some("srv-alpha"))
    };
    assert_eq!(level(&store), Level::None, "the channel's own level wins");
    change(&mut store, |s| s.set_channel_level("ch-general", None));
    assert_eq!(
        store.notification_settings().channel_override("ch-general"),
        None
    );
    assert_eq!(level(&store), Level::All, "back to the server's");
}
