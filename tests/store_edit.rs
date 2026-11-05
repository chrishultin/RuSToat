//! Editing: the local edit shown at once and rolled back if refused, the
//! echo changing nothing, Up's "last own message", and the draft set aside.

use serde_json::json;
use stoat_gpui::api::models::{BulkMessageResponse, PartialMessage};
use stoat_gpui::draft::{Draft, EditStash, ReplyTarget};
use stoat_gpui::store::{ChatStore, MessageRow, PendingMessage, PendingState, TimelineRow};

const CH: &str = "ch-edit";
const ADA: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";
const MINE: &str = "01M6GDSC10BBBBBBBBBBBBBBBB";
const SYSTEM: &str = "01M6GEW5A0CCCCCCCCCCCCCCCC";
const ADA_LATER: &str = "01M6GEW5A1DDDDDDDDDDDDDDDD";

fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": ADA, "channel": CH, "author": "u-ada", "content": "hi" },
        { "_id": MINE, "channel": CH, "author": "u-me", "content": "helo" },
        { "_id": SYSTEM, "channel": CH, "author": "u-me",
          "system": { "type": "text", "content": "renamed" } },
        { "_id": ADA_LATER, "channel": CH, "author": "u-ada", "content": "?" },
    ]))
    .unwrap();
    store.set_history(CH, page, 50);
    store
}

fn row(store: &ChatStore, id: &str) -> MessageRow {
    store
        .timeline(CH)
        .into_iter()
        .find_map(|row| match row {
            TimelineRow::Message(m) if m.id == id => Some(m),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_local_edit_shows_at_once_and_a_refusal_restores_it() {
    let mut store = store();
    let before = row(&store, MINE);
    assert!(!before.edited);

    let edit = store.apply_local_edit(CH, MINE, "hello").unwrap();
    let edited = row(&store, MINE);
    assert_eq!(edited.body.as_deref(), Some("hello"));
    assert!(edited.edited);
    assert_ne!(edited.render_key, before.render_key, "a fresh parse");

    assert!(store.undo_local_edit(CH, MINE, edit));
    assert_eq!(row(&store, MINE), before, "text, render key and edited flag back");
}

#[test]
fn a_refusal_doesnt_undo_a_newer_change() {
    let mut store = store();
    let edit = store.apply_local_edit(CH, MINE, "hello").unwrap();
    // Edited again elsewhere before our refusal arrived.
    let data: PartialMessage = serde_json::from_value(json!({ "content": "hello!!" })).unwrap();
    store.apply_update(CH, MINE, data, &[]);
    assert!(!store.undo_local_edit(CH, MINE, edit));
    assert_eq!(row(&store, MINE).body.as_deref(), Some("hello!!"));
}

#[test]
fn the_echo_of_our_edit_changes_nothing_visible() {
    let mut store = store();
    store.apply_local_edit(CH, MINE, "hello").unwrap();
    let local = store.timeline(CH);
    // Stoat's own edited time differs from ours; the row doesn't.
    let echo: PartialMessage = serde_json::from_value(json!({
        "content": "hello", "edited": "2026-11-05T17:00:00.000Z"
    }))
    .unwrap();
    assert!(store.apply_update(CH, MINE, echo, &[]));
    assert_eq!(store.timeline(CH), local);
}

#[test]
fn editing_an_unloaded_message_does_nothing() {
    let mut store = store();
    assert!(store.apply_local_edit(CH, "01M6GF00000000000000000000", "x").is_none());
    assert!(store.apply_local_edit("elsewhere", MINE, "x").is_none());
}

#[test]
fn up_edits_our_newest_real_message() {
    let mut store = store();
    // Not Ada's later one, not our system message, not a pending one.
    store.add_pending(PendingMessage {
        key: "n1".into(),
        nonce: "n1".into(),
        channel: CH.into(),
        content: "pending".into(),
        replies: Vec::new(),
        state: PendingState::Sending,
        sent_ms: 0,
        nonce_used: false,
    });
    assert_eq!(store.last_own_message(CH), Some(MINE));
    assert_eq!(store.raw_content(CH, MINE), Some("helo"));
    assert_eq!(ChatStore::new("u-me").last_own_message(CH), None);
}

fn draft(text: &str) -> Draft {
    Draft {
        text: text.into(),
        replies: vec![ReplyTarget {
            id: ADA.into(),
            author: "Ada".into(),
            mention: true,
        }],
    }
}

#[test]
fn the_draft_is_set_aside_while_editing_and_comes_back() {
    let mut stash = EditStash::default();
    assert_eq!(stash.editing(), None);
    assert_eq!(stash.end(), None, "nothing to put back");

    stash.begin(MINE.into(), draft("half a thought"));
    assert_eq!(stash.editing(), Some(MINE));
    // Switching to another message mid-edit: the edit's text isn't a
    // draft, so the original one is still what comes back.
    stash.begin("other".into(), draft("the first edit's text"));
    assert_eq!(stash.editing(), Some("other"));
    assert_eq!(stash.end(), Some(draft("half a thought")));
    assert_eq!(stash.editing(), None);
}
