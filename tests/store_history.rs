//! Message history: both response shapes, display order, author names,
//! system text, mentions, grouping, and the start-of-channel marker.

use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::api::models::Message;
use stoat_gpui::attachment::{AttachmentView, FileKind};
use stoat_gpui::store::{ChatStore, MessageRow, PendingMessage, PendingState, TimelineRow};

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
            _ => None,
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
    // Mentions become internal links; the stray `<` is escaped so it isn't HTML.
    assert_eq!(
        rows[0].body.as_deref(),
        Some(
            r"Hello [@bob](stoat:user/u-bob), see [#random](stoat:channel/ch-random) \<not a mention>"
        )
    );
    assert!(rows[3].system);
    // A system message's people are links to their cards.
    assert_eq!(
        rows[3].body.as_deref(),
        Some("[Carol](stoat:user/u-carol) joined")
    );
}

#[test]
fn attachments_and_edits_are_flagged() {
    let rows = store_with_page(50).timeline("ch-general");
    let look = messages(&rows)[4];
    assert_eq!(look.body.as_deref(), Some("look at this"));
    // No file server in this store, so even the image is a card without a link.
    assert_eq!(
        look.attachments,
        vec![AttachmentView::File {
            name: "cat.png".into(),
            size_label: "12.1 KB".into(),
            url: None,
            kind: FileKind::Image,
        }]
    );
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

#[test]
fn render_key_changes_when_the_drawn_text_changes() {
    use stoat_gpui::store::render_key;
    let id = "01M6GDKW80AAAAAAAAAAAAAAAA";
    assert_eq!(render_key(id, Some("hi")), render_key(id, Some("hi")));
    assert_ne!(
        render_key(id, Some("hi")),
        render_key(id, Some("hi (edited)"))
    );
    assert_ne!(render_key(id, Some("hi")), render_key(id, None));
    assert!(render_key(id, Some("hi")).starts_with(id));
}

#[test]
fn groups_know_where_they_end() {
    let rows = store_with_page(50).timeline("ch-general");
    let shape: Vec<(bool, bool)> = messages(&rows)
        .iter()
        .map(|m| (m.continuation, m.group_end))
        .collect();
    // (continues the previous one, ends its group)
    assert_eq!(
        shape,
        vec![
            (false, false), // Ada starts a group…
            (true, true),   // …and her follow-up ends it.
            (false, true),  // Bob, alone.
            (false, true),  // system message
            (false, true),  // Ada again, 16 minutes later: a new group
            (false, true),  // bot
            (false, true),  // webhook
        ]
    );
}

#[test]
fn own_messages_are_marked_mine() {
    let mut store = store_with_page(50);
    // Same history, seen as Ada.
    let mut as_ada = ChatStore::new("u-ada");
    let Some(ServerEvent::Ready(ready)) = parse_frame(&fixture("ready.json")).pop() else {
        panic!("ready fixture");
    };
    as_ada.apply_ready(*ready);
    let page: BulkMessageResponse = serde_json::from_str(&fixture("messages_page.json")).unwrap();
    as_ada.set_history("ch-general", page, 50);

    let mine = |store: &mut ChatStore| -> Vec<bool> {
        messages(&store.timeline("ch-general"))
            .iter()
            .map(|m| m.mine)
            .collect()
    };
    assert!(mine(&mut store).iter().all(|m| !m));
    assert_eq!(
        mine(&mut as_ada),
        vec![true, true, false, false, true, false, false]
    );
}

// --- History windows (jump to message) ------------------------------------

/// The Ready fixture's store with nothing loaded. `ch-random` has no
/// messages at all as far as Ready knows.
fn empty_store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let Some(ServerEvent::Ready(ready)) = parse_frame(&fixture("ready.json")).pop() else {
        panic!("ready fixture");
    };
    store.apply_ready(*ready);
    store
}

const CH: &str = "ch-random";

/// The ID of the `n`th message of the channel: later ones sort later.
fn id(n: u32) -> String {
    format!("01M7{n:022}")
}

fn message_n(n: u32) -> Message {
    serde_json::from_value(serde_json::json!({
        "_id": id(n), "channel": CH, "author": "u-ada", "content": format!("message {n}")
    }))
    .unwrap()
}

/// Messages `from..=to`, newest first, as the server sends them.
fn page(from: u32, to: u32) -> BulkMessageResponse {
    BulkMessageResponse::JustMessages((from..=to).rev().map(message_n).collect())
}

fn loaded_ids(store: &ChatStore) -> Vec<String> {
    store
        .history(CH)
        .unwrap()
        .messages
        .iter()
        .map(|m| m.id.clone())
        .collect()
}

