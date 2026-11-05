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
        render_key: stoat_gpui::store::render_key(id, Some(body)),
        author: "a".into(),
        avatar: None,
        time: String::new(),
        body: Some(body.into()),
        attachments: Vec::new(),
        system: false,
        edited: false,
        continuation: false,
        group_end: true,
        mine: false,
        jumbo: false,
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
