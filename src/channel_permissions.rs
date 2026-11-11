//! A channel's permissions, as Channel Settings' Permissions page shows
//! them: a server channel's overrides (Everyone's, and each role's), or a
//! group's member permissions; who may change which; and what Stoat
//! compares an edit with. Plain data with no GPUI, so it's unit-testable.
//! The page is [`crate::ui::channel_permissions`].
//!
//! Stoat (0.16.2) checks a channel override with the same grant rule as a
//! role's ([`crate::permission_matrix::may_grant`]), the user's permissions
//! being theirs **in that channel**. Everyone's is compared with the
//! channel's stored default override; a role's is compared with **that
//! role's server-wide permissions**, not its override in the channel. A
//! group's member permissions have no grant check at all.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_DIRECT_MESSAGE, Override};

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::Channel;
use crate::permission_matrix::override_of;
use crate::permissions::{acting_rank, channel_permissions, has};
use crate::profile_edit::error_text;
use crate::roles::role_rows;
use crate::store::ChatStore;

/// Whose override in a server channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OverrideTarget {
    /// The channel's default override, for every member.
    Everyone,
    /// A role's override, by ID.
    Role(String),
}

/// One role in a channel's list of overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideRow {
    pub role: String,
    pub name: String,
    /// The role's colour as `#rrggbb`, if it's a solid colour.
    pub colour: Option<String>,
    /// How many permissions the override allows, and denies.
    pub allows: u32,
    pub denies: u32,
}

impl OverrideRow {
    /// "Allows 2, denies 1".
    pub fn summary(&self) -> String {
        format!("Allows {}, denies {}", self.allows, self.denies)
    }
}

/// Whether the user may change `channel`'s permissions: a server channel or
/// a group, with Manage Permissions in it (overrides count), as Stoat
/// checks.
pub fn can_manage_channel_permissions(store: &ChatStore, channel: &str) -> bool {
    matches!(
        store.channel(channel),
        Some(Channel::TextChannel { .. } | Channel::Group { .. })
    ) && has(
        channel_permissions(store, store.me(), channel),
        ChannelPermission::ManagePermissions,
    )
}

/// The permission bits the user has in `channel`.
pub fn held(store: &ChatStore, channel: &str) -> u64 {
    channel_permissions(store, store.me(), channel)
}

/// The server roles of a server channel, in rank order, split into those
/// with an override in it (any bit set) and those without. Empty for
/// anything else.
pub fn override_rows(store: &ChatStore, channel: &str) -> (Vec<OverrideRow>, Vec<OverrideRow>) {
    let Some(Channel::TextChannel {
        server,
        role_permissions,
        ..
    }) = store.channel(channel)
    else {
        return (Vec::new(), Vec::new());
    };
    role_rows(store, server)
        .into_iter()
        .map(|row| {
            let value = role_permissions
                .get(&row.id)
                .map(|field| override_of(*field))
                .unwrap_or_default();
            OverrideRow {
                role: row.id,
                name: row.name,
                colour: row.colour,
                allows: value.allow.count_ones(),
                denies: value.deny.count_ones(),
            }
        })
        .partition(|row| row.allows + row.denies > 0)
}

/// `target`'s override in `channel` as Stoat stores it, all neutral when
/// there's none (which is the same thing to Stoat).
pub fn stored_override(channel: &Channel, target: &OverrideTarget) -> Override {
    let Channel::TextChannel {
        default_permissions,
        role_permissions,
        ..
    } = channel
    else {
        return Override::default();
    };
    let field = match target {
        OverrideTarget::Everyone => default_permissions.as_ref(),
        OverrideTarget::Role(role) => role_permissions.get(role),
    };
    field.map(|field| override_of(*field)).unwrap_or_default()
}

/// A group's member permissions: its stored value, or Stoat's default for
/// groups when it has none. 0 for anything else.
pub fn group_permissions(channel: &Channel) -> u64 {
    match channel {
        Channel::Group { permissions, .. } => {
            permissions.map_or(*DEFAULT_PERMISSION_DIRECT_MESSAGE, |bits| bits as u64)
        }
        _ => 0,
    }
}

/// The server channel `channel`'s server, and the role `role` there, with
/// whether it ranks below the user's best role (always, for the owner).
fn role_below_us(store: &ChatStore, channel: &str, role: &str) -> Option<bool> {
    let Some(Channel::TextChannel { server, .. }) = store.channel(channel) else {
        return None;
    };
    let role = store.server(server)?.roles.get(role)?;
    Some(role.rank > acting_rank(store, server))
}

/// Whether the user may change `target`'s override in `channel`, as Stoat
/// checks: Manage Permissions there, and for a role, the role ranking below
/// their best one (the owner may change any). Which settings they may
/// choose is the grant rule's to say.
pub fn may_edit_override(store: &ChatStore, channel: &str, target: &OverrideTarget) -> bool {
    if !matches!(store.channel(channel), Some(Channel::TextChannel { .. }))
        || !can_manage_channel_permissions(store, channel)
    {
        return false;
    }
    match target {
        OverrideTarget::Everyone => true,
        OverrideTarget::Role(role) => role_below_us(store, channel, role) == Some(true),
    }
}

/// Whether the user may change a group's member permissions: Manage
/// Permissions in it (the owner's, unless members are given it).
pub fn may_edit_group_permissions(store: &ChatStore, channel: &str) -> bool {
    matches!(store.channel(channel), Some(Channel::Group { .. }))
        && can_manage_channel_permissions(store, channel)
}

/// Why the user can't change `target`'s override in `channel`, a line
/// each. Empty when they can.
pub fn override_lock_notes(
    store: &ChatStore,
    channel: &str,
    target: &OverrideTarget,
) -> Vec<&'static str> {
    if !can_manage_channel_permissions(store, channel) {
        return vec!["You don't have permission to change this channel's permissions."];
    }
    match target {
        OverrideTarget::Role(role) if role_below_us(store, channel, role) == Some(false) => {
            vec!["This role ranks at or above your highest role."]
        }
        _ => Vec::new(),
    }
}

/// Why the user can't change a group's member permissions, if they can't.
pub fn group_lock_note(store: &ChatStore, channel: &str) -> Option<&'static str> {
    (!may_edit_group_permissions(store, channel)).then_some(
        "Only the group's owner, or members allowed Manage Permissions, can change this.",
    )
}

/// What Stoat compares an edit of `target`'s override in `channel` with,
/// for the grant rule: for Everyone, the stored default override (none is
/// the same as all neutral). `None` if the channel isn't a server's.
pub fn grant_basis(store: &ChatStore, channel: &str, target: &OverrideTarget) -> Option<Override> {
    let channel = store.channel(channel)?;
    if !matches!(channel, Channel::TextChannel { .. }) {
        return None;
    }
    match target {
        OverrideTarget::Everyone => Some(stored_override(channel, target)),
        OverrideTarget::Role(_) => None,
    }
}

/// Why Stoat refused a change to a channel's permissions.
pub fn channel_permissions_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::CannotGiveMissingPermissions) => {
            "You can't grant permissions you don't have.".into()
        }
        Some(ErrorType::NotElevated) => "That role ranks at or above your highest role.".into(),
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to change this channel's permissions.".into()
        }
        Some(ErrorType::NotFound) => "That role or channel was deleted.".into(),
        Some(ErrorType::InvalidOperation) => "Stoat can't set that here.".into(),
        _ => error_text(err, "Couldn't save the permissions."),
    }
}
