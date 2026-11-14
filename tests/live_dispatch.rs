//! `live::apply_event`: what each live event does to the store, and what it
//! reports as changed. Events are parsed from frames as they arrive.

use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::live::{Changed, apply_event};
use stoat_gpui::store::{ChannelEntry, ChatStore};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn event(frame: Value) -> ServerEvent {
    let mut events = parse_frame(&frame.to_string());
    assert_eq!(events.len(), 1, "{frame}");
    events.remove(0)
}

fn apply(store: &mut ChatStore, frame: Value) -> Changed {
    apply_event(store, event(frame), 0)
}

/// A store after the fixture's `Ready`, applied through `apply_event`.
fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let ready = parse_frame(&fixture("ready.json")).pop().unwrap();
    let changed = apply_event(&mut store, ready, 0);
    assert_eq!(
        changed,
        Changed {
            ready: true,
            lists: true,
            timelines: vec![]
        }
    );
    store
}

fn channel(store: &ChatStore, server: &str, id: &str) -> ChannelEntry {
    store
        .server_channels(server)
        .into_iter()
        .flat_map(|s| s.channels)
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("{id} not listed"))
}

const NEWER: &str = "01JC0000000000000000000099";

#[test]
fn ready_fills_servers_and_channels() {
    let store = store();
    let servers: Vec<_> = store.servers().into_iter().map(|s| s.name).collect();
    assert_eq!(servers, ["beta lounge", "Alpha Team"]);
    let titles: Vec<_> = store
        .server_channels("srv-alpha")
        .into_iter()
        .map(|s| s.title)
        .collect();
    assert!(titles.contains(&Some("Chat".into())), "{titles:?}");
    let home: Vec<_> = store.home_channels().into_iter().map(|c| c.id).collect();
    assert_eq!(home.first().map(String::as_str), Some("ch-saved"));
    assert!(home.contains(&"dm-ada".to_string()));
    assert!(!home.contains(&"dm-closed".to_string()));
}

#[test]
fn a_message_in_an_unviewed_channel_marks_it_unread() {
    let mut store = store();
    // Read everything first.
    for id in ["ch-general", "ch-random", "ch-welcome"] {
        apply(
            &mut store,
            json!({ "type": "ChannelAck", "id": id, "user": "u-me", "message_id": NEWER }),
        );
    }
    assert!(!channel(&store, "srv-alpha", "ch-random").unread);

    let changed = apply(
        &mut store,
        json!({ "type": "Message", "_id": "01JC00000000000000000000A0",
                "channel": "ch-random", "author": "u-ada", "content": "hello" }),
    );
    assert!(changed.lists);
    assert!(
        changed.timelines.is_empty(),
        "nothing is loaded for #random, so there's no timeline to redraw"
    );
    assert!(channel(&store, "srv-alpha", "ch-random").unread);
    assert!(store.servers().iter().any(|s| s.id == "srv-alpha" && s.unread));
}

#[test]
fn a_mention_increments_mentions() {
    let mut store = store();
    let before = channel(&store, "srv-alpha", "ch-general").mentions;
    apply(
        &mut store,
        json!({ "type": "Message", "_id": "01JC00000000000000000000A1",
                "channel": "ch-general", "author": "u-ada", "content": "<@u-me> look",
                "mentions": ["u-me"] }),
    );
    assert_eq!(channel(&store, "srv-alpha", "ch-general").mentions, before + 1);
    let alpha = store.servers().into_iter().find(|s| s.id == "srv-alpha").unwrap();
    assert!(alpha.mentions >= 1);
}

#[test]
fn a_message_in_a_loaded_channel_changes_its_timeline() {
    let mut store = store();
    let page: BulkMessageResponse =
        serde_json::from_str(&fixture("messages_page.json")).unwrap();
    store.set_history("ch-general", page, 50);
    let changed = apply(
        &mut store,
        json!({ "type": "Message", "_id": "01M6GFZZZZZZZZZZZZZZZZZZZZ",
                "channel": "ch-general", "author": "u-ada", "content": "new" }),
    );
    assert_eq!(changed.timelines, ["ch-general"]);

    let changed = apply(
        &mut store,
        json!({ "type": "MessageUpdate", "id": "01M6GFZZZZZZZZZZZZZZZZZZZZ",
                "channel": "ch-general", "data": { "content": "edited" }, "clear": [] }),
    );
    assert_eq!(changed.timelines, ["ch-general"]);
    assert!(!changed.lists);

    let changed = apply(
        &mut store,
        json!({ "type": "MessageDelete", "id": "01M6GFZZZZZZZZZZZZZZZZZZZZ",
                "channel": "ch-general" }),
    );
    assert_eq!(changed.timelines, ["ch-general"]);
    assert!(!store.has_message("ch-general", "01M6GFZZZZZZZZZZZZZZZZZZZZ"));
}

