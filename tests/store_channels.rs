//! Channels coming and going in the store: a deleted channel leaves its
//! server's list, its category and the sidebar, with everything loaded for
//! it. Built on the `Ready` fixture.

use serde_json::json;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::store::ChatStore;

fn store() -> ChatStore {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let Some(ServerEvent::Ready(ready)) = parse_frame(&text).pop() else {
        panic!("Ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

/// The sidebar of `srv-alpha`: each section's category ID and channel IDs.
fn sidebar(store: &ChatStore) -> Vec<(Option<String>, Vec<String>)> {
    store
        .server_channels("srv-alpha")
        .into_iter()
        .map(|s| (s.id, s.channels.into_iter().map(|c| c.id).collect()))
        .collect()
}

fn category_channels(store: &ChatStore, category: &str) -> Option<Vec<String>> {
    let server = store.server("srv-alpha").unwrap();
    server
        .categories
        .iter()
        .flatten()
        .find(|c| c.id == category)
        .map(|c| c.channels.clone())
}

#[test]
fn a_deleted_channel_leaves_its_server_category_and_history() {
    let mut store = store();
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": "01M6GDKW80AAAAAAAAAAAAAAAA", "channel": "ch-general", "author": "u-ada", "content": "hi" }
    ]))
    .unwrap();
    store.set_history("ch-general", page, 50);
    assert!(store.history("ch-general").is_some());

    assert!(store.remove_channel("ch-general"));

    assert!(store.channel("ch-general").is_none());
    assert!(store.history("ch-general").is_none());
    let server = store.server("srv-alpha").unwrap();
    assert!(!server.channels.iter().any(|c| c == "ch-general"));
    // The category stays, without it (and still with the stale ID Stoat
    // left there from before).
    assert_eq!(
        category_channels(&store, "cat-chat").unwrap(),
        ["ch-random", "ch-secret"]
    );
    assert_eq!(
        sidebar(&store),
        [
            (None, vec!["ch-welcome".to_string()]),
            (Some("cat-chat".into()), vec!["ch-random".into()]),
            (Some("cat-voice".into()), vec!["ch-voice".into()]),
        ]
    );
    assert!(!store.shows_channel("ch-general"));
}

#[test]
fn removing_a_channel_twice_does_nothing_the_second_time() {
    let mut store = store();
    assert!(store.remove_channel("ch-random"));
    let before = sidebar(&store);
    assert!(!store.remove_channel("ch-random"));
    assert_eq!(sidebar(&store), before);
    assert!(!store.remove_channel("ch-never-was"));
}

#[test]
fn other_servers_and_channels_are_untouched() {
    let mut store = store();
    assert!(store.remove_channel("ch-voice"));
    assert_eq!(store.server("srv-beta").unwrap().channels, ["ch-beta"]);
    assert!(store.channel("ch-beta").is_some());
    assert_eq!(
        category_channels(&store, "cat-voice").unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        category_channels(&store, "cat-chat").unwrap(),
        ["ch-general", "ch-random", "ch-secret"]
    );
}

#[test]
fn any_kind_of_channel_can_be_removed() {
    let mut store = store();
    assert!(store.remove_channel("dm-ada"));
    assert!(store.channel("dm-ada").is_none());
    assert!(store.remove_channel("grp-weekend"));
    assert!(store.channel("grp-weekend").is_none());
}
