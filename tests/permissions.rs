//! Who can see and do what in a channel, worked out by Stoat's own
//! `revolt-permissions` over the store's data.

use revolt_permissions::{
    ChannelPermission, DEFAULT_PERMISSION_DIRECT_MESSAGE, DEFAULT_PERMISSION_SERVER,
};
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::permissions::{
    can_bypass_slowmode, can_change_avatar, can_change_nickname, can_mention_everyone,
    can_mention_roles, can_pin, can_send_embeds, can_view, channel_permissions, has,
    server_permissions,
};
use stoat_gpui::store::ChatStore;

const VIEW: u64 = ChannelPermission::ViewChannel as u64;
const SEND: u64 = ChannelPermission::SendMessage as u64;
const MANAGE: u64 = ChannelPermission::ManageMessages as u64;
const NICKNAME: u64 = ChannelPermission::ChangeNickname as u64;
const AVATAR: u64 = ChannelPermission::ChangeAvatar as u64;

fn member(user: &str, roles: &[&str], extra: Value) -> Value {
    let mut member = json!({
        "_id": { "server": "srv", "user": user },
        "joined_at": "2026-01-01T00:00:00.000Z",
        "roles": roles,
    });
    member
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    member
}

fn user(id: &str, extra: Value) -> Value {
    let mut user = json!({
        "_id": id, "username": id, "discriminator": "0001",
        "relationship": "None", "online": true
    });
    user.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    user
}

/// Roles, from the highest-ranked down: `mod` may manage messages, `staff`
/// is a rank below, `muted` may not send, `rookie` is denied what `mod`
/// allows, and `anonymous` (the lowest) may not change their nickname or
/// avatar.
fn server_json() -> Value {
    json!({
        "_id": "srv", "owner": "u-owner", "name": "Test", "channels": ["ch-open", "ch-staff", "ch-no-rookies"],
        "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
        "approximate_member_count": 6,
        "roles": {
            "mod": { "_id": "mod", "name": "Mod", "rank": 1, "permissions": { "a": MANAGE, "d": 0 } },
            "staff": { "_id": "staff", "name": "Staff", "rank": 2, "permissions": { "a": 0, "d": 0 } },
            "muted": { "_id": "muted", "name": "Muted", "rank": 3, "permissions": { "a": 0, "d": SEND } },
            "rookie": { "_id": "rookie", "name": "Rookie", "rank": 4, "permissions": { "a": 0, "d": MANAGE } },
            "anonymous": { "_id": "anonymous", "name": "Anonymous", "rank": 5, "permissions": { "a": 0, "d": NICKNAME | AVATAR } }
        }
    })
}

