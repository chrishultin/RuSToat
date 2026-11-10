//! The permission matrix: every permission Stoat has, listed once and
//! grouped, and a role's setting for each.

use std::collections::HashSet;

use revolt_permissions::{ChannelPermission, Override, OverrideField};
use revolt_result::ErrorType;
use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::permission_matrix::{
    PERMISSIONS, PermissionEntry, Setting, default_change, follow_value, missing_from_default,
    missing_note, override_of, permissions_error_text, setting_of, value_enabled, with_setting,
};

/// Every single-bit permission `revolt-permissions` names.
fn every_permission() -> Vec<ChannelPermission> {
    (0..64)
        .filter_map(|n| ChannelPermission::try_from(1u64 << n).ok())
        .collect()
}

#[test]
fn every_permission_is_listed_exactly_once() {
    let known = every_permission();
    assert_eq!(known.len(), 35);
    let listed: Vec<ChannelPermission> = PERMISSIONS.iter().map(|entry| entry.permission).collect();
    for permission in &known {
        let times = listed.iter().filter(|listed| *listed == permission).count();
        assert_eq!(times, 1, "{permission:?} is listed {times} times");
    }
    for permission in &listed {
        assert!(
            known.contains(permission),
            "{permission:?} isn't a single permission"
        );
    }
    for entry in &PERMISSIONS {
        assert_eq!(entry.bit().count_ones(), 1, "{}", entry.title);
    }
}

#[test]
fn the_two_the_web_client_hides_are_listed() {
    let titles: Vec<&str> = PERMISSIONS.iter().map(|entry| entry.title).collect();
    assert!(titles.contains(&"View Audit Logs"));
    assert!(titles.contains(&"Use External Emojis"));
}

#[test]
fn headings_start_each_group_in_order() {
    let headings: Vec<&str> = PERMISSIONS
        .iter()
        .filter_map(|entry| entry.heading)
        .collect();
    assert_eq!(
        headings,
        [
            "Admin",
            "Members",
            "Channels",
            "Messaging",
            "Voice",
            "Mentions"
        ]
    );
    assert_eq!(PERMISSIONS[0].heading, Some("Admin"));
    let starts = |title: &str| {
        PERMISSIONS
            .iter()
            .find(|entry| entry.title == title)
            .and_then(|entry| entry.heading)
    };
    assert_eq!(starts("Kick Members"), Some("Members"));
    assert_eq!(starts("View Channels"), Some("Channels"));
    assert_eq!(starts("Send Embeds"), Some("Messaging"));
    assert_eq!(starts("Connect"), Some("Voice"));
    assert_eq!(starts("Mention Everyone"), Some("Mentions"));
    assert_eq!(starts("Send Messages"), None);
}

#[test]
fn every_entry_is_described_and_titles_are_unique() {
    let mut titles = HashSet::new();
    for entry in &PERMISSIONS {
        assert!(!entry.server_description.is_empty(), "{}", entry.title);
        // Channel descriptions come with channel overrides (milestone 27).
        assert_eq!(entry.channel_description, None, "{}", entry.title);
        assert!(titles.insert(entry.title), "{} twice", entry.title);
    }
}

const SEND: u64 = ChannelPermission::SendMessage as u64;
const REACT: u64 = ChannelPermission::React as u64;

#[test]
fn a_setting_reads_from_allow_and_deny() {
    let value = Override {
        allow: SEND,
        deny: REACT,
    };
    assert_eq!(setting_of(&value, SEND), Setting::Allow);
    assert_eq!(setting_of(&value, REACT), Setting::Deny);
    assert_eq!(
        setting_of(&value, ChannelPermission::Video as u64),
        Setting::Neutral
    );
    // Both: Stoat applies the allow, then the deny.
    let both = Override {
        allow: SEND,
        deny: SEND,
    };
    assert_eq!(setting_of(&both, SEND), Setting::Deny);
}

#[test]
fn with_setting_round_trips_and_clears_the_other_side() {
    let start = Override {
        allow: SEND | REACT,
        deny: 0,
    };
    for setting in Setting::ALL {
        let set = with_setting(start.clone(), SEND, setting);
        assert_eq!(setting_of(&set, SEND), setting, "{setting:?}");
        // The other permission is untouched.
        assert_eq!(setting_of(&set, REACT), Setting::Allow, "{setting:?}");
        assert_eq!(set.allow & set.deny, 0, "{setting:?}");
    }
    let denied = with_setting(start.clone(), SEND, Setting::Deny);
    assert_eq!(
        denied,
        Override {
            allow: REACT,
            deny: SEND
        }
    );
    let back = with_setting(denied, SEND, Setting::Allow);
    assert_eq!(back, start);
    let neutral = with_setting(back, SEND, Setting::Neutral);
    assert_eq!(
        neutral,
        Override {
            allow: REACT,
            deny: 0
        }
    );
}

