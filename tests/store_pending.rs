//! Sending: pending messages shown at once and confirmed by nonce (POST
//! response or live echo, either first), failures with retry and discard,
//! and the message box's rules.

use serde_json::json;
use stoat_gpui::api::models::{BulkMessageResponse, Message};
use stoat_gpui::draft::{
    Drafts, MAX_MESSAGE_CHARS, SendCheck, char_count, check_send, new_nonce,
};
use stoat_gpui::store::{
    ChatStore, MessageRow, PendingMessage, PendingState, RowOp, TimelineRow, diff_rows,
};

const CH: &str = "ch-send";
/// When the loaded messages were sent (ms since the epoch).
const T0: u64 = 1_793_700_000_000;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A ULID created at `millis`, with `tail` (16 characters) after the time.
fn ulid_at(millis: u64, tail: &str) -> String {
    let mut prefix = [b'0'; 10];
    let mut n = millis;
    for slot in prefix.iter_mut().rev() {
        *slot = CROCKFORD[(n % 32) as usize];
        n /= 32;
    }
    format!("{}{tail}", std::str::from_utf8(&prefix).unwrap())
}

fn message(id: &str, author: &str, content: &str, nonce: Option<&str>) -> Message {
    serde_json::from_value(json!({
        "_id": id, "channel": CH, "author": author, "content": content, "nonce": nonce
    }))
    .unwrap()
}

/// A channel with Ada's message, then one of ours (at `T0`).
fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": ulid_at(T0 - 60_000, "AAAAAAAAAAAAAAAA"), "channel": CH,
          "author": "u-ada", "content": "hi" },
        { "_id": ulid_at(T0, "BBBBBBBBBBBBBBBB"), "channel": CH,
          "author": "u-me", "content": "hello" },
    ]))
    .unwrap();
    store.set_history(CH, page, 50);
    store
}

fn pending(key: &str, content: &str, sent_ms: u64) -> PendingMessage {
    PendingMessage {
        key: key.into(),
        nonce: key.into(),
        channel: CH.into(),
        content: content.into(),
        replies: Vec::new(),
        state: PendingState::Sending,
        sent_ms,
        nonce_used: false,
    }
}

fn rows(store: &ChatStore) -> Vec<MessageRow> {
    store
        .timeline(CH)
        .into_iter()
        .filter_map(|row| match row {
            TimelineRow::Message(m) => Some(m),
            TimelineRow::StartOfChannel { .. } => None,
        })
        .collect()
}

/// (body, pending state) of each row.
fn summary(store: &ChatStore) -> Vec<(String, Option<PendingState>)> {
    rows(store)
        .into_iter()
        .map(|r| (r.body.unwrap_or_default(), r.pending))
        .collect()
}

fn real(body: &str) -> (String, Option<PendingState>) {
    (body.into(), None)
}

fn sending(body: &str) -> (String, Option<PendingState>) {
    (body.into(), Some(PendingState::Sending))
}

#[test]
fn a_pending_message_shows_after_the_loaded_ones() {
    let mut store = store();
    store.add_pending(pending("n1", "on its way", T0 + 1_000));
    assert_eq!(
        summary(&store),
        vec![real("hi"), real("hello"), sending("on its way")]
    );
    let row = rows(&store).pop().unwrap();
    assert!(row.mine);
    assert_eq!(row.id, "n1");
    assert_eq!(row.key, "n1");
    assert!(row.reactions.is_empty());
}

#[test]
fn the_echo_with_our_nonce_confirms_it() {
    let mut store = store();
    store.add_pending(pending("n1", "on its way", T0 + 1_000));
    let id = ulid_at(T0 + 1_200, "CCCCCCCCCCCCCCCC");
    assert!(store.apply_message(message(&id, "u-me", "on its way", Some("n1"))));
    assert_eq!(
        summary(&store),
        vec![real("hi"), real("hello"), real("on its way")]
    );
    assert!(store.pending(CH).is_empty());
}

