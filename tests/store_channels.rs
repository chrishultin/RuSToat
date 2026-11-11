//! Channels coming and going in the store: a deleted channel leaves its
//! server's list, its category and the sidebar, with everything loaded for
//! it; a created one shows once, whichever of the response and its
//! `ChannelCreate` comes first, and moves with the categories; a deleted
//! category leaves its channels uncategorized. Built on the `Ready`
//! fixture.

use serde_json::json;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{BulkMessageResponse, Category, Channel};
use stoat_gpui::server_edit::without_category;
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
            // Empty: it only lists a channel deleted before.
            (Some("cat-archive".into()), vec![]),
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

/// `ch-new` in `srv-alpha`, as `POST /servers/srv-alpha/channels` answers.
fn created() -> Channel {
    serde_json::from_value(json!({
        "channel_type": "TextChannel", "_id": "ch-new", "server": "srv-alpha", "name": "new"
    }))
    .unwrap()
}

/// `ch-new`'s `ChannelCreate`, as the events connection delivers it.
fn channel_create() -> Channel {
    let frame = json!({
        "type": "ChannelCreate", "channel_type": "TextChannel", "_id": "ch-new",
        "server": "srv-alpha", "name": "new"
    });
    let Some(ServerEvent::ChannelCreate(channel)) = parse_frame(&frame.to_string()).pop() else {
        panic!("ChannelCreate");
    };
    *channel
}

/// The sidebar with `ch-new` once, last of the uncategorized channels.
fn with_new_channel() -> Vec<(Option<String>, Vec<String>)> {
    vec![
        (None, vec!["ch-welcome".into(), "ch-new".into()]),
        (
            Some("cat-chat".into()),
            vec!["ch-general".into(), "ch-random".into()],
        ),
        (Some("cat-voice".into()), vec!["ch-voice".into()]),
        (Some("cat-archive".into()), vec![]),
    ]
}

#[test]
fn a_created_channel_shows_once_when_its_event_comes_first() {
    let mut store = store();
    store.add_channel(channel_create());
    store.add_channel(created());
    assert_eq!(sidebar(&store), with_new_channel());
    let server = store.server("srv-alpha").unwrap();
    assert_eq!(server.channels.iter().filter(|c| *c == "ch-new").count(), 1);
}

#[test]
fn a_created_channel_shows_once_when_its_event_comes_last() {
    let mut store = store();
    store.add_channel(created());
    store.add_channel(channel_create());
    assert_eq!(sidebar(&store), with_new_channel());
    let server = store.server("srv-alpha").unwrap();
    assert_eq!(server.channels.iter().filter(|c| *c == "ch-new").count(), 1);
}

#[test]
fn new_categories_move_the_rows() {
    let mut store = store();
    store.add_channel(created());
    let categories: Vec<Category> = serde_json::from_value(json!([
        { "id": "cat-chat", "title": "Chat", "channels": ["ch-general", "ch-random", "ch-new"] },
        { "id": "cat-voice", "title": "Voice", "channels": [] },
        { "id": "cat-archive", "title": "Archive", "channels": ["ch-voice"] }
    ]))
    .unwrap();
    store.set_categories("srv-alpha", categories);
    assert_eq!(
        sidebar(&store),
        [
            (None, vec!["ch-welcome".to_string()]),
            (
                Some("cat-chat".into()),
                vec!["ch-general".into(), "ch-random".into(), "ch-new".into()]
            ),
            (Some("cat-voice".into()), vec![]),
            (Some("cat-archive".into()), vec!["ch-voice".into()]),
        ]
    );
    assert_eq!(
        store.category_of("srv-alpha", "ch-new").as_deref(),
        Some("cat-chat")
    );
    assert_eq!(store.category_of("srv-alpha", "ch-welcome"), None);
    assert_eq!(store.category_of("srv-unknown", "ch-new"), None);
}

#[test]
fn categories_of_an_unknown_server_are_ignored() {
    let mut store = store();
    let before = sidebar(&store);
    store.set_categories("srv-unknown", Vec::new());
    assert_eq!(sidebar(&store), before);
}

#[test]
fn a_deleted_category_leaves_its_channels_uncategorized() {
    let mut store = store();
    let categories = store.server("srv-alpha").unwrap().categories.clone();
    let edited = without_category(categories.as_deref().unwrap(), "cat-chat").unwrap();
    store.set_categories("srv-alpha", edited);
    assert_eq!(
        sidebar(&store),
        [
            (
                None,
                vec![
                    "ch-welcome".to_string(),
                    "ch-general".into(),
                    "ch-random".into()
                ]
            ),
            (Some("cat-voice".into()), vec!["ch-voice".into()]),
            (Some("cat-archive".into()), vec![]),
        ]
    );
    // None of its channels went with it.
    assert!(store.channel("ch-general").is_some());
    assert!(store.channel("ch-random").is_some());
    assert!(!store.has_category("cat-chat"));
    assert_eq!(store.category_of("srv-alpha", "ch-general"), None);
}

