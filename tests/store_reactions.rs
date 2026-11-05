//! Reactions: decoding the three live events (with their `channel_id` field
//! name), building chips, and keeping them up to date.

use serde_json::json;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::{
    COMMON_REACTIONS, ChatStore, MAX_REACTIONS, ReactionChip, ReactionEmoji, RowOp, TimelineRow,
    diff_rows,
};

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

// --- Reacting from this client -------------------------------------------

#[test]
fn an_optimistic_reaction_survives_its_echo_and_rolls_back_cleanly() {
    let mut store = store_with(json!({ "👍": ["u-bob"] }));
    assert!(!store.has_reacted(CH, MSG, "👍"));

    // Click: shown at once.
    assert!(store.apply_react(CH, MSG, "u-me", "👍", true));
    assert!(store.has_reacted(CH, MSG, "👍"));
    assert_eq!(summary(&store), vec![(s("👍"), 2, true)]);

    // The server's echo of our own reaction: still 2, not 3.
    let Some(ServerEvent::MessageReact {
        id,
        channel,
        user,
        emoji,
    }) = parse_frame(
        &json!({ "type": "MessageReact", "id": MSG, "channel_id": CH,
                 "user_id": "u-me", "emoji_id": "👍" })
        .to_string(),
    )
    .pop()
    else {
        panic!();
    };
    assert!(!store.apply_react(&channel, &id, &user, &emoji, true));
    assert_eq!(summary(&store), vec![(s("👍"), 2, true)]);

    // A refused reaction is undone, leaving everyone else's.
    assert!(store.apply_react(CH, MSG, "u-me", "🎉", true));
    assert!(store.apply_react(CH, MSG, "u-me", "🎉", false));
    assert_eq!(summary(&store), vec![(s("👍"), 2, true)]);
    // Same for a refused removal.
    assert!(store.apply_react(CH, MSG, "u-me", "👍", false));
    assert!(store.apply_react(CH, MSG, "u-me", "👍", true));
    assert_eq!(summary(&store), vec![(s("👍"), 2, true)]);
}

