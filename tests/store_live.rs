//! Live message events, older-history paging, and the scroller diff.

use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{BulkMessageResponse, FieldsMessage, Message, PartialMessage};
use stoat_gpui::store::{ChatStore, MessageRow, RowOp, TimelineRow, diff_rows};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn store_with_page(limit: usize) -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let Some(ServerEvent::Ready(ready)) = parse_frame(&fixture("ready.json")).pop() else {
        panic!("ready fixture");
    };
    store.apply_ready(*ready);
    let page: BulkMessageResponse = serde_json::from_str(&fixture("messages_page.json")).unwrap();
    store.set_history("ch-general", page, limit);
    store
}

fn message(id: &str, channel: &str, author: &str, content: &str) -> Message {
    serde_json::from_value(serde_json::json!({
        "_id": id, "channel": channel, "author": author, "content": content
    }))
    .unwrap()
}

fn ids(store: &ChatStore, channel: &str) -> Vec<String> {
    store
        .history(channel)
        .unwrap()
        .messages
        .iter()
        .map(|m| m.id.clone())
        .collect()
}

// Fixture IDs (oldest first): m1 01M6GDKW80…, m7 01M6GEW5A0….
const M1: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";
const M3: &str = "01M6GDSC10CCCCCCCCCCCCCCCC";
const M7: &str = "01M6GEW5A0GGGGGGGGGGGGGGGG";
const LATER: &str = "01M6GFZZZZZZZZZZZZZZZZZZZZ";

#[test]
fn live_message_appends_to_a_loaded_channel() {
    let mut store = store_with_page(50);
    assert!(store.apply_message(message(LATER, "ch-general", "u-ada", "new!")));
    assert_eq!(
        ids(&store, "ch-general").last().map(String::as_str),
        Some(LATER)
    );
    // A repeat of the same event doesn't duplicate it.
    assert!(store.apply_message(message(LATER, "ch-general", "u-ada", "new!")));
    assert_eq!(ids(&store, "ch-general").len(), 8);
}

#[test]
fn live_message_for_an_unloaded_channel_only_reorders_home() {
    let mut store = store_with_page(50);
    assert_eq!(store.home_channels()[1].id, "dm-bob");
    // dm-ada's last message was older than dm-bob's; a new one moves it up.
    assert!(!store.apply_message(message(LATER, "dm-ada", "u-ada", "hey")));
    assert!(store.history("dm-ada").is_none());
    assert_eq!(store.home_channels()[1].id, "dm-ada");
}

#[test]
fn update_merges_only_the_fields_sent() {
    let mut store = store_with_page(50);
    let data: PartialMessage = serde_json::from_value(serde_json::json!({
        "content": "Hello again", "edited": "2026-11-04T13:00:00.000Z"
    }))
    .unwrap();
    assert!(store.apply_update("ch-general", M1, data, &[FieldsMessage::Pinned]));
    let edited = &store.history("ch-general").unwrap().messages[0];
    assert_eq!(edited.content.as_deref(), Some("Hello again"));
    assert!(edited.edited.is_some());
    assert_eq!(edited.author, "u-ada", "untouched fields stay");
    assert_eq!(edited.pinned, None);
    // Updates for messages we don't have are ignored.
    let data: PartialMessage =
        serde_json::from_value(serde_json::json!({ "content": "x" })).unwrap();
    assert!(!store.apply_update("ch-general", "nope", data, &[]));
}

#[test]
fn deletes_remove_messages() {
    let mut store = store_with_page(50);
    assert!(store.apply_delete("ch-general", &[M3.to_string()]));
    assert!(!ids(&store, "ch-general").contains(&M3.to_string()));
    assert!(store.apply_delete("ch-general", &[M1.to_string(), M7.to_string()]));
    assert_eq!(ids(&store, "ch-general").len(), 4);
    assert!(!store.apply_delete("ch-general", &["nope".to_string()]));
}

