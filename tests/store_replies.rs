//! Reply previews: finding the original (loaded history, then the cache,
//! else fetching it), deleted originals, snippets, and the mention flag.

use serde_json::json;
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::markdown::{Names, snippet};
use stoat_gpui::store::{ChatStore, MessageRow, ReplyPreview, ReplyState, RowOp, TimelineRow, diff_rows};

const CH: &str = "ch-replies";
const OLD: &str = "01M6GD00000000000000000000";
const A: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";
const B: &str = "01M6GDSC10BBBBBBBBBBBBBBBB";
const C: &str = "01M6GEW5A0CCCCCCCCCCCCCCCC";

fn store_with(messages: serde_json::Value) -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let page: BulkMessageResponse = serde_json::from_value(json!({
        "messages": messages,
        "users": [
            { "_id": "u-ada", "username": "ada", "discriminator": "0001", "relationship": "None", "online": false },
            { "_id": "u-me", "username": "me", "discriminator": "0002", "relationship": "None", "online": false }
        ]
    }))
    .unwrap();
    store.set_history(CH, page, 50);
    store
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

fn previews(store: &ChatStore, id: &str) -> Vec<ReplyPreview> {
    rows(store).into_iter().find(|r| r.id == id).unwrap().replies
}

/// A page as `fetch_messages_around` returns it: the messages and their authors.
fn around(messages: serde_json::Value, users: serde_json::Value) -> BulkMessageResponse {
    serde_json::from_value(json!({ "messages": messages, "users": users })).unwrap()
}

#[test]
fn a_reply_to_a_loaded_message_previews_it() {
    let store = store_with(json!([
        { "_id": A, "channel": CH, "author": "u-ada", "content": "**Big** news" },
        { "_id": B, "channel": CH, "author": "u-me", "content": "wow", "replies": [A] },
    ]));
    let preview = &previews(&store, B)[0];
    assert_eq!(preview.state, ReplyState::Loaded);
    assert_eq!(preview.id, A);
    assert_eq!(preview.author, "ada");
    assert_eq!(preview.author_id.as_deref(), Some("u-ada"));
    assert_eq!(preview.snippet, "Big news");
    assert!(!preview.mentions_author);
    assert!(store.missing_reply_sources(CH).is_empty());
}

#[test]
fn unloaded_originals_are_listed_once_then_shown_once_fetched() {
    let mut store = store_with(json!([
        { "_id": B, "channel": CH, "author": "u-me", "content": "re", "replies": [OLD] },
        { "_id": C, "channel": CH, "author": "u-me", "content": "again", "replies": [OLD] },
    ]));
    assert_eq!(store.missing_reply_sources(CH), vec![OLD.to_string()]);
    assert_eq!(previews(&store, B)[0].state, ReplyState::Loading);

    let before = store.timeline(CH);
    // Its author wasn't known before: the page brings them.
    store.add_reply_sources(
        around(
            json!([{ "_id": OLD, "channel": CH, "author": "u-bob", "content": "ancient" }]),
            json!([{ "_id": "u-bob", "username": "bob", "discriminator": "0003", "relationship": "None", "online": false }]),
        ),
        &[OLD.to_string()],
    );
    let preview = &previews(&store, B)[0];
    assert_eq!(preview.state, ReplyState::Loaded);
    assert_eq!(preview.author, "bob");
    assert_eq!(preview.snippet, "ancient");
    assert!(store.missing_reply_sources(CH).is_empty());

    // Both replies are re-measured (row 0 is the start of the channel).
    assert_eq!(
        diff_rows(&before, &store.timeline(CH)),
        vec![RowOp::Remeasure { at: 1 }, RowOp::Remeasure { at: 2 }]
    );
}

#[test]
fn an_original_missing_from_the_fetch_was_deleted() {
    let mut store = store_with(json!([
        { "_id": B, "channel": CH, "author": "u-me", "content": "re", "replies": [OLD] },
    ]));
    // The page has the neighbours, not the original itself.
    store.add_reply_sources(
        around(
            json!([{ "_id": A, "channel": CH, "author": "u-ada", "content": "next one" }]),
            json!([]),
        ),
        &[OLD.to_string()],
    );
    assert_eq!(previews(&store, B)[0].state, ReplyState::Deleted);
    assert!(store.missing_reply_sources(CH).is_empty(), "not fetched again");
}

#[test]
fn deleting_a_loaded_original_marks_its_replies_without_a_fetch() {
    let mut store = store_with(json!([
        { "_id": A, "channel": CH, "author": "u-ada", "content": "oops" },
        { "_id": B, "channel": CH, "author": "u-me", "content": "re", "replies": [A] },
    ]));
    assert!(store.apply_delete(CH, &[A.to_string()]));
    assert_eq!(previews(&store, B)[0].state, ReplyState::Deleted);
    assert!(store.missing_reply_sources(CH).is_empty());
}

#[test]
fn a_failed_fetch_says_so_and_is_retried_after_a_reconnect() {
    let mut store = store_with(json!([
        { "_id": B, "channel": CH, "author": "u-me", "content": "re", "replies": [OLD] },
    ]));
    store.reply_source_unavailable(OLD);
    assert_eq!(previews(&store, B)[0].state, ReplyState::Unavailable);
    assert!(store.missing_reply_sources(CH).is_empty());

    store.clear_histories();
    store.set_history(
        CH,
        serde_json::from_value(json!([
            { "_id": B, "channel": CH, "author": "u-me", "content": "re", "replies": [OLD] },
        ]))
        .unwrap(),
        50,
    );
    assert_eq!(store.missing_reply_sources(CH), vec![OLD.to_string()]);
}

