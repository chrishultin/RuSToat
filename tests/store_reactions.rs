//! Reactions: decoding the three live events (with their `channel_id` field
//! name), building chips, and keeping them up to date.

use serde_json::json;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::{ChatStore, ReactionChip, ReactionEmoji, RowOp, TimelineRow, diff_rows};

const MSG: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";
const CH: &str = "ch-react";
const PARTY: &str = "01ARYZ6S41TSV4RRFFQ69G5FAV";

/// A channel with one message by Ada, with `reactions` already on it.
fn store_with(reactions: serde_json::Value) -> ChatStore {
    let mut store = ChatStore::new("u-me");
    store.set_media(MediaUrls::new(
        "https://api.example",
        Some("https://cdn.example".into()),
        None,
    ));
    let page: BulkMessageResponse = serde_json::from_value(json!([{
        "_id": MSG, "channel": CH, "author": "u-ada", "content": "hi",
        "reactions": reactions,
    }]))
    .unwrap();
    store.set_history(CH, page, 50);
    store
}

fn chips(store: &ChatStore) -> Vec<ReactionChip> {
    store
        .timeline(CH)
        .into_iter()
        .find_map(|row| match row {
            TimelineRow::Message(m) => Some(m.reactions),
            TimelineRow::StartOfChannel { .. } => None,
        })
        .unwrap()
}

/// (emoji key, count, mine) for each chip.
fn summary(store: &ChatStore) -> Vec<(String, usize, bool)> {
    chips(store)
        .into_iter()
        .map(|c| (c.key, c.count, c.mine))
        .collect()
}

fn s(v: &str) -> String {
    v.to_string()
}

#[test]
fn reaction_events_decode_with_their_field_names() {
    let events = parse_frame(
        &json!({ "type": "Bulk", "v": [
            { "type": "MessageReact", "id": MSG, "channel_id": CH,
              "user_id": "u-bob", "emoji_id": "👍" },
            { "type": "MessageUnreact", "id": MSG, "channel_id": CH,
              "user_id": "u-bob", "emoji_id": PARTY },
            { "type": "MessageRemoveReaction", "id": MSG, "channel_id": CH,
              "emoji_id": "👍" },
            // `channel` (as other message events use) isn't what these send.
            { "type": "MessageReact", "id": MSG, "channel": CH,
              "user_id": "u-bob", "emoji_id": "👍" }
        ]})
        .to_string(),
    );
    assert!(matches!(
        &events[0],
        ServerEvent::MessageReact { id, channel, user, emoji }
            if id == MSG && channel == CH && user == "u-bob" && emoji == "👍"
    ));
    assert!(matches!(
        &events[1],
        ServerEvent::MessageUnreact { channel, emoji, .. } if channel == CH && emoji == PARTY
    ));
    assert!(matches!(
        &events[2],
        ServerEvent::MessageRemoveReaction { id, channel, emoji }
            if id == MSG && channel == CH && emoji == "👍"
    ));
    assert!(matches!(&events[3], ServerEvent::Unknown(_)));
}

#[test]
fn chips_follow_the_reaction_order_with_counts_and_mine() {
    let store = store_with(json!({
        "😂": ["u-ada"],
        PARTY: ["u-ada", "u-me"],
        "👍": ["u-bob", "u-ada", "u-carol"]
    }));
    assert_eq!(
        summary(&store),
        vec![
            (s("😂"), 1, false),
            (s(PARTY), 2, true),
            (s("👍"), 3, false)
        ]
    );
    let chips = chips(&store);
    assert_eq!(chips[0].emoji, ReactionEmoji::Unicode(s("😂")));
    // Custom emoji (told apart by being a ULID) load from the emoji CDN; this
    // one isn't in the store, so its name is generic.
    assert_eq!(
        chips[1].emoji,
        ReactionEmoji::Custom {
            id: s(PARTY),
            url: Some(format!("https://cdn.example/emojis/{PARTY}")),
            name: s(":emoji:"),
        }
    );
}

