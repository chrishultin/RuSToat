//! A channel's permissions: who may change them, the list of overrides,
//! the stored values, and the rank rule.

use revolt_permissions::{
    ChannelPermission, DEFAULT_PERMISSION_DIRECT_MESSAGE, DEFAULT_PERMISSION_SERVER, Override,
};
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::channel_permissions::{
    OverrideRow, OverrideTarget, can_manage_channel_permissions, group_lock_note,
    group_permissions, may_edit_group_permissions, may_edit_override, override_lock_notes,
    override_rows, stored_override,
};
use stoat_gpui::channel_settings::{ChannelAccess, access};
use stoat_gpui::store::ChatStore;

const MANAGE_PERMISSIONS: u64 = ChannelPermission::ManagePermissions as u64;
const SEND: u64 = ChannelPermission::SendMessage as u64;
const REACT: u64 = ChannelPermission::React as u64;
const PIN: u64 = ChannelPermission::ManageMessages as u64;

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

/// `srv`, owned by `u-owner`. Roles by rank: `top` (0), `mod` (1, Manage
/// Permissions), `low` (2) and `plain` (3). `u-mod` has `mod`; `u-local`
/// has `plain`, which `ch-b`'s override gives Manage Permissions;
/// `u-plain` has nothing. `ch-a` has overrides for Everyone, `low` and
/// (all neutral) `top`. Groups: `grp` (no stored permissions) and `strict`
/// (members may only send), both owned by `u-owner`.
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
                "top": { "_id": "top", "name": "Top", "rank": 0, "colour": "#ff0000", "permissions": { "a": 0, "d": 0 } },
                "mod": { "_id": "mod", "name": "Mod", "rank": 1, "permissions": { "a": MANAGE_PERMISSIONS, "d": 0 } },
                "low": { "_id": "low", "name": "Low", "rank": 2, "permissions": { "a": 0, "d": 0 } },
                "plain": { "_id": "plain", "name": "Plain", "rank": 3, "permissions": { "a": 0, "d": 0 } }
            }
        }],
        "channels": [
            {
                "channel_type": "TextChannel", "_id": "ch-a", "server": "srv", "name": "a",
                "default_permissions": { "a": 0, "d": SEND },
                "role_permissions": {
                    "low": { "a": SEND | REACT, "d": PIN },
                    "top": { "a": 0, "d": 0 }
                }
            },
            {
                "channel_type": "TextChannel", "_id": "ch-b", "server": "srv", "name": "b",
                "role_permissions": { "plain": { "a": MANAGE_PERMISSIONS, "d": 0 } }
            },
            {
                "channel_type": "Group", "_id": "grp", "name": "Friends", "owner": "u-owner",
                "recipients": everyone
            },
            {
                "channel_type": "Group", "_id": "strict", "name": "Strict", "owner": "u-owner",
                "recipients": everyone, "permissions": SEND as i64
            },
            {
                "channel_type": "DirectMessage", "_id": "dm", "active": true,
                "recipients": [me, "u-owner"]
            }
        ],
        "members": [
            member("u-owner", &[]),
            member("u-mod", &["mod"]),
            member("u-local", &["plain"]),
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

fn role(id: &str) -> OverrideTarget {
    OverrideTarget::Role(id.into())
}

#[test]
fn manage_permissions_in_the_channel_is_what_counts() {
    // The owner and a server-wide holder, everywhere.
    for me in ["u-owner", "u-mod"] {
        let store = store(me);
        assert!(can_manage_channel_permissions(&store, "ch-a"), "{me}");
        assert!(can_manage_channel_permissions(&store, "ch-b"), "{me}");
    }
    // Through one channel's override only.
    let local = store("u-local");
    assert!(can_manage_channel_permissions(&local, "ch-b"));
    assert!(!can_manage_channel_permissions(&local, "ch-a"));
    // Nobody else, and never in a DM.
    let plain = store("u-plain");
    assert!(!can_manage_channel_permissions(&plain, "ch-a"));
    assert!(!can_manage_channel_permissions(&plain, "dm"));
}

#[test]
fn only_a_groups_owner_has_it_by_default() {
    assert!(can_manage_channel_permissions(&store("u-owner"), "grp"));
    assert!(may_edit_group_permissions(&store("u-owner"), "grp"));
    assert!(!can_manage_channel_permissions(&store("u-plain"), "grp"));
    assert!(!may_edit_group_permissions(&store("u-plain"), "grp"));
    assert_eq!(group_lock_note(&store("u-owner"), "grp"), None);
    assert!(group_lock_note(&store("u-plain"), "grp").is_some());
    // A server channel isn't a group.
    assert!(!may_edit_group_permissions(&store("u-owner"), "ch-a"));
}

#[test]
fn roles_split_by_whether_they_have_an_override_in_rank_order() {
    let (set, unset) = override_rows(&store("u-owner"), "ch-a");
    assert_eq!(
        set,
        [OverrideRow {
            role: "low".into(),
            name: "Low".into(),
            colour: None,
            allows: 2,
            denies: 1,
        }]
    );
    assert_eq!(set[0].summary(), "Allows 2, denies 1");
    // An all-neutral override is no override.
    let unset: Vec<&str> = unset.iter().map(|row| row.role.as_str()).collect();
    assert_eq!(unset, ["top", "mod", "plain"]);
    // Anything but a server channel has none.
    assert_eq!(override_rows(&store("u-owner"), "grp"), (vec![], vec![]));
}

#[test]
fn stored_overrides_read_as_allow_and_deny_or_neutral() {
    let store = store("u-owner");
    let a = store.channel("ch-a").unwrap();
    assert_eq!(
        stored_override(a, &OverrideTarget::Everyone),
        Override {
            allow: 0,
            deny: SEND
        }
    );
    assert_eq!(
        stored_override(a, &role("low")),
        Override {
            allow: SEND | REACT,
            deny: PIN
        }
    );
    assert_eq!(stored_override(a, &role("mod")), Override::default());
    let b = store.channel("ch-b").unwrap();
    assert_eq!(
        stored_override(b, &OverrideTarget::Everyone),
        Override::default()
    );
}

#[test]
fn a_group_without_stored_permissions_has_stoats_default() {
    let store = store("u-owner");
    assert_eq!(
        group_permissions(store.channel("grp").unwrap()),
        *DEFAULT_PERMISSION_DIRECT_MESSAGE
    );
    assert_eq!(group_permissions(store.channel("strict").unwrap()), SEND);
    assert_eq!(group_permissions(store.channel("ch-a").unwrap()), 0);
}

#[test]
fn a_roles_override_needs_it_to_rank_below_yours() {
    let owner = store("u-owner");
    for target in [
        OverrideTarget::Everyone,
        role("top"),
        role("mod"),
        role("low"),
    ] {
        assert!(may_edit_override(&owner, "ch-a", &target), "{target:?}");
    }
    let moderator = store("u-mod");
    assert!(may_edit_override(
        &moderator,
        "ch-a",
        &OverrideTarget::Everyone
    ));
    assert!(may_edit_override(&moderator, "ch-a", &role("low")));
    assert!(!may_edit_override(&moderator, "ch-a", &role("mod")));
    assert!(!may_edit_override(&moderator, "ch-a", &role("top")));
    // A role that's gone, or a channel that isn't a server's.
    assert!(!may_edit_override(&owner, "ch-a", &role("gone")));
    assert!(!may_edit_override(&owner, "grp", &OverrideTarget::Everyone));
    // Without Manage Permissions there, nothing.
    let plain = store("u-plain");
    assert!(!may_edit_override(
        &plain,
        "ch-a",
        &OverrideTarget::Everyone
    ));
}

#[test]
fn a_role_with_no_rank_edits_no_roles_override() {
    // `u-local` holds Manage Permissions in `ch-b` through `plain`, the
    // lowest role, so every role ranks at or above theirs.
    let local = store("u-local");
    assert!(may_edit_override(&local, "ch-b", &OverrideTarget::Everyone));
    assert!(!may_edit_override(&local, "ch-b", &role("plain")));
    assert!(!may_edit_override(&local, "ch-b", &role("low")));
}

#[test]
fn lock_notes_say_why() {
    assert!(override_lock_notes(&store("u-owner"), "ch-a", &role("top")).is_empty());
    assert_eq!(
        override_lock_notes(&store("u-mod"), "ch-a", &role("top")),
        ["This role ranks at or above your highest role."]
    );
    assert!(override_lock_notes(&store("u-mod"), "ch-a", &OverrideTarget::Everyone).is_empty());
    assert_eq!(
        override_lock_notes(&store("u-plain"), "ch-a", &OverrideTarget::Everyone),
        ["You don't have permission to change this channel's permissions."]
    );
}

#[test]
fn manage_permissions_alone_opens_only_the_permissions_page() {
    // `u-local` has Manage Permissions in `ch-b`, but not Manage Channels.
    let local = store("u-local");
    assert_eq!(
        access(&local, "ch-b"),
        ChannelAccess {
            overview: false,
            permissions: true
        }
    );
    assert!(!access(&local, "ch-a").any());
}