#[test]
fn older_page_merges_without_duplicates() {
    // A full page (7 of 7) leaves more history to load.
    let mut store = store_with_page(7);
    assert!(!store.history("ch-general").unwrap().exhausted);
    assert_eq!(store.oldest_message_id("ch-general"), Some(M1));

    // The older page overlaps by one message (M1) and has two new, older ones.
    let older: BulkMessageResponse = serde_json::from_value(serde_json::json!([
        { "_id": M1, "channel": "ch-general", "author": "u-ada", "content": "dup" },
        { "_id": "01M6GCZZZZBBBBBBBBBBBBBBBB", "channel": "ch-general", "author": "u-bob", "content": "older" },
        { "_id": "01M6GCAAAAAAAAAAAAAAAAAAAA", "channel": "ch-general", "author": "u-bob", "content": "oldest" }
    ]))
    .unwrap();
    assert!(store.prepend_page("ch-general", older, 50));

    let ids = ids(&store, "ch-general");
    assert_eq!(ids.len(), 9);
    assert_eq!(ids[0], "01M6GCAAAAAAAAAAAAAAAAAAAA");
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    // 3 < 50: that was the last page.
    assert!(store.history("ch-general").unwrap().exhausted);
    assert!(matches!(
        store.timeline("ch-general").first(),
        Some(TimelineRow::StartOfChannel { .. })
    ));
}

// --- diff_rows -----------------------------------------------------------

fn row(id: &str, body: &str) -> TimelineRow {
    TimelineRow::Message(MessageRow {
        id: id.into(),
        key: id.into(),
        render_key: stoat_gpui::store::render_key(id, Some(body)),
        author: "a".into(),
        avatar: None,
        time: String::new(),
        body: Some(body.into()),
        attachments: Vec::new(),
        embeds: Vec::new(),
        reactions: Vec::new(),
        allowed_reactions: None,
        system: false,
        edited: false,
        continuation: false,
        group_end: true,
        mine: false,
        jumbo: false,
        pending: None,
        replies: Vec::new(),
    })
}

fn start() -> TimelineRow {
    TimelineRow::StartOfChannel {
        name: "general".into(),
    }
}

/// Replay `ops` on `old` the way the scroller would, filling inserted slots
/// from `new`, and return the result.
fn replay(old: &[TimelineRow], new: &[TimelineRow], ops: &[RowOp]) -> Vec<TimelineRow> {
    let mut list: Vec<Option<TimelineRow>> = old.iter().cloned().map(Some).collect();
    for op in ops {
        match *op {
            RowOp::Reset(n) => list = vec![None; n],
            RowOp::Prepend(n) => list.splice(0..0, vec![None; n]).for_each(drop),
            RowOp::Append(n) => list.extend(vec![None; n]),
            RowOp::Remove { at, count } => list.drain(at..at + count).for_each(drop),
            RowOp::Insert { at, count } => list.splice(at..at, vec![None; count]).for_each(drop),
            RowOp::Remeasure { at } => list[at] = None,
        }
    }
    assert_eq!(list.len(), new.len(), "row count after ops");
    // Untouched rows must already equal the new ones; touched slots take new content.
    list.into_iter()
        .zip(new)
        .map(|(slot, wanted)| {
            if let Some(kept) = &slot {
                assert_eq!(kept, wanted, "a row the ops didn't touch changed");
            }
            wanted.clone()
        })
        .collect()
}

fn check(old: &[TimelineRow], new: &[TimelineRow]) -> Vec<RowOp> {
    let ops = diff_rows(old, new);
    assert_eq!(replay(old, new, &ops), new);
    ops
}

#[test]
fn appending_a_live_message_is_one_append() {
    let old = vec![row("a", "1"), row("b", "2")];
    let new = vec![row("a", "1"), row("b", "2"), row("c", "3")];
    assert_eq!(check(&old, &new), vec![RowOp::Append(1)]);
}

#[test]
fn an_older_page_is_one_prepend_keeping_the_anchor() {
    let old = vec![row("c", "3"), row("d", "4")];
    let new = vec![
        start(),
        row("a", "1"),
        row("b", "2"),
        row("c", "3"),
        row("d", "4"),
    ];
    assert_eq!(check(&old, &new), vec![RowOp::Prepend(3)]);
}

#[test]
fn an_edit_remeasures_just_that_row() {
    let old = vec![row("a", "1"), row("b", "2"), row("c", "3")];
    let new = vec![row("a", "1"), row("b", "2 (edited)"), row("c", "3")];
    assert_eq!(check(&old, &new), vec![RowOp::Remeasure { at: 1 }]);
}

