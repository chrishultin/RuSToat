//! A channel's permissions: who may change them, the list of overrides,
//! the stored values, and the rank rule.

use revolt_permissions::PermissionValue;
use revolt_permissions::{
    ChannelPermission, DEFAULT_PERMISSION_DIRECT_MESSAGE, DEFAULT_PERMISSION_SERVER, Override,
};
use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::channel_permissions::{
    OverrideRow, OverrideTarget, can_manage_channel_permissions, group_lock_note,
    group_permissions, may_edit_group_permissions, may_edit_override, override_lock_notes,
    override_rows, stored_override,
};
use stoat_gpui::channel_permissions::{
    channel_permissions_error_text, grant_basis, ungrantable, ungrantable_note,
};
use stoat_gpui::channel_settings::{ChannelAccess, access};
use stoat_gpui::permission_matrix::{Setting, may_grant, setting_enabled, with_setting};
use stoat_gpui::store::ChatStore;

const MANAGE_PERMISSIONS: u64 = ChannelPermission::ManagePermissions as u64;
const SEND: u64 = ChannelPermission::SendMessage as u64;
const REACT: u64 = ChannelPermission::React as u64;
const PIN: u64 = ChannelPermission::ManageMessages as u64;
const WEBHOOKS: u64 = ChannelPermission::ManageWebhooks as u64;

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
/// Permissions), `low` (2; allowed Manage Messages and denied Manage
/// Webhooks server-wide, neither of which `mod` has) and `plain` (3). `u-mod` has `mod`; `u-local`
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
                "low": { "_id": "low", "name": "Low", "rank": 2, "permissions": { "a": PIN, "d": WEBHOOKS } },
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

#[test]
fn everyones_basis_is_the_stored_default_or_neutral() {
    let store = store("u-owner");
    assert_eq!(
        grant_basis(&store, "ch-a", &OverrideTarget::Everyone),
        Some(Override {
            allow: 0,
            deny: SEND
        })
    );
    assert_eq!(
        grant_basis(&store, "ch-b", &OverrideTarget::Everyone),
        Some(Override::default())
    );
    // Only a server channel has one.
    assert_eq!(grant_basis(&store, "grp", &OverrideTarget::Everyone), None);
    assert_eq!(grant_basis(&store, "nope", &OverrideTarget::Everyone), None);
}

/// Stoat's own check, from `revolt-permissions`, with `current` as Stoat
/// stores it (`None` when the channel has no default override).
fn stoat_grants(held: u64, current: Option<Override>, edited: &Override) -> bool {
    futures::executor::block_on(
        PermissionValue::from(held).throw_permission_override(current, edited),
    )
    .is_ok()
}

/// Every override of `bits`, each Deny, Neutral or Allow.
fn every_override(bits: &[u64]) -> Vec<Override> {
    bits.iter().fold(vec![Override::default()], |values, bit| {
        values
            .into_iter()
            .flat_map(|value| {
                Setting::ALL.map(|setting| with_setting(value.clone(), *bit, setting))
            })
            .collect()
    })
}

/// Each subset of `bits`, as held permissions.
fn every_held(bits: &[u64]) -> Vec<u64> {
    (0..1u64 << bits.len())
        .map(|subset| {
            bits.iter()
                .enumerate()
                .filter(|(n, _)| subset & (1 << n) != 0)
                .fold(0, |held, (_, bit)| held | bit)
        })
        .collect()
}

#[test]
fn everyones_rule_is_stoats_with_and_without_a_stored_default() {
    let bits = [SEND, REACT, PIN];
    let overrides = every_override(&bits);
    for held in every_held(&bits) {
        for edited in &overrides {
            // No stored default: the basis is all neutral.
            assert_eq!(
                may_grant(held, &Override::default(), edited),
                stoat_grants(held, None, edited),
                "held {held:#x}, none -> {edited:?}"
            );
            for stored in &overrides {
                assert_eq!(
                    may_grant(held, stored, edited),
                    stoat_grants(held, Some(stored.clone()), edited),
                    "held {held:#x}, {stored:?} -> {edited:?}"
                );
            }
        }
    }
}

