//! Unread state: read positions from Ready, live messages, mentions, acks,
//! and the per-server / Home summaries the sidebar shows.

use stoat_gpui::api::events::{Ready, ServerEvent, parse_frame};
use stoat_gpui::api::models::{BulkMessageResponse, ChannelUnread, Message};
use stoat_gpui::store::{ChatStore, TimelineRow, with_unread_divider};

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

#[test]
fn account_summary_covers_home_and_servers() {
    // Home: dm-ada (1 mention), dm-bob and the group are unread; servers are read.
    let mut store = store();
    assert_eq!(store.account_summary(), (true, 1));

    store.apply_message(message(
        "ch-random",
        "u-ada",
        serde_json::json!({ "mentions": ["u-me"] }),
    ));
    assert_eq!(store.account_summary(), (true, 2));
}

#[test]
fn account_summary_is_clear_when_everything_is_read() {
    let mut store = store();
    store.apply_ack("dm-ada", DM_ADA_LAST);
    store.apply_ack("dm-bob", "01JC0000000000000000000007");
    store.apply_ack("grp-weekend", "01JC0000000000000000000006");
    assert_eq!(store.home_summary(), (false, 0), "fixture check");
    assert_eq!(store.account_summary(), (false, 0));
}

#[test]
fn an_empty_store_has_nothing_to_show() {
    assert_eq!(ChatStore::new("u-me").account_summary(), (false, 0));
}

// --- friend requests -----------------------------------------------------

/// `user` now relates to the user as `relationship` (a `UserRelationship`).
fn relate(store: &mut ChatStore, user: &str, relationship: &str) {
    let frame = serde_json::json!({
        "type": "UserRelationship", "id": "u-me",
        "user": {
            "_id": user, "username": user, "discriminator": "0001",
            "relationship": relationship, "online": true
        }
    });
    let Some(ServerEvent::UserRelationship { user }) = parse_frame(&frame.to_string()).pop() else {
        panic!("UserRelationship");
    };
    store.apply_relationship(*user);
}

/// Everything read, so only friend requests count.
fn read_store() -> ChatStore {
    let mut store = store();
    store.apply_ack("dm-ada", DM_ADA_LAST);
    store.apply_ack("dm-bob", "01JC0000000000000000000007");
    store.apply_ack("grp-weekend", "01JC0000000000000000000006");
    store
}

fn requests_entry(store: &ChatStore) -> Option<usize> {
    store
        .attention_breakdown()
        .into_iter()
        .find(|(label, _)| label == "Friend requests")
        .map(|(_, n)| n)
}

#[test]
fn each_friend_request_counts_as_a_mention_without_a_dot() {
    let mut store = read_store();
    assert_eq!(store.attention_count(), 0, "fixture check");

    relate(&mut store, "u-carol", "Incoming");
    assert_eq!(store.home_summary(), (false, 1));
    assert_eq!(store.account_summary(), (false, 1));
    assert_eq!(store.attention_count(), 1);
    assert_eq!(requests_entry(&store), Some(1));

    relate(&mut store, "u-dave", "Incoming");
    assert_eq!(store.home_summary(), (false, 2));
    assert_eq!(store.account_summary(), (false, 2));
    assert_eq!(store.attention_count(), 2);
    assert_eq!(requests_entry(&store), Some(2));

    // Accepted, then rejected: the counts fall back.
    relate(&mut store, "u-carol", "Friend");
    assert_eq!(store.home_summary(), (false, 1));
    assert_eq!(store.attention_count(), 1);
    relate(&mut store, "u-dave", "None");
    assert_eq!(store.home_summary(), (false, 0));
    assert_eq!(store.account_summary(), (false, 0));
    assert_eq!(store.attention_count(), 0);
    assert_eq!(requests_entry(&store), None);
}

#[test]
fn outgoing_requests_and_friends_add_nothing() {
    let mut store = read_store();
    relate(&mut store, "u-carol", "Outgoing");
    relate(&mut store, "u-dave", "Friend");
    relate(&mut store, "u-erin", "Blocked");
    assert_eq!(store.home_summary(), (false, 0));
    assert_eq!(store.account_summary(), (false, 0));
    assert_eq!(store.attention_count(), 0);
    assert_eq!(requests_entry(&store), None);
}

