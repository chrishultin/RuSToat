//! A server's custom emoji, as Server Settings' Emojis page shows them: the
//! list, who may manage them, and why Stoat refused, in words. Plain data
//! with no GPUI, so it's unit-testable. The page is
//! [`crate::ui::emoji_page`]. (Unicode emoji are [`crate::emoji`].)
//!
//! Stoat (0.16): uploading needs Manage Customisation across the server,
//! as renaming does; deleting does too, unless the user created that
//! emoji. A server has at most `limits.global.server_emoji` (100).

use std::path::Path;

use revolt_permissions::ChannelPermission;
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::permissions::{has, server_permissions};
use crate::profile_edit::error_text;
use crate::store::{ChatStore, display_name};

/// Stoat's default for the most custom emoji a server may have.
pub const DEFAULT_SERVER_EMOJI: u32 = 100;

/// The longest emoji name Stoat takes, in characters.
pub const EMOJI_NAME_MAX: usize = 32;

/// The file server's tag for emoji.
pub const EMOJI_TAG: &str = "emojis";

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

/// `text` as a name Stoat takes for an emoji (trimmed, 1 to
/// [`EMOJI_NAME_MAX`] characters of `a`–`z`, `0`–`9` and `_`), or why not.
pub fn check_emoji_name(text: &str) -> Result<String, &'static str> {
    let name = text.trim();
    if name.is_empty() {
        return Err("An emoji needs a name.");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err("Use lowercase letters, numbers and underscores only.");
    }
    if name.chars().count() > EMOJI_NAME_MAX {
        return Err("Emoji names are at most 32 characters.");
    }
    Ok(name.to_string())
}

/// A name for an emoji uploaded from `path`: its file name without the
/// extension, lowercased, with anything else made `_` (runs squeezed, and
/// none at either end), cut to [`EMOJI_NAME_MAX`]. Empty if nothing's left.
pub fn name_from_file(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut name = String::new();
    for c in stem.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            name.push(c);
        } else if !name.is_empty() && !name.ends_with('_') {
            name.push('_');
        }
    }
    let name: String = name
        .trim_end_matches('_')
        .chars()
        .take(EMOJI_NAME_MAX)
        .collect();
    name.trim_end_matches('_').to_string()
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

/// Why "Upload emoji…" is disabled, if it is: the server has `max` already.
pub fn at_limit(count: usize, max: u32) -> Option<String> {
    (count >= max as usize).then(|| format!("This server has {max} emoji, the most Stoat allows."))
}