fn range_ids(from: u32, to: u32) -> Vec<String> {
    (from..=to).map(id).collect()
}

/// A window of 50 around message 50: 26 messages up to it (24..=49), and 26
/// from it on (50..=75).
fn windowed() -> ChatStore {
    let mut store = empty_store();
    store.set_window(CH, page(24, 75), &id(50), 50);
    store
}

#[test]
fn the_latest_page_is_at_the_present() {
    let mut store = empty_store();
    store.set_history(CH, page(1, 50), 50);
    assert!(store.history(CH).unwrap().at_present);
    assert!(store.is_at_present(CH));
    // Nothing loaded: nothing to be away from.
    assert!(store.is_at_present("ch-general"));
}

#[test]
fn a_full_window_is_neither_exhausted_nor_at_the_present() {
    let store = windowed();
    let history = store.history(CH).unwrap();
    assert_eq!(loaded_ids(&store), range_ids(24, 75));
    assert!(!history.exhausted);
    assert!(!history.at_present);
    assert!(!store.is_at_present(CH));
    assert_eq!(store.newest_message_id(CH), Some(id(75).as_str()));
}

#[test]
fn a_window_replaces_what_was_loaded() {
    let mut store = empty_store();
    store.set_history(CH, page(200, 249), 50);
    store.set_window(CH, page(24, 75), &id(50), 50);
    assert_eq!(loaded_ids(&store), range_ids(24, 75));
}

#[test]
fn a_short_older_side_is_the_start_of_the_channel() {
    let mut store = empty_store();
    // Only 10 messages (40..=49) before the target, of 26 possible.
    store.set_window(CH, page(40, 75), &id(50), 50);
    let history = store.history(CH).unwrap();
    assert!(history.exhausted);
    assert!(!history.at_present);
    assert!(matches!(
        store.timeline(CH).first(),
        Some(TimelineRow::StartOfChannel { .. })
    ));
}

#[test]
fn a_short_newer_side_is_the_present() {
    let mut store = empty_store();
    // 11 messages from the target on (50..=60), of 26 possible.
    store.set_window(CH, page(24, 60), &id(50), 50);
    let history = store.history(CH).unwrap();
    assert!(history.at_present);
    assert!(!history.exhausted);
}

#[test]
fn a_window_holding_the_last_message_is_at_the_present() {
    let mut store = empty_store();
    // A live message for a channel that isn't loaded still sets its last message.
    assert!(!store.apply_message(message_n(75)));
    // Full on both sides, but the newest message is in it.
    store.set_window(CH, page(24, 75), &id(50), 50);
    assert!(store.history(CH).unwrap().at_present);
}

#[test]
fn a_deleted_target_is_just_missing_from_its_window() {
    let mut store = empty_store();
    let mut messages: Vec<Message> = (24..=75).filter(|n| *n != 50).map(message_n).collect();
    messages.reverse();
    store.set_window(CH, BulkMessageResponse::JustMessages(messages), &id(50), 50);
    let history = store.history(CH).unwrap();
    assert!(!store.has_message(CH, &id(50)));
    // Messages before the target: 24..=49. From it on: 51..=75, one short.
    assert!(!history.exhausted);
    assert!(history.at_present);
}

#[test]
fn newer_pages_extend_a_window_until_a_short_one_reaches_the_present() {
    let mut store = windowed();
    // Messages up to 150 exist; the channel's last message is known to be 150.
    store.apply_message(message_n(150));
    assert!(!store.is_at_present(CH));

    // A full page (76..=125): more to come.
    assert!(store.append_page(CH, page(76, 125), 50));
    assert_eq!(loaded_ids(&store), range_ids(24, 125));
    assert!(!store.is_at_present(CH));

    // 126..=150 is short of 50: that's the rest.
    assert!(store.append_page(CH, page(126, 150), 50));
    assert_eq!(loaded_ids(&store), range_ids(24, 150));
    assert!(store.is_at_present(CH));
}

#[test]
fn a_full_newer_page_that_holds_the_last_message_reaches_the_present() {
    let mut store = windowed();
    store.apply_message(message_n(125));
    assert!(store.append_page(CH, page(76, 125), 50));
    assert!(store.is_at_present(CH));
}

#[test]
fn an_empty_newer_page_adds_nothing_but_ends_the_window() {
    let mut store = windowed();
    assert!(!store.append_page(CH, BulkMessageResponse::JustMessages(Vec::new()), 50));
    assert!(store.is_at_present(CH));
    assert_eq!(loaded_ids(&store), range_ids(24, 75));
}

