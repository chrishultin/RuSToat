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

fn slowmodes(frame: serde_json::Value) -> Vec<stoat_gpui::api::models::ChannelSlowmode> {
    use stoat_gpui::api::events::{ServerEvent, parse_frame};
    let Some(ServerEvent::UserSlowmodes(list)) = parse_frame(&frame.to_string()).pop() else {
        panic!("user slowmodes");
    };
    list
}

#[test]
fn an_event_starts_a_countdown_that_runs_out() {
    let mut store = store();
    let list = slowmodes(json!({
        "type": "UserSlowmodes",
        "slowmodes": [{ "channel_id": "ch-slow", "duration": 30, "retry_after": 30 }]
    }));
    store.apply_slowmodes(&list, 1_000_000);
    assert_eq!(store.cooldown_left("ch-slow", 1_000_000), Some(30));
    assert_eq!(store.cooldown_left("ch-slow", 1_005_500), Some(25));
    assert_eq!(store.cooldown_left("ch-slow", 1_029_001), Some(1));
    assert_eq!(store.cooldown_ends("ch-slow", 1_000_000), Some(1_030_000));
    assert_eq!(store.cooldown_left("ch-slow", 1_030_000), None);
    assert_eq!(store.cooldown_ends("ch-slow", 1_030_000), None);
    store.prune_cooldowns(1_030_000);
    assert_eq!(store.cooldown_left("ch-slow", 1_000_000), None);
}

#[test]
fn a_later_event_replaces_the_cooldown() {
    let mut store = store();
    store.start_cooldown("ch-slow", 30, 1_000_000);
    // The server's own figure, a little later: it wins.
    let list = slowmodes(json!({
        "type": "UserSlowmodes",
        "slowmodes": [{ "channel_id": "ch-slow", "duration": 30, "retry_after": 28 }]
    }));
    store.apply_slowmodes(&list, 1_003_000);
    assert_eq!(store.cooldown_left("ch-slow", 1_003_000), Some(28));
    assert_eq!(store.cooldown_ends("ch-slow", 1_003_000), Some(1_031_000));
}

#[test]
fn a_cooldown_can_start_from_a_messages_time() {
    let mut store = store();
    // The ULID README's example ID was made at 1469918176385 ms.
    let sent = stoat_gpui::time::ulid_millis("01ARYZ6S41TSV4RRFFQ69G5FAV").unwrap();
    store.start_cooldown("ch-slow", 30, sent);
    assert_eq!(store.cooldown_left("ch-slow", 1_469_918_176_385), Some(30));
    assert_eq!(store.cooldown_left("ch-slow", 1_469_918_196_385), Some(10));
    assert_eq!(store.cooldown_left("ch-slow", 1_469_918_206_385), None);
}

#[test]
fn cooldowns_are_per_channel() {
    let mut store = store();
    let list = slowmodes(json!({
        "type": "UserSlowmodes",
        "slowmodes": [
            { "channel_id": "ch-slow", "duration": 30, "retry_after": 30 },
            { "channel_id": "ch-plain", "duration": 600, "retry_after": 120 }
        ]
    }));
    store.apply_slowmodes(&list, 0);
    assert_eq!(store.cooldown_left("ch-slow", 0), Some(30));
    assert_eq!(store.cooldown_left("ch-plain", 0), Some(120));
    assert_eq!(store.cooldown_left("ch-zero", 0), None);
    // An event about one channel leaves the others' cooldowns alone.
    let list = slowmodes(json!({
        "type": "UserSlowmodes",
        "slowmodes": [{ "channel_id": "ch-slow", "duration": 30, "retry_after": 5 }]
    }));
    store.apply_slowmodes(&list, 0);
    assert_eq!(store.cooldown_left("ch-slow", 0), Some(5));
    assert_eq!(store.cooldown_left("ch-plain", 0), Some(120));
}

#[test]
fn cooldowns_survive_a_new_ready() {
    let mut store = store();
    store.start_cooldown("ch-slow", 30, 0);
    store.apply_ready(Default::default());
    assert_eq!(store.cooldown_left("ch-slow", 0), Some(30));
}
