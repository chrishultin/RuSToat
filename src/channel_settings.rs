//! A channel's or group's settings: who may see the Channel Settings
//! window, what its Overview form sends (name, description and icon), and
//! why Stoat refused, in words. Plain data with no GPUI, so it's
//! unit-testable. The window is [`crate::ui::channel_settings`].

use std::path::Path;

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{Channel, DataEditChannel, FieldsChannel, File};
use crate::profile_edit::{ProfileImage, error_text};
use crate::server_edit::valid_name;
use crate::server_settings::image_change;
use crate::store::ChatStore;

pub use crate::server_edit::NAME_MAX;

/// The longest channel description Stoat takes, in characters.
pub const DESCRIPTION_MAX: usize = 1024;

/// The file server's tag for channel and group icons.
pub const ICON_TAG: &str = "icons";

/// Which pages of a channel's settings the user may see.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelAccess {
    /// Name, description and icon (Manage Channels on the channel).
    pub overview: bool,
}

impl ChannelAccess {
    /// Whether any page shows: the window opens only then, and closes once
    /// none does.
    pub fn any(&self) -> bool {
        self.overview
    }
}

/// Which pages of `channel`'s settings the user may see.
pub fn access(store: &ChatStore, channel: &str) -> ChannelAccess {
    ChannelAccess {
        overview: store.can_edit_channel(channel),
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
/// empty string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelForm {
    pub name: String,
    pub description: String,
    pub icon: ProfileImage,
}

impl ChannelForm {
    /// The form filled with `channel`'s values. `None` for a channel with
    /// no settings (a DM or Saved Notes).
    pub fn of(channel: &Channel) -> Option<Self> {
        let (name, description, icon) = match channel {
            Channel::TextChannel {
                name,
                description,
                icon,
                ..
            }
            | Channel::Group {
                name,
                description,
                icon,
                ..
            } => (name, description, icon),
            _ => return None,
        };
        Some(Self {
            name: name.clone(),
            description: description.clone().unwrap_or_default(),
            icon: image(icon),
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
    /// it's [`ProfileImage::Current`] with its new ID). `None` when nothing
    /// changed, the form isn't valid, or the channel has no settings.
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
