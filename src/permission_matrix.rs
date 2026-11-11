//! The permission matrix: every permission Stoat has, grouped and described
//! as the Roles page shows them, a role's setting for each (allow, neutral
//! or deny), which settings the user may choose, and why Stoat refused, in
//! words. Plain data with no GPUI, so it's unit-testable, and
//! shared by every kind of permission editor (a role's, the server's
//! default permissions, and later a channel's overrides). The view is
//! [`crate::ui::permission_matrix`].
//!
//! What the user may choose is Stoat's grant rule (0.16.2,
//! `throw_permission_override`): for the server's default permissions,
//! they must hold every permission the new value allows; for an override,
//! every permission newly allowed or no longer denied. So a permission they
//! don't hold can always be made stricter, never looser.

use revolt_permissions::{ChannelPermission, Override, OverrideField};
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::profile_edit::error_text;

/// One permission as the matrix lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionEntry {
    pub permission: ChannelPermission,
    pub title: &'static str,
    /// What it allows across a server.
    pub server_description: &'static str,
    /// What it allows in one server channel, if it means anything there
    /// (Kick Members, say, doesn't).
    pub channel_description: Option<&'static str>,
    /// What it allows in a group, if it means anything there.
    pub group_description: Option<&'static str>,
    /// The heading of the group this entry starts, if it starts one.
    pub heading: Option<&'static str>,
}

impl PermissionEntry {
    /// The permission's bit.
    pub fn bit(&self) -> u64 {
        self.permission as u64
    }

    /// What it allows in `context`, or `None` if it means nothing there
    /// (so the matrix leaves it out). Every permission means something
    /// across a server.
    pub fn description(&self, context: MatrixContext) -> Option<&'static str> {
        match context {
            MatrixContext::Server => Some(self.server_description),
            MatrixContext::Channel => self.channel_description,
            MatrixContext::Group => self.group_description,
        }
    }
}

/// Where a matrix's permissions apply: which rows it shows, and how each
/// reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MatrixContext {
    /// A server's roles and default permissions: every permission.
    #[default]
    Server,
    /// One server channel's overrides.
    Channel,
    /// A group's member permissions.
    Group,
}

/// One row of a matrix in some context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShownEntry {
    pub entry: &'static PermissionEntry,
    /// What it allows there.
    pub description: &'static str,
    /// The heading to show above it: its group's, if it's the first of the
    /// group shown.
    pub heading: Option<&'static str>,
}

/// The rows of a matrix in `context`, in order. A group of permissions
/// none of which mean anything there has no heading; one whose first
/// permission is left out has its heading on the first one shown.
pub fn entries(context: MatrixContext) -> Vec<ShownEntry> {
    let mut pending = None;
    let mut shown = Vec::new();
    for entry in &PERMISSIONS {
        if entry.heading.is_some() {
            pending = entry.heading;
        }
        if let Some(description) = entry.description(context) {
            shown.push(ShownEntry {
                entry,
                description,
                heading: pending.take(),
            });
        }
    }
    shown
}

const fn entry(
    permission: ChannelPermission,
    title: &'static str,
    server_description: &'static str,
) -> PermissionEntry {
    PermissionEntry {
        permission,
        title,
        server_description,
        channel_description: None,
        group_description: None,
        heading: None,
    }
}

const fn starts(heading: &'static str, entry: PermissionEntry) -> PermissionEntry {
    PermissionEntry {
        heading: Some(heading),
        ..entry
    }
}

