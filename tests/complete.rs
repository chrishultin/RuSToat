//! Autocomplete in the message box: when `@`, `#` and `:` open
//! suggestions, how people rank, mentions found in wire text (for chips),
//! and who can be mentioned where.

use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::complete::{
    Candidate, Insertion, MAX_SUGGESTIONS, Query, QueryKind, Suggestion, TokenRef,
    completion_query, mention_spans, rank_people,
};
use stoat_gpui::store::ChatStore;

/// What's being completed with the cursor at the end of `text`.
fn query(text: &str) -> Option<(QueryKind, String)> {
    completion_query(text, text.len()).map(|q| (q.kind, q.query))
}

fn mention(q: &str) -> Option<(QueryKind, String)> {
    Some((QueryKind::Mention, q.into()))
}

#[test]
fn at_opens_at_the_start_after_a_space_or_opening_punctuation() {
    assert_eq!(query("@"), mention(""));
    assert_eq!(query("@ad"), mention("ad"));
    assert_eq!(query("hi @ad"), mention("ad"));
    assert_eq!(query("hi\n@ad"), mention("ad"));
    assert_eq!(query("(@ad"), mention("ad"));
    assert_eq!(query("\"@ad"), mention("ad"));
    assert_eq!(query("hi @Ada.L"), mention("Ada.L"));
    assert_eq!(query("#gen"), Some((QueryKind::Channel, "gen".into())));
    assert_eq!(query("see #"), Some((QueryKind::Channel, String::new())));
}

#[test]
fn the_query_range_covers_the_trigger_and_what_follows() {
    assert_eq!(
        completion_query("hi @ad", 6),
        Some(Query {
            kind: QueryKind::Mention,
            range: 3..6,
            query: "ad".into()
        })
    );
    // Multi-byte text before it: byte offsets.
    let text = "héllo @é";
    assert_eq!(completion_query(text, text.len()).unwrap().range, 7..text.len());
}

#[test]
fn at_doesnt_open_inside_words_code_or_after_the_query_ends() {
    assert_eq!(query("email@x"), None);
    assert_eq!(query("hi @ada "), None);
    assert_eq!(query("`@ad"), None);
    assert_eq!(query("`code` and `@ad"), None);
    assert_eq!(query("`code` @ad"), mention("ad"));
    assert_eq!(query("```\nlet x = @ad"), None);
    assert_eq!(query("```\ncode\n```\n@ad"), mention("ad"));
    // Right after a chip's wire text.
    assert_eq!(query("<@01M6GDKW80AAAAAAAAAAAAAAAA>"), None);
    // The cursor in the middle of a word.
    assert_eq!(completion_query("@adam", 3), None);
    assert_eq!(completion_query("@ad bob", 3), Some(Query {
        kind: QueryKind::Mention,
        range: 0..3,
        query: "ad".into()
    }));
    // Out of range, or not on a character boundary.
    assert_eq!(completion_query("@a", 9), None);
    assert_eq!(completion_query("@é", 2), None);
}

#[test]
fn colon_needs_two_shortcode_characters_and_skips_times_and_urls() {
    assert_eq!(query(":sm"), Some((QueryKind::Emoji, "sm".into())));
    assert_eq!(query("nice :+1"), Some((QueryKind::Emoji, "+1".into())));
    assert_eq!(query(":s"), None);
    assert_eq!(query(":"), None);
    assert_eq!(query("at 12:30"), None);
    assert_eq!(query("ratio 1:2"), None);
    assert_eq!(query("http://x"), None);
    assert_eq!(query(":)"), None);
    assert_eq!(query(":a.b"), None);
}

fn person(id: &str, name: &str, username: &str, recent: bool) -> Candidate {
    Candidate {
        id: id.into(),
        name: name.into(),
        display_name: None,
        username: username.into(),
        avatar: None,
        recent,
    }
}

fn ids(people: &[Candidate]) -> Vec<&str> {
    people.iter().map(|p| p.id.as_str()).collect()
}

