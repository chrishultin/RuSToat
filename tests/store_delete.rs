//! Deleting: the message taken out at once and put back in place if Stoat
//! refuses, the echo changing nothing, and replies to it.

use serde_json::json;
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::store::{ChatStore, ReplyState, RowOp, TimelineRow, diff_rows};

const CH: &str = "ch-delete";
const A: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";
const B: &str = "01M6GDSC10BBBBBBBBBBBBBBBB";
const C: &str = "01M6GEW5A0CCCCCCCCCCCCCCCC";

fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": A, "channel": CH, "author": "u-ada", "content": "first" },
        { "_id": B, "channel": CH, "author": "u-me", "content": "oops" },
        { "_id": C, "channel": CH, "author": "u-ada", "content": "what?", "replies": [B] },
    ]))
    .unwrap();
    store.set_history(CH, page, 50);
    store
}

fn ids(store: &ChatStore) -> Vec<String> {
    store
        .history(CH)
        .unwrap()
        .messages
        .iter()
        .map(|m| m.id.clone())
        .collect()
}

fn reply_state(store: &ChatStore) -> ReplyState {
    store
        .timeline(CH)
        .into_iter()
        .find_map(|row| match row {
            TimelineRow::Message(m) if m.id == C => Some(m.replies[0].state),
            _ => None,
        })
        .unwrap()
}

#[test]
fn remove_then_restore_puts_it_back_in_its_exact_place() {
    let mut store = store();
    let before = store.timeline(CH);
    let removed = store.remove_local(CH, B).unwrap();
    assert_eq!(ids(&store), vec![A, C]);
    let gone = store.timeline(CH);

    assert!(store.restore_local(CH, removed));
    assert_eq!(ids(&store), vec![A, B, C]);
    let back = store.timeline(CH);
    assert_eq!(back, before);

    // Row 0 is the start of the channel. Ada's two messages, apart, are
    // their own groups either way; the reply's preview changes.
    assert_eq!(
        diff_rows(&before, &gone),
        vec![RowOp::Remove { at: 2, count: 1 }, RowOp::Remeasure { at: 2 }]
    );
    assert_eq!(
        diff_rows(&gone, &back),
        vec![RowOp::Insert { at: 2, count: 1 }, RowOp::Remeasure { at: 3 }]
    );
}

#[test]
fn the_echo_after_a_local_remove_is_a_no_op() {
    let mut store = store();
    store.remove_local(CH, B).unwrap();
    let after = store.timeline(CH);
    assert!(!store.apply_delete(CH, &[B.to_string()]));
    assert_eq!(store.timeline(CH), after);
}

#[test]
fn replies_say_the_original_was_deleted_without_fetching_it() {
    let mut store = store();
    assert_eq!(reply_state(&store), ReplyState::Loaded);
    let removed = store.remove_local(CH, B).unwrap();
    assert_eq!(reply_state(&store), ReplyState::Deleted);
    assert!(store.missing_reply_sources(CH).is_empty(), "no fetch race");
    // Refused: back to normal.
    store.restore_local(CH, removed);
    assert_eq!(reply_state(&store), ReplyState::Loaded);
}

#[test]
fn removing_what_isnt_loaded_does_nothing() {
    let mut store = store();
    assert!(store.remove_local(CH, "01M6GF00000000000000000000").is_none());
    assert!(store.remove_local("elsewhere", B).is_none());
    let removed = store.remove_local(CH, B).unwrap();
    // Restoring twice adds it once.
    assert!(store.restore_local(CH, removed.clone()));
    assert!(!store.restore_local(CH, removed));
    assert_eq!(ids(&store), vec![A, B, C]);
}

#[test]
fn the_delete_dialog_previews_the_message() {
    let store = store();
    assert_eq!(store.message_snippet(CH, B).as_deref(), Some("oops"));
    assert_eq!(store.message_snippet(CH, "nope"), None);
}