/// Every permission in `revolt-permissions`' `ChannelPermission` (but the
/// `GrantAll` pair, which aren't single permissions), in display order, as
/// the web client lists them, plus View Audit Logs and Use External Emojis,
/// which it leaves out but Stoat enforces.
pub const PERMISSIONS: [PermissionEntry; 35] = with_contexts({
    use ChannelPermission::*;
    [
        starts(
            "Admin",
            entry(
                ManageChannel,
                "Manage Channels",
                "Create, edit and delete channels",
            ),
        ),
        entry(
            ManageServer,
            "Manage Server",
            "Edit the server's information and settings",
        ),
        entry(
            ManagePermissions,
            "Manage Permissions",
            "Edit any permissions on the server",
        ),
        entry(ManageRole, "Manage Roles", "Create and edit server roles"),
        entry(
            ManageCustomisation,
            "Manage Customisation",
            "Create server emoji",
        ),
        entry(
            ViewAuditLogs,
            "View Audit Logs",
            "See the server's audit log",
        ),
        starts(
            "Members",
            entry(
                KickMembers,
                "Kick Members",
                "Kick lower-ranking members from the server",
            ),
        ),
        entry(
            BanMembers,
            "Ban Members",
            "Ban lower-ranking members from the server",
        ),
        entry(
            TimeoutMembers,
            "Timeout Members",
            "Temporarily prevent lower-ranking members from interacting",
        ),
        entry(
            AssignRoles,
            "Assign Roles",
            "Assign lower-ranked roles to lower-ranking members",
        ),
        entry(ChangeNickname, "Change Nickname", "Change own nickname"),
        entry(
            ManageNicknames,
            "Manage Nicknames",
            "Change other members' nicknames",
        ),
        entry(ChangeAvatar, "Change Avatar", "Change own avatar"),
        entry(
            RemoveAvatars,
            "Remove Avatars",
            "Remove other members' avatars",
        ),
        starts(
            "Channels",
            entry(
                ViewChannel,
                "View Channels",
                "Able to access channels on this server",
            ),
        ),
        entry(
            ReadMessageHistory,
            "Read Message History",
            "Read past messages sent in channels",
        ),
        entry(SendMessage, "Send Messages", "Send messages in channels"),
        entry(
            ManageMessages,
            "Manage Messages",
            "Delete and pin messages sent by other members",
        ),
        entry(
            ManageWebhooks,
            "Manage Webhooks",
            "Create and edit webhooks",
        ),
        entry(
            InviteOthers,
            "Invite Others",
            "Create invites for others to use",
        ),
        starts(
            "Messaging",
            entry(
                SendEmbeds,
                "Send Embeds",
                "Send embedded content such as link embeds or custom embeds",
            ),
        ),
        entry(UploadFiles, "Upload Files", "Send attachments to chat"),
        entry(
            Masquerade,
            "Masquerade",
            "Allow members to change name and avatar per-message",
        ),
        entry(React, "React", "React to messages with emoji"),
        entry(
            UseExternalEmojis,
            "Use External Emojis",
            "Use custom emoji from other servers",
        ),
        entry(
            BypassSlowmode,
            "Bypass Slowmode",
            "Bypasses slowmode in channels",
        ),
        starts(
            "Voice",
            entry(Connect, "Connect", "Connect to voice channel"),
        ),
        entry(Speak, "Speak", "Able to speak in voice call"),
        entry(Video, "Video", "Share camera or screen in voice call"),
        entry(
            MuteMembers,
            "Mute Members",
            "Mute lower-ranking members in voice call",
        ),
        entry(
            DeafenMembers,
            "Deafen Members",
            "Deafen lower-ranking members in voice call",
        ),
        entry(
            MoveMembers,
            "Move Members",
            "Move members between voice channels",
        ),
        entry(Listen, "Listen", "Hear other people and see their video"),
        starts(
            "Mentions",
            entry(
                MentionEveryone,
                "Mention Everyone",
                "Mention everyone and online members inside the server",
            ),
        ),
        entry(MentionRoles, "Mention Roles", "Mention specific roles"),
    ]
});

/// What each permission that means something in one server channel allows
/// there: the web client's 22, and Use External Emojis, which Stoat checks
/// per channel.
const CHANNEL_DESCRIPTIONS: [(ChannelPermission, &str); 23] = {
    use ChannelPermission::*;
    [
        (ManageChannel, "Edit and delete this channel"),
        (
            ManagePermissions,
            "Edit this channel's role and default permissions",
        ),
        (ViewChannel, "Able to access this channel"),
        (
            ReadMessageHistory,
            "Read past messages sent in this channel",
        ),
        (SendMessage, "Send messages in this channel"),
        (
            ManageMessages,
            "Delete and pin messages sent by other members",
        ),
        (ManageWebhooks, "Create and edit webhooks"),
        (InviteOthers, "Create invites for others to use"),
        (
            SendEmbeds,
            "Send embedded content such as link embeds or custom embeds",
        ),
        (UploadFiles, "Send attachments to chat"),
        (
            Masquerade,
            "Allow members to change name and avatar per-message",
        ),
        (React, "React to messages with emoji"),
        (UseExternalEmojis, "Use custom emoji from other servers"),
        (BypassSlowmode, "Bypass this channel's slowmode"),
        (Connect, "Connect to this voice channel"),
        (Speak, "Able to speak in voice call"),
        (Video, "Share camera or screen in voice call"),
        (MuteMembers, "Mute lower-ranking members in voice call"),
        (DeafenMembers, "Deafen lower-ranking members in voice call"),
        (MoveMembers, "Move members between voice channels"),
        (Listen, "Hear other people and see their video"),
        (
            MentionEveryone,
            "Mention everyone and online members inside the server",
        ),
        (MentionRoles, "Mention specific roles"),
    ]
};