#[test]
fn a_full_newer_page_with_nothing_new_makes_no_progress() {
    let mut store = windowed();
    store.apply_message(message_n(150));
    // Everything in the page is already loaded: nothing added, and not the
    // present, so a caller must not simply ask again.
    assert!(!store.append_page(CH, page(26, 75), 50));
    assert!(!store.is_at_present(CH));
}

#[test]
fn appending_to_an_unloaded_channel_does_nothing() {
    let mut store = empty_store();
    assert!(!store.append_page(CH, page(1, 10), 50));
    assert!(store.history(CH).is_none());
}

#[test]
fn live_messages_away_from_the_present_aren_t_inserted_but_count_as_unread() {
    let mut store = windowed();
    let mentioning: Message = serde_json::from_value(serde_json::json!({
        "_id": id(100), "channel": CH, "author": "u-ada", "content": "hi @me",
        "mentions": ["u-me"]
    }))
    .unwrap();
    // No gap in the list.
    assert!(!store.apply_message(mentioning));
    assert!(!store.apply_message(message_n(101)));
    assert_eq!(loaded_ids(&store), range_ids(24, 75));

    // But the channel is unread, with a mention, and the newest message is
    // what reading it would acknowledge.
    assert!(store.is_unread(CH));
    assert_eq!(store.mention_count(CH), 1);
    assert_eq!(store.unacked_latest(CH), Some(id(101)));
    assert!(!store.has_read(CH, &id(76)));
}

#[test]
fn messages_that_arrived_while_away_show_once_the_pages_reach_them() {
    let mut store = windowed();
    store.apply_message(message_n(76));
    store.apply_message(message_n(77));
    assert!(store.append_page(CH, page(76, 77), 50));
    assert!(store.is_at_present(CH));
    let ids = loaded_ids(&store);
    assert_eq!(ids, range_ids(24, 77), "each exactly once, in order");
    // At the present, live messages are inserted again.
    assert!(store.apply_message(message_n(78)));
    assert_eq!(loaded_ids(&store), range_ids(24, 78));
}

#[test]
fn edits_deletes_and_reactions_still_apply_inside_a_window() {
    use stoat_gpui::api::models::PartialMessage;
    let mut store = windowed();

    let data: PartialMessage =
        serde_json::from_value(serde_json::json!({ "content": "changed" })).unwrap();
    assert!(store.apply_update(CH, &id(50), data, &[]));
    assert_eq!(store.raw_content(CH, &id(50)), Some("changed"));

    assert!(store.apply_react(CH, &id(51), "u-bob", "👍", true));
    assert!(!store.has_reacted(CH, &id(51), "👍"));
    assert!(store.apply_react(CH, &id(51), "u-me", "👍", true));
    assert!(store.has_reacted(CH, &id(51), "👍"));

    assert!(store.apply_delete(CH, &[id(52)]));
    assert!(!store.has_message(CH, &id(52)));
    assert!(!store.is_at_present(CH), "still a window");
}

#[test]
fn pending_messages_wait_for_the_present() {
    let mut store = windowed();
    store.add_pending(PendingMessage {
        key: "k1".into(),
        nonce: "k1".into(),
        channel: CH.into(),
        content: "sent from afar".into(),
        replies: Vec::new(),
        attachments: Vec::new(),
        state: PendingState::Sending,
        sent_ms: 1_000,
        nonce_used: false,
    });
    // Drawn after the window's last message it would sit after a gap.
    let rows = store.timeline(CH);
    assert!(messages(&rows).iter().all(|m| m.pending.is_none()));

    // Back at the present (the latest page reloaded), it's drawn last.
    store.set_history(CH, page(100, 149), 50);
    let rows = store.timeline(CH);
    let rows = messages(&rows);
    assert_eq!(
        rows.last().and_then(|m| m.pending.clone()),
        Some(PendingState::Sending)
    );
}

#[test]
fn the_highlight_is_on_one_row_until_cleared() {
    let mut store = windowed();
    let highlighted = |store: &ChatStore| -> Vec<String> {
        messages(&store.timeline(CH))
            .iter()
            .filter(|m| m.highlighted)
            .map(|m| m.id.clone())
            .collect()
    };
    assert!(highlighted(&store).is_empty());

    store.set_highlight(&id(50));
    assert_eq!(highlighted(&store), vec![id(50)]);

    // A later jump moves it.
    store.set_highlight(&id(60));
    assert_eq!(highlighted(&store), vec![id(60)]);

    // The first jump's timer firing late doesn't clear the second's.
    assert!(!store.clear_highlight(&id(50)));
    assert_eq!(highlighted(&store), vec![id(60)]);
    assert!(store.clear_highlight(&id(60)));
    assert!(highlighted(&store).is_empty());
}
