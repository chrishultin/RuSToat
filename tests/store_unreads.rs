//! Unread state: read positions from Ready, live messages, mentions, acks,
//! and the per-server / Home summaries the sidebar shows.

use stoat_gpui::api::events::{Ready, ServerEvent, parse_frame};
use stoat_gpui::api::models::{ChannelUnread, Message};
use stoat_gpui::store::ChatStore;

// From tests/fixtures/ready.json: #general's last message, and two DMs'.
const GENERAL_LAST: &str = "01JC0000000000000000000009";
const DM_ADA_LAST: &str = "01JC0000000000000000000005";
const NEWER: &str = "01JC00000000000000000000ZZ";

fn ready() -> Ready {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let Some(ServerEvent::Ready(ready)) =
        parse_frame(&std::fs::read_to_string(path).unwrap()).pop()
    else {
        panic!("ready fixture");
    };
    *ready
}

fn unread(channel: &str, user: &str, last_id: Option<&str>, mentions: &[&str]) -> ChannelUnread {
    serde_json::from_value(serde_json::json!({
        "_id": { "channel": channel, "user": user },
        "last_id": last_id,
        "mentions": mentions,
    }))
    .unwrap()
}

/// `#general` read to its latest message, dm-ada behind with one unread mention.
fn store_with(mut ready: Ready, edit: impl FnOnce(&mut Ready)) -> ChatStore {
    ready.unreads = vec![
        unread("ch-general", "u-me", Some(GENERAL_LAST), &[]),
        unread(
            "dm-ada",
            "u-me",
            Some("01JC0000000000000000000001"),
            &[DM_ADA_LAST],
        ),
        // Another user's position must be ignored.
        unread("dm-bob", "u-ada", Some("01JC0000000000000000000007"), &[]),
    ];
    edit(&mut ready);
    let mut store = ChatStore::new("u-me");
    store.apply_ready(ready);
    store
}

fn store() -> ChatStore {
    store_with(ready(), |_| {})
}

fn message(channel: &str, author: &str, extra: serde_json::Value) -> Message {
    let mut value =
        serde_json::json!({ "_id": NEWER, "channel": channel, "author": author, "content": "hi" });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

#[test]
fn read_positions_from_ready() {
    let store = store();
    assert!(
        !store.is_unread("ch-general"),
        "read up to its last message"
    );
    assert!(store.is_unread("dm-ada"), "read position is behind");
    assert_eq!(store.mention_count("dm-ada"), 1);
    // No position for us (only Ada's): unread, like the web client.
    assert!(store.is_unread("dm-bob"));
    // No messages at all: never unread.
    assert!(!store.is_unread("ch-welcome"));
    // Saved Notes and closed DMs never show as unread.
    assert!(!store.is_unread("ch-saved"));
    assert!(!store.is_unread("dm-closed"));
}

#[test]
fn a_new_message_makes_a_channel_unread() {
    let mut store = store();
    store.apply_message(message("ch-general", "u-ada", serde_json::json!({})));
    assert!(store.is_unread("ch-general"));
    assert_eq!(store.mention_count("ch-general"), 0);
}

#[test]
fn direct_mentions_count() {
    let mut store = store();
    store.apply_message(message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "mentions": ["u-me"] }),
    ));
    assert_eq!(store.mention_count("ch-general"), 1);
}

#[test]
fn role_mentions_count_when_we_have_the_role() {
    let mut store = store_with(ready(), |r| r.members[0].roles = vec!["role-mods".into()]);
    store.apply_message(message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "role_mentions": ["role-other"] }),
    ));
    assert_eq!(store.mention_count("ch-general"), 0);
    let mut later = message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "role_mentions": ["role-mods"] }),
    );
    later.id = "01JC00000000000000000000ZY".into();
    store.apply_message(later);
    assert_eq!(store.mention_count("ch-general"), 1);
}

#[test]
fn everyone_and_online_flags_are_bit_positions() {
    // MessageFlags::MentionsEveryone = 2 → mask 1 << 2.
    let everyone = message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "flags": 1 << 2 }),
    );
    // MessageFlags::MentionsOnline = 3 → mask 1 << 3.
    let online = message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "flags": 1 << 3 }),
    );
    // MessageFlags::SuppressNotifications = 1 → mask 1 << 1: not a mention.
    let suppressed = message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "flags": 1 << 1 }),
    );

    let store = store();
    assert!(store.is_mentioned(&everyone));
    assert!(store.is_mentioned(&online), "we're online in the fixture");
    assert!(!store.is_mentioned(&suppressed));

    let offline = store_with(ready(), |r| {
        r.users.iter_mut().find(|u| u.id == "u-me").unwrap().online = false;
    });
    assert!(!offline.is_mentioned(&online));
    assert!(offline.is_mentioned(&everyone));
}

