//! A channel's or group's settings: who may open them, what the Overview
//! sends, and why Stoat refused, in words.

use revolt_permissions::{
    ChannelPermission, DEFAULT_PERMISSION_DIRECT_MESSAGE, DEFAULT_PERMISSION_SERVER,
};
use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{Channel, FieldsChannel};
use stoat_gpui::channel_settings::{
    ChannelAccess, ChannelForm, ChannelKind, MaturePrompt, SLOWMODE_CHOICES, access,
    empty_channel_edit, mature_edit, mature_error_text, mature_prompt, mature_state,
    save_error_text, slowmode_choices, slowmode_label, window_title,
};
use stoat_gpui::profile_edit::ProfileImage;
use stoat_gpui::store::ChatStore;

const MANAGE_CHANNEL: u64 = ChannelPermission::ManageChannel as u64;

fn user(id: &str) -> Value {
    json!({
        "_id": id, "username": id, "discriminator": "0001",
        "relationship": "None", "online": true
    })
}

fn member(user: &str, roles: &[&str]) -> Value {
    json!({
        "_id": { "server": "srv", "user": user },
        "joined_at": "2026-01-01T00:00:00.000Z",
        "roles": roles,
    })
}

/// `srv`, owned by `u-owner`: `u-mod` may manage every channel, `u-local`
/// only `ch-b` (through its override), and `u-plain` none. `grp` is owned
/// by `u-owner`, with default permissions; `strict` is owned by `u-owner`
/// and lets members do nothing beyond the basics. `dm` is a DM.
fn store(me: &str) -> ChatStore {
    let users: Vec<Value> = ["u-owner", "u-mod", "u-local", "u-plain"].map(user).into();
    let everyone = ["u-owner", "u-mod", "u-local", "u-plain"];
    let ready = json!({
        "type": "Ready",
        "users": users,
        "servers": [{
            "_id": "srv", "owner": "u-owner", "name": "Test",
            "channels": ["ch-a", "ch-b"],
            "approximate_member_count": 4,
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "roles": {
                "mod": { "_id": "mod", "name": "Mod", "rank": 0, "permissions": { "a": MANAGE_CHANNEL, "d": 0 } },
                "local": { "_id": "local", "name": "Local", "rank": 1, "permissions": { "a": 0, "d": 0 } }
            }
        }],
        "channels": [
            {
                "channel_type": "TextChannel", "_id": "ch-a", "server": "srv", "name": "a",
                "description": "About a", "icon": icon("ic-a")
            },
            {
                "channel_type": "TextChannel", "_id": "ch-b", "server": "srv", "name": "b",
                "slowmode": 45,
                "role_permissions": { "local": { "a": MANAGE_CHANNEL, "d": 0 } }
            },
            {
                "channel_type": "Group", "_id": "grp", "name": "Friends", "owner": "u-owner",
                "recipients": everyone
            },
            {
                "channel_type": "Group", "_id": "strict", "name": "Strict", "owner": "u-owner",
                "recipients": everyone,
                "permissions": (*DEFAULT_PERMISSION_DIRECT_MESSAGE & !MANAGE_CHANNEL) as i64
            },
            {
                "channel_type": "DirectMessage", "_id": "dm", "active": true,
                "recipients": [me, "u-owner"]
            }
        ],
        "members": [
            member("u-owner", &[]),
            member("u-mod", &["mod"]),
            member("u-local", &["local"]),
            member("u-plain", &[]),
        ],
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new(me);
    store.apply_ready(*ready);
    store
}

fn icon(id: &str) -> Value {
    json!({
        "_id": id, "tag": "icons", "filename": "i.png",
        "metadata": { "type": "Image", "width": 64, "height": 64 },
        "content_type": "image/png", "size": 100
    })
}

fn channel<'a>(store: &'a ChatStore, id: &str) -> &'a Channel {
    store.channel(id).expect("channel")
}

fn api_error(body: Value) -> ApiError {
    ApiError::from_response(
        reqwest::StatusCode::BAD_REQUEST,
        body.to_string().as_bytes(),
    )
}