#[test]
fn prefix_matches_rank_above_word_prefixes_above_substrings() {
    let people = [
        person("substring", "Madam", "madam", false),
        person("word", "Lady Ada", "lady", false),
        person("prefix", "Adam", "adam", false),
        person("username", "Zed", "ada_z", false),
        person("none", "Bob", "bob", false),
    ];
    assert_eq!(
        ids(&rank_people("ADA", &people)),
        vec!["prefix", "username", "word", "substring"]
    );
}

#[test]
fn recent_authors_come_first_within_a_tier_and_empty_matches_all() {
    let people = [
        person("old", "Ada Old", "ao", false),
        person("new", "Ada New", "an", true),
        person("bob", "Bob", "bob", true),
    ];
    assert_eq!(ids(&rank_people("ada", &people)), vec!["new", "old"]);
    assert_eq!(ids(&rank_people("", &people)), vec!["new", "bob", "old"]);
}

#[test]
fn nicknames_and_display_names_both_match() {
    let mut nick = person("nick", "Captain", "cap", false);
    nick.display_name = Some("Grace Hopper".into());
    assert_eq!(ids(&rank_people("cap", &[nick.clone()])), vec!["nick"]);
    assert_eq!(ids(&rank_people("hop", std::slice::from_ref(&nick))), vec!["nick"]);
}

#[test]
fn at_most_eight_suggestions() {
    let people: Vec<Candidate> = (0..20)
        .map(|n| person(&format!("u{n}"), &format!("Ada {n}"), &format!("ada{n}"), false))
        .collect();
    let ranked = rank_people("ada", &people);
    assert_eq!(ranked.len(), MAX_SUGGESTIONS);
    assert_eq!(ranked[0].id, "u0");
}

#[test]
fn a_person_suggestion_inserts_a_mention_chip() {
    let mut ada = person("01M6GDKW80AAAAAAAAAAAAAAAA", "Ada", "ada", true);
    ada.avatar = Some("https://cdn/avatars/a".into());
    let suggestion = Suggestion::person(&ada);
    assert_eq!(suggestion.title, "Ada");
    assert_eq!(suggestion.detail.as_deref(), Some("@ada"));
    assert_eq!(suggestion.image.as_deref(), Some("https://cdn/avatars/a"));
    let Insertion::Token { token, label } = suggestion.insert else {
        panic!("not a chip");
    };
    assert_eq!(label, "@Ada");
    assert_eq!(token.wire_text(), "<@01M6GDKW80AAAAAAAAAAAAAAAA>");
    assert_eq!(token.token_id(), "user:01M6GDKW80AAAAAAAAAAAAAAAA");
    assert_eq!(TokenRef::from_token_id(&token.token_id()), Some(token));
}

const ADA: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";
const GENERAL: &str = "01M6GDSC10BBBBBBBBBBBBBBBB";
const PARTY: &str = "01M6GEW5A0CCCCCCCCCCCCCCCC";

#[test]
fn mention_spans_find_people_channels_and_custom_emoji() {
    let text = format!("hi <@{ADA}>, see <#{GENERAL}> :{PARTY}: ok");
    let spans = mention_spans(&text);
    assert_eq!(
        spans.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
        vec![
            TokenRef::User(ADA.into()),
            TokenRef::Channel(GENERAL.into()),
            TokenRef::Emoji(PARTY.into()),
        ]
    );
    // Each range is exactly the wire text.
    for (range, token) in &spans {
        assert_eq!(&text[range.clone()], token.wire_text());
    }
}

#[test]
fn mention_spans_ignore_things_that_arent_ids() {
    assert!(mention_spans("<@notanid> <@ada> :smile: <#general>").is_empty());
    // Lowercase, or with I/L/O/U (not Crockford base 32), or the wrong length.
    assert!(mention_spans(&format!("<@{}>", ADA.to_lowercase())).is_empty());
    assert!(mention_spans("<@01M6GDKW80AAAAAAAAAAAAAAAI>").is_empty());
    assert!(mention_spans(&format!("<@{ADA}A>")).is_empty());
    assert!(mention_spans(&format!("<@{ADA}")).is_empty());
    // Back to back, and next to multi-byte text.
    let text = format!("é<@{ADA}><@{ADA}>é");
    assert_eq!(mention_spans(&text).len(), 2);
}