#[test]
fn settings_order_from_strictest() {
    assert!(Setting::Deny < Setting::Neutral);
    assert!(Setting::Neutral < Setting::Allow);
    assert_eq!(
        Setting::ALL.map(Setting::label),
        ["Deny", "Neutral", "Allow"]
    );
}

#[test]
fn a_stored_override_reads_as_allow_and_deny() {
    let field = OverrideField {
        a: SEND as i64,
        d: REACT as i64,
    };
    assert_eq!(
        override_of(field),
        Override {
            allow: SEND,
            deny: REACT
        }
    );
}

const MANAGE_SERVER: u64 = ChannelPermission::ManageServer as u64;
const VIEW: u64 = ChannelPermission::ViewChannel as u64;

#[test]
fn everyone_may_lose_any_permission_but_gain_only_held_ones() {
    let held = SEND | VIEW;
    // Held: on and off.
    assert!(value_enabled(held, SEND, Setting::Allow));
    assert!(value_enabled(held, SEND, Setting::Neutral));
    // Not held: off, never on, whatever's stored.
    assert!(value_enabled(held, MANAGE_SERVER, Setting::Neutral));
    assert!(!value_enabled(held, MANAGE_SERVER, Setting::Allow));
}

fn titles(entries: &[&PermissionEntry]) -> Vec<&'static str> {
    entries.iter().map(|entry| entry.title).collect()
}

#[test]
fn missing_lists_only_unheld_permissions_that_are_on() {
    let held = SEND | VIEW;
    assert!(missing_from_default(held, SEND | VIEW).is_empty());
    assert!(missing_from_default(held, SEND).is_empty());
    // In list order: Manage Server before React.
    let missing = missing_from_default(held, REACT | SEND | MANAGE_SERVER);
    assert_eq!(titles(&missing), ["Manage Server", "React"]);
    // The owner holds everything.
    assert!(missing_from_default(u64::MAX, u64::MAX).is_empty());
}

#[test]
fn the_missing_note_names_each_permission() {
    assert_eq!(missing_note(&[]), None);
    let one = missing_from_default(0, MANAGE_SERVER);
    assert_eq!(
        missing_note(&one).unwrap(),
        "You don't have Manage Server, so Everyone can't keep it. Turn it off to save."
    );
    let two = missing_from_default(0, MANAGE_SERVER | REACT);
    assert_eq!(
        missing_note(&two).unwrap(),
        "You don't have Manage Server and React, so Everyone can't keep them. Turn them off \
         to save."
    );
    let three = missing_from_default(0, MANAGE_SERVER | VIEW | REACT);
    assert_eq!(
        missing_note(&three).unwrap(),
        "You don't have Manage Server, View Channels and React, so Everyone can't keep them. \
         Turn them off to save."
    );
}

#[test]
fn an_unchanged_default_sends_nothing() {
    assert_eq!(default_change(SEND | VIEW, SEND | VIEW), None);
    assert_eq!(default_change(SEND | VIEW, VIEW), Some(VIEW));
}

#[test]
fn untouched_bits_follow_and_edited_ones_stay() {
    let filled = SEND | VIEW;
    // The user turned Send off.
    let edited = VIEW;
    // Elsewhere, React was turned on and View off.
    let now = SEND | REACT;
    assert_eq!(follow_value(filled, edited, now), REACT);
    // Nothing edited: the matrix is the new value.
    assert_eq!(follow_value(filled, filled, now), now);
}

fn api_error(body: Value) -> ApiError {
    ApiError::from_response(reqwest::StatusCode::FORBIDDEN, body.to_string().as_bytes())
}

#[test]
fn refusals_read_plainly() {
    let text = |body: Value| permissions_error_text(&api_error(body));
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
        "You don't have permission to manage permissions."
    );
    assert_eq!(
        text(json!({ "type": "NotFound" })),
        "That role was deleted."
    );
    assert_eq!(
        text(json!({ "type": "InternalError" })),
        "Couldn't save the permissions."
    );
    assert!(matches!(
        api_error(json!({ "type": "CannotGiveMissingPermissions" })).error_type(),
        Some(ErrorType::CannotGiveMissingPermissions)
    ));
}