/// What each permission that means something in a group allows there, as
/// the web client lists them.
const GROUP_DESCRIPTIONS: [(ChannelPermission, &str); 12] = {
    use ChannelPermission::*;
    [
        (ManageChannel, "Edit the group's name, description and icon"),
        (ManagePermissions, "Change what members may do in the group"),
        (SendMessage, "Send messages in the group"),
        (
            ManageMessages,
            "Delete and pin messages sent by other members",
        ),
        (ManageWebhooks, "Create and edit webhooks"),
        (InviteOthers, "Add new members to the group"),
        (
            SendEmbeds,
            "Send embedded content such as link embeds or custom embeds",
        ),
        (UploadFiles, "Send attachments to chat"),
        (
            Masquerade,
            "Allow members to change name and avatar per-message",
        ),
        (React, "React to messages with emoji"),
        (MentionEveryone, "Mention everyone in the group"),
        (MentionRoles, "Mention specific roles"),
    ]
};

/// `list` with each entry's channel and group descriptions filled in from
/// [`CHANNEL_DESCRIPTIONS`] and [`GROUP_DESCRIPTIONS`].
const fn with_contexts(mut list: [PermissionEntry; 35]) -> [PermissionEntry; 35] {
    let mut i = 0;
    while i < list.len() {
        let bit = list[i].permission as u64;
        let mut j = 0;
        while j < CHANNEL_DESCRIPTIONS.len() {
            if CHANNEL_DESCRIPTIONS[j].0 as u64 == bit {
                list[i].channel_description = Some(CHANNEL_DESCRIPTIONS[j].1);
            }
            j += 1;
        }
        let mut j = 0;
        while j < GROUP_DESCRIPTIONS.len() {
            if GROUP_DESCRIPTIONS[j].0 as u64 == bit {
                list[i].group_description = Some(GROUP_DESCRIPTIONS[j].1);
            }
            j += 1;
        }
        i += 1;
    }
    list
}

/// What a role (or a channel override) does with one permission. Ordered
/// from strictest: `Deny < Neutral < Allow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Setting {
    /// Takes the permission away, whatever lower roles give.
    Deny,
    /// Leaves it to the roles below.
    Neutral,
    /// Gives the permission.
    Allow,
}

impl Setting {
    /// The three, in the order the control shows them.
    pub const ALL: [Setting; 3] = [Setting::Deny, Setting::Neutral, Setting::Allow];

    /// "Deny", "Neutral" or "Allow".
    pub fn label(self) -> &'static str {
        match self {
            Setting::Deny => "Deny",
            Setting::Neutral => "Neutral",
            Setting::Allow => "Allow",
        }
    }
}

/// `value`'s setting for `bit`. A bit both allowed and denied is denied, as
/// Stoat applies an override (allow, then deny).
pub fn setting_of(value: &Override, bit: u64) -> Setting {
    if value.deny & bit != 0 {
        Setting::Deny
    } else if value.allow & bit != 0 {
        Setting::Allow
    } else {
        Setting::Neutral
    }
}

/// `value` with `bit` set to `setting`, and cleared from the other side.
pub fn with_setting(value: Override, bit: u64, setting: Setting) -> Override {
    let (allow, deny) = (value.allow & !bit, value.deny & !bit);
    match setting {
        Setting::Deny => Override {
            allow,
            deny: deny | bit,
        },
        Setting::Neutral => Override { allow, deny },
        Setting::Allow => Override {
            allow: allow | bit,
            deny,
        },
    }
}

