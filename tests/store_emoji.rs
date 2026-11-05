//! Server emoji: loaded from Ready, kept live, used by message rendering.

use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::markdown::Names;
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::{ChatStore, TimelineRow};

const STOAT: &str = "01ARYZ6S41TSV4RRFFQ69G5FAV";
const NEW: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

fn emoji(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "_id": id, "parent": { "type": "Server", "id": "srv-alpha" },
        "creator_id": "u-ada", "name": name
    })
}

fn store() -> ChatStore {
    let ready = serde_json::json!({
        "type": "Ready", "users": [], "servers": [], "channels": [], "members": [],
        "emojis": [emoji(STOAT, "stoat"), { "_id": "broken" }]
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    assert_eq!(ready.emojis.len(), 1);
    assert_eq!(ready.skipped, 1, "the broken emoji is skipped, not fatal");
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store.set_media(MediaUrls::new(
        "https://api.example",
        Some("https://cdn.example".into()),
        None,
    ));
    store
}

#[test]
fn ready_emoji_are_named_and_located() {
    let store = store();
    assert_eq!(store.emoji_name(STOAT).as_deref(), Some("stoat"));
    assert_eq!(
        store.emoji_url(STOAT).as_deref(),
        Some("https://cdn.example/emojis/01ARYZ6S41TSV4RRFFQ69G5FAV")
    );
    assert_eq!(store.emoji_name(NEW), None);
}

#[test]
fn emoji_events_add_and_remove() {
    let mut store = store();
    let frame = serde_json::json!({ "type": "Bulk", "v": [
        { "type": "EmojiCreate", "_id": NEW, "parent": { "type": "Server", "id": "srv-alpha" },
          "creator_id": "u-ada", "name": "otter" },
        { "type": "EmojiDelete", "id": STOAT }
    ]});
    for event in parse_frame(&frame.to_string()) {
        match event {
            ServerEvent::EmojiCreate(emoji) => store.add_emoji(*emoji),
            ServerEvent::EmojiDelete { id } => store.remove_emoji(&id),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(store.emoji_name(NEW).as_deref(), Some("otter"));
    assert_eq!(store.emoji_name(STOAT), None);
}

#[test]
fn emoji_only_messages_are_flagged_jumbo() {
    let mut store = store();
    let page: BulkMessageResponse = serde_json::from_value(serde_json::json!([
        { "_id": "01M6GDKW80AAAAAAAAAAAAAAAA", "channel": "c", "author": "u", "content": format!(":{STOAT}:") },
        { "_id": "01M6GDKW80BBBBBBBBBBBBBBBB", "channel": "c", "author": "u", "content": format!("look :{STOAT}:") }
    ]))
    .unwrap();
    store.set_history("c", page, 50);
    let flags: Vec<(bool, bool)> = store
        .timeline("c")
        .into_iter()
        .filter_map(|row| match row {
            TimelineRow::Message(m) => Some((m.jumbo, m.body.unwrap().contains("width=\"96\""))),
            _ => None,
        })
        .collect();
    // (jumbo, 96px image): only the emoji-only message is both.
    assert_eq!(flags, vec![(true, true), (false, false)]);
}
