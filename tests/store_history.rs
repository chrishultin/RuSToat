//! Message history: both response shapes, display order, author names,
//! system text, mentions, grouping, and the start-of-channel marker.

use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::store::{ChatStore, MessageRow, TimelineRow};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

/// The Ready fixture's store (servers, channels, users), plus one page of history.
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

fn messages(rows: &[TimelineRow]) -> Vec<&MessageRow> {
    rows.iter()
        .filter_map(|row| match row {
            TimelineRow::Message(m) => Some(m),
            TimelineRow::StartOfChannel { .. } => None,
        })
        .collect()
}

#[test]
fn both_bulk_response_shapes_parse() {
    let page: BulkMessageResponse = serde_json::from_str(&fixture("messages_page.json")).unwrap();
    assert!(
        matches!(page, BulkMessageResponse::MessagesAndUsers { ref messages, .. } if messages.len() == 7)
    );
    let bare: BulkMessageResponse = serde_json::from_str(&fixture("messages_bare.json")).unwrap();
    assert!(matches!(bare, BulkMessageResponse::JustMessages(ref m) if m.len() == 1));
}

#[test]
fn newest_first_pages_display_oldest_first() {
    let store = store_with_page(50);
    let history = store.history("ch-general").unwrap();
    let ids: Vec<&str> = history.messages.iter().map(|m| m.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(ids.first(), Some(&"01M6GDKW80AAAAAAAAAAAAAAAA"));
}

#[test]
fn author_names_follow_precedence() {
    let rows = store_with_page(50).timeline("ch-general");
    let authors: Vec<&str> = messages(&rows).iter().map(|m| m.author.as_str()).collect();
    assert_eq!(
        authors,
        vec![
            "Ada L.", // display name (from Ready)
            "Ada L.",
            "Bobby",        // server nickname (from the page's members) beats username
            "Unknown user", // system message author (no such user)
            "Ada L.",
            "Bridge Bot", // masquerade beats the real user ("bridge")
            "GitHub",     // webhook name
        ]
    );
}

#[test]
fn mentions_and_system_messages_use_names() {
    let rows = store_with_page(50).timeline("ch-general");
    let rows = messages(&rows);
    assert_eq!(
        rows[0].body.as_deref(),
        Some("Hello @bob, see #random <not a mention>")
    );
    assert!(rows[3].system);
    assert_eq!(rows[3].body.as_deref(), Some("Carol joined"));
}

#[test]
fn attachments_and_edits_are_flagged() {
    let rows = store_with_page(50).timeline("ch-general");
    let look = messages(&rows)[4];
    assert_eq!(look.body.as_deref(), Some("look at this"));
    assert_eq!(look.attachments, vec!["cat.png"]);
    assert!(look.edited);
}

#[test]
fn consecutive_messages_from_one_author_are_grouped() {
    let rows = store_with_page(50).timeline("ch-general");
    let grouped: Vec<bool> = messages(&rows).iter().map(|m| m.continuation).collect();
    // Ada, Ada +2 min (grouped), Bob, system, Ada +16 min, bot, webhook.
    assert_eq!(
        grouped,
        vec![false, true, false, false, false, false, false]
    );
}

#[test]
fn short_page_marks_the_start_of_the_channel() {
    // 7 messages back from a request for 50: that's all of it.
    let rows = store_with_page(50).timeline("ch-general");
    assert_eq!(
        rows.first(),
        Some(&TimelineRow::StartOfChannel {
            name: "general".into()
        })
    );
    // A full page (7 of 7 requested) might have more before it.
    let rows = store_with_page(7).timeline("ch-general");
    assert!(matches!(rows.first(), Some(TimelineRow::Message(_))));
}

#[test]
fn unloaded_channels_have_no_timeline() {
    assert!(store_with_page(50).timeline("ch-random").is_empty());
}