#[test]
fn the_post_response_then_the_echo_give_one_row() {
    let mut store = store();
    store.add_pending(pending("n1", "once", T0 + 1_000));
    let id = ulid_at(T0 + 1_200, "CCCCCCCCCCCCCCCC");
    // The POST response first, then the same message as a live event.
    assert!(store.apply_message(message(&id, "u-me", "once", Some("n1"))));
    assert!(store.apply_message(message(&id, "u-me", "once", Some("n1"))));
    assert_eq!(summary(&store), vec![real("hi"), real("hello"), real("once")]);
}

#[test]
fn the_echo_then_the_post_response_give_one_row() {
    let mut store = store();
    store.add_pending(pending("n1", "once", T0 + 1_000));
    let id = ulid_at(T0 + 1_200, "CCCCCCCCCCCCCCCC");
    let echo = message(&id, "u-me", "once", Some("n1"));
    assert!(store.apply_message(echo.clone()));
    // The response arriving second must not bring the pending row back, or
    // add a second copy.
    assert!(store.apply_message(echo));
    assert_eq!(summary(&store), vec![real("hi"), real("hello"), real("once")]);
}

#[test]
fn someone_elses_message_with_the_same_nonce_doesnt_confirm_ours() {
    let mut store = store();
    store.add_pending(pending("n1", "mine", T0 + 1_000));
    let id = ulid_at(T0 + 1_200, "CCCCCCCCCCCCCCCC");
    store.apply_message(message(&id, "u-ada", "theirs", Some("n1")));
    assert_eq!(
        summary(&store),
        vec![real("hi"), real("hello"), real("theirs"), sending("mine")]
    );
}

#[test]
fn a_history_load_that_has_the_message_confirms_it() {
    let mut store = store();
    store.add_pending(pending("n1", "sent while offline", T0 + 1_000));
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": ulid_at(T0 + 1_200, "CCCCCCCCCCCCCCCC"), "channel": CH,
          "author": "u-me", "content": "sent while offline", "nonce": "n1" },
    ]))
    .unwrap();
    store.set_history(CH, page, 50);
    assert_eq!(summary(&store), vec![real("sent while offline")]);
}

#[test]
fn failing_then_retrying_gives_sending_again_and_discard_removes_it() {
    let mut store = store();
    store.add_pending(pending("n1", "try me", T0 + 1_000));
    assert!(store.fail_pending(CH, "n1", "Couldn't reach Stoat.".into(), false));
    assert_eq!(
        rows(&store).pop().unwrap().pending,
        Some(PendingState::Failed("Couldn't reach Stoat.".into()))
    );

    let retried = store.retry_pending(CH, "n1", || panic!("keeps its nonce")).unwrap();
    assert_eq!(retried.state, PendingState::Sending);
    assert_eq!(retried.nonce, "n1", "Stoat may have it: same key");
    // Retrying something that's still sending does nothing.
    assert!(store.retry_pending(CH, "n1", || "n2".into()).is_none());

    assert!(store.fail_pending(CH, "n1", "nope".into(), false));
    assert!(store.discard_pending(CH, "n1"));
    assert_eq!(summary(&store), vec![real("hi"), real("hello")]);
    assert!(!store.discard_pending(CH, "n1"));
}

#[test]
fn a_retry_after_stoat_refused_uses_a_fresh_nonce_but_keeps_its_row() {
    let mut store = store();
    store.add_pending(pending("n1", "try me", T0 + 1_000));
    // Stoat answered (rate limited, say): it has seen "n1", so it would
    // refuse it as a duplicate.
    assert!(store.fail_pending(CH, "n1", "Sending too fast.".into(), true));
    let retried = store.retry_pending(CH, "n1", || "n2".into()).unwrap();
    assert_eq!(retried.nonce, "n2");
    assert_eq!(retried.key, "n1");
    assert_eq!(rows(&store).pop().unwrap().key, "n1");

    // The new nonce is what confirms it now.
    let id = ulid_at(T0 + 2_000, "CCCCCCCCCCCCCCCC");
    store.apply_message(message(&id, "u-me", "try me", Some("n1")));
    assert_eq!(store.pending(CH).len(), 1, "the old nonce isn't ours any more");
    store.apply_message(message(&id, "u-me", "try me", Some("n2")));
    assert!(store.pending(CH).is_empty());
}