// Who can be mentioned.

/// The Ready fixture, plus `u-cy` (a member of `srv-alpha` with a
/// nickname) and `u-dee` (a user, not a member).
fn store() -> ChatStore {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let mut ready: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for (id, name) in [("u-cy", "cy"), ("u-dee", "dee")] {
        ready["users"].as_array_mut().unwrap().push(json!({
            "_id": id, "username": name, "discriminator": "0009", "relationship": "None",
            "online": false
        }));
    }
    ready["members"].as_array_mut().unwrap().push(json!({
        "_id": { "server": "srv-alpha", "user": "u-cy" },
        "joined_at": "2026-01-01T00:00:00.000Z", "nickname": "Cyclone"
    }));
    let ServerEvent::Ready(ready) = parse_frame(&ready.to_string()).remove(0) else {
        panic!("not Ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

fn with_history(store: &mut ChatStore, channel: &str, authors: &[&str]) {
    let messages: Vec<Value> = authors
        .iter()
        .enumerate()
        .map(|(n, author)| {
            json!({ "_id": format!("01JC00000000000000000000{n:02}"), "channel": channel,
                    "author": author, "content": "hi" })
        })
        .collect();
    let page: BulkMessageResponse = serde_json::from_value(json!(messages)).unwrap();
    store.set_history(channel, page, 50);
}

#[test]
fn in_a_dm_the_other_person_can_be_mentioned_but_not_me() {
    let store = store();
    let people = store.mention_candidates("dm-ada");
    assert_eq!(ids(&people), vec!["u-ada"]);
    assert_eq!(people[0].username, "ada");
    assert!(!people[0].recent);
    assert!(people[0].avatar.is_some());
}

#[test]
fn in_a_group_recent_authors_come_first() {
    let mut store = store();
    assert_eq!(ids(&store.mention_candidates("grp-weekend")), vec!["u-ada", "u-bob"]);
    with_history(&mut store, "grp-weekend", &["u-ada", "u-me", "u-bob"]);
    let people = store.mention_candidates("grp-weekend");
    // Newest author first, each once.
    assert_eq!(ids(&people), vec!["u-bob", "u-ada"]);
    assert!(people.iter().all(|p| p.recent));
}

#[test]
fn in_a_server_recent_authors_then_known_members_with_nicknames() {
    let mut store = store();
    with_history(&mut store, "ch-general", &["u-dee", "u-me"]);
    let people = store.mention_candidates("ch-general");
    // Dee wrote recently; Cy is a member; Me is never suggested.
    assert_eq!(ids(&people), vec!["u-dee", "u-cy"]);
    let cy = &people[1];
    assert_eq!(cy.name, "Cyclone");
    assert_eq!(cy.display_name.as_deref(), Some("cy"));
    assert!(!cy.recent);
    assert_eq!(ids(&rank_people("cyc", &people)), vec!["u-cy"]);
    // Saved messages: nobody.
    assert!(store.mention_candidates("ch-saved").is_empty());
}

#[test]
fn chip_labels_name_people_channels_and_emoji() {
    let store = store();
    assert_eq!(store.token_label("dm-ada", &TokenRef::User("u-ada".into())), "@Ada L.");
    assert_eq!(
        store.token_label("ch-general", &TokenRef::User("u-me".into())),
        "@Captain Me"
    );
    assert_eq!(
        store.token_label("ch-general", &TokenRef::User("u-nobody".into())),
        "@Unknown user"
    );
    assert_eq!(
        store.token_label("dm-ada", &TokenRef::Channel("ch-general".into())),
        "#general"
    );
}
