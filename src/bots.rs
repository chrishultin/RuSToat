//! The user's bots, as Settings' My Bots section shows them: the list, the
//! username check, the account's limit, and why Stoat refused, in words.
//! Plain data with no GPUI, so it's unit-testable. The section is
//! [`crate::ui::bots_page`].
//!
//! Stoat (0.16.4) lets only a bot's owner list, change or delete it (anyone
//! else gets `NotFound`). Every answer about a bot carries its token, so
//! [`Bot`] values are never logged: their `Debug` prints it.

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::RootConfig;
use crate::api::models::{Bot, DataEditBot, FieldsBot, User};
use crate::media::MediaUrls;
use crate::profile_edit::error_text;

/// The shortest and longest username Stoat takes, in characters.
pub const BOT_USERNAME_MIN: usize = 2;
pub const BOT_USERNAME_MAX: usize = 32;

/// How many bots an account may own when the instance doesn't say (Stoat's
/// default for established accounts).
pub const DEFAULT_BOT_LIMIT: u32 = 5;

/// Usernames Stoat refuses whatever their case.
const BLOCKED_USERNAMES: [&str; 3] = ["admin", "revolt", "stoat"];

/// One bot in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotRow {
    pub id: String,
    /// Empty if the bot's user wasn't sent.
    pub username: String,
    /// Its display name, else its username, else its ID.
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub public: bool,
}

/// `bots` with their users, by name. Stoat sends the two lists apart, each
/// sorted by ID, so they're matched by ID.
pub fn bot_rows(bots: &[Bot], users: &[User], media: &MediaUrls) -> Vec<BotRow> {
    let mut rows: Vec<BotRow> = bots
        .iter()
        .map(|bot| {
            let user = users.iter().find(|user| user.id == bot.id);
            let username = user.map(|user| user.username.clone()).unwrap_or_default();
            let display_name = user
                .and_then(|user| user.display_name.clone())
                .filter(|name| !name.trim().is_empty())
                .or_else(|| (!username.is_empty()).then(|| username.clone()))
                .unwrap_or_else(|| bot.id.clone());
            BotRow {
                id: bot.id.clone(),
                username,
                display_name,
                avatar_url: user
                    .and_then(|user| user.avatar.as_ref())
                    .and_then(|file| media.file(file)),
                public: bot.public,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    rows
}

/// Why a username can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsernameProblem {
    TooShort,
    TooLong,
    /// A character other than a letter, a digit, `_`, `.` or `-`.
    BadCharacter,
    /// One of the names Stoat keeps for itself.
    Reserved,
}

impl std::fmt::Display for UsernameProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooShort => "Usernames are at least 2 characters.",
            Self::TooLong => "Usernames are at most 32 characters.",
            Self::BadCharacter => "Use letters, numbers, _, . and - only.",
            Self::Reserved => "That username can't be used. Try another.",
        })
    }
}

/// `text` as a username Stoat takes, trimmed: 2 to 32 letters (in any
/// script), digits, `_`, `.` and `-`, and not a reserved name.
pub fn check_bot_username(text: &str) -> Result<String, UsernameProblem> {
    let name = text.trim();
    let count = name.chars().count();
    if count < BOT_USERNAME_MIN {
        return Err(UsernameProblem::TooShort);
    }
    if count > BOT_USERNAME_MAX {
        return Err(UsernameProblem::TooLong);
    }
    if !name
        .chars()
        .all(|c| c.is_alphabetic() || c.is_numeric() || matches!(c, '_' | '.' | '-'))
    {
        return Err(UsernameProblem::BadCharacter);
    }
    if BLOCKED_USERNAMES.contains(&name.to_lowercase().as_str()) {
        return Err(UsernameProblem::Reserved);
    }
    Ok(name.to_string())
}

/// How many bots an account may own, from `GET /`: the established
/// accounts' limit (new accounts' is lower, and Stoat says so if it
/// applies), else the larger one sent.
pub fn bot_limit(root: &RootConfig) -> Option<u32> {
    let limits = &root.features.limits;
    limits
        .default
        .bots
        .or(limits.new_user.bots)
        .filter(|&max| max > 0)
}

/// Whether `count` bots is as many as `max` allows.
pub fn at_limit(count: usize, max: u32) -> bool {
    count >= max as usize
}

/// The count over the list: "2 of 5 bots".
pub fn count_line(count: usize, max: u32) -> String {
    format!("{count} of {max} bots")
}

/// The confirmation before deleting bot `name`: its title and what happens.
pub fn delete_prompt(name: &str) -> (String, &'static str) {
    (
        format!("Delete {name}?"),
        "It leaves every server and group it's in. This can't be undone.",
    )
}

