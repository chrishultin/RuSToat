//! Groups' members and owners in the store: people added and removed live
//! (`ChannelGroupJoin`, `ChannelGroupLeave`), and owners handed on by a
//! live `channel_ownership_changed` message but not by one in a history
//! page, or directly (the user's own handover). Built on the `Ready`
//! fixture, where `grp-weekend` is Ada's, with the user and Bob in it.

use serde_json::json;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{BulkMessageResponse, Channel, Message};
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

fn recipients(store: &ChatStore, channel: &str) -> Vec<String> {
    match store.channel(channel) {
        Some(Channel::Group { recipients, .. }) => recipients.clone(),
        other => panic!("not a group: {other:?}"),
    }
}

fn owner(store: &ChatStore, channel: &str) -> String {
    match store.channel(channel) {
        Some(Channel::Group { owner, .. }) => owner.clone(),
        other => panic!("not a group: {other:?}"),
    }
}

/// Stoat's system message for `from` handing the group to `to`.
fn handover(id: &str, from: &str, to: &str) -> Message {
    serde_json::from_value(json!({
        "_id": id,
        "channel": "grp-weekend",
        "author": "00000000000000000000000000",
        "system": { "type": "channel_ownership_changed", "from": from, "to": to },
    }))
    .unwrap()
}

#[test]
fn someone_added_twice_is_in_the_group_once() {
    let mut store = store();

    assert!(store.add_recipient("grp-weekend", "u-broken"));
    assert!(!store.add_recipient("grp-weekend", "u-broken"));

    assert_eq!(
        recipients(&store, "grp-weekend"),
        ["u-me", "u-ada", "u-bob", "u-broken"]
    );
    assert_eq!(store.member_list("grp-weekend").title, "4 members");
    assert!(store.lists_member("grp-weekend", "u-broken"));
}

#[test]
fn only_groups_take_recipients() {
    let mut store = store();
    assert!(!store.add_recipient("dm-ada", "u-bob"));
    assert!(!store.add_recipient("ch-general", "u-bob"));
    assert!(!store.add_recipient("grp-nowhere", "u-bob"));
    assert!(!store.remove_recipient("dm-ada", "u-ada"));
    assert!(!store.remove_recipient("grp-nowhere", "u-ada"));
}

#[test]
fn someone_removed_leaves_the_group_and_its_member_list() {
    let mut store = store();

    assert!(store.remove_recipient("grp-weekend", "u-bob"));
    assert!(!store.remove_recipient("grp-weekend", "u-bob"));

    assert_eq!(recipients(&store, "grp-weekend"), ["u-me", "u-ada"]);
    assert_eq!(store.member_list("grp-weekend").title, "2 members");
    assert!(!store.lists_member("grp-weekend", "u-bob"));
}

#[test]
fn a_group_the_user_is_removed_from_leaves_home() {
    let mut store = store();
    assert!(store.home_channels().iter().any(|c| c.id == "grp-weekend"));

    assert!(store.remove_channel("grp-weekend"));

    assert!(!store.home_channels().iter().any(|c| c.id == "grp-weekend"));
    assert!(store.channel("grp-weekend").is_none());
}

#[test]
fn a_live_ownership_message_hands_the_group_on() {
    let mut store = store();
    assert!(!store.is_group_owner("grp-weekend"));

    store.apply_message(handover("01JC0000000000000000000010", "u-ada", "u-me"));
    assert_eq!(owner(&store, "grp-weekend"), "u-me");
    assert!(store.is_group_owner("grp-weekend"));

    // And on again: the later message wins.
    store.apply_message(handover("01JC0000000000000000000011", "u-me", "u-bob"));
    assert_eq!(owner(&store, "grp-weekend"), "u-bob");
    assert!(!store.is_group_owner("grp-weekend"));
}

#[test]
fn a_group_is_handed_on_directly() {
    let mut store = store();
    assert!(store.set_group_owner("grp-weekend", "u-me"));
    assert!(store.is_group_owner("grp-weekend"));
    // A repeat changes nothing.
    assert!(!store.set_group_owner("grp-weekend", "u-me"));
    assert!(store.set_group_owner("grp-weekend", "u-bob"));
    assert_eq!(owner(&store, "grp-weekend"), "u-bob");
    // Only groups have owners to hand on.
    for channel in ["ch-beta", "dm-ada", "grp-nowhere"] {
        assert!(!store.set_group_owner(channel, "u-me"), "{channel}");
    }
}

#[test]
fn an_ownership_message_in_a_history_page_changes_nothing() {
    let mut store = store();
    // An old handover, from before the one that made Ada the owner.
    let old = handover("01JC0000000000000000000001", "u-me", "u-bob");
    let page = BulkMessageResponse::JustMessages(vec![old]);

    store.set_history("grp-weekend", page, 50);

    assert_eq!(owner(&store, "grp-weekend"), "u-ada");
}

#[test]
fn only_owned_groups_are_the_users() {
    let store = store();
    // The user owns `srv-beta`, but that's a server, not a group.
    assert!(!store.is_group_owner("ch-beta"));
    assert!(!store.is_group_owner("dm-ada"));
    assert!(!store.is_group_owner("grp-nowhere"));
}
