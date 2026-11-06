//! Blocked users' messages: folded into "N blocked messages" until clicked.

use serde_json::{Value, json};
use stoat_gpui::api::models::{BulkMessageResponse, User};
use stoat_gpui::store::{
    BLOCKED_AUTHOR, ChatStore, ReplyState, RowOp, TimelineRow, diff_rows, with_unread_divider,
};

const CH: &str = "ch-blocked";

/// Message `n`'s ID. Sorts by `n`, minutes apart.
fn id(n: u32) -> String {
    format!("01M6GD{:02}00000000000000{n:04}", n % 60)
}

fn user(id: &str, relationship: &str) -> User {
    serde_json::from_value(json!({
        "_id": id, "username": id, "discriminator": "0001",
        "relationship": relationship, "online": false
    }))
    .unwrap()
}

fn message(n: u32, author: &str) -> Value {
    json!({ "_id": id(n), "channel": CH, "author": author, "content": format!("m{n}") })
}

/// `messages` loaded in a store where `u-bob` is blocked, `u-cat` blocked
/// the user, and `u-ada` is no one in particular.
fn store(messages: Vec<Value>) -> ChatStore {
    let mut store = ChatStore::new("u-me");
    store.apply_relationship(user("u-bob", "Blocked"));
    store.apply_relationship(user("u-cat", "BlockedOther"));
    store.apply_relationship(user("u-ada", "None"));
    let page: BulkMessageResponse = serde_json::from_value(Value::Array(messages)).unwrap();
    store.set_history(CH, page, 50);
    store
}

/// Each row as `"new"`, `"start"`, a message's number, or `[n m …]` for a
/// blocked run.
fn shape(rows: &[TimelineRow]) -> Vec<String> {
    let n = |id: &str| id[22..].trim_start_matches('0').to_string();
    rows.iter()
        .map(|row| match row {
            TimelineRow::StartOfChannel { .. } => "start".into(),
            TimelineRow::NewMessages => "new".into(),
            TimelineRow::Message(m) => n(&m.id),
            TimelineRow::Blocked { ids } => {
                format!(
                    "[{}]",
                    ids.iter().map(|i| n(i)).collect::<Vec<_>>().join(" ")
                )
            }
        })
        .collect()
}

fn mixed() -> ChatStore {
    store(vec![
        message(1, "u-ada"),
        message(2, "u-bob"),
        message(3, "u-bob"),
        message(4, "u-bob"),
        message(5, "u-ada"),
        message(6, "u-bob"),
        message(7, "u-bob"),
    ])
}

#[test]
fn consecutive_blocked_messages_fold_into_one_row() {
    let store = mixed();
    assert_eq!(
        shape(&store.timeline(CH)),
        ["start", "1", "[2 3 4]", "5", "[6 7]"]
    );
}

#[test]
fn a_system_message_splits_a_run() {
    let mut system = message(3, "u-bob");
    system["system"] = json!({ "type": "text", "content": "renamed" });
    let store = store(vec![message(2, "u-bob"), system, message(4, "u-bob")]);
    assert_eq!(shape(&store.timeline(CH)), ["start", "[2]", "3", "[4]"]);
}

#[test]
fn being_blocked_by_someone_hides_nothing() {
    let store = store(vec![message(1, "u-cat"), message(2, "u-cat")]);
    assert_eq!(shape(&store.timeline(CH)), ["start", "1", "2"]);
}

#[test]
fn revealing_shows_that_run_only() {
    let mut store = mixed();
    let before = store.timeline(CH);
    let TimelineRow::Blocked { ids } = &before[2] else {
        panic!("{:?}", shape(&before));
    };
    store.reveal_blocked(ids.clone());
    let after = store.timeline(CH);
    assert_eq!(shape(&after), ["start", "1", "2", "3", "4", "5", "[6 7]"]);
    // A group of their own, as any other author's.
    let TimelineRow::Message(three) = &after[3] else {
        panic!()
    };
    assert!(three.continuation);

    // The run becomes its first message's row, and the rest are inserted:
    // no reset, so the reader keeps their place.
    assert_eq!(
        diff_rows(&before, &after),
        [
            RowOp::Remeasure { at: 2 },
            RowOp::Insert { at: 3, count: 1 },
            RowOp::Insert { at: 4, count: 1 },
        ]
    );
}

#[test]
fn revealing_the_run_of_a_message_to_jump_to_it() {
    let mut store = mixed();
    assert!(!store.reveal_blocked_run_of(CH, &id(5)), "not hidden");
    assert!(store.reveal_blocked_run_of(CH, &id(7)));
    assert_eq!(
        shape(&store.timeline(CH)),
        ["start", "1", "[2 3 4]", "5", "6", "7"]
    );
}

#[test]
fn the_unread_divider_goes_before_an_unread_run() {
    let store = mixed();
    assert_eq!(
        shape(&with_unread_divider(store.timeline(CH), &id(1))),
        ["start", "1", "new", "[2 3 4]", "5", "[6 7]"]
    );
}

#[test]
fn a_run_read_partway_puts_the_divider_after_it() {
    let store = mixed();
    assert_eq!(
        shape(&with_unread_divider(store.timeline(CH), &id(3))),
        ["start", "1", "[2 3 4]", "new", "5", "[6 7]"]
    );
}

#[test]
fn a_reply_to_a_blocked_user_hides_who_and_what() {
    let mut reply = message(3, "u-ada");
    reply["replies"] = json!([id(2)]);
    let store = store(vec![message(1, "u-ada"), message(2, "u-bob"), reply]);
    let rows = store.timeline(CH);
    let Some(TimelineRow::Message(reply)) = rows.last() else {
        panic!("{:?}", shape(&rows));
    };
    let preview = &reply.replies[0];
    assert_eq!(preview.author, BLOCKED_AUTHOR);
    assert_eq!(preview.snippet, "");
    assert_eq!(preview.author_id, None);
    assert_eq!(preview.avatar, None);
    assert_eq!(preview.state, ReplyState::Loaded);
}

#[test]
fn unblocking_unfolds_and_blocking_folds() {
    let mut store = mixed();
    store.apply_relationship(user("u-bob", "None"));
    assert_eq!(
        shape(&store.timeline(CH)),
        ["start", "1", "2", "3", "4", "5", "6", "7"]
    );
    store.apply_relationship(user("u-bob", "Blocked"));
    assert_eq!(
        shape(&store.timeline(CH)),
        ["start", "1", "[2 3 4]", "5", "[6 7]"]
    );
}