#[test]
fn an_ack_from_another_user_changes_nothing() {
    let mut store = store();
    let changed = apply(
        &mut store,
        json!({ "type": "ChannelAck", "id": "ch-general", "user": "u-ada", "message_id": NEWER }),
    );
    assert!(changed.is_empty());
}

#[test]
fn an_own_ack_clears_unread() {
    let mut store = store();
    assert!(channel(&store, "srv-alpha", "ch-general").unread);
    let changed = apply(
        &mut store,
        json!({ "type": "ChannelAck", "id": "ch-general", "user": "u-me", "message_id": NEWER }),
    );
    assert!(changed.lists);
    assert!(!channel(&store, "srv-alpha", "ch-general").unread);
}

#[test]
fn channel_delete_removes_it() {
    let mut store = store();
    let changed = apply(&mut store, json!({ "type": "ChannelDelete", "id": "ch-random" }));
    assert!(changed.lists);
    assert!(
        store
            .server_channels("srv-alpha")
            .iter()
            .flat_map(|s| &s.channels)
            .all(|c| c.id != "ch-random")
    );
    // Deleting it again is a no-op.
    let changed = apply(&mut store, json!({ "type": "ChannelDelete", "id": "ch-random" }));
    assert!(changed.is_empty());
}

#[test]
fn server_update_renames_it() {
    let mut store = store();
    let changed = apply(
        &mut store,
        json!({ "type": "ServerUpdate", "id": "srv-alpha",
                "data": { "name": "Alpha Squad" }, "clear": [] }),
    );
    assert!(changed.lists);
    let alpha = store.servers().into_iter().find(|s| s.id == "srv-alpha").unwrap();
    assert_eq!(alpha.name, "Alpha Squad");
}

#[test]
fn server_delete_removes_it_and_its_channels() {
    let mut store = store();
    apply(&mut store, json!({ "type": "ServerDelete", "id": "srv-beta" }));
    assert!(store.servers().iter().all(|s| s.id != "srv-beta"));
    assert!(store.server_channels("srv-beta").is_empty());
}

#[test]
fn leaving_a_server_removes_it() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "ServerMemberLeave", "id": "srv-beta", "user": "u-me", "reason": "Leave" }),
    );
    assert!(store.servers().iter().all(|s| s.id != "srv-beta"));
}

#[test]
fn a_user_update_renames_their_dm() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "UserUpdate", "id": "u-bob",
                "data": { "display_name": "Robert" }, "clear": [] }),
    );
    let dm = store.channel_entry("dm-bob").unwrap();
    assert_eq!(dm.name, "Robert");
}

#[test]
fn leaving_a_group_elsewhere_removes_it_from_home() {
    let mut store = store();
    let changed = apply(
        &mut store,
        json!({ "type": "ChannelGroupLeave", "id": "grp-weekend", "user": "u-me" }),
    );
    assert!(changed.lists);
    assert!(store.home_channels().iter().all(|c| c.id != "grp-weekend"));
}

#[test]
fn typing_changes_nothing_the_lists_show() {
    let mut store = store();
    let changed = apply(
        &mut store,
        json!({ "type": "ChannelStartTyping", "id": "ch-general", "user": "u-ada" }),
    );
    assert!(changed.is_empty());
}

#[test]
fn changes_merge_without_repeating_channels() {
    let mut a = Changed {
        timelines: vec!["x".into()],
        ..Changed::default()
    };
    a.merge(Changed {
        lists: true,
        timelines: vec!["x".into(), "y".into()],
        ..Changed::default()
    });
    assert_eq!(
        a,
        Changed {
            ready: false,
            lists: true,
            timelines: vec!["x".into(), "y".into()]
        }
    );
}
