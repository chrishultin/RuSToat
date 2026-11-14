//! A channel's webhooks, as Channel Settings' Webhooks page shows them: the
//! list, who may manage them, the URL to post to, and why Stoat refused, in
//! words. Plain data with no GPUI, so it's unit-testable. The page is
//! [`crate::ui::webhooks_page`].
//!
//! Stoat (0.16) needs Manage Webhooks in the channel (a server channel or a
//! group) to list, make, change or delete one. Only the list and a new
//! webhook carry its token; the events leave it out. Instances may turn
//! webhooks off, and then their routes don't exist (a 404).

use revolt_permissions::ChannelPermission;
use revolt_result::ErrorType;

use std::path::Path;

use crate::api::ApiError;
use crate::api::models::{Channel, DataEditWebhook, FieldsWebhook, Webhook};
use crate::media::MediaUrls;
use crate::permissions::{channel_permissions, has};
use crate::profile_edit::{ProfileImage, error_text};
use crate::server_settings::image_change;
use crate::store::ChatStore;

/// The longest webhook name Stoat takes, in characters.
pub const WEBHOOK_NAME_MAX: usize = 32;

/// The file server's tag for webhook avatars.
pub const WEBHOOK_AVATAR_TAG: &str = "avatars";

/// One webhook in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookRow {
    pub id: String,
    pub name: String,
    pub avatar_url: Option<String>,
    /// Its token is known, so its URL can be copied at once.
    pub token_known: bool,
}

/// `channel`'s webhooks, by name, if the list has been fetched.
pub fn webhook_rows(store: &ChatStore, channel: &str) -> Option<Vec<WebhookRow>> {
    let mut rows: Vec<WebhookRow> = store
        .webhooks(channel)?
        .iter()
        .map(|webhook| WebhookRow {
            id: webhook.id.clone(),
            name: webhook.name.clone(),
            avatar_url: avatar_url(webhook, store.media()),
            token_known: webhook.token.is_some(),
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Some(rows)
}

/// A webhook's avatar, on the file server.
pub fn avatar_url(webhook: &Webhook, media: &MediaUrls) -> Option<String> {
    let avatar = webhook.avatar.as_ref()?;
    media.file(avatar)
}

/// Whether the user may manage `channel`'s webhooks: a server channel or a
/// group, with Manage Webhooks in it, as Stoat checks.
pub fn can_manage_webhooks(store: &ChatStore, channel: &str) -> bool {
    matches!(
        store.channel(channel),
        Some(Channel::TextChannel { .. } | Channel::Group { .. })
    ) && has(
        channel_permissions(store, store.me(), channel),
        ChannelPermission::ManageWebhooks,
    )
}

/// The URL to post to: `{api}/webhooks/{id}/{token}`.
pub fn webhook_url(api: &str, id: &str, token: &str) -> String {
    format!("{}/webhooks/{id}/{token}", api.trim_end_matches('/'))
}

/// The URL as shown, its token hidden.
pub fn shown_url(api: &str, id: &str) -> String {
    format!("{}/webhooks/{id}/••••••", api.trim_end_matches('/'))
}

/// `text` as a webhook name Stoat takes (trimmed, 1 to
/// [`WEBHOOK_NAME_MAX`] characters), or `None`.
pub fn check_webhook_name(text: &str) -> Option<String> {
    let name = text.trim();
    (1..=WEBHOOK_NAME_MAX)
        .contains(&name.chars().count())
        .then(|| name.to_string())
}

/// The confirmation before deleting webhook `name`: its title and what
/// happens.
pub fn delete_prompt(name: &str) -> (String, &'static str) {
    (
        format!("Delete {name}?"),
        "Anything posting to its URL will stop working.",
    )
}

/// Whether `err` means the instance has no webhooks: its route doesn't
/// exist, so the answer isn't one of Stoat's own errors.
pub fn webhooks_unavailable(err: &ApiError) -> bool {
    matches!(err, ApiError::Unexpected { status: 404, .. })
}

/// Why the list couldn't be fetched.
pub fn list_error_text(err: &ApiError) -> String {
    if webhooks_unavailable(err) {
        return "This Stoat instance doesn't have webhooks.".into();
    }
    webhook_error_text(err)
}

/// Why Stoat refused a change to a webhook.
pub fn webhook_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to manage webhooks here.".into()
        }
        Some(ErrorType::FailedValidation { .. }) => "Webhook names are 1–32 characters.".into(),
        Some(ErrorType::NotFound) => "That webhook was deleted.".into(),
        Some(ErrorType::InvalidOperation) => "Webhooks can't be made here.".into(),
        _ => error_text(err, "Couldn't change the webhook."),
    }
}

/// A webhook's editor, as typed and picked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebhookForm {
    pub name: String,
    pub avatar: ProfileImage,
}

impl WebhookForm {
    /// The form filled with `webhook`'s values.
    pub fn of(webhook: &Webhook) -> Self {
        Self {
            name: webhook.name.clone(),
            avatar: webhook.avatar.as_ref().map_or(ProfileImage::None, |file| {
                ProfileImage::Current(file.id.clone())
            }),
        }
    }

    /// Whether the name is one Stoat takes, so Save can send it.
    pub fn is_valid(&self) -> bool {
        check_webhook_name(&self.name).is_some()
    }

    /// The avatar picked to upload before saving.
    pub fn new_file(&self) -> Option<&Path> {
        self.avatar.new_file()
    }

    /// Whether Save has anything to do: a change, or an avatar to upload.
    pub fn has_changes(&self, webhook: &Webhook) -> bool {
        self.new_file().is_some() || self.changes(webhook).is_some()
    }

    /// The `PATCH /webhooks/{id}` that turns `webhook` into this form: a
    /// trimmed new name, a new avatar's file ID, or `remove: [Avatar]`. A
    /// picked avatar is left out until it's uploaded. `None` when nothing
    /// changed or the form isn't valid.
    pub fn changes(&self, webhook: &Webhook) -> Option<DataEditWebhook> {
        let name = check_webhook_name(&self.name)?;
        let stored = Self::of(webhook);
        let mut edit = empty_webhook_edit();
        if name != webhook.name {
            edit.name = Some(name);
        }
        match image_change(&stored.avatar, &self.avatar) {
            Some(Some(id)) => edit.avatar = Some(id),
            Some(None) => edit.remove.push(FieldsWebhook::Avatar),
            None => {}
        }
        (edit != empty_webhook_edit()).then_some(edit)
    }
}

/// A `DataEditWebhook` that changes nothing (it has no `Default`).
pub fn empty_webhook_edit() -> DataEditWebhook {
    DataEditWebhook {
        name: None,
        avatar: None,
        permissions: None,
        remove: Vec::new(),
    }
}
