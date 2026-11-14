//! A channel's or group's settings: who may see the Channel Settings
//! window, what its Overview form sends (name, description, icon and
//! slowmode), marking it mature, and why Stoat refused, in words. Plain data with no GPUI, so it's
//! unit-testable. The window is [`crate::ui::channel_settings`].

use std::path::Path;

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{Channel, DataEditChannel, FieldsChannel, File};
use crate::profile_edit::{ProfileImage, error_text};
use crate::server_edit::valid_name;
use crate::server_settings::image_change;
use crate::store::ChatStore;
use crate::time::format_duration_short;

pub use crate::server_edit::NAME_MAX;

/// The longest channel description Stoat takes, in characters.
pub const DESCRIPTION_MAX: usize = 1024;

/// The file server's tag for channel and group icons.
pub const ICON_TAG: &str = "icons";

/// Which pages of a channel's settings the user may see.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelAccess {
    /// Name, description, icon, slowmode and the mature flag (Manage
    /// Channels on the channel).
    pub overview: bool,
    /// Its overrides, or a group's member permissions (Manage Permissions
    /// in it).
    pub permissions: bool,
    /// Its webhooks (Manage Webhooks in it).
    pub webhooks: bool,
}

impl ChannelAccess {
    /// Whether any page shows: the window opens only then, and closes once
    /// none does.
    pub fn any(&self) -> bool {
        self.overview || self.permissions || self.webhooks
    }
}

/// Which pages of `channel`'s settings the user may see.
pub fn access(store: &ChatStore, channel: &str) -> ChannelAccess {
    ChannelAccess {
        overview: store.can_edit_channel(channel),
        permissions: crate::channel_permissions::can_manage_channel_permissions(store, channel),
        webhooks: crate::webhooks::can_manage_webhooks(store, channel),
    }
}

/// What the settings are for: the words differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// A server's channel (text or voice).
    Channel,
    Group,
}

impl ChannelKind {
    /// The kind of `channel`, if it has settings.
    pub fn of(channel: &Channel) -> Option<Self> {
        match channel {
            Channel::TextChannel { .. } => Some(Self::Channel),
            Channel::Group { .. } => Some(Self::Group),
            _ => None,
        }
    }

    /// "channel" or "group".
    pub fn noun(self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Group => "group",
        }
    }
}

/// "Channel Settings — #{name}" or "Group Settings — {name}".
pub fn window_title(channel: Option<&Channel>) -> String {
    match channel {
        Some(Channel::TextChannel { name, .. }) => format!("Channel Settings — #{name}"),
        Some(Channel::Group { name, .. }) => format!("Group Settings — {name}"),
        _ => "Channel Settings".into(),
    }
}

/// The Overview form, as typed and picked. A missing description is the
/// empty string, and a missing slowmode 0 (off).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelForm {
    pub name: String,
    pub description: String,
    pub icon: ProfileImage,
    /// Seconds between a member's messages. Only server channels have
    /// one; a group's is always 0.
    pub slowmode: u64,
}

impl ChannelForm {
    /// The form filled with `channel`'s values. `None` for a channel with
    /// no settings (a DM or Saved Notes).
    pub fn of(channel: &Channel) -> Option<Self> {
        let (name, description, icon, slowmode) = match channel {
            Channel::TextChannel {
                name,
                description,
                icon,
                slowmode,
                ..
            } => (name, description, icon, slowmode.unwrap_or_default()),
            Channel::Group {
                name,
                description,
                icon,
                ..
            } => (name, description, icon, 0),
            _ => return None,
        };
        Some(Self {
            name: name.clone(),
            description: description.clone().unwrap_or_default(),
            icon: image(icon),
            slowmode,
        })
    }

    /// Whether the name is 1 to [`NAME_MAX`] characters and the description
    /// at most [`DESCRIPTION_MAX`] (both trimmed), so Save can send them.
    pub fn is_valid(&self) -> bool {
        valid_name(&self.name).is_some()
            && self.description.trim().chars().count() <= DESCRIPTION_MAX
    }

    /// The icon picked to upload before saving.
    pub fn new_file(&self) -> Option<&Path> {
        self.icon.new_file()
    }

    /// Whether Save has anything to do: a change to send, or an icon to
    /// upload first.
    pub fn has_changes(&self, channel: &Channel) -> bool {
        self.new_file().is_some() || self.changes(channel).is_some()
    }

