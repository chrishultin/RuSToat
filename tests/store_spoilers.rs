//! Spoilers: hidden until clicked, then shown for the session.

use serde_json::json;
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::store::{ChatStore, MessageRow, TimelineRow};

const CH: &str = "ch-spoilers";
const TWO: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";
const OTHER: &str = "01M6GDSC10BBBBBBBBBBBBBBBB";

fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": TWO, "channel": CH, "author": "u-ada", "content": "||one|| and ||two||" },
        { "_id": OTHER, "channel": CH, "author": "u-ada", "content": "||one||" },
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
fn spoilers_start_hidden() {
    let store = store();
    assert_eq!(
        row(&store, TWO).body.as_deref(),
        Some("[one](stoat:spoiler/0/0/1) and [two](stoat:spoiler/1/0/1)")
    );
}

#[test]
fn revealing_a_spoiler_changes_only_its_row() {
    let mut store = store();
    let (before, other) = (row(&store, TWO), row(&store, OTHER));

    store.reveal_spoiler(TWO, 1);
    let after = row(&store, TWO);
    assert_eq!(
        after.body.as_deref(),
        Some("[one](stoat:spoiler/0/0/1) and `two`")
    );
    assert_ne!(after.render_key, before.render_key, "a fresh parse");
    // The other message's first spoiler is its own.
    assert_eq!(row(&store, OTHER), other);

    // Revealing it again changes nothing.
    store.reveal_spoiler(TWO, 1);
    assert_eq!(row(&store, TWO), after);
}

#[test]
fn reveals_outlast_the_history_being_replaced() {
    let mut store = store();
    store.reveal_spoiler(OTHER, 0);
    // The channel loaded again (as when switching back to it).
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": OTHER, "channel": CH, "author": "u-ada", "content": "||one||" },
    ]))
    .unwrap();
    store.set_history(CH, page, 50);
    assert_eq!(row(&store, OTHER).body.as_deref(), Some("`one`"));
}
