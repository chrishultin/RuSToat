//! The permission matrix: every permission Stoat has, grouped and described
//! as the Roles page shows them, and a role's setting for each (allow,
//! neutral or deny). Plain data with no GPUI, so it's unit-testable, and
//! shared by every kind of permission editor (a role's, the server's
//! default permissions, and later a channel's overrides). The view is
//! [`crate::ui::permission_matrix`].

use revolt_permissions::{ChannelPermission, Override, OverrideField};

/// One permission as the matrix lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionEntry {
    pub permission: ChannelPermission,
    pub title: &'static str,
    /// What it allows across a server.
    pub server_description: &'static str,
    /// What it allows in one channel, where that reads differently.
    pub channel_description: Option<&'static str>,
    /// The heading of the group this entry starts, if it starts one.
    pub heading: Option<&'static str>,
}

impl PermissionEntry {
    /// The permission's bit.
    pub fn bit(&self) -> u64 {
        self.permission as u64
    }
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
pub const PERMISSIONS: [PermissionEntry; 35] = {
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
};

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
