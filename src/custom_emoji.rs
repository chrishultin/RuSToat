//! A server's custom emoji, as Server Settings' Emojis page shows them: the
//! list, who may manage them, and why Stoat refused, in words. Plain data
//! with no GPUI, so it's unit-testable. The page is
//! [`crate::ui::emoji_page`]. (Unicode emoji are [`crate::emoji`].)
//!
//! Stoat (0.16): uploading needs Manage Customisation across the server,
//! as renaming does; deleting does too, unless the user created that
//! emoji. A server has at most `limits.global.server_emoji` (100).

use revolt_permissions::ChannelPermission;
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::permissions::{has, server_permissions};
use crate::profile_edit::error_text;
use crate::store::{ChatStore, display_name};

/// Stoat's default for the most custom emoji a server may have.
pub const DEFAULT_SERVER_EMOJI: u32 = 100;

/// One emoji in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmojiRow {
    pub id: String,
    pub name: String,
    /// The image, when the file server is known.
    pub url: Option<String>,
    pub animated: bool,
    /// Who uploaded it, by ID.
    pub creator: String,
    /// Their display name, or "Unknown user" if they aren't cached.
    pub creator_name: String,
}

/// `server`'s custom emoji, by name.
pub fn emoji_rows(store: &ChatStore, server: &str) -> Vec<EmojiRow> {
    store
        .server_emojis(server)
        .into_iter()
        .map(|emoji| EmojiRow {
            id: emoji.id.clone(),
            name: emoji.name.clone(),
            url: store.media().emoji(&emoji.id),
            animated: emoji.animated,
            creator: emoji.creator_id.clone(),
            creator_name: store.user(&emoji.creator_id).map_or_else(
                || "Unknown user".into(),
                |user| display_name(user).to_string(),
            ),
        })
        .collect()
}

/// Whether the user may upload, rename and delete `server`'s emoji: its
/// owner, or a known member with Manage Customisation, as Stoat checks.
pub fn can_manage_emoji(store: &ChatStore, server: &str) -> bool {
    let me = store.me();
    let Some(known) = store.server(server) else {
        return false;
    };
    (known.owner == me || store.member(server, me).is_some())
        && has(
            server_permissions(store, me, server),
            ChannelPermission::ManageCustomisation,
        )
}

/// "3 of 100 emoji".
pub fn count_line(count: usize, max: u32) -> String {
    format!("{count} of {max} emoji")
}

/// The confirmation before deleting `:name:`: its title and what happens.
pub fn delete_prompt(name: &str) -> (String, &'static str) {
    (
        format!("Delete :{name}:?"),
        "Messages and reactions that use it won't show it any more.",
    )
}

/// Why Stoat refused a change to a server's emoji.
pub fn emoji_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::TooManyEmoji { max }) => {
            format!("This server has {max} emoji, the most Stoat allows.")
        }
        Some(ErrorType::FailedValidation { .. }) => {
            "Emoji names are 1–32 lowercase letters, numbers and underscores.".into()
        }
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to manage this server's emoji.".into()
        }
        Some(ErrorType::NotFound) => "That emoji was deleted.".into(),
        _ => error_text(err, "Couldn't change the emoji."),
    }
}
