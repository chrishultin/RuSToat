//! Slowmode: how long members of a server text channel must wait between
//! messages.

use serde_json::json;
use stoat_gpui::api::models::{Channel, FieldsChannel, PartialChannel};
use stoat_gpui::store::ChatStore;

fn channel(value: serde_json::Value) -> Channel {
    serde_json::from_value(value).unwrap()
}

fn partial(value: serde_json::Value) -> PartialChannel {
    serde_json::from_value(value).unwrap()
}

fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    for value in [
        json!({ "channel_type": "TextChannel", "_id": "ch-slow", "server": "srv", "name": "slow", "slowmode": 30 }),
        json!({ "channel_type": "TextChannel", "_id": "ch-zero", "server": "srv", "name": "zero", "slowmode": 0 }),
        json!({ "channel_type": "TextChannel", "_id": "ch-plain", "server": "srv", "name": "plain" }),
        json!({ "channel_type": "Group", "_id": "ch-group", "name": "group", "owner": "u-me",
                "recipients": ["u-me", "u-ada"] }),
        json!({ "channel_type": "DirectMessage", "_id": "ch-dm", "active": true,
                "recipients": ["u-me", "u-ada"] }),
        json!({ "channel_type": "SavedMessages", "_id": "ch-notes", "user": "u-me" }),
    ] {
        store.add_channel(channel(value));
    }
    store
}

#[test]
fn only_a_text_channel_with_a_delay_has_slowmode() {
    let store = store();
    assert_eq!(store.slowmode("ch-slow"), Some(30));
    assert_eq!(store.slowmode("ch-zero"), None);
    assert_eq!(store.slowmode("ch-plain"), None);
    assert_eq!(store.slowmode("ch-group"), None);
    assert_eq!(store.slowmode("ch-dm"), None);
    assert_eq!(store.slowmode("ch-notes"), None);
    assert_eq!(store.slowmode("ch-unknown"), None);
}

#[test]
fn a_channel_update_sets_and_changes_slowmode() {
    let mut store = store();
    store.apply_channel_update("ch-plain", partial(json!({ "slowmode": 10 })), &[]);
    assert_eq!(store.slowmode("ch-plain"), Some(10));
    store.apply_channel_update("ch-slow", partial(json!({ "slowmode": 0 })), &[]);
    assert_eq!(store.slowmode("ch-slow"), None);
}

#[test]
fn removing_slowmode_turns_it_off() {
    let mut store = store();
    store.apply_channel_update("ch-slow", partial(json!({})), &[FieldsChannel::Slowmode]);
    assert_eq!(store.slowmode("ch-slow"), None);
}

#[test]
fn removing_it_from_a_live_frame_turns_it_off() {
    use stoat_gpui::api::events::{ServerEvent, parse_frame};
    let mut store = store();
    let frame = json!({
        "type": "ChannelUpdate", "id": "ch-slow", "data": {}, "clear": ["Slowmode"]
    });
    let Some(ServerEvent::ChannelUpdate { id, data, clear }) =
        parse_frame(&frame.to_string()).pop()
    else {
        panic!("channel update");
    };
    store.apply_channel_update(&id, *data, &clear);
    assert_eq!(store.slowmode("ch-slow"), None);
}