/// The Ready fixture (srv-alpha with ch-general, srv-beta, DMs), with one
/// custom emoji in each server, plus one detached.
fn store_with_emoji() -> ChatStore {
    let ready = std::fs::read_to_string(format!(
        "{}/tests/fixtures/ready.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready).pop() else {
        panic!();
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store.set_media(MediaUrls::new(
        "https://api.example",
        Some("https://cdn.example".into()),
        None,
    ));
    for (id, server, name) in [
        ("01ARYZ6S41TSV4RRFFQ69G5FA1", Some("srv-alpha"), "zebra"),
        ("01ARYZ6S41TSV4RRFFQ69G5FA2", Some("srv-alpha"), "aardvark"),
        ("01ARYZ6S41TSV4RRFFQ69G5FA3", Some("srv-beta"), "beta"),
        ("01ARYZ6S41TSV4RRFFQ69G5FA4", None, "detached"),
    ] {
        let parent = match server {
            Some(id) => json!({ "type": "Server", "id": id }),
            None => json!({ "type": "Detached" }),
        };
        let Some(ServerEvent::EmojiCreate(emoji)) = parse_frame(
            &json!({ "type": "EmojiCreate", "_id": id, "parent": parent,
                     "creator_id": "u", "name": name })
            .to_string(),
        )
        .pop() else {
            panic!();
        };
        store.add_emoji(*emoji);
    }
    store
}

#[test]
fn the_picker_offers_common_emoji_then_the_servers_own_by_name() {
    let store = store_with_emoji();
    let choices = store.reaction_choices("ch-general");
    let keys: Vec<&str> = choices.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys.len(), COMMON_REACTIONS.len() + 2);
    assert_eq!(&keys[..COMMON_REACTIONS.len()], &COMMON_REACTIONS[..]);
    // srv-alpha's two, by name; not srv-beta's or the detached one.
    assert_eq!(
        &keys[COMMON_REACTIONS.len()..],
        &["01ARYZ6S41TSV4RRFFQ69G5FA2", "01ARYZ6S41TSV4RRFFQ69G5FA1"]
    );
    assert_eq!(
        choices.last().unwrap().emoji,
        ReactionEmoji::Custom {
            id: s("01ARYZ6S41TSV4RRFFQ69G5FA1"),
            url: Some(s("https://cdn.example/emojis/01ARYZ6S41TSV4RRFFQ69G5FA1")),
            name: s(":zebra:"),
        }
    );
    // In a DM: every server's emoji (still not detached ones).
    let dm: Vec<String> = store
        .reaction_choices("dm-ada")
        .into_iter()
        .skip(COMMON_REACTIONS.len())
        .map(|c| c.key)
        .collect();
    assert_eq!(
        dm,
        vec![
            s("01ARYZ6S41TSV4RRFFQ69G5FA2"),
            s("01ARYZ6S41TSV4RRFFQ69G5FA3"),
            s("01ARYZ6S41TSV4RRFFQ69G5FA1")
        ]
    );
}

#[test]
fn restricted_messages_limit_the_picker() {
    let mut store = ChatStore::new("u-me");
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": MSG, "channel": CH, "author": "u-bot", "content": "vote",
          "interactions": { "reactions": ["✅", PARTY], "restrict_reactions": true } },
        // A preset list without the restriction is only a suggestion.
        { "_id": "01M6GDKW90BBBBBBBBBBBBBBBB", "channel": CH, "author": "u-bot",
          "content": "free", "interactions": { "reactions": ["✅"] } }
    ]))
    .unwrap();
    store.set_history(CH, page, 50);
    let rows: Vec<_> = store
        .timeline(CH)
        .into_iter()
        .filter_map(|row| match row {
            TimelineRow::Message(m) => Some(m),
            TimelineRow::StartOfChannel { .. } => None,
        })
        .collect();
    let allowed: Vec<String> = rows[0]
        .allowed_reactions
        .clone()
        .unwrap()
        .into_iter()
        .map(|c| c.key)
        .collect();
    assert_eq!(allowed, vec![s("✅"), s(PARTY)]);
    assert_eq!(rows[1].allowed_reactions, None);
}

#[test]
fn a_full_message_only_offers_its_existing_reactions() {
    // 20 different emoji: Stoat's limit.
    let keys: Vec<String> = COMMON_REACTIONS[..MAX_REACTIONS]
        .iter()
        .map(|k| k.to_string())
        .collect();
    let reactions: serde_json::Map<String, serde_json::Value> =
        keys.iter().map(|k| (k.clone(), json!(["u-bob"]))).collect();
    let mut store = store_with(json!(reactions));

    // A new emoji would be the 21st: refused before sending.
    assert!(store.would_exceed_reactions(CH, MSG, "🦆"));
    // Joining an existing one is fine.
    assert!(!store.would_exceed_reactions(CH, MSG, &keys[0]));

    let row = store
        .timeline(CH)
        .into_iter()
        .find_map(|row| match row {
            TimelineRow::Message(m) => Some(m),
            TimelineRow::StartOfChannel { .. } => None,
        })
        .unwrap();
    assert!(row.reactions_full());
    let usual = store.reaction_choices(CH);
    let choices = row.picker_choices(&usual);
    assert!(choices.full);
    let offered: Vec<String> = choices.emoji.into_iter().map(|c| c.key).collect();
    assert_eq!(offered, keys);

    // One fewer and the usual picker is back.
    assert!(store.apply_remove_reaction(CH, MSG, &keys[19]));
    assert!(!store.would_exceed_reactions(CH, MSG, "🦆"));
    let row = store
        .timeline(CH)
        .into_iter()
        .find_map(|row| match row {
            TimelineRow::Message(m) => Some(m),
            TimelineRow::StartOfChannel { .. } => None,
        })
        .unwrap();
    let choices = row.picker_choices(&usual);
    assert!(!choices.full);
    assert_eq!(choices.emoji.len(), usual.len());
}