#[test]
fn the_mention_flag_comes_from_the_replys_mentions() {
    let store = store_with(json!([
        { "_id": A, "channel": CH, "author": "u-ada", "content": "hi" },
        { "_id": B, "channel": CH, "author": "u-me", "content": "pinging",
          "replies": [A], "mentions": ["u-ada"] },
    ]));
    assert!(previews(&store, B)[0].mentions_author);
}

#[test]
fn attachment_only_and_system_originals_get_a_label() {
    let file = |content_type: &str| {
        json!({
            "_id": "f1", "tag": "attachments", "filename": "x", "content_type": content_type,
            "size": 10, "metadata": { "type": "File" }
        })
    };
    let store = store_with(json!([
        { "_id": OLD, "channel": CH, "author": "u-ada", "attachments": [file("image/png")] },
        { "_id": A, "channel": CH, "author": "u-ada", "attachments": [file("application/pdf")] },
        { "_id": B, "channel": CH, "author": "u-me", "content": "nice", "replies": [OLD, A] },
    ]));
    let snippets: Vec<String> = previews(&store, B).into_iter().map(|p| p.snippet).collect();
    assert_eq!(snippets, vec!["[image]", "[file]"]);
}

#[test]
fn at_most_five_previews_and_replies_start_a_group() {
    let ids: Vec<String> = (0..7).map(|n| format!("01M6GD000000000000000000{n:02}")).collect();
    let store = store_with(json!([
        { "_id": A, "channel": CH, "author": "u-me", "content": "first" },
        { "_id": B, "channel": CH, "author": "u-me", "content": "many", "replies": ids },
    ]));
    let row = rows(&store).into_iter().find(|r| r.id == B).unwrap();
    assert_eq!(row.replies.len(), 5);
    // Same author, a moment later, but a reply: it gets its own header.
    assert!(!row.continuation);
}

struct NoNames;

impl Names for NoNames {
    fn user_name(&self, id: &str) -> Option<String> {
        (id == "01ARYZ6S41TSV4RRFFQ69G5FAV").then(|| "Ada".into())
    }
    fn channel_name(&self, _: &str) -> Option<String> {
        None
    }
}

fn snip(text: &str) -> String {
    snippet(text, &NoNames, 100)
}

#[test]
fn snippets_are_one_plain_line() {
    assert_eq!(snip("\n\nfirst line\nsecond"), "first line");
    assert_eq!(snip("**bold** and *it* and __under__"), "bold and it and under");
    assert_eq!(snip("~~gone~~ ||secret|| `code`"), "gone secret code");
    assert_eq!(snip("see [the docs](https://example.com) now"), "see the docs now");
    assert_eq!(snip("# Heading"), "Heading");
    assert_eq!(snip("> quoted"), "quoted");
    assert_eq!(snip("- item"), "item");
    assert_eq!(snip("12. step"), "step");
    assert_eq!(snip("```rust\nlet x = 1;\n```"), "let x = 1;");
    assert_eq!(snip("hi <@01ARYZ6S41TSV4RRFFQ69G5FAV>!"), "hi @Ada!");
    // A lone underscore stays (snake_case), and so does a #hashtag.
    assert_eq!(snip("call some_fn #tag"), "call some_fn #tag");
    assert_eq!(snip("a    lot   of\tspace"), "a lot of space");
}

#[test]
fn snippets_are_cut_on_character_boundaries() {
    let long = "é".repeat(150);
    let cut = snippet(&long, &NoNames, 100);
    assert_eq!(cut.chars().count(), 101);
    assert!(cut.ends_with('…'));
    // Emoji are several bytes; cutting mustn't split one.
    let emoji = "🦦".repeat(5);
    assert_eq!(snippet(&emoji, &NoNames, 3), "🦦🦦🦦…");
    assert_eq!(snippet("short", &NoNames, 3), "sho…");
    assert_eq!(snippet("abc", &NoNames, 3), "abc");
}

#[test]
fn a_live_edit_of_a_cached_original_updates_its_preview() {
    let mut store = store_with(json!([
        { "_id": B, "channel": CH, "author": "u-me", "content": "re", "replies": [OLD] },
    ]));
    store.add_reply_sources(
        around(
            json!([{ "_id": OLD, "channel": CH, "author": "u-ada", "content": "typo" }]),
            json!([]),
        ),
        &[OLD.to_string()],
    );
    let data = serde_json::from_value(json!({ "content": "fixed" })).unwrap();
    assert!(store.apply_update(CH, OLD, data, &[]));
    assert_eq!(previews(&store, B)[0].snippet, "fixed");
}

#[test]
fn only_loaded_originals_by_people_have_an_author_to_open() {
    let store = store_with(json!([
        { "_id": A, "channel": CH, "author": "01JCWEBHOOK000000000000000", "content": "hook",
          "webhook": { "name": "GitHub" } },
        { "_id": B, "channel": CH, "author": "u-me", "content": "re", "replies": [A, OLD] },
    ]));
    let previews = previews(&store, B);
    assert_eq!(previews[0].state, ReplyState::Loaded);
    assert_eq!(previews[0].author_id, None);
    assert_eq!(previews[1].state, ReplyState::Loading);
    assert_eq!(previews[1].author_id, None);
    // The reply row's own author is set.
    let row = rows(&store).into_iter().find(|r| r.id == B).unwrap();
    assert_eq!(row.author_id.as_deref(), Some("u-me"));
}
