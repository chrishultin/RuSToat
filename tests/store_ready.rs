//! `Ready` decoding and the sidebar rules built on it, using an anonymized
//! fixture with the awkward cases in it: unparseable items, channels we can't
//! see, an empty category, a closed DM, an unknown DM partner, and a voice channel.

use stoat_gpui::api::events::{Ready, ServerEvent, parse_frame};
use stoat_gpui::store::{ChannelEntry, ChannelKind, ChatStore, ServerEntry, fold_sections};

fn ready() -> Ready {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut events = parse_frame(&text);
    assert_eq!(events.len(), 1);
    match events.remove(0) {
        ServerEvent::Ready(ready) => *ready,
        other => panic!("expected Ready, got {other:?}"),
    }
}

fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    store.apply_ready(ready());
    store
}

fn alpha_icon() -> stoat_gpui::api::models::File {
    serde_json::from_value(serde_json::json!({
        "_id": "file-alpha-icon", "tag": "icons", "filename": "alpha.png",
        "metadata": { "type": "Image", "width": 128, "height": 128 },
        "content_type": "image/png", "size": 2048
    }))
    .unwrap()
}

fn names(entries: &[ChannelEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn ready_parses_leniently_and_counts_what_it_skipped() {
    let ready = ready();
    // `u-broken` (missing required fields) and `FancyNewChannel` are dropped.
    assert_eq!(ready.skipped, 2);
    assert_eq!(ready.users.len(), 3);
    assert_eq!(ready.servers.len(), 2);
    assert_eq!(ready.channels.len(), 11);
    assert_eq!(ready.members.len(), 2);
    assert_eq!(ready.server_order, vec!["srv-beta", "srv-alpha"]);
}

#[test]
fn servers_follow_the_saved_ordering() {
    assert_eq!(
        store().servers(),
        vec![
            ServerEntry {
                id: "srv-beta".into(),
                name: "beta lounge".into(),
                initials: "BL".into(),
                icon: None,
                // Its only channel has no messages.
                unread: false,
                mentions: 0,
            },
            ServerEntry {
                id: "srv-alpha".into(),
                name: "Alpha Team".into(),
                initials: "AT".into(),
                icon: Some(alpha_icon()),
                // #general has messages and the fixture has no read positions.
                unread: true,
                mentions: 0,
            },
        ]
    );
}

#[test]
fn servers_without_saved_ordering_keep_ready_order() {
    let mut ready = ready();
    ready.server_order.clear();
    let mut store = ChatStore::new("u-me");
    store.apply_ready(ready);
    let ids: Vec<String> = store.servers().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec!["srv-alpha", "srv-beta"]);
}

#[test]
fn server_channels_are_grouped_uncategorized_first() {
    let sections = store().server_channels("srv-alpha");
    let shape: Vec<(Option<&str>, Option<&str>, Vec<&str>)> = sections
        .iter()
        .map(|s| (s.title.as_deref(), s.id.as_deref(), names(&s.channels)))
        .collect();
    assert_eq!(
        shape,
        vec![
            // `ch-no-access` is listed by the server but absent from Ready: hidden.
            (None, None, vec!["welcome"]),
            // `ch-secret` is likewise hidden; the category itself stays.
            (Some("Chat"), Some("cat-chat"), vec!["general", "random"]),
            (Some("Voice"), Some("cat-voice"), vec!["hangout"]),
            // "Archive" only references a missing channel, so it's omitted.
        ]
    );
}

#[test]
fn folded_categories_keep_the_open_channel_and_mentions() {
    let mut sections = store().server_channels("srv-alpha");
    // A mention in #random, which the fixture has none of.
    sections[1].channels[1].mentions = 1;
    let shape =
        |sections: &[stoat_gpui::store::ChannelSection]| -> Vec<(Option<String>, Vec<String>)> {
            sections
                .iter()
                .map(|s| {
                    (
                        s.title.clone(),
                        names(&s.channels).into_iter().map(String::from).collect(),
                    )
                })
                .collect()
        };
    let folded = |ids: &[&str], open: Option<&str>| {
        let collapsed = ids.iter().map(|id| id.to_string()).collect();
        shape(&fold_sections(sections.clone(), &collapsed, open))
    };
    let s = |title: Option<&str>, names: &[&str]| {
        (
            title.map(String::from),
            names.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
        )
    };
    // Nothing folded: everything shows.
    assert_eq!(folded(&[], None), shape(&sections));
    // Chat folded with #general open: #general (open) and #random (mention).
    assert_eq!(
        folded(&["cat-chat"], Some("ch-general")),
        [
            s(None, &["welcome"]),
            s(Some("Chat"), &["general", "random"]),
            s(Some("Voice"), &["hangout"])
        ]
    );
    // Something else open: only the mention. Voice folded empties, but its
    // heading stays to unfold it. Unknown IDs do nothing.
    assert_eq!(
        folded(&["cat-chat", "cat-voice", "cat-gone"], Some("ch-welcome")),
        [
            s(None, &["welcome"]),
            s(Some("Chat"), &["random"]),
            s(Some("Voice"), &[])
        ]
    );
}

#[test]
fn categories_are_known_by_id() {
    let store = store();
    assert!(store.has_category("cat-chat"));
    // Even one with no channels we can see.
    assert!(store.has_category("cat-archive"));
    assert!(!store.has_category("cat-deleted"));
}

#[test]
fn voice_capable_channels_are_listed_as_text_with_voice() {
    let sections = store().server_channels("srv-alpha");
    let hangout = sections
        .iter()
        .flat_map(|s| &s.channels)
        .find(|c| c.id == "ch-voice")
        .unwrap();
    assert_eq!(hangout.kind, ChannelKind::Text { voice: true });
}

#[test]
fn home_lists_saved_notes_then_conversations_newest_first() {
    let home = store().home_channels();
    assert_eq!(
        names(&home),
        vec![
            "Saved Notes",
            "bob",           // last message …07
            "Weekend plans", // …06
            "Ada L.",        // …05, named by display name
            "Unknown user",  // no messages; partner not in Ready
        ]
    );
    // The closed DM is hidden.
    assert!(home.iter().all(|e| e.id != "dm-closed"));
    assert_eq!(home[0].kind, ChannelKind::SavedMessages);
    assert_eq!(home[2].kind, ChannelKind::Group);
}

#[test]
fn own_membership_is_available_for_nicknames() {
    let store = store();
    let member = store.member("srv-alpha", "u-me").unwrap();
    assert_eq!(member.nickname.as_deref(), Some("Captain Me"));
    assert!(store.member("srv-beta", "u-me").unwrap().nickname.is_none());
}

#[test]
fn a_new_ready_replaces_the_old_state() {
    let mut store = store();
    let mut smaller = ready();
    smaller.servers.retain(|s| s.id == "srv-beta");
    smaller.server_order.clear();
    store.apply_ready(smaller);
    let ids: Vec<String> = store.servers().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec!["srv-beta"]);
    assert!(store.server_channels("srv-alpha").is_empty());
}
