//! The permission matrix: every permission Stoat has, listed once and
//! grouped, and a role's setting for each.

use std::collections::HashSet;

use revolt_permissions::{ChannelPermission, Override, OverrideField, PermissionValue};
use revolt_result::ErrorType;
use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::permission_matrix::{
    MatrixContext, PERMISSIONS, PermissionEntry, Setting, default_change, entries, follow_override,
    follow_value, may_grant, missing_from_default, missing_note, override_change, override_of,
    permissions_error_text, setting_enabled, setting_of, value_enabled, with_setting, with_value,
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
        for description in [entry.channel_description, entry.group_description]
            .into_iter()
            .flatten()
        {
            assert!(!description.is_empty(), "{}", entry.title);
        }
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

const VIDEO: u64 = ChannelPermission::Video as u64;

/// An override with each of `bits` set to its setting.
fn with(settings: &[(u64, Setting)]) -> Override {
    settings
        .iter()
        .fold(Override::default(), |value, (bit, setting)| {
            with_setting(value, *bit, *setting)
        })
}

#[test]
fn a_held_permission_offers_every_setting() {
    for stored in Setting::ALL {
        let stored = with(&[(SEND, stored)]);
        for setting in Setting::ALL {
            assert!(setting_enabled(SEND, &stored, SEND, setting));
        }
    }
}

#[test]
fn an_unheld_permission_offers_only_settings_no_looser_than_stored() {
    let enabled = |stored: Setting| {
        let stored = with(&[(SEND, stored)]);
        Setting::ALL
            .into_iter()
            .filter(|setting| setting_enabled(VIEW, &stored, SEND, *setting))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        enabled(Setting::Allow),
        [Setting::Deny, Setting::Neutral, Setting::Allow]
    );
    assert_eq!(enabled(Setting::Neutral), [Setting::Deny, Setting::Neutral]);
    assert_eq!(enabled(Setting::Deny), [Setting::Deny]);
}

/// Stoat's own check, from `revolt-permissions`.
fn stoat_grants(held: u64, stored: &Override, edited: &Override) -> bool {
    futures::executor::block_on(
        PermissionValue::from(held).throw_permission_override(stored.clone(), edited),
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

#[test]
fn may_grant_is_stoats_rule_for_every_case_over_three_permissions() {
    let bits = [SEND, REACT, VIDEO];
    let overrides = every_override(&bits);
    assert_eq!(overrides.len(), 27);
    for held in 0..8u64 {
        // Each of the three bits, held or not.
        let held = bits
            .iter()
            .enumerate()
            .filter(|(n, _)| held & (1 << n) != 0)
            .fold(0, |held, (_, bit)| held | bit);
        for stored in &overrides {
            for edited in &overrides {
                assert_eq!(
                    may_grant(held, stored, edited),
                    stoat_grants(held, stored, edited),
                    "held {held:#x}, {stored:?} -> {edited:?}"
                );
            }
        }
    }
}

#[test]
fn each_enabled_setting_is_one_stoat_grants() {
    let bits = [SEND, REACT];
    for held in [0, SEND, SEND | REACT] {
        for stored in every_override(&bits) {
            for bit in bits {
                for setting in Setting::ALL {
                    let edited = with_setting(stored.clone(), bit, setting);
                    assert_eq!(
                        setting_enabled(held, &stored, bit, setting),
                        stoat_grants(held, &stored, &edited),
                        "held {held:#x}, {stored:?}, {bit:#x} -> {setting:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn several_stricter_steps_are_granted_but_a_loosening_one_is_not() {
    let held = REACT;
    let stored = with(&[(SEND, Setting::Allow), (VIDEO, Setting::Neutral)]);
    // Send: Allow -> Deny; Video: Neutral -> Deny; React (held): -> Allow.
    let stricter = with(&[
        (SEND, Setting::Deny),
        (VIDEO, Setting::Deny),
        (REACT, Setting::Allow),
    ]);
    assert!(may_grant(held, &stored, &stricter));
    // Video, unheld, can't go from Neutral to Allow, even alongside others.
    let looser = with(&[(SEND, Setting::Deny), (VIDEO, Setting::Allow)]);
    assert!(!may_grant(held, &stored, &looser));
    // An unheld Deny can't be lifted.
    let denied = with(&[(VIDEO, Setting::Deny)]);
    assert!(!may_grant(held, &denied, &Override::default()));
}

#[test]
fn an_unchanged_override_sends_nothing() {
    let stored = with(&[(SEND, Setting::Allow)]);
    assert_eq!(override_change(&stored, &stored.clone()), None);
    let edited = with(&[(SEND, Setting::Deny)]);
    assert_eq!(override_change(&stored, &edited), Some(edited));
}

#[test]
fn untouched_settings_follow_and_edited_ones_stay() {
    let filled = with(&[(SEND, Setting::Allow), (REACT, Setting::Allow)]);
    // The user denied Send.
    let edited = with(&[(SEND, Setting::Deny), (REACT, Setting::Allow)]);
    // Elsewhere, Send was made neutral, React denied and Video allowed.
    let now = with(&[(REACT, Setting::Deny), (VIDEO, Setting::Allow)]);
    let followed = follow_override(&filled, &edited, &now);
    assert_eq!(setting_of(&followed, SEND), Setting::Deny);
    assert_eq!(setting_of(&followed, REACT), Setting::Deny);
    assert_eq!(setting_of(&followed, VIDEO), Setting::Allow);
    assert_eq!(follow_override(&filled, &filled, &now), now);
}

/// The permissions `context` shows, in order.
fn shown(context: MatrixContext) -> Vec<ChannelPermission> {
    entries(context)
        .iter()
        .map(|shown| shown.entry.permission)
        .collect()
}

/// The headings `context` shows, each with the permission it's above.
fn headings(context: MatrixContext) -> Vec<(&'static str, ChannelPermission)> {
    entries(context)
        .iter()
        .filter_map(|shown| Some((shown.heading?, shown.entry.permission)))
        .collect()
}

#[test]
fn the_server_context_is_every_permission_as_before() {
    let server = entries(MatrixContext::Server);
    assert_eq!(server.len(), PERMISSIONS.len());
    for (shown, entry) in server.iter().zip(&PERMISSIONS) {
        assert_eq!(shown.entry, entry);
        assert_eq!(shown.description, entry.server_description);
        assert_eq!(shown.heading, entry.heading);
    }
}

#[test]
fn a_channel_shows_the_23_that_mean_something_there() {
    use ChannelPermission::*;
    assert_eq!(
        shown(MatrixContext::Channel),
        [
            ManageChannel,
            ManagePermissions,
            ViewChannel,
            ReadMessageHistory,
            SendMessage,
            ManageMessages,
            ManageWebhooks,
            InviteOthers,
            SendEmbeds,
            UploadFiles,
            Masquerade,
            React,
            UseExternalEmojis,
            BypassSlowmode,
            Connect,
            Speak,
            Video,
            MuteMembers,
            DeafenMembers,
            MoveMembers,
            Listen,
            MentionEveryone,
            MentionRoles,
        ]
    );
    // Members has nothing there, so no heading.
    assert_eq!(
        headings(MatrixContext::Channel),
        [
            ("Admin", ManageChannel),
            ("Channels", ViewChannel),
            ("Messaging", SendEmbeds),
            ("Voice", Connect),
            ("Mentions", MentionEveryone),
        ]
    );
}

#[test]
fn a_group_shows_the_12_that_mean_something_there() {
    use ChannelPermission::*;
    assert_eq!(
        shown(MatrixContext::Group),
        [
            ManageChannel,
            ManagePermissions,
            SendMessage,
            ManageMessages,
            ManageWebhooks,
            InviteOthers,
            SendEmbeds,
            UploadFiles,
            Masquerade,
            React,
            MentionEveryone,
            MentionRoles,
        ]
    );
    // View Channel, which starts Channels, isn't shown in a group, so the
    // heading moves to Send Messages.
    assert_eq!(
        headings(MatrixContext::Group),
        [
            ("Admin", ManageChannel),
            ("Channels", SendMessage),
            ("Messaging", SendEmbeds),
            ("Mentions", MentionEveryone),
        ]
    );
}

#[test]
fn contexts_describe_permissions_their_own_way() {
    let describe = |context, permission| {
        entries(context)
            .into_iter()
            .find(|shown| shown.entry.permission == permission)
            .map(|shown| shown.description)
    };
    use ChannelPermission::*;
    assert_eq!(
        describe(MatrixContext::Channel, ViewChannel),
        Some("Able to access this channel")
    );
    assert_eq!(
        describe(MatrixContext::Group, InviteOthers),
        Some("Add new members to the group")
    );
    assert_eq!(
        describe(MatrixContext::Server, InviteOthers),
        Some("Create invites for others to use")
    );
    assert_eq!(describe(MatrixContext::Channel, KickMembers), None);
    assert_eq!(describe(MatrixContext::Group, UseExternalEmojis), None);
}

#[test]
fn a_checkbox_change_touches_only_its_bit() {
    let value = SEND | REACT;
    assert_eq!(with_value(value, SEND, Setting::Neutral), REACT);
    assert_eq!(with_value(value, SEND, Setting::Deny), REACT);
    assert_eq!(with_value(REACT, SEND, Setting::Allow), SEND | REACT);
    assert_eq!(with_value(value, SEND, Setting::Allow), value);
}