/// A role's stored permissions (`{ a, d }`) as allow and deny bits.
pub fn override_of(field: OverrideField) -> Override {
    field.into()
}

/// Whether the user, holding `held`, may set `bit` to `setting` in the
/// server's default permissions: off always, on only if they hold it.
/// (Stoat checks every bit the new value allows, so one they don't hold
/// can't stay on either: see [`missing_from_default`].)
pub fn value_enabled(held: u64, bit: u64, setting: Setting) -> bool {
    setting != Setting::Allow || held & bit == bit
}

/// The permissions `value` gives Everyone that the user, holding `held`,
/// doesn't have, in list order. Stoat refuses to save default permissions
/// with any of these on.
pub fn missing_from_default(held: u64, value: u64) -> Vec<&'static PermissionEntry> {
    PERMISSIONS
        .iter()
        .filter(|entry| value & entry.bit() != 0 && held & entry.bit() == 0)
        .collect()
}

/// Why Everyone's permissions can't be saved as they are: "You don't have
/// Manage Server, so Everyone can't keep it. Turn it off to save." `None`
/// when nothing's missing.
pub fn missing_note(missing: &[&PermissionEntry]) -> Option<String> {
    let titles: Vec<&str> = missing.iter().map(|entry| entry.title).collect();
    let them = if titles.len() == 1 { "it" } else { "them" };
    let titles = match titles.as_slice() {
        [] => return None,
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    Some(format!(
        "You don't have {titles}, so Everyone can't keep {them}. Turn {them} off to save."
    ))
}

/// The default permissions to send, if `edited` differs from `stored`.
pub fn default_change(stored: u64, edited: u64) -> Option<u64> {
    (stored != edited).then_some(edited)
}

/// `value` with `bit` on (for [`Setting::Allow`]) or off (otherwise), and
/// every other bit as it was: a checkbox's change.
pub fn with_value(value: u64, bit: u64, setting: Setting) -> u64 {
    match setting {
        Setting::Allow => value | bit,
        Setting::Neutral | Setting::Deny => value & !bit,
    }
}

/// Permission bits as edited, after the stored value changed from `filled`
/// to `now` (elsewhere): bits the user changed keep their edit, the rest
/// follow.
pub fn follow_value(filled: u64, edited: u64, now: u64) -> u64 {
    let touched = filled ^ edited;
    (now & !touched) | (edited & touched)
}

/// Why Stoat refused to save permissions.
pub fn permissions_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::CannotGiveMissingPermissions) => {
            "You can't grant permissions you don't have.".into()
        }
        Some(ErrorType::NotElevated) => "That role ranks at or above your highest role.".into(),
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to manage permissions.".into()
        }
        Some(ErrorType::NotFound) => "That role was deleted.".into(),
        _ => error_text(err, "Couldn't save the permissions."),
    }
}

/// Whether the user, holding `held`, may set `bit` to `setting` in an
/// override whose stored value is `stored`: any setting for a permission
/// they hold; for one they don't, only settings no looser than the stored
/// one (Deny < Neutral < Allow). So they can always revoke.
pub fn setting_enabled(held: u64, stored: &Override, bit: u64, setting: Setting) -> bool {
    held & bit == bit || setting <= setting_of(stored, bit)
}

/// Whether Stoat would take `edited` in place of `stored` from a user
/// holding `held` (`throw_permission_override`): they hold every
/// permission newly allowed, and every one no longer denied.
pub fn may_grant(held: u64, stored: &Override, edited: &Override) -> bool {
    let holds = |bits: u64| held & bits == bits;
    holds(!stored.allow & edited.allow) && holds(stored.deny & !edited.deny)
}

/// The override to send, if `edited` differs from `stored`.
pub fn override_change(stored: &Override, edited: &Override) -> Option<Override> {
    (stored != edited).then(|| edited.clone())
}

/// An override as edited, after the stored one changed from `filled` to
/// `now` (elsewhere): permissions the user changed keep their edit, the
/// rest follow.
pub fn follow_override(filled: &Override, edited: &Override, now: &Override) -> Override {
    let touched = (filled.allow ^ edited.allow) | (filled.deny ^ edited.deny);
    Override {
        allow: (now.allow & !touched) | (edited.allow & touched),
        deny: (now.deny & !touched) | (edited.deny & touched),
    }
}