/// The Overview, without Manage Permissions.
const SHOWN: ChannelAccess = ChannelAccess {
    overview: true,
    permissions: false,
    webhooks: false,
};

/// Both pages.
const EVERY_PAGE: ChannelAccess = ChannelAccess {
    overview: true,
    permissions: true,
    webhooks: true,
};

#[test]
fn the_owner_and_a_channel_manager_may_edit_every_channel() {
    let owner = store("u-owner");
    assert_eq!(access(&owner, "ch-a"), EVERY_PAGE);
    assert_eq!(access(&owner, "ch-b"), EVERY_PAGE);
    let moderator = store("u-mod");
    assert_eq!(access(&moderator, "ch-a"), SHOWN);
    assert_eq!(access(&moderator, "ch-b"), SHOWN);
}

#[test]
fn an_override_gives_one_channel_only() {
    let store = store("u-local");
    assert_eq!(access(&store, "ch-b"), SHOWN);
    assert_eq!(access(&store, "ch-a"), ChannelAccess::default());
    assert!(!access(&store, "ch-a").any());
}

#[test]
fn a_plain_member_may_edit_no_server_channel() {
    let store = store("u-plain");
    assert!(!access(&store, "ch-a").any());
    assert!(!access(&store, "ch-b").any());
}

#[test]
fn a_group_is_editable_by_its_owner_and_by_default_its_members() {
    assert!(access(&store("u-owner"), "grp").any());
    assert!(access(&store("u-plain"), "grp").any());
    // A group that takes Manage Channels from its members.
    assert!(access(&store("u-owner"), "strict").any());
    assert!(!access(&store("u-plain"), "strict").any());
    // Only the owner may change a group's permissions, by default.
    assert_eq!(access(&store("u-owner"), "grp"), EVERY_PAGE);
    assert_eq!(access(&store("u-plain"), "grp"), SHOWN);
}

#[test]
fn a_dm_has_no_settings() {
    let store = store("u-plain");
    assert!(!access(&store, "dm").any());
    assert_eq!(ChannelKind::of(channel(&store, "dm")), None);
    assert_eq!(ChannelForm::of(channel(&store, "dm")), None);
}

#[test]
fn the_window_title_names_the_channel_or_group() {
    let store = store("u-owner");
    assert_eq!(
        window_title(Some(channel(&store, "ch-a"))),
        "Channel Settings — #a"
    );
    assert_eq!(
        window_title(Some(channel(&store, "grp"))),
        "Group Settings — Friends"
    );
    assert_eq!(window_title(None), "Channel Settings");
    assert_eq!(ChannelKind::Channel.noun(), "channel");
    assert_eq!(ChannelKind::Group.noun(), "group");
}

#[test]
fn the_form_starts_with_the_stored_values() {
    let store = store("u-owner");
    assert_eq!(
        ChannelForm::of(channel(&store, "ch-a")),
        Some(ChannelForm {
            name: "a".into(),
            description: "About a".into(),
            icon: ProfileImage::Current("ic-a".into()),
            slowmode: 0,
        })
    );
    assert_eq!(
        ChannelForm::of(channel(&store, "grp")),
        Some(ChannelForm {
            name: "Friends".into(),
            description: String::new(),
            icon: ProfileImage::None,
            slowmode: 0,
        })
    );
}

#[test]
fn an_unchanged_form_sends_nothing() {
    let store = store("u-owner");
    for id in ["ch-a", "ch-b", "grp"] {
        let channel = channel(&store, id);
        let form = ChannelForm::of(channel).unwrap();
        assert_eq!(form.changes(channel), None, "{id}");
        assert!(!form.has_changes(channel), "{id}");
    }
}

#[test]
fn a_new_name_is_sent_trimmed() {
    let store = store("u-owner");
    let a = channel(&store, "ch-a");
    let mut form = ChannelForm::of(a).unwrap();
    form.name = "  general  ".into();
    let edit = form.changes(a).unwrap();
    assert_eq!(edit.name.as_deref(), Some("general"));
    assert_eq!(edit.description, None);
    assert!(edit.remove.is_empty());
    // Only spaces around the same name is no change.
    form.name = " a ".into();
    assert_eq!(form.changes(a), None);
}