#[test]
fn friend_requests_add_to_unread_mentions() {
    // dm-ada's one mention, plus a request.
    let mut store = store();
    relate(&mut store, "u-carol", "Incoming");
    assert_eq!(store.home_summary(), (true, 2));
    assert_eq!(store.account_summary(), (true, 2));
}

// The "New messages" divider.

const M1: &str = "01JD0000000000000000000001";
const M2: &str = "01JD0000000000000000000002";
const M3: &str = "01JD0000000000000000000003";
const M4: &str = "01JD0000000000000000000004";

/// `#general` with `authors` as its loaded messages (M1, M2, ...), all
/// within a minute so same-author runs group. `whole` = there's nothing
/// older (a start-of-channel row).
fn general_with(authors: &[&str], whole: bool) -> ChatStore {
    let mut store = store();
    let messages = [M1, M2, M3, M4]
        .iter()
        .zip(authors)
        .map(|(id, author)| message("ch-general", author, serde_json::json!({ "_id": id })))
        .collect::<Vec<_>>();
    let limit = if whole {
        messages.len() + 1
    } else {
        messages.len()
    };
    store.set_history(
        "ch-general",
        BulkMessageResponse::JustMessages(messages),
        limit,
    );
    store
}

/// Each row as `"new"`, `"start"` or its message ID, with `+` after a
/// continuation (no header).
fn shape(rows: &[TimelineRow]) -> Vec<String> {
    rows.iter()
        .map(|row| match row {
            TimelineRow::StartOfChannel { .. } => "start".to_string(),
            TimelineRow::NewMessages => "new".to_string(),
            TimelineRow::Message(m) if m.continuation => format!("{}+", &m.id[22..]),
            TimelineRow::Message(m) => m.id[22..].to_string(),
        })
        .collect()
}

fn divided(store: &ChatStore, after: &str) -> Vec<String> {
    shape(&with_unread_divider(store.timeline("ch-general"), after))
}

#[test]
fn the_read_position_is_recorded() {
    let store = store();
    assert_eq!(store.read_position("ch-general"), Some(GENERAL_LAST));
    assert_eq!(store.read_position("dm-bob"), None, "only Ada's position");
}

#[test]
fn the_divider_sits_before_the_first_unread_message() {
    let store = general_with(&["u-ada", "u-ada", "u-ada", "u-bob"], false);
    assert_eq!(
        shape(&store.timeline("ch-general")),
        ["0001", "0002+", "0003+", "0004"]
    );
    // The message after the divider gets its header back.
    assert_eq!(
        divided(&store, M2),
        ["0001", "0002+", "new", "0003", "0004"]
    );
    let rows = with_unread_divider(store.timeline("ch-general"), M2);
    let TimelineRow::Message(before) = &rows[1] else {
        panic!("message row");
    };
    assert!(before.group_end, "the group ends at the divider");
}

#[test]
fn no_divider_when_everything_is_read() {
    let store = general_with(&["u-ada", "u-bob", "u-ada", "u-bob"], false);
    assert_eq!(divided(&store, M4), ["0001", "0002", "0003", "0004"]);
}

#[test]
fn the_users_own_unread_messages_get_no_divider() {
    let store = general_with(&["u-ada", "u-bob", "u-me", "u-me"], false);
    assert_eq!(divided(&store, M2), ["0001", "0002", "0003", "0004+"]);
    // Others' messages after the user's own: the divider goes before theirs.
    let store = general_with(&["u-ada", "u-me", "u-bob", "u-bob"], false);
    assert_eq!(
        divided(&store, M1),
        ["0001", "0002", "new", "0003", "0004+"]
    );
}

#[test]
fn no_divider_when_the_read_position_is_older_than_the_loaded_history() {
    let store = general_with(&["u-ada", "u-bob", "u-ada", "u-bob"], false);
    let older = "01JC0000000000000000000001";
    assert_eq!(divided(&store, older), ["0001", "0002", "0003", "0004"]);
    // Unless that's the whole history: then everything is new.
    let store = general_with(&["u-ada", "u-bob", "u-ada", "u-bob"], true);
    assert_eq!(
        divided(&store, older),
        ["start", "new", "0001", "0002", "0003", "0004"]
    );
}

#[test]
fn no_divider_when_the_read_position_is_newer_than_the_loaded_history() {
    let store = general_with(&["u-ada", "u-bob", "u-ada", "u-bob"], false);
    assert_eq!(divided(&store, NEWER), ["0001", "0002", "0003", "0004"]);
}