#[test]
fn each_option_enabled_for_everyone_is_one_stoat_takes() {
    let bits = [SEND, REACT];
    for held in every_held(&bits) {
        for stored in every_override(&bits) {
            for bit in bits {
                for setting in Setting::ALL {
                    let edited = with_setting(stored.clone(), bit, setting);
                    if setting_enabled(held, &stored, bit, setting) {
                        assert!(
                            stoat_grants(held, Some(stored.clone()), &edited),
                            "held {held:#x}, {stored:?}: {bit:#x} to {setting:?}"
                        );
                    }
                }
            }
        }
    }
}

fn api_error(body: Value) -> ApiError {
    ApiError::from_response(
        reqwest::StatusCode::BAD_REQUEST,
        body.to_string().as_bytes(),
    )
}

#[test]
fn refusals_read_plainly() {
    let text = |body| channel_permissions_error_text(&api_error(body));
    assert_eq!(
        text(json!({ "type": "CannotGiveMissingPermissions" })),
        "You can't grant permissions you don't have."
    );
    assert_eq!(
        text(json!({ "type": "NotElevated" })),
        "That role ranks at or above your highest role."
    );
    assert_eq!(
        text(json!({ "type": "MissingPermission", "permission": "ManagePermissions" })),
        "You don't have permission to change this channel's permissions."
    );
    assert_eq!(
        text(json!({ "type": "NotFound" })),
        "That role or channel was deleted."
    );
    assert_eq!(
        text(json!({ "type": "InvalidOperation" })),
        "Stoat can't set that here."
    );
    assert_eq!(
        text(json!({ "type": "InternalError" })),
        "Couldn't save the permissions."
    );
}

#[test]
fn a_roles_basis_is_its_server_wide_permissions() {
    let store = store("u-owner");
    // Not its override in the channel (which allows Send and React, and
    // denies Manage Messages).
    assert_eq!(
        grant_basis(&store, "ch-a", &role("low")),
        Some(Override {
            allow: PIN,
            deny: WEBHOOKS
        })
    );
    assert_eq!(
        grant_basis(&store, "ch-a", &role("mod")),
        Some(Override {
            allow: MANAGE_PERMISSIONS,
            deny: 0
        })
    );
    assert_eq!(grant_basis(&store, "ch-a", &role("gone")), None);
}

/// The options enabled for `bit` in `low`'s override in `ch-a`, as the
/// moderator.
fn moderators_options(bit: u64) -> Vec<Setting> {
    let store = store("u-mod");
    let held = stoat_gpui::channel_permissions::held(&store, "ch-a");
    let basis = grant_basis(&store, "ch-a", &role("low")).unwrap();
    Setting::ALL
        .into_iter()
        .filter(|setting| setting_enabled(held, &basis, bit, *setting))
        .collect()
}

#[test]
fn an_unheld_permission_the_role_has_server_wide_may_be_allowed() {
    let held = stoat_gpui::channel_permissions::held(&store("u-mod"), "ch-a");
    assert_eq!(held & PIN, 0, "the moderator doesn't have Manage Messages");
    // Stored here as denied, but allowed for the role server-wide: Stoat
    // takes any setting.
    assert_eq!(
        moderators_options(PIN),
        [Setting::Deny, Setting::Neutral, Setting::Allow]
    );
    // From an override that's otherwise grantable (Manage Webhooks denied,
    // as server-wide).
    let basis = Override {
        allow: PIN,
        deny: WEBHOOKS,
    };
    let start = Override {
        allow: 0,
        deny: WEBHOOKS,
    };
    let allowed = with_setting(start, PIN, Setting::Allow);
    assert!(may_grant(held, &basis, &allowed));
    assert!(stoat_grants(held, Some(basis), &allowed));
}