#[test]
fn a_blank_name_or_long_description_cant_be_sent() {
    let store = store("u-owner");
    let a = channel(&store, "ch-a");
    let mut form = ChannelForm::of(a).unwrap();
    form.name = "   ".into();
    assert!(!form.is_valid());
    assert_eq!(form.changes(a), None);
    form.name = "x".repeat(33);
    assert!(!form.is_valid());
    form.name = "x".repeat(32);
    form.description = "d".repeat(1025);
    assert!(!form.is_valid());
    form.description = "d".repeat(1024);
    assert!(form.is_valid());
}

#[test]
fn a_blank_description_is_removed_and_a_new_one_sent() {
    let store = store("u-owner");
    let a = channel(&store, "ch-a");
    let mut form = ChannelForm::of(a).unwrap();
    form.description = "  ".into();
    let edit = form.changes(a).unwrap();
    assert_eq!(edit.description, None);
    assert_eq!(edit.remove, vec![FieldsChannel::Description]);

    let group = channel(&store, "grp");
    let mut form = ChannelForm::of(group).unwrap();
    form.description = " For friends ".into();
    let edit = form.changes(group).unwrap();
    assert_eq!(edit.description.as_deref(), Some("For friends"));
    assert!(edit.remove.is_empty());
}

#[test]
fn an_icon_is_removed_replaced_or_uploaded_first() {
    let store = store("u-owner");
    let a = channel(&store, "ch-a");
    let mut form = ChannelForm::of(a).unwrap();

    form.icon = ProfileImage::None;
    assert_eq!(form.changes(a).unwrap().remove, vec![FieldsChannel::Icon]);

    form.icon = ProfileImage::Current("ic-new".into());
    let edit = form.changes(a).unwrap();
    assert_eq!(edit.icon.as_deref(), Some("ic-new"));
    assert!(edit.remove.is_empty());

    // A picked file isn't sent until it's uploaded, but Save has work.
    form.icon = ProfileImage::New("/tmp/icon.png".into());
    assert_eq!(form.changes(a), None);
    assert!(form.has_changes(a));
    assert!(form.new_file().is_some());
}

#[test]
fn the_edit_never_carries_anything_the_form_lacks() {
    let store = store("u-owner");
    let group = channel(&store, "grp");
    let mut form = ChannelForm::of(group).unwrap();
    form.name = "Pals".into();
    let edit = form.changes(group).unwrap();
    assert_eq!(
        (
            edit.owner,
            edit.nsfw,
            edit.archived,
            edit.voice,
            edit.slowmode
        ),
        (None, None, None, None, None)
    );
    assert!(empty_channel_edit().remove.is_empty());
}

#[test]
fn refusals_read_plainly() {
    assert_eq!(
        save_error_text(&api_error(
            json!({ "type": "MissingPermission", "permission": "ManageChannel" })
        )),
        "You don't have permission to change this channel."
    );
    assert_eq!(
        save_error_text(&api_error(
            json!({ "type": "FailedValidation", "error": "name: length" })
        )),
        "Channel names are 1–32 characters, and descriptions at most 1024."
    );
    assert_eq!(
        save_error_text(&api_error(json!({ "type": "NotFound" }))),
        "That channel was deleted."
    );
    assert_eq!(
        save_error_text(&api_error(json!({ "type": "InternalError" }))),
        "Couldn't save the channel's settings."
    );
}

#[test]
fn slowmode_starts_as_stored_and_off_when_unset() {
    let store = store("u-owner");
    assert_eq!(
        ChannelForm::of(channel(&store, "ch-b")).unwrap().slowmode,
        45
    );
    assert_eq!(
        ChannelForm::of(channel(&store, "ch-a")).unwrap().slowmode,
        0
    );
    assert_eq!(ChannelForm::of(channel(&store, "grp")).unwrap().slowmode, 0);
}

