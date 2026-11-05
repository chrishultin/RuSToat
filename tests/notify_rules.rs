//! Notification rules, mirroring Stoat's web client (NotificationsWorker.tsx):
//! one test per skip rule, plus titles, bodies, and muted-channel indicators.

use stoat_gpui::api::events::{Ready, ServerEvent, parse_frame};
use stoat_gpui::api::models::Message;
use stoat_gpui::notify::{Notice, NotificationSettings, Viewing, should_notify};
use stoat_gpui::store::ChatStore;

const NOW: i64 = 1_800_000_000_000;
const AWAY: Viewing<'static> = Viewing {
    channel: None,
    window_active: false,
};

fn ready() -> Ready {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let Some(ServerEvent::Ready(ready)) =
        parse_frame(&std::fs::read_to_string(path).unwrap()).pop()
    else {
        panic!("ready fixture");
    };
    *ready
}

/// The fixture store, with notification settings (JSON) and our presence.
fn store(settings: &str, presence: Option<&str>) -> ChatStore {
    let mut ready = ready();
    ready.notifications = Some(NotificationSettings::parse(settings));
    ready.users.push(
        serde_json::from_value(serde_json::json!({
            "_id": "u-blocked", "username": "troll", "discriminator": "6666",
            "relationship": "Blocked", "online": true
        }))
        .unwrap(),
    );
    if let Some(presence) = presence {
        let me = ready.users.iter_mut().find(|u| u.id == "u-me").unwrap();
        me.status =
            Some(serde_json::from_value(serde_json::json!({ "presence": presence })).unwrap());
    }
    let mut store = ChatStore::new("u-me");
    store.apply_ready(ready);
    store
}

fn default_store() -> ChatStore {
    store("{}", None)
}