#[test]
fn an_unheld_permission_the_role_is_denied_server_wide_stays_denied() {
    let held = stoat_gpui::channel_permissions::held(&store("u-mod"), "ch-a");
    assert_eq!(
        held & WEBHOOKS,
        0,
        "the moderator doesn't have Manage Webhooks"
    );
    // Neutral in the channel, but the role is denied it server-wide: only
    // Deny is enabled, and Stoat refuses neutral.
    assert_eq!(moderators_options(WEBHOOKS), [Setting::Deny]);
    let basis = Override {
        allow: PIN,
        deny: WEBHOOKS,
    };
    let neutral = Override::default();
    assert!(!may_grant(held, &basis, &neutral));
    assert!(!stoat_grants(held, Some(basis.clone()), &neutral));
    let denied = with_setting(neutral, WEBHOOKS, Setting::Deny);
    assert!(may_grant(held, &basis, &denied));
    assert!(stoat_grants(held, Some(basis), &denied));
}

#[test]
fn a_saved_role_override_then_its_event_are_one_state() {
    // `u-local` has `plain`; an override allowing Manage Messages lets them
    // pin in `ch-a` at once.
    let mut store = store("u-local");
    assert!(!stoat_gpui::permissions::can_pin(&store, "u-local", "ch-a"));
    let overrides = json!({
        "low": { "a": SEND | REACT, "d": PIN },
        "top": { "a": 0, "d": 0 },
        "plain": { "a": PIN, "d": 0 }
    });
    let saved: stoat_gpui::api::models::Channel = serde_json::from_value(json!({
        "channel_type": "TextChannel", "_id": "ch-a", "server": "srv", "name": "a",
        "default_permissions": { "a": 0, "d": SEND },
        "role_permissions": overrides
    }))
    .unwrap();
    assert!(store.apply_channel_permissions(&saved));
    assert!(stoat_gpui::permissions::can_pin(&store, "u-local", "ch-a"));
    let after_save = store.channel("ch-a").cloned();

    let frame = json!({
        "type": "ChannelUpdate", "id": "ch-a",
        "data": { "role_permissions": overrides }, "clear": []
    });
    let Some(ServerEvent::ChannelUpdate { id, data, clear }) =
        parse_frame(&frame.to_string()).pop()
    else {
        panic!("ChannelUpdate");
    };
    store.apply_channel_update(&id, *data, &clear);
    assert_eq!(store.channel("ch-a").cloned(), after_save);
}

#[test]
fn a_stored_override_may_itself_be_ungrantable_and_says_why() {
    // The moderator lacks Send Messages in `ch-a` (Everyone is denied it
    // there) and Manage Webhooks. `low`'s override allows the first, which
    // the role doesn't have server-wide, and leaves the second neutral,
    // where the role is denied it server-wide: Stoat would refuse it as it
    // is, whichever row the moderator changes.
    let store = store("u-mod");
    let held = stoat_gpui::channel_permissions::held(&store, "ch-a");
    let basis = grant_basis(&store, "ch-a", &role("low")).unwrap();
    let stored = stored_override(store.channel("ch-a").unwrap(), &role("low"));
    let blocking = ungrantable(held, &basis, &stored);
    let titles: Vec<&str> = blocking.iter().map(|entry| entry.title).collect();
    assert_eq!(titles, ["Send Messages", "Manage Webhooks"]);
    assert!(!stoat_grants(held, Some(basis.clone()), &stored));
    assert_eq!(
        ungrantable_note(&role("low"), &blocking).as_deref(),
        Some(
            "You don't have Send Messages and Manage Webhooks, so this role's override can't \
             set them looser than the role's server-wide permissions do. Make them stricter \
             to save."
        )
    );
    assert_eq!(ungrantable_note(&role("low"), &[]), None);
    // Once both are stricter, it's fine.
    let fixed = with_setting(
        with_setting(stored, SEND, Setting::Neutral),
        WEBHOOKS,
        Setting::Deny,
    );
    assert!(ungrantable(held, &basis, &fixed).is_empty());
    assert!(stoat_grants(held, Some(basis), &fixed));
}

#[test]
fn nothing_blocks_exactly_when_stoat_grants() {
    let bits = [SEND, REACT, PIN];
    let overrides = every_override(&bits);
    for held in every_held(&bits) {
        for basis in &overrides {
            for edited in &overrides {
                assert_eq!(
                    ungrantable(held, basis, edited).is_empty(),
                    stoat_grants(held, Some(basis.clone()), edited),
                    "held {held:#x}, {basis:?} -> {edited:?}"
                );
            }
        }
    }
}