#[test]
fn a_delete_removes_one_row() {
    let old = vec![row("a", "1"), row("b", "2"), row("c", "3")];
    let new = vec![row("a", "1"), row("c", "3")];
    assert_eq!(check(&old, &new), vec![RowOp::Remove { at: 1, count: 1 }]);
}

#[test]
fn deleting_the_oldest_row_keeps_the_rest() {
    let old = vec![row("a", "1"), row("b", "2"), row("c", "3")];
    let new = vec![row("b", "2"), row("c", "3")];
    assert_eq!(check(&old, &new), vec![RowOp::Remove { at: 0, count: 1 }]);
}

#[test]
fn mixed_changes_still_replay_exactly() {
    let old = vec![row("c", "3"), row("d", "4"), row("e", "5"), row("g", "7")];
    let new = vec![
        row("a", "1"),
        row("c", "3 edited"),
        row("e", "5"),
        row("f", "6"),
        row("g", "7"),
        row("h", "8"),
    ];
    check(&old, &new);
}

#[test]
fn nothing_in_common_resets() {
    let old = vec![row("a", "1")];
    let new = vec![row("x", "9"), row("y", "10")];
    assert_eq!(check(&old, &new), vec![RowOp::Reset(2)]);
    assert_eq!(diff_rows(&[], &new), vec![RowOp::Reset(2)]);
}

#[test]
fn channel_create_and_update_reach_the_sidebar() {
    let mut store = store_with_page(50);
    let names = |store: &ChatStore| -> Vec<String> {
        store
            .server_channels("srv-alpha")
            .into_iter()
            .flat_map(|s| s.channels)
            .map(|c| c.name)
            .collect()
    };

    store.add_channel(
        serde_json::from_value(serde_json::json!({
            "channel_type": "TextChannel", "_id": "ch-new", "server": "srv-alpha",
            "name": "brand-new", "description": "fresh"
        }))
        .unwrap(),
    );
    assert!(names(&store).contains(&"brand-new".to_string()));

    store.apply_channel_update(
        "ch-new",
        serde_json::from_value(serde_json::json!({
            "name": "renamed", "last_message_id": "01JC0000000000000000000010"
        }))
        .unwrap(),
        &[stoat_gpui::api::models::FieldsChannel::Description],
    );
    assert!(names(&store).contains(&"renamed".to_string()));
    let Some(stoat_gpui::api::models::Channel::TextChannel {
        description,
        last_message_id,
        ..
    }) = store.channel("ch-new")
    else {
        panic!("still a text channel");
    };
    assert_eq!(description, &None, "cleared");
    assert_eq!(
        last_message_id.as_deref(),
        Some("01JC0000000000000000000010")
    );

    // An older last message (a stale update) doesn't move it back.
    store.apply_channel_update(
        "ch-new",
        serde_json::from_value(
            serde_json::json!({ "last_message_id": "01JC0000000000000000000001" }),
        )
        .unwrap(),
        &[],
    );
    assert!(matches!(
        store.channel("ch-new"),
        Some(stoat_gpui::api::models::Channel::TextChannel { last_message_id: Some(l), .. })
            if l == "01JC0000000000000000000010"
    ));
}

// Live member and user updates (`ServerMemberJoin`, `ServerMemberLeave`,
// `ServerMemberUpdate`, `UserUpdate`).

mod members_and_users {
    use super::*;
    use stoat_gpui::api::models::{
        FieldsMember, FieldsUser, Member, MemberCompositeKey, PartialMember, PartialUser,
    };
    use stoat_gpui::complete::TokenRef;
    use stoat_gpui::media::MediaUrls;

    fn mentionable(store: &ChatStore, channel: &str) -> Vec<String> {
        store
            .mention_candidates(channel)
            .into_iter()
            .map(|c| c.id)
            .collect()
    }

    fn member(server: &str, user: &str) -> Member {
        serde_json::from_value(serde_json::json!({
            "_id": { "server": server, "user": user },
            "joined_at": "2026-11-05T12:00:00.000Z"
        }))
        .unwrap()
    }

    fn key(server: &str, user: &str) -> MemberCompositeKey {
        MemberCompositeKey {
            server: server.into(),
            user: user.into(),
        }
    }

    fn partial_member(json: serde_json::Value) -> PartialMember {
        serde_json::from_value(json).unwrap()
    }

    fn partial_user(json: serde_json::Value) -> PartialUser {
        serde_json::from_value(json).unwrap()
    }