#[test]
fn reacting_counts_up_and_unreacting_to_zero_removes_the_chip() {
    let mut store = store_with(json!({}));
    assert!(summary(&store).is_empty());

    assert!(store.apply_react(CH, MSG, "u-bob", "👍", true));
    assert_eq!(summary(&store), vec![(s("👍"), 1, false)]);
    // A second person: the same chip, count 2.
    assert!(store.apply_react(CH, MSG, "u-carol", "👍", true));
    assert_eq!(summary(&store), vec![(s("👍"), 2, false)]);
    // Our own reaction marks it mine.
    assert!(store.apply_react(CH, MSG, "u-me", "👍", true));
    assert_eq!(summary(&store), vec![(s("👍"), 3, true)]);

    assert!(store.apply_react(CH, MSG, "u-me", "👍", false));
    assert!(store.apply_react(CH, MSG, "u-bob", "👍", false));
    assert!(store.apply_react(CH, MSG, "u-carol", "👍", false));
    // Gone, not "0".
    assert!(summary(&store).is_empty());
    assert!(store.history(CH).unwrap().messages[0].reactions.is_empty());
}

#[test]
fn repeats_and_unknown_targets_change_nothing() {
    let mut store = store_with(json!({ "👍": ["u-me"] }));
    // Our own reaction echoed back after we've already applied it.
    assert!(!store.apply_react(CH, MSG, "u-me", "👍", true));
    assert_eq!(summary(&store), vec![(s("👍"), 1, true)]);
    // Taking back a reaction that isn't there.
    assert!(!store.apply_react(CH, MSG, "u-bob", "👍", false));
    assert!(!store.apply_react(CH, MSG, "u-bob", "🎉", false));
    // Messages that aren't loaded.
    assert!(!store.apply_react(CH, "01M6GZZZZZZZZZZZZZZZZZZZZZ", "u-bob", "👍", true));
    assert!(!store.apply_react("ch-other", MSG, "u-bob", "👍", true));
    assert!(!store.apply_remove_reaction("ch-other", MSG, "👍"));
}

#[test]
fn remove_reaction_clears_every_user_of_one_emoji() {
    let mut store = store_with(json!({
        "👍": ["u-me", "u-bob"],
        "🎉": ["u-bob"]
    }));
    assert!(store.apply_remove_reaction(CH, MSG, "👍"));
    assert_eq!(summary(&store), vec![(s("🎉"), 1, false)]);
    assert!(!store.apply_remove_reaction(CH, MSG, "👍"));
}

#[test]
fn removing_one_reaction_keeps_the_others_in_order() {
    let mut store = store_with(json!({ "a": ["u-bob"], "b": ["u-bob"], "c": ["u-bob"] }));
    assert!(store.apply_react(CH, MSG, "u-bob", "b", false));
    // A reaction added later goes at the end.
    assert!(store.apply_react(CH, MSG, "u-bob", "b", true));
    let keys: Vec<String> = summary(&store).into_iter().map(|(k, ..)| k).collect();
    assert_eq!(keys, vec![s("a"), s("c"), s("b")]);
}

#[test]
fn custom_emoji_chips_use_the_known_name() {
    let mut store = store_with(json!({ PARTY: ["u-bob"] }));
    let Some(ServerEvent::EmojiCreate(emoji)) = parse_frame(
        &json!({ "type": "EmojiCreate", "_id": PARTY, "parent": { "type": "Server", "id": "s" },
                 "creator_id": "u", "name": "party" })
        .to_string(),
    )
    .pop() else {
        panic!();
    };
    store.add_emoji(*emoji);
    assert!(matches!(
        &chips(&store)[0].emoji,
        ReactionEmoji::Custom { name, .. } if name == ":party:"
    ));
}

#[test]
fn a_live_reaction_remeasures_just_its_row() {
    let mut store = store_with(json!({}));
    let before = store.timeline(CH);
    let Some(ServerEvent::MessageReact {
        id,
        channel,
        user,
        emoji,
    }) = parse_frame(
        &json!({ "type": "MessageReact", "id": MSG, "channel_id": CH,
                 "user_id": "u-bob", "emoji_id": "👍" })
        .to_string(),
    )
    .pop()
    else {
        panic!();
    };
    assert!(store.apply_react(&channel, &id, &user, &emoji, true));
    let after = store.timeline(CH);
    // Row 0 is the start-of-channel marker.
    assert_eq!(diff_rows(&before, &after), vec![RowOp::Remeasure { at: 1 }]);
}