    /// The `PATCH /channels/{id}` that turns `channel` into this form: only
    /// what changed, trimmed, with a blank description or a removed icon in
    /// `remove`. A picked icon is left out until it's uploaded (and then
    /// it's [`ProfileImage::Current`] with its new ID). Slowmode is sent as
    /// its number of seconds, 0 for off (as the web client does), and only
    /// for a server channel. `None` when nothing changed, the form isn't
    /// valid, or the channel has no settings.
    pub fn changes(&self, channel: &Channel) -> Option<DataEditChannel> {
        if !self.is_valid() {
            return None;
        }
        let stored = Self::of(channel)?;
        let mut edit = empty_channel_edit();
        let name = self.name.trim();
        if name != stored.name.trim() {
            edit.name = Some(name.to_string());
        }
        let description = self.description.trim();
        if description != stored.description.trim() {
            if description.is_empty() {
                edit.remove.push(FieldsChannel::Description);
            } else {
                edit.description = Some(description.to_string());
            }
        }
        match image_change(&stored.icon, &self.icon) {
            Some(Some(id)) => edit.icon = Some(id),
            Some(None) => edit.remove.push(FieldsChannel::Icon),
            None => {}
        }
        if matches!(channel, Channel::TextChannel { .. }) && self.slowmode != stored.slowmode {
            edit.slowmode = Some(self.slowmode);
        }
        (edit != empty_channel_edit()).then_some(edit)
    }
}

/// A stored icon as a form's image.
fn image(file: &Option<File>) -> ProfileImage {
    file.as_ref().map_or(ProfileImage::None, |file| {
        ProfileImage::Current(file.id.clone())
    })
}

/// A `DataEditChannel` that changes nothing (it has no `Default`).
pub fn empty_channel_edit() -> DataEditChannel {
    DataEditChannel {
        name: None,
        description: None,
        owner: None,
        icon: None,
        nsfw: None,
        archived: None,
        voice: None,
        slowmode: None,
        remove: Vec::new(),
    }
}

/// The slowmodes offered, as the web client offers them: seconds, and
/// their words.
pub const SLOWMODE_CHOICES: [(u64, &str); 11] = [
    (0, "Off"),
    (5, "5 seconds"),
    (10, "10 seconds"),
    (30, "30 seconds"),
    (60, "1 minute"),
    (300, "5 minutes"),
    (600, "10 minutes"),
    (1800, "30 minutes"),
    (3600, "1 hour"),
    (7200, "2 hours"),
    (21600, "6 hours"),
];

/// A slowmode in words: a choice's own, or else
/// [`format_duration_short`]'s ("45s", "1m 30s").
pub fn slowmode_label(secs: u64) -> String {
    SLOWMODE_CHOICES
        .iter()
        .find(|(choice, _)| *choice == secs)
        .map_or_else(
            || format_duration_short(secs),
            |(_, label)| label.to_string(),
        )
}

/// The slowmodes to choose from: [`SLOWMODE_CHOICES`], plus `current` in
/// its place if it isn't one (set elsewhere), so opening and saving the
/// page never changes it.
pub fn slowmode_choices(current: u64) -> Vec<(u64, String)> {
    let mut choices: Vec<(u64, String)> = SLOWMODE_CHOICES
        .iter()
        .map(|(secs, label)| (*secs, label.to_string()))
        .collect();
    if !choices.iter().any(|(secs, _)| *secs == current) {
        let at = choices.partition_point(|(secs, _)| *secs < current);
        choices.insert(at, (current, slowmode_label(current)));
    }
    choices
}

/// The `PATCH /channels/{id}` that marks a channel mature or not: `nsfw`
/// alone, as the web client sends it.
pub fn mature_edit(mature: bool) -> DataEditChannel {
    DataEditChannel {
        nsfw: Some(mature),
        ..empty_channel_edit()
    }
}

/// The confirmation before (un)marking a channel or group mature, in the
/// web client's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaturePrompt {
    pub title: String,
    pub text: String,
    /// The button that does it.
    pub confirm: &'static str,
}

/// What to ask before making a `kind` that's `mature` now the other way.
pub fn mature_prompt(kind: ChannelKind, mature: bool) -> MaturePrompt {
    let noun = kind.noun();
    if mature {
        MaturePrompt {
            title: format!("Unmark this {noun} as mature?"),
            text: format!(
                "Users will no longer be asked to confirm their age before joining this {noun}. \
                 Please ensure the content is appropriate for all ages."
            ),
            confirm: "Unmark as mature",
        }
    } else {
        MaturePrompt {
            title: format!("Mark this {noun} as mature?"),
            text: format!("Users will be asked to confirm their age before joining this {noun}."),
            confirm: "Mark as mature",
        }
    }
}

/// The Overview's line about the mature flag, as it is.
pub fn mature_state(kind: ChannelKind, mature: bool) -> String {
    if mature {
        format!(
            "Members are asked to confirm their age before opening this {}.",
            kind.noun()
        )
    } else {
        "Members aren't asked to confirm their age.".into()
    }
}

/// Why Stoat refused to (un)mark a channel mature.
pub fn mature_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. } | ErrorType::NotFound) => save_error_text(err),
        _ => error_text(err, "Couldn't change whether the channel is mature."),
    }
}

/// Why Stoat refused to save a channel's settings.
pub fn save_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to change this channel.".into()
        }
        Some(ErrorType::FailedValidation { .. }) => format!(
            "Channel names are 1–{NAME_MAX} characters, and descriptions at most \
             {DESCRIPTION_MAX}."
        ),
        Some(ErrorType::NotFound) => "That channel was deleted.".into(),
        _ => error_text(err, "Couldn't save the channel's settings."),
    }
}