    fn avatar(id: &str) -> serde_json::Value {
        serde_json::json!({
            "_id": id, "tag": "avatars", "filename": "a.png",
            "metadata": { "type": "Image", "width": 64, "height": 64 },
            "content_type": "image/png", "size": 100
        })
    }

    fn first_by<'a>(store: &'a ChatStore, channel: &str, author: &str) -> &'a Message {
        store
            .history(channel)
            .unwrap()
            .messages
            .iter()
            .find(|m| m.author == author)
            .unwrap()
    }

    #[test]
    fn a_join_makes_someone_mentionable_and_a_leave_removes_them() {
        let mut store = store_with_page(50);
        // ch-random isn't loaded: suggestions come from cached members only.
        assert_eq!(mentionable(&store, "ch-random"), ["u-bob"]);

        store.apply_member_join(member("srv-alpha", "u-ada"));
        assert_eq!(mentionable(&store, "ch-random"), ["u-ada", "u-bob"]);
        // Members are per server.
        assert!(mentionable(&store, "ch-beta").is_empty());

        assert!(!store.apply_member_leave("srv-alpha", "u-ada"));
        assert_eq!(mentionable(&store, "ch-random"), ["u-bob"]);
    }

    #[test]
    fn someone_unknown_who_joins_is_mentionable_once_their_user_is_known() {
        let mut store = store_with_page(50);
        store.apply_member_join(member("srv-alpha", "u-new"));
        assert!(!store.knows_user("u-new"));
        assert!(!mentionable(&store, "ch-random").contains(&"u-new".to_string()));

        store.add_user(
            serde_json::from_value(serde_json::json!({
                "_id": "u-new", "username": "newbie", "discriminator": "0001",
                "relationship": "None", "online": true
            }))
            .unwrap(),
        );
        assert!(mentionable(&store, "ch-random").contains(&"u-new".to_string()));
    }

    #[test]
    fn a_nickname_set_or_cleared_changes_names_and_labels() {
        let mut store = store_with_page(50);
        let bob = first_by(&store, "ch-general", "u-bob").clone();
        assert_eq!(store.name_in("ch-general", "u-bob"), "Bobby");

        assert!(store.apply_member_update(
            &key("srv-alpha", "u-bob"),
            partial_member(serde_json::json!({ "nickname": "Robert" })),
            &[],
        ));
        assert_eq!(store.name_in("ch-general", "u-bob"), "Robert");
        assert_eq!(store.author_name(&bob), "Robert");
        assert_eq!(
            store.token_label("ch-general", &TokenRef::User("u-bob".into())),
            "@Robert"
        );
        let candidates = store.mention_candidates("ch-general");
        let candidate = candidates.iter().find(|c| c.id == "u-bob").unwrap();
        assert_eq!(candidate.name, "Robert");

        assert!(store.apply_member_update(
            &key("srv-alpha", "u-bob"),
            partial_member(serde_json::json!({})),
            &[FieldsMember::Nickname],
        ));
        assert_eq!(store.name_in("ch-general", "u-bob"), "bob");
        assert_eq!(store.author_name(&bob), "bob");
    }

    #[test]
    fn a_member_update_beats_the_snapshot_a_message_came_with() {
        let mut store = store_with_page(50);
        let mut live: Message = serde_json::from_value(serde_json::json!({
            "_id": LATER, "channel": "ch-general", "author": "u-ada", "content": "hi",
            "member": {
                "_id": { "server": "srv-alpha", "user": "u-ada" },
                "joined_at": "2026-01-01T00:00:00.000Z",
                "nickname": "Old Nick"
            }
        }))
        .unwrap();
        store.apply_message(live.clone());
        assert_eq!(store.author_name(&live), "Old Nick");

        assert!(store.apply_member_update(
            &key("srv-alpha", "u-ada"),
            partial_member(serde_json::json!({ "nickname": "New Nick" })),
            &[],
        ));
        assert_eq!(store.author_name(&live), "New Nick");
        // Without a cached member, the snapshot still counts.
        live.channel = "ch-beta".into();
        assert_eq!(store.author_name(&live), "Old Nick");
    }

    #[test]
    fn a_member_update_for_someone_not_cached_is_ignored() {
        let mut store = store_with_page(50);
        assert!(!store.apply_member_update(
            &key("srv-alpha", "u-stranger"),
            partial_member(serde_json::json!({ "nickname": "X" })),
            &[],
        ));
        assert!(store.member("srv-alpha", "u-stranger").is_none());
    }

    #[test]
    fn a_user_update_changes_author_names_and_avatars() {
        let mut store = store_with_page(50);
        store.set_media(MediaUrls::new(
            "https://api.example",
            Some("https://files.example".into()),
            None,
        ));
        let ada = first_by(&store, "ch-general", "u-ada").clone();
        assert_eq!(store.author_name(&ada), "Ada L.");
        assert_eq!(
            store.user_avatar_url("u-ada"),
            "https://api.example/users/u-ada/default_avatar"
        );

        assert!(store.apply_user_update(
            "u-ada",
            partial_user(serde_json::json!({
                "display_name": "Ada Lovelace", "avatar": avatar("av1")
            })),
            &[],
        ));
        assert_eq!(store.author_name(&ada), "Ada Lovelace");
        assert_eq!(
            store.user_avatar_url("u-ada"),
            "https://files.example/avatars/av1"
        );
        assert_eq!(
            store.avatar_url(&ada).as_deref(),
            Some("https://files.example/avatars/av1")
        );
        assert_eq!(store.channels_showing("u-ada"), ["ch-general"]);

        // Clearing the avatar and display name goes back to the defaults.
        assert!(store.apply_user_update(
            "u-ada",
            partial_user(serde_json::json!({})),
            &[FieldsUser::Avatar, FieldsUser::DisplayName],
        ));
        assert_eq!(store.author_name(&ada), "ada");
        assert_eq!(
            store.user_avatar_url("u-ada"),
            "https://api.example/users/u-ada/default_avatar"
        );
    }

    #[test]
    fn a_presence_only_update_changes_nothing_shown() {
        let mut store = store_with_page(50);
        assert!(!store.apply_user_update(
            "u-ada",
            partial_user(serde_json::json!({
                "online": true, "status": { "presence": "Busy", "text": "heads down" }
            })),
            &[],
        ));
        assert!(!store.apply_user_update(
            "u-ada",
            partial_user(serde_json::json!({})),
            &[FieldsUser::StatusText, FieldsUser::StatusPresence],
        ));
        // Unknown users are ignored.
        assert!(!store.apply_user_update(
            "u-nobody",
            partial_user(serde_json::json!({ "display_name": "Who" })),
            &[],
        ));
    }

    #[test]
    fn channels_showing_lists_loaded_channels_with_that_person() {
        let mut store = store_with_page(50);
        let page: BulkMessageResponse = serde_json::from_value(serde_json::json!([
            { "_id": M1, "channel": "dm-ada", "author": "u-me", "content": "hey" }
        ]))
        .unwrap();
        store.set_history("dm-ada", page, 50);
        // dm-ada: Ada is a recipient. ch-general: she wrote there.
        assert_eq!(store.channels_showing("u-ada"), ["ch-general", "dm-ada"]);
        assert_eq!(store.channels_showing("u-bob"), ["ch-general"]);
        assert!(store.channels_showing("u-nobody").is_empty());
    }

    #[test]
    fn the_user_leaving_removes_the_server_and_its_channels() {
        let mut store = store_with_page(50);
        assert!(store.servers().iter().any(|s| s.id == "srv-alpha"));
        assert!(store.apply_member_leave("srv-alpha", "u-me"));

        let mut removed = store.remove_server("srv-alpha");
        removed.sort();
        // ch-no-access is listed by the server but the user can't see it,
        // so it was never a known channel.
        assert_eq!(
            removed,
            ["ch-general", "ch-random", "ch-voice", "ch-welcome"]
        );
        assert!(!store.servers().iter().any(|s| s.id == "srv-alpha"));
        assert!(store.server_channels("srv-alpha").is_empty());
        assert!(store.channel("ch-general").is_none());
        assert!(store.history("ch-general").is_none());
        assert!(store.member("srv-alpha", "u-bob").is_none());
        // Other servers stay.
        assert!(store.servers().iter().any(|s| s.id == "srv-beta"));
        assert!(store.channel("ch-beta").is_some());
        // Removing it again does nothing.
        assert!(store.remove_server("srv-alpha").is_empty());
    }
}