#[test]
fn deleting_the_last_category_leaves_every_channel_uncategorized() {
    let mut store = store();
    store.set_categories("srv-alpha", Vec::new());
    assert_eq!(
        sidebar(&store),
        [(
            None,
            vec![
                "ch-welcome".to_string(),
                "ch-general".into(),
                "ch-random".into(),
                "ch-voice".into()
            ]
        )]
    );
}

/// A `ChannelUpdate` frame for `id`, as Stoat sends it.
fn channel_update(id: &str, data: serde_json::Value, clear: &[&str]) -> ServerEvent {
    let frame = json!({ "type": "ChannelUpdate", "id": id, "data": data, "clear": clear });
    parse_frame(&frame.to_string())
        .pop()
        .expect("ChannelUpdate")
}

fn apply(store: &mut ChatStore, event: ServerEvent) {
    let ServerEvent::ChannelUpdate { id, data, clear } = event else {
        panic!("ChannelUpdate");
    };
    store.apply_channel_update(&id, *data, &clear);
}

#[test]
fn a_saved_channel_edit_then_its_event_are_one_state() {
    let mut store = store();
    let saved: Channel = serde_json::from_value(json!({
        "channel_type": "TextChannel", "_id": "ch-general", "server": "srv-alpha",
        "name": "lobby", "description": "Say hi", "nsfw": true, "slowmode": 30,
        "icon": {
            "_id": "ic-1", "tag": "icons", "filename": "i.png",
            "metadata": { "type": "Image", "width": 64, "height": 64 },
            "content_type": "image/png", "size": 100
        }
    }))
    .unwrap();
    assert!(store.apply_channel_edit(&saved));
    let after_save = store.channel("ch-general").cloned();
    let Some(Channel::TextChannel {
        name,
        description,
        icon,
        nsfw,
        slowmode,
        voice,
        ..
    }) = &after_save
    else {
        panic!("a text channel");
    };
    assert_eq!(name, "lobby");
    assert_eq!(description.as_deref(), Some("Say hi"));
    assert_eq!(icon.as_ref().map(|i| i.id.as_str()), Some("ic-1"));
    assert!(*nsfw);
    assert_eq!(*slowmode, Some(30));
    assert_eq!(*voice, None);
    assert_eq!(store.slowmode("ch-general"), Some(30));
    assert!(store.is_mature("ch-general"));

    apply(
        &mut store,
        channel_update(
            "ch-general",
            json!({ "name": "lobby", "description": "Say hi", "nsfw": true, "slowmode": 30 }),
            &[],
        ),
    );
    assert_eq!(store.channel("ch-general").cloned(), after_save);
}

#[test]
fn a_saved_edit_clears_what_the_answer_lacks() {
    let mut store = store();
    apply(
        &mut store,
        channel_update(
            "ch-general",
            json!({ "description": "Old", "slowmode": 10 }),
            &[],
        ),
    );
    let saved: Channel = serde_json::from_value(json!({
        "channel_type": "TextChannel", "_id": "ch-general", "server": "srv-alpha",
        "name": "general"
    }))
    .unwrap();
    assert!(store.apply_channel_edit(&saved));
    let Some(Channel::TextChannel {
        description,
        slowmode,
        nsfw,
        ..
    }) = store.channel("ch-general")
    else {
        panic!("a text channel");
    };
    assert_eq!((description, slowmode, nsfw), (&None, &None, &false));
}

#[test]
fn a_saved_group_edit_keeps_its_owner_and_members() {
    let mut store = store();
    // An answer that (wrongly) names other people changes only the
    // group's settings.
    let saved: Channel = serde_json::from_value(json!({
        "channel_type": "Group", "_id": "grp-weekend", "name": "Weekend",
        "owner": "u-someone", "recipients": ["u-someone"], "permissions": 0,
        "description": "Plans"
    }))
    .unwrap();
    assert!(store.apply_channel_edit(&saved));
    let Some(Channel::Group {
        name,
        owner,
        recipients,
        description,
        permissions,
        ..
    }) = store.channel("grp-weekend")
    else {
        panic!("a group");
    };
    assert_eq!(name, "Weekend");
    assert_eq!(description.as_deref(), Some("Plans"));
    assert_eq!(owner, "u-ada");
    assert_eq!(recipients, &["u-me", "u-ada", "u-bob"]);
    assert_eq!(*permissions, None);
}

#[test]
fn a_saved_edit_of_an_unknown_or_unsettable_channel_does_nothing() {
    let mut store = store();
    let unknown: Channel = serde_json::from_value(json!({
        "channel_type": "TextChannel", "_id": "ch-nope", "server": "srv-alpha", "name": "x"
    }))
    .unwrap();
    assert!(!store.apply_channel_edit(&unknown));
    assert!(store.channel("ch-nope").is_none());
    let dm = store.channel("dm-ada").cloned().unwrap();
    assert!(!store.apply_channel_edit(&dm));
}
