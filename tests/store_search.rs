//! Search results: taking them in leaves the channel's history alone, their
//! authors are named from what came with them, and every result row stands
//! on its own, with its author's header.

use serde_json::json;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{BulkMessageResponse, Message};
use stoat_gpui::store::{ChatStore, MessageRow};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

/// The Ready fixture's store, with its one page of `ch-general` history.
fn store_with_history() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let Some(ServerEvent::Ready(ready)) = parse_frame(&fixture("ready.json")).pop() else {
        panic!("ready fixture");
    };
    store.apply_ready(*ready);
    let page: BulkMessageResponse = serde_json::from_str(&fixture("messages_page.json")).unwrap();
    store.set_history("ch-general", page, 50);
    store
}

fn history_ids(store: &ChatStore) -> Vec<String> {
    let history = store.history("ch-general").unwrap();
    history.messages.iter().map(|m| m.id.clone()).collect()
}

/// Matches far older than the loaded history, as Stoat would send them,
/// newest first, with their authors.
fn results() -> BulkMessageResponse {
    serde_json::from_value(json!({
        "messages": [
            // Two in a row from one person, a minute apart.
            { "_id": "01J00000010000000000000002", "channel": "ch-general", "author": "u-dave",
              "content": "deploy done" },
            { "_id": "01J00000010000000000000001", "channel": "ch-general", "author": "u-dave",
              "content": "starting the deploy" },
            // A known author, and the user themself (who has a nickname).
            { "_id": "01J00000000000000000000009", "channel": "ch-general", "author": "u-ada",
              "content": "who owns the deploy?" },
            { "_id": "01J00000000000000000000008", "channel": "ch-general", "author": "u-me",
              "content": "I do, see deploy notes" },
            // A reply to something that isn't loaded.
            { "_id": "01J00000000000000000000007", "channel": "ch-general", "author": "u-bob",
              "content": "deploy it", "replies": ["01J00000000000000000000001"] },
            // Not from the channel searched.
            { "_id": "01J00000000000000000000006", "channel": "ch-random", "author": "u-ada",
              "content": "deploy elsewhere" }
        ],
        "users": [
            { "_id": "u-dave", "username": "dave", "discriminator": "4242",
              "display_name": "Dave", "relationship": "None", "online": false }
        ],
        "members": [
            { "_id": { "server": "srv-alpha", "user": "u-dave" },
              "joined_at": "2026-01-01T00:00:00.000Z", "nickname": "Davey", "roles": [] }
        ]
    }))
    .unwrap()
}

fn rows(store: &mut ChatStore, page: BulkMessageResponse) -> Vec<MessageRow> {
    let messages = store.absorb_results(page);
    store.result_rows("ch-general", &messages)
}

#[test]
fn results_do_not_change_the_history() {
    let mut store = store_with_history();
    let before = history_ids(&store);
    let at_present = store.is_at_present("ch-general");

    let messages = store.absorb_results(results());
    assert_eq!(messages.len(), 6);

    assert_eq!(history_ids(&store), before);
    assert_eq!(store.is_at_present("ch-general"), at_present);
    assert!(!store.has_message("ch-general", "01J00000010000000000000002"));
    // Nor is a channel that was never opened given one.
    assert!(store.history("ch-random").is_none());
}

#[test]
fn results_come_in_the_order_stoat_sent_them() {
    let mut store = store_with_history();
    let ids: Vec<String> = rows(&mut store, results())
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(
        ids,
        [
            "01J00000010000000000000002",
            "01J00000010000000000000001",
            "01J00000000000000000000009",
            "01J00000000000000000000008",
            "01J00000000000000000000007",
        ]
    );
}

#[test]
fn authors_are_named_from_the_users_that_came_with_the_results() {
    let mut store = store_with_history();
    assert!(!store.knows_user("u-dave"));

    let rows = rows(&mut store, results());
    assert!(store.knows_user("u-dave"));
    // Their nickname in the server (from the results' members), then others
    // by the usual rules.
    let authors: Vec<&str> = rows.iter().map(|r| r.author.as_str()).collect();
    assert_eq!(authors, ["Davey", "Davey", "Ada L.", "Captain Me", "Bobby"]);
    assert!(rows[3].mine);
    assert!(rows[0].avatar.is_some());
}

#[test]
fn every_result_row_carries_its_header() {
    let mut store = store_with_history();
    let rows = rows(&mut store, results());
    // The two from Dave, a minute apart, would be one group in a timeline.
    assert!(rows.iter().all(|r| !r.continuation));
    assert!(rows.iter().all(|r| r.group_end));
    assert!(rows.iter().all(|r| !r.time.is_empty()));
}

#[test]
fn results_from_other_channels_are_left_out() {
    let mut store = store_with_history();
    let messages: Vec<Message> = store.absorb_results(results());
    assert!(
        store
            .result_rows("ch-random", &messages)
            .iter()
            .all(|r| r.id.ends_with('6'))
    );
    assert!(
        store
            .result_rows("ch-general", &messages)
            .iter()
            .all(|r| !r.id.ends_with('6'))
    );
}

#[test]
fn result_rows_have_no_reply_previews_or_highlight() {
    let mut store = store_with_history();
    // A jump just landed on one of them.
    store.set_highlight("01J00000000000000000000007");

    let rows = rows(&mut store, results());
    let reply = rows.iter().find(|r| r.id.ends_with('7')).unwrap();
    assert!(reply.replies.is_empty());
    assert!(rows.iter().all(|r| !r.highlighted));
}

#[test]
fn results_for_a_channel_that_isnt_loaded_still_show() {
    let mut store = ChatStore::new("u-me");
    let Some(ServerEvent::Ready(ready)) = parse_frame(&fixture("ready.json")).pop() else {
        panic!("ready fixture");
    };
    store.apply_ready(*ready);
    assert!(store.history("ch-general").is_none());

    let rows = rows(&mut store, results());
    assert_eq!(rows.len(), 5);
    assert!(store.history("ch-general").is_none());
}

#[test]
fn a_bare_array_of_results_has_no_users_to_add() {
    let mut store = store_with_history();
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": "01J00000000000000000000009", "channel": "ch-general", "author": "u-ada",
          "content": "a match" }
    ]))
    .unwrap();
    let rows = rows(&mut store, page);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].author, "Ada L.");
}