fn message(channel: &str, author: &str, extra: serde_json::Value) -> Message {
    let mut value = serde_json::json!({
        "_id": "01JC00000000000000000000ZZ", "channel": channel, "author": author, "content": "hello there"
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

fn notify(store: &ChatStore, message: &Message, viewing: Viewing<'_>) -> Option<Notice> {
    should_notify(store, message, viewing, NOW)
}

fn mention() -> serde_json::Value {
    serde_json::json!({ "mentions": ["u-me"] })
}

#[test]
fn a_dm_notifies_by_default_with_the_web_title() {
    let notice = notify(
        &default_store(),
        &message("dm-ada", "u-ada", serde_json::json!({})),
        AWAY,
    )
    .expect("DMs default to all");
    assert_eq!(notice.title, "@Ada L.");
    assert_eq!(notice.body, "hello there");
    assert_eq!(notice.tag, "dm-ada");
    assert_eq!(notice.channel, "dm-ada");
}

#[test]
fn looking_at_the_channel_only_skips_while_the_window_is_active() {
    let store = default_store();
    let msg = message("dm-ada", "u-ada", serde_json::json!({}));
    let looking = |active| Viewing {
        channel: Some("dm-ada"),
        window_active: active,
    };
    assert!(notify(&store, &msg, looking(true)).is_none());
    assert!(notify(&store, &msg, looking(false)).is_some());
    // Looking at a different channel doesn't count.
    let elsewhere = Viewing {
        channel: Some("ch-general"),
        window_active: true,
    };
    assert!(notify(&store, &msg, elsewhere).is_some());
}

#[test]
fn own_and_blocked_authors_never_notify() {
    let store = default_store();
    assert!(
        notify(
            &store,
            &message("dm-ada", "u-me", serde_json::json!({})),
            AWAY
        )
        .is_none()
    );
    assert!(notify(&store, &message("dm-ada", "u-blocked", mention()), AWAY).is_none());
}

#[test]
fn suppressed_messages_never_notify() {
    // MessageFlags::SuppressNotifications = 1 → mask 1 << 1.
    let msg = message("dm-ada", "u-ada", serde_json::json!({ "flags": 1 << 1 }));
    assert!(notify(&default_store(), &msg, AWAY).is_none());
}

#[test]
fn servers_default_to_mentions_only() {
    let store = default_store();
    assert!(
        notify(
            &store,
            &message("ch-general", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_none()
    );
    let notice = notify(&store, &message("ch-general", "u-ada", mention()), AWAY).unwrap();
    assert_eq!(notice.title, "@Ada L. (#general, Alpha Team)");
}

#[test]
fn levels_resolve_channel_then_server_then_default() {
    let all = store(r#"{"server":{"srv-alpha":"all"}}"#, None);
    assert!(
        notify(
            &all,
            &message("ch-general", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_some()
    );

    // The channel's own level beats its server's, even for mentions.
    let silenced = store(
        r#"{"server":{"srv-alpha":"all"},"channel":{"ch-general":"none"}}"#,
        None,
    );
    assert!(notify(&silenced, &message("ch-general", "u-ada", mention()), AWAY).is_none());
    // Other channels in the server still follow the server.
    assert!(
        notify(
            &silenced,
            &message("ch-random", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_some()
    );

    // DMs can be turned down too.
    let quiet_dm = store(r#"{"channel":{"dm-ada":"mention"}}"#, None);
    assert!(
        notify(
            &quiet_dm,
            &message("dm-ada", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_none()
    );
    assert!(notify(&quiet_dm, &message("dm-ada", "u-ada", mention()), AWAY).is_some());
}

#[test]
fn mutes_silence_even_mentions_until_they_expire() {
    let channel_muted = store(r#"{"channel_mutes":{"dm-ada":{}}}"#, None);
    assert!(notify(&channel_muted, &message("dm-ada", "u-ada", mention()), AWAY).is_none());

    let server_muted = store(r#"{"server_mutes":{"srv-alpha":{}}}"#, None);
    assert!(
        notify(
            &server_muted,
            &message("ch-general", "u-ada", mention()),
            AWAY
        )
        .is_none()
    );

    let expired = store(
        &format!(
            r#"{{"channel_mutes":{{"dm-ada":{{"until":{}}}}}}}"#,
            NOW - 1
        ),
        None,
    );
    assert!(
        notify(
            &expired,
            &message("dm-ada", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_some()
    );

    // Legacy "muted" level means muted.
    let legacy = store(r#"{"channel":{"dm-ada":"muted"}}"#, None);
    assert!(notify(&legacy, &message("dm-ada", "u-ada", mention()), AWAY).is_none());
}

#[test]
fn busy_silences_everything_and_focus_allows_mentions() {
    let busy = store("{}", Some("Busy"));
    assert!(notify(&busy, &message("dm-ada", "u-ada", mention()), AWAY).is_none());

    let focus = store("{}", Some("Focus"));
    assert!(
        notify(
            &focus,
            &message("dm-ada", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_none()
    );
    assert!(notify(&focus, &message("dm-ada", "u-ada", mention()), AWAY).is_some());

    let online = store("{}", Some("Online"));
    assert!(
        notify(
            &online,
            &message("dm-ada", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_some()
    );
}

#[test]
fn saved_notes_never_notify() {
    assert!(
        notify(
            &default_store(),
            &message("ch-saved", "u-ada", mention()),
            AWAY
        )
        .is_none()
    );
}

#[test]
fn group_titles_and_system_messages() {
    let store = default_store();
    let notice = notify(
        &store,
        &message("grp-weekend", "u-ada", serde_json::json!({})),
        AWAY,
    )
    .unwrap();
    assert_eq!(notice.title, "@Ada L. - Weekend plans");

    let system = message(
        "grp-weekend",
        "00000000000000000000000000",
        serde_json::json!({ "content": null, "system": { "type": "user_added", "id": "u-bob", "by": "u-ada" } }),
    );
    let notice = notify(&store, &system, AWAY).unwrap();
    assert_eq!(notice.title, "Weekend plans");
    assert_eq!(notice.body, "Ada L. added bob");
}

#[test]
fn bodies_are_plain_and_truncated() {
    let store = default_store();
    let msg = message(
        "dm-ada",
        "u-ada",
        serde_json::json!({ "content": "hey <@u-bob>, see <#ch-general>" }),
    );
    assert_eq!(
        notify(&store, &msg, AWAY).unwrap().body,
        "hey @bob, see #general"
    );

    let long = message(
        "dm-ada",
        "u-ada",
        serde_json::json!({ "content": "x".repeat(500) }),
    );
    let body = notify(&store, &long, AWAY).unwrap().body;
    assert_eq!(body.chars().count(), 201);
    assert!(body.ends_with('…'));
}

#[test]
fn muted_channels_hide_their_dot_but_keep_mentions() {
    let mut store = store(r#"{"channel_mutes":{"ch-random":{}}}"#, None);
    // The fixture has no read positions; read #general so only #random matters.
    store.apply_ack("ch-general", "01JC0000000000000000000009");
    store.apply_message(message("ch-random", "u-ada", mention()));
    assert!(store.is_unread("ch-random"), "still unread underneath");
    let random = store
        .server_channels("srv-alpha")
        .into_iter()
        .flat_map(|s| s.channels)
        .find(|c| c.id == "ch-random")
        .unwrap();
    assert!(!random.unread, "no dot while muted");
    assert_eq!(random.mentions, 1, "mentions still count");
    let alpha = store
        .servers()
        .into_iter()
        .find(|s| s.id == "srv-alpha")
        .unwrap();
    assert!(!alpha.unread);
    assert_eq!(alpha.mentions, 1);
}