#[test]
fn a_changed_slowmode_is_sent_including_off() {
    let store = store("u-owner");
    let a = channel(&store, "ch-a");
    let mut form = ChannelForm::of(a).unwrap();
    form.slowmode = 10;
    let edit = form.changes(a).unwrap();
    assert_eq!(edit.slowmode, Some(10));
    assert_eq!((edit.name, edit.description, edit.icon), (None, None, None));
    assert!(edit.remove.is_empty());

    let b = channel(&store, "ch-b");
    let mut form = ChannelForm::of(b).unwrap();
    form.slowmode = 0;
    assert_eq!(form.changes(b).unwrap().slowmode, Some(0));
    form.slowmode = 45;
    assert_eq!(form.changes(b), None);
}

#[test]
fn a_group_never_sends_a_slowmode() {
    let store = store("u-owner");
    let group = channel(&store, "grp");
    let mut form = ChannelForm::of(group).unwrap();
    form.slowmode = 30;
    assert_eq!(form.changes(group), None);
    form.name = "Pals".into();
    assert_eq!(form.changes(group).unwrap().slowmode, None);
}

#[test]
fn the_slowmode_choices_are_the_web_clients() {
    let secs: Vec<u64> = SLOWMODE_CHOICES.iter().map(|(secs, _)| *secs).collect();
    assert_eq!(secs, [0, 5, 10, 30, 60, 300, 600, 1800, 3600, 7200, 21600]);
    assert_eq!(slowmode_label(0), "Off");
    assert_eq!(slowmode_label(60), "1 minute");
    assert_eq!(slowmode_label(21600), "6 hours");
    // Anything else in short words.
    assert_eq!(slowmode_label(45), "45s");
    assert_eq!(slowmode_label(90), "1m 30s");
    // A listed value adds nothing.
    assert_eq!(slowmode_choices(30).len(), 11);
}

#[test]
fn a_stored_slowmode_off_the_list_is_offered_in_its_place() {
    let choices = slowmode_choices(45);
    assert_eq!(choices.len(), 12);
    assert_eq!(choices[3], (30, "30 seconds".to_string()));
    assert_eq!(choices[4], (45, "45s".to_string()));
    assert_eq!(choices[5], (60, "1 minute".to_string()));
    // Beyond the last one, it goes last.
    assert_eq!(
        slowmode_choices(21601).last(),
        Some(&(21601, "6h".to_string()))
    );
}

#[test]
fn marking_mature_sends_only_nsfw() {
    let edit = mature_edit(true);
    assert_eq!(edit.nsfw, Some(true));
    assert_eq!(
        edit,
        stoat_gpui::api::models::DataEditChannel {
            nsfw: Some(true),
            ..empty_channel_edit()
        }
    );
    assert_eq!(mature_edit(false).nsfw, Some(false));
}

#[test]
fn the_mature_prompt_is_the_web_clients() {
    assert_eq!(
        mature_prompt(ChannelKind::Channel, false),
        MaturePrompt {
            title: "Mark this channel as mature?".into(),
            text: "Users will be asked to confirm their age before joining this channel.".into(),
            confirm: "Mark as mature",
        }
    );
    assert_eq!(
        mature_prompt(ChannelKind::Group, true),
        MaturePrompt {
            title: "Unmark this group as mature?".into(),
            text: "Users will no longer be asked to confirm their age before joining this \
                   group. Please ensure the content is appropriate for all ages."
                .into(),
            confirm: "Unmark as mature",
        }
    );
    assert_eq!(
        mature_state(ChannelKind::Channel, true),
        "Members are asked to confirm their age before opening this channel."
    );
    assert_eq!(
        mature_state(ChannelKind::Group, false),
        "Members aren't asked to confirm their age."
    );
}

#[test]
fn mature_refusals_read_plainly() {
    assert_eq!(
        mature_error_text(&api_error(
            json!({ "type": "MissingPermission", "permission": "ManageChannel" })
        )),
        "You don't have permission to change this channel."
    );
    assert_eq!(
        mature_error_text(&api_error(json!({ "type": "NotFound" }))),
        "That channel was deleted."
    );
    assert_eq!(
        mature_error_text(&api_error(json!({ "type": "InternalError" }))),
        "Couldn't change whether the channel is mature."
    );
}