#[test]
fn pending_rows_group_with_our_last_message() {
    let mut store = store();
    store.add_pending(pending("n1", "one", T0 + 60_000));
    store.add_pending(pending("n2", "two", T0 + 61_000));
    // Long after: a new group.
    store.add_pending(pending("n3", "three", T0 + 20 * 60_000));
    let grouping: Vec<(bool, bool)> = rows(&store)
        .iter()
        .map(|r| (r.continuation, r.group_end))
        .collect();
    assert_eq!(
        grouping,
        vec![
            (false, true), // Ada
            (false, false), // ours, loaded
            (true, false),  // n1 continues it
            (true, true),   // n2 too, and ends it
            (false, true),  // n3 starts its own
        ]
    );
}

#[test]
fn diff_appends_when_sending_and_remeasures_when_confirmed_or_failed() {
    let mut store = store();
    let before = store.timeline(CH);
    store.add_pending(pending("n1", "hey", T0 + 1_000));
    let sent = store.timeline(CH);
    // Row 0 is the start of the channel. Sending also ends our earlier
    // message's group, so that's re-measured.
    assert_eq!(
        diff_rows(&before, &sent),
        vec![RowOp::Remeasure { at: 2 }, RowOp::Append(1)]
    );

    store.fail_pending(CH, "n1", "nope".into(), false);
    let failed = store.timeline(CH);
    assert_eq!(diff_rows(&sent, &failed), vec![RowOp::Remeasure { at: 3 }]);

    store.retry_pending(CH, "n1", || "n2".into());
    let retrying = store.timeline(CH);
    let id = ulid_at(T0 + 1_200, "CCCCCCCCCCCCCCCC");
    store.apply_message(message(&id, "u-me", "hey", Some("n1")));
    let confirmed = store.timeline(CH);
    assert_eq!(
        diff_rows(&retrying, &confirmed),
        vec![RowOp::Remeasure { at: 3 }]
    );
}

#[test]
fn pending_markdown_is_prepared_like_real_messages() {
    let mut store = store();
    store.add_pending(pending("n1", "line one\nline two", T0 + 1_000));
    let pending_body = rows(&store).pop().unwrap().body;
    let id = ulid_at(T0 + 1_200, "CCCCCCCCCCCCCCCC");
    store.apply_message(message(&id, "u-me", "line one\nline two", Some("n1")));
    assert_eq!(rows(&store).pop().unwrap().body, pending_body);
}

#[test]
fn only_messages_with_something_in_them_can_be_sent() {
    assert_eq!(check_send(""), SendCheck::Empty);
    assert_eq!(check_send("  \n\t \n"), SendCheck::Empty);
    assert_eq!(check_send("hello"), SendCheck::Ready("hello".into()));
    // Blank lines before and whitespace after go; indentation and inner
    // blank lines stay.
    assert_eq!(
        check_send("\n\n    code\n\nmore  \n"),
        SendCheck::Ready("    code\n\nmore".into())
    );
}

#[test]
fn the_limit_counts_characters_not_bytes() {
    let at_limit = "é".repeat(MAX_MESSAGE_CHARS);
    assert!(matches!(check_send(&at_limit), SendCheck::Ready(_)));
    let over = "é".repeat(MAX_MESSAGE_CHARS + 5);
    assert_eq!(check_send(&over), SendCheck::TooLong(MAX_MESSAGE_CHARS + 5));

    assert_eq!(char_count("short"), None);
    assert_eq!(char_count(&"a".repeat(1800)), Some((1800, false)));
    assert_eq!(char_count(&"a".repeat(2001)), Some((2001, true)));
}

#[test]
fn drafts_are_kept_per_channel() {
    let mut drafts = Drafts::default();
    drafts.save("a", "half a thought");
    drafts.save("b", "   ");
    assert_eq!(drafts.take("a"), "half a thought");
    assert_eq!(drafts.take("a"), "", "taken");
    assert_eq!(drafts.take("b"), "", "blank drafts aren't kept");
}

#[test]
fn nonces_are_unique_and_fit_stoats_limit() {
    let a = new_nonce(T0);
    let b = new_nonce(T0);
    assert_ne!(a, b);
    assert!(a.len() <= 64, "{a}");
    assert!(new_nonce(u64::MAX).len() <= 64);
}
