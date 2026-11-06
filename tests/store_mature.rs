//! Mature (NSFW) channels: server channels and groups marked `nsfw`.

use serde_json::json;
use stoat_gpui::api::models::{Channel, PartialChannel};
use stoat_gpui::store::ChatStore;

fn channel(value: serde_json::Value) -> Channel {
    serde_json::from_value(value).unwrap()
}

fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    for value in [
        json!({ "channel_type": "TextChannel", "_id": "ch-adult", "server": "srv", "name": "adult", "nsfw": true }),
        json!({ "channel_type": "TextChannel", "_id": "ch-plain", "server": "srv", "name": "plain" }),
        json!({ "channel_type": "Group", "_id": "ch-group", "name": "group", "owner": "u-me",
                "recipients": ["u-me", "u-ada"], "nsfw": true }),
        json!({ "channel_type": "DirectMessage", "_id": "ch-dm", "active": true,
                "recipients": ["u-me", "u-ada"] }),
        json!({ "channel_type": "SavedMessages", "_id": "ch-notes", "user": "u-me" }),
    ] {
        store.add_channel(channel(value));
    }
    store
}

#[test]
fn server_channels_and_groups_can_be_mature() {
    let store = store();
    assert!(store.is_mature("ch-adult"));
    assert!(store.is_mature("ch-group"));
    assert!(!store.is_mature("ch-plain"));
    assert!(!store.is_mature("ch-dm"));
    assert!(!store.is_mature("ch-notes"));
    assert!(!store.is_mature("ch-unknown"));
}

#[test]
fn a_channel_update_can_make_it_mature_or_not() {
    let mut store = store();
    let nsfw =
        |on: bool| -> PartialChannel { serde_json::from_value(json!({ "nsfw": on })).unwrap() };
    store.apply_channel_update("ch-plain", nsfw(true), &[]);
    assert!(store.is_mature("ch-plain"));
    store.apply_channel_update("ch-adult", nsfw(false), &[]);
    assert!(!store.is_mature("ch-adult"));
}