#[test]
fn our_own_messages_mark_the_channel_read() {
    let mut store = store();
    store.apply_message(message("dm-ada", "u-me", serde_json::json!({})));
    assert!(!store.is_unread("dm-ada"));
    assert_eq!(
        store.mention_count("dm-ada"),
        0,
        "mentions before it are cleared"
    );
    // And we never "mention" ourselves.
    assert!(!store.is_mentioned(&message(
        "dm-ada",
        "u-me",
        serde_json::json!({ "mentions": ["u-me"] })
    )));
}

#[test]
fn acks_move_forward_only() {
    let mut store = store();
    store.apply_ack("dm-ada", DM_ADA_LAST);
    assert!(!store.is_unread("dm-ada"));
    assert_eq!(store.mention_count("dm-ada"), 0);
    // A stale ack (from another session, say) doesn't move the position back.
    store.apply_ack("dm-ada", "01JC0000000000000000000001");
    assert!(!store.is_unread("dm-ada"));
}

#[test]
fn summaries_roll_up_to_servers_and_home() {
    let mut store = store();
    let alpha = |store: &ChatStore| {
        store
            .servers()
            .into_iter()
            .find(|s| s.id == "srv-alpha")
            .unwrap()
    };
    assert!(
        !alpha(&store).unread,
        "#general is read and the rest have no messages"
    );

    store.apply_message(message(
        "ch-random",
        "u-ada",
        serde_json::json!({ "mentions": ["u-me"] }),
    ));
    let summary = alpha(&store);
    assert!(summary.unread);
    assert_eq!(summary.mentions, 1);

    // Home: dm-ada (1 mention) and dm-bob are unread.
    assert_eq!(store.home_summary(), (true, 1));
    let entries = store.home_channels();
    let ada = entries.iter().find(|e| e.id == "dm-ada").unwrap();
    assert!(ada.unread);
    assert_eq!(ada.mentions, 1);
}

#[test]
fn unacked_latest_is_the_newest_message_only_when_something_is_unread() {
    let mut store = store();
    assert_eq!(store.unacked_latest("ch-general"), None, "already read");
    assert_eq!(store.unacked_latest("dm-ada").as_deref(), Some(DM_ADA_LAST));
    assert_eq!(store.unacked_latest("ch-welcome"), None, "no messages");
    store.apply_message(message("ch-general", "u-ada", serde_json::json!({})));
    assert_eq!(store.unacked_latest("ch-general").as_deref(), Some(NEWER));
}

#[test]
fn a_server_ack_clears_every_channel_in_it() {
    let mut store = store();
    store.apply_message(message(
        "ch-random",
        "u-ada",
        serde_json::json!({ "mentions": ["u-me"] }),
    ));
    let mut general = message("ch-general", "u-ada", serde_json::json!({}));
    general.id = "01JC00000000000000000000ZY".into();
    store.apply_message(general);
    assert!(store.is_unread("ch-random") && store.is_unread("ch-general"));

    store.apply_server_ack("srv-alpha");

    assert!(!store.is_unread("ch-random"));
    assert!(!store.is_unread("ch-general"));
    assert_eq!(store.mention_count("ch-random"), 0);
    let alpha = store
        .servers()
        .into_iter()
        .find(|s| s.id == "srv-alpha")
        .unwrap();
    assert!(!alpha.unread);
    assert_eq!(alpha.mentions, 0);
    // DMs aren't part of a server.
    assert!(store.is_unread("dm-ada"));
}

#[test]
fn a_closed_dm_with_an_old_mention_doesnt_pin_the_badge() {
    // Seen live: a closed DM (not listed anywhere, so it can't be read) still
    // had one unread mention in Stoat's read state, keeping the badge at 1.
    let base = store().attention_count();
    let with_closed = store_with(ready(), |r| {
        r.unreads.push(unread(
            "dm-closed",
            "u-me",
            Some("01JC0000000000000000000001"),
            &["01JC0000000000000000000003"],
        ))
    });
    assert_eq!(with_closed.mention_count("dm-closed"), 1);
    assert_eq!(with_closed.attention_count(), base);
}

#[test]
fn mentions_at_or_before_the_read_position_are_already_read() {
    let store = store_with(ready(), |r| {
        r.unreads[1] = unread(
            "dm-ada",
            "u-me",
            Some(DM_ADA_LAST),
            &["01JC0000000000000000000002", DM_ADA_LAST],
        )
    });
    assert_eq!(store.mention_count("dm-ada"), 0);
    assert!(!store.is_unread("dm-ada"));
}

#[test]
fn has_read_follows_the_read_position() {
    let mut store = store();
    // dm-ada is read up to …01; its newest message is …05.
    assert!(store.has_read("dm-ada", "01JC0000000000000000000001"));
    assert!(!store.has_read("dm-ada", DM_ADA_LAST));
    // No position for us at all.
    assert!(!store.has_read("dm-bob", "01JC0000000000000000000007"));

    store.apply_ack("dm-ada", DM_ADA_LAST);
    assert!(store.has_read("dm-ada", DM_ADA_LAST));

    // Posting reads everything before it.
    store.apply_message(message("dm-bob", "u-me", serde_json::json!({})));
    assert!(store.has_read("dm-bob", "01JC0000000000000000000007"));
}