/// Why Stoat refused a change to a bot. `owned` is how many bots the user
/// has, when known (Stoat's `ReachedMaximumBots` doesn't say).
pub fn bot_error_text(err: &ApiError, owned: Option<usize>) -> String {
    match err.error_type() {
        Some(ErrorType::InvalidUsername) => "That username can't be used. Try another.".into(),
        Some(ErrorType::UsernameTaken) => "That username is taken.".into(),
        Some(ErrorType::ReachedMaximumBots) => match owned {
            Some(1) => "You have 1 bot, the most Stoat allows.".into(),
            Some(n) if n > 0 => format!("You have {n} bots, the most Stoat allows."),
            _ => "You have as many bots as Stoat allows.".into(),
        },
        Some(ErrorType::IsBot) => "Bots can't manage bots.".into(),
        Some(ErrorType::BotIsPrivate) => "That bot is private.".into(),
        Some(ErrorType::NotFound) => "That bot doesn't exist, or it's private.".into(),
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to add bots there.".into()
        }
        Some(ErrorType::AlreadyInServer | ErrorType::AlreadyInGroup) => {
            "The bot is already there.".into()
        }
        Some(ErrorType::CannotGiveMissingPermissions) => {
            "You can't grant permissions you don't have.".into()
        }
        Some(ErrorType::FailedValidation { .. }) => {
            "Check the bot's name and interactions URL.".into()
        }
        _ => error_text(err, "Couldn't change the bot."),
    }
}

/// The longest interactions URL Stoat takes, in characters.
pub const INTERACTIONS_URL_MAX: usize = 2048;

/// A bot's settings, as edited: everything the bar's Save sends. Its
/// username has its own Save (a rename can fail on its own).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BotForm {
    pub public: bool,
    /// As typed; blank means none.
    pub interactions_url: String,
    /// The permissions it asks for when added to a server.
    pub default_permissions: u64,
}

impl BotForm {
    /// The form filled with `bot`'s values.
    pub fn of(bot: &Bot) -> Self {
        Self {
            public: bot.public,
            interactions_url: bot.interactions_url.clone(),
            default_permissions: bot.default_permissions.unwrap_or(0) as u64,
        }
    }

    /// Whether Stoat would take it: the URL is at most
    /// [`INTERACTIONS_URL_MAX`] characters.
    pub fn is_valid(&self) -> bool {
        self.interactions_url.trim().chars().count() <= INTERACTIONS_URL_MAX
    }

    /// The `PATCH /bots/{id}` that turns `bot` into this form: the public
    /// flag, a trimmed URL (a blank one becomes `remove: [InteractionsURL]`)
    /// and the requested permissions, each only if changed. `None` when
    /// nothing changed or the form isn't valid.
    pub fn changes(&self, bot: &Bot) -> Option<DataEditBot> {
        if !self.is_valid() {
            return None;
        }
        let mut edit = DataEditBot::default();
        if self.public != bot.public {
            edit.public = Some(self.public);
        }
        let url = self.interactions_url.trim();
        if url != bot.interactions_url {
            if url.is_empty() {
                edit.remove.push(FieldsBot::InteractionsURL);
            } else {
                edit.interactions_url = Some(url.to_string());
            }
        }
        if self.default_permissions != Self::of(bot).default_permissions {
            edit.default_permissions = Some(self.default_permissions as i64);
        }
        (edit != DataEditBot::default()).then_some(edit)
    }
}

/// The rename to send: `typed` as a username Stoat takes, if it isn't the
/// bot's username already.
pub fn username_change(typed: &str, current: &str) -> Option<String> {
    check_bot_username(typed)
        .ok()
        .filter(|name| name != current)
}

/// The rename's `PATCH /bots/{id}`.
pub fn rename_edit(name: String) -> DataEditBot {
    DataEditBot {
        name: Some(name),
        ..Default::default()
    }
}

/// The `PATCH /bots/{id}` that gives the bot a new token: `remove: [Token]`
/// and nothing else.
pub fn token_reset_edit() -> DataEditBot {
    DataEditBot {
        remove: vec![FieldsBot::Token],
        ..Default::default()
    }
}

/// The confirmation before resetting bot `name`'s token: its title and
/// what happens.
pub fn reset_prompt(name: &str) -> (String, &'static str) {
    (
        format!("Reset {name}'s token?"),
        "The old token stops working at once. Anything using it needs the new one.",
    )
}

/// The link that adds the bot, on the instance's web client `app`:
/// `{app}/bot/{id}`.
pub fn invite_url(app: &str, id: &str) -> String {
    format!("{}/bot/{id}", app.trim_end_matches('/'))
}