fn store_with(channels: Value, extra_users: Vec<Value>, members: Vec<Value>) -> ChatStore {
    let mut users = vec![user("u-me", json!({}))];
    users.extend(extra_users);
    let ready = json!({
        "type": "Ready",
        "users": users,
        "servers": [server_json()],
        "channels": channels,
        "members": members,
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

/// A server with three channels: an open one, one hidden from everyone but
/// `staff`, and one hidden from `rookie`.
fn server_store() -> ChatStore {
    let channels = json!([
        { "channel_type": "TextChannel", "_id": "ch-open", "server": "srv", "name": "open" },
        {
            "channel_type": "TextChannel", "_id": "ch-staff", "server": "srv", "name": "staff",
            "default_permissions": { "a": 0, "d": VIEW },
            "role_permissions": { "staff": { "a": VIEW, "d": 0 } }
        },
        {
            "channel_type": "TextChannel", "_id": "ch-no-rookies", "server": "srv", "name": "grown-ups",
            "role_permissions": { "rookie": { "a": 0, "d": VIEW } }
        }
    ]);
    store_with(
        channels,
        vec![
            user("u-owner", json!({})),
            user("u-plain", json!({})),
            user("u-mod", json!({})),
            user("u-staff", json!({})),
            user("u-muted", json!({})),
            user("u-rookie", json!({})),
            user("u-timed", json!({})),
            user("u-lapsed", json!({})),
            user("u-stranger", json!({})),
            user("u-anon", json!({})),
        ],
        vec![
            member("u-me", &[], json!({})),
            member("u-owner", &[], json!({})),
            member("u-plain", &[], json!({})),
            member("u-mod", &["mod"], json!({})),
            member("u-staff", &["staff"], json!({})),
            member("u-muted", &["muted"], json!({})),
            member("u-rookie", &["mod", "rookie"], json!({})),
            member("u-anon", &["anonymous"], json!({})),
            member(
                "u-timed",
                &[],
                json!({ "timeout": "2999-01-01T00:00:00.000Z" }),
            ),
            member(
                "u-lapsed",
                &[],
                json!({ "timeout": "2001-01-01T00:00:00.000Z" }),
            ),
        ],
    )
}

#[test]
fn the_owner_can_do_everything_even_where_a_channel_hides_it() {
    let store = server_store();
    for channel in ["ch-open", "ch-staff", "ch-no-rookies"] {
        let bits = channel_permissions(&store, "u-owner", channel);
        assert_eq!(bits, ChannelPermission::GrantAllSafe as u64, "{channel}");
    }
}

#[test]
fn a_plain_member_gets_the_servers_defaults() {
    let store = server_store();
    let bits = channel_permissions(&store, "u-plain", "ch-open");
    assert_eq!(bits, *DEFAULT_PERMISSION_SERVER);
    assert!(has(bits, ChannelPermission::ViewChannel));
    assert!(has(bits, ChannelPermission::SendMessage));
    assert!(!has(bits, ChannelPermission::ManageMessages));
}

#[test]
fn a_role_can_allow_something() {
    let store = server_store();
    let bits = channel_permissions(&store, "u-mod", "ch-open");
    assert_eq!(bits & MANAGE, MANAGE);
    assert_eq!(bits & SEND, SEND);
}

#[test]
fn a_role_can_deny_something() {
    let store = server_store();
    let bits = channel_permissions(&store, "u-muted", "ch-open");
    assert_eq!(bits & SEND, 0);
    assert_eq!(bits & VIEW, VIEW);
}

#[test]
fn the_highest_ranked_role_wins_a_clash() {
    // `mod` (rank 1) allows managing messages and `rookie` (rank 4) denies
    // it: the higher-ranked role is applied last.
    let store = server_store();
    let bits = channel_permissions(&store, "u-rookie", "ch-open");
    assert_eq!(bits & MANAGE, MANAGE);
}

#[test]
fn a_channel_override_hides_it_from_everyone_but_a_role() {
    let store = server_store();
    assert!(!can_view(&store, "u-plain", "ch-staff"));
    assert_eq!(channel_permissions(&store, "u-plain", "ch-staff"), 0);
    assert!(can_view(&store, "u-staff", "ch-staff"));
    assert!(can_view(&store, "u-plain", "ch-open"));
}

#[test]
fn a_channel_override_can_hide_it_from_one_role() {
    let store = server_store();
    assert!(can_view(&store, "u-plain", "ch-no-rookies"));
    assert!(!can_view(&store, "u-rookie", "ch-no-rookies"));
}

#[test]
fn a_timed_out_member_can_only_look() {
    let store = server_store();
    let bits = channel_permissions(&store, "u-timed", "ch-open");
    assert_eq!(bits, VIEW | ChannelPermission::ReadMessageHistory as u64);
    // Once the timeout is over they're back to normal.
    assert_eq!(
        channel_permissions(&store, "u-lapsed", "ch-open"),
        *DEFAULT_PERMISSION_SERVER
    );
}

#[test]
fn someone_who_is_not_a_member_has_nothing() {
    let store = server_store();
    assert_eq!(channel_permissions(&store, "u-stranger", "ch-open"), 0);
    assert!(!can_view(&store, "u-stranger", "ch-open"));
}

#[test]
fn unknown_channels_give_nothing() {
    let store = server_store();
    assert_eq!(channel_permissions(&store, "u-plain", "ch-nope"), 0);
}

fn conversations() -> ChatStore {
    let channels = json!([
        { "channel_type": "SavedMessages", "_id": "ch-saved", "user": "u-me" },
        { "channel_type": "SavedMessages", "_id": "ch-saved-other", "user": "u-friend" },
        { "channel_type": "DirectMessage", "_id": "dm-friend", "active": true, "recipients": ["u-me", "u-friend"] },
        { "channel_type": "DirectMessage", "_id": "dm-blocked", "active": true, "recipients": ["u-me", "u-blocked"] },
        { "channel_type": "DirectMessage", "_id": "dm-other", "active": true, "recipients": ["u-friend", "u-blocked"] },
        {
            "channel_type": "Group", "_id": "grp", "name": "Group", "owner": "u-friend",
            "recipients": ["u-me", "u-friend"]
        },
        {
            "channel_type": "Group", "_id": "grp-quiet", "name": "Quiet", "owner": "u-friend",
            "recipients": ["u-me", "u-friend"], "permissions": VIEW | ChannelPermission::ReadMessageHistory as u64
        },
        {
            "channel_type": "Group", "_id": "grp-mine", "name": "Mine", "owner": "u-me",
            "recipients": ["u-me", "u-friend"], "permissions": VIEW
        }
    ]);
    store_with(
        channels,
        vec![
            user("u-friend", json!({ "relationship": "Friend" })),
            user("u-blocked", json!({ "relationship": "Blocked" })),
        ],
        vec![],
    )
}

#[test]
fn saved_notes_are_all_yours_and_nobody_elses() {
    let store = conversations();
    let bits = channel_permissions(&store, "u-me", "ch-saved");
    assert!(has(bits, ChannelPermission::ManageMessages));
    assert!(has(bits, ChannelPermission::SendMessage));
    assert_eq!(channel_permissions(&store, "u-me", "ch-saved-other"), 0);
}

#[test]
fn a_dm_with_a_friend_can_be_written_in() {
    let store = conversations();
    assert_eq!(
        channel_permissions(&store, "u-me", "dm-friend"),
        *DEFAULT_PERMISSION_DIRECT_MESSAGE
    );
}

#[test]
fn a_dm_with_someone_you_blocked_is_view_only() {
    let store = conversations();
    let bits = channel_permissions(&store, "u-me", "dm-blocked");
    assert!(has(bits, ChannelPermission::ViewChannel));
    assert!(!has(bits, ChannelPermission::SendMessage));
}

#[test]
fn a_dm_you_are_not_in_gives_nothing() {
    let store = conversations();
    assert_eq!(channel_permissions(&store, "u-me", "dm-other"), 0);
}

#[test]
fn a_group_gives_its_members_the_groups_permissions() {
    let store = conversations();
    // Unset: the usual DM permissions, which let you write but not manage.
    let bits = channel_permissions(&store, "u-me", "grp");
    assert!(has(bits, ChannelPermission::SendMessage));
    assert!(!has(bits, ChannelPermission::ManageMessages));
    // Set to look-only.
    let bits = channel_permissions(&store, "u-me", "grp-quiet");
    assert!(has(bits, ChannelPermission::ViewChannel));
    assert!(!has(bits, ChannelPermission::SendMessage));
    // Its owner can do everything, whatever it says.
    let bits = channel_permissions(&store, "u-me", "grp-mine");
    assert!(has(bits, ChannelPermission::ManageMessages));
    assert!(has(bits, ChannelPermission::SendMessage));
    // Not in the group: nothing.
    assert_eq!(channel_permissions(&store, "u-blocked", "grp"), 0);
}

#[test]
fn only_those_who_manage_messages_can_pin_in_a_server() {
    let store = server_store();
    assert!(can_pin(&store, "u-owner", "ch-open"));
    assert!(can_pin(&store, "u-mod", "ch-open"));
    // `rookie` also holds `mod`, which outranks the role that denies it.
    assert!(can_pin(&store, "u-rookie", "ch-open"));
    assert!(!can_pin(&store, "u-plain", "ch-open"));
    assert!(!can_pin(&store, "u-staff", "ch-open"));
    assert!(!can_pin(&store, "u-timed", "ch-open"));
    assert!(!can_pin(&store, "u-stranger", "ch-open"));
}

#[test]
fn a_channel_override_can_take_pinning_away() {
    let channels = json!([
        { "channel_type": "TextChannel", "_id": "ch-open", "server": "srv", "name": "open" },
        {
            "channel_type": "TextChannel", "_id": "ch-no-pins", "server": "srv", "name": "no-pins",
            "role_permissions": { "mod": { "a": 0, "d": MANAGE } }
        }
    ]);
    let store = store_with(
        channels,
        vec![user("u-mod", json!({}))],
        vec![
            member("u-me", &[], json!({})),
            member("u-mod", &["mod"], json!({})),
        ],
    );
    assert!(can_pin(&store, "u-mod", "ch-open"));
    assert!(!can_pin(&store, "u-mod", "ch-no-pins"));
}

#[test]
fn the_user_can_pin_in_a_dm_and_in_their_saved_notes() {
    let store = conversations();
    // Either person in a DM may, whatever the DM's permissions.
    assert!(can_pin(&store, "u-me", "dm-friend"));
    assert!(can_pin(&store, "u-me", "dm-blocked"));
    assert!(can_pin(&store, "u-me", "ch-saved"));
    // Not someone else's notes.
    assert!(!can_pin(&store, "u-me", "ch-saved-other"));
    assert!(!can_pin(&store, "u-me", "ch-nope"));
}

#[test]
fn a_group_owner_can_pin_but_a_plain_member_cannot() {
    let store = conversations();
    assert!(can_pin(&store, "u-me", "grp-mine"));
    assert!(!can_pin(&store, "u-me", "grp"));
    assert!(!can_pin(&store, "u-me", "grp-quiet"));
}

#[test]
fn the_store_asks_about_the_logged_in_user() {
    let store = conversations();
    assert!(store.can_pin("dm-friend"));
    assert!(store.can_pin("grp-mine"));
    assert!(!store.can_pin("grp"));
    assert!(!store.can_pin("ch-nope"));
    // `u-me` is a plain member of the server channels.
    let store = server_store();
    assert!(!store.can_pin("ch-open"));
}

const MENTION_ROLES: u64 = ChannelPermission::MentionRoles as u64;
const MENTION_EVERYONE: u64 = ChannelPermission::MentionEveryone as u64;

#[test]
fn mentioning_roles_and_everyone_takes_their_permissions() {
    let mut server = server_json();
    server["roles"]["herald"] = json!({
        "_id": "herald", "name": "Herald", "rank": 0,
        "permissions": { "a": MENTION_ROLES | MENTION_EVERYONE, "d": 0 }
    });
    let channels = json!([
        { "channel_type": "TextChannel", "_id": "ch-open", "server": "srv", "name": "open" },
        {
            "channel_type": "TextChannel", "_id": "ch-quiet", "server": "srv", "name": "quiet",
            "role_permissions": { "herald": { "a": 0, "d": MENTION_ROLES | MENTION_EVERYONE } }
        }
    ]);
    let ready = json!({
        "type": "Ready",
        "users": [user("u-me", json!({})), user("u-owner", json!({})), user("u-herald", json!({}))],
        "servers": [server],
        "channels": channels,
        "members": [
            member("u-me", &[], json!({})),
            member("u-owner", &[], json!({})),
            member("u-herald", &["herald"], json!({})),
        ],
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);

    // A plain member hasn't the bits by default.
    assert!(!can_mention_roles(&store, "u-me", "ch-open"));
    assert!(!can_mention_everyone(&store, "u-me", "ch-open"));
    assert!(!store.can_mention_roles("ch-open"));
    // A role can grant them, and a channel override take them away.
    assert!(can_mention_roles(&store, "u-herald", "ch-open"));
    assert!(can_mention_everyone(&store, "u-herald", "ch-open"));
    assert!(!can_mention_roles(&store, "u-herald", "ch-quiet"));
    assert!(!can_mention_everyone(&store, "u-herald", "ch-quiet"));
    // The owner may, everywhere.
    for channel in ["ch-open", "ch-quiet"] {
        assert!(can_mention_roles(&store, "u-owner", channel), "{channel}");
        assert!(
            can_mention_everyone(&store, "u-owner", channel),
            "{channel}"
        );
    }
}

#[test]
fn nobody_mentions_roles_or_everyone_outside_a_server() {
    let store = conversations();
    for channel in ["dm-friend", "grp", "saved"] {
        assert!(!can_mention_roles(&store, "u-me", channel), "{channel}");
        assert!(!can_mention_everyone(&store, "u-me", channel), "{channel}");
    }
}

const BYPASS_SLOWMODE: u64 = ChannelPermission::BypassSlowmode as u64;

#[test]
fn bypassing_slowmode_takes_its_own_permission() {
    let mut server = server_json();
    server["roles"]["regular"] = json!({
        "_id": "regular", "name": "Regular", "rank": 0,
        "permissions": { "a": BYPASS_SLOWMODE, "d": 0 }
    });
    let channels = json!([
        { "channel_type": "TextChannel", "_id": "ch-open", "server": "srv", "name": "open", "slowmode": 30 },
        {
            "channel_type": "TextChannel", "_id": "ch-strict", "server": "srv", "name": "strict", "slowmode": 30,
            "role_permissions": { "regular": { "a": 0, "d": BYPASS_SLOWMODE } }
        }
    ]);
    let ready = json!({
        "type": "Ready",
        "users": [
            user("u-me", json!({})), user("u-owner", json!({})),
            user("u-regular", json!({})), user("u-mod", json!({}))
        ],
        "servers": [server],
        "channels": channels,
        "members": [
            member("u-me", &[], json!({})),
            member("u-owner", &[], json!({})),
            member("u-regular", &["regular"], json!({})),
            member("u-mod", &["mod"], json!({})),
        ],
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);

    // The owner has every permission, so is exempt everywhere.
    assert!(can_bypass_slowmode(&store, "u-owner", "ch-open"));
    assert!(can_bypass_slowmode(&store, "u-owner", "ch-strict"));
    // A role can grant it, and a channel override take it away.
    assert!(can_bypass_slowmode(&store, "u-regular", "ch-open"));
    assert!(!can_bypass_slowmode(&store, "u-regular", "ch-strict"));
    // A plain member waits, and so does one who may only manage messages.
    assert!(!can_bypass_slowmode(&store, "u-me", "ch-open"));
    assert!(!store.can_bypass_slowmode("ch-open"));
    assert!(!can_bypass_slowmode(&store, "u-mod", "ch-open"));
}

#[test]
fn nobody_bypasses_slowmode_outside_a_server() {
    let store = conversations();
    for channel in ["dm-friend", "grp-mine", "ch-saved"] {
        assert!(!can_bypass_slowmode(&store, "u-me", channel), "{channel}");
    }
}

const EMBEDS: u64 = ChannelPermission::SendEmbeds as u64;

#[test]
fn sending_embeds_follows_its_permission() {
    let channels = json!([
        { "channel_type": "TextChannel", "_id": "ch-open", "server": "srv", "name": "open" },
        {
            "channel_type": "TextChannel", "_id": "ch-plain-text", "server": "srv", "name": "plain-text",
            "default_permissions": { "a": 0, "d": EMBEDS }
        }
    ]);
    let store = store_with(
        channels,
        vec![user("u-owner", json!({}))],
        vec![
            member("u-me", &[], json!({})),
            member("u-owner", &[], json!({})),
        ],
    );
    // Members may by default, unless the channel takes it away.
    assert!(can_send_embeds(&store, "u-me", "ch-open"));
    assert!(store.can_send_embeds("ch-open"));
    assert!(!can_send_embeds(&store, "u-me", "ch-plain-text"));
    assert!(!store.can_send_embeds("ch-plain-text"));
    // The owner always may.
    assert!(can_send_embeds(&store, "u-owner", "ch-plain-text"));
    // In a DM with a friend, and the user's saved notes.
    let store = conversations();
    assert!(can_send_embeds(&store, "u-me", "dm-friend"));
    assert!(can_send_embeds(&store, "u-me", "ch-saved"));
}

#[test]
fn the_owner_can_change_their_nickname_and_avatar() {
    let store = server_store();
    assert_eq!(
        server_permissions(&store, "u-owner", "srv"),
        ChannelPermission::GrantAllSafe as u64
    );
    assert!(can_change_nickname(&store, "u-owner", "srv"));
    assert!(can_change_avatar(&store, "u-owner", "srv"));
}

#[test]
fn stoats_default_server_permissions_allow_a_nickname_and_avatar() {
    let store = server_store();
    assert_eq!(
        server_permissions(&store, "u-plain", "srv"),
        *DEFAULT_PERMISSION_SERVER
    );
    assert!(can_change_nickname(&store, "u-plain", "srv"));
    assert!(can_change_avatar(&store, "u-plain", "srv"));
}

#[test]
fn a_role_can_deny_a_nickname_and_avatar() {
    let store = server_store();
    assert!(!can_change_nickname(&store, "u-anon", "srv"));
    assert!(!can_change_avatar(&store, "u-anon", "srv"));
    // Other permissions stay.
    assert!(has(
        server_permissions(&store, "u-anon", "srv"),
        ChannelPermission::SendMessage
    ));
}

#[test]
fn outside_a_server_there_is_nothing_to_change() {
    let store = server_store();
    assert_eq!(server_permissions(&store, "u-stranger", "srv"), 0);
    assert!(!can_change_nickname(&store, "u-stranger", "srv"));
    assert_eq!(server_permissions(&store, "u-plain", "srv-unknown"), 0);
}
