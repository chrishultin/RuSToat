//! Making and changing servers: Stoat's rules for a name, edits to a
//! server's categories, why a request was refused, in words, and what the
//! channel header shows of a description. Plain data with no GPUI, so it's
//! unit-testable.

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::Category;
use crate::profile_edit::error_text;

/// Server, channel and category names: 1 to 32 characters, as Stoat
/// counts them (characters, not bytes).
pub const NAME_MAX: usize = 32;

/// The Acceptable Use Policy, linked from Create server on the official
/// instance.
pub const AUP_URL: &str = "https://stoat.chat/aup";

/// `text` as a name Stoat accepts: trimmed, 1 to [`NAME_MAX`] characters.
/// `None` when it's blank or too long.
pub fn valid_name(text: &str) -> Option<String> {
    let name = text.trim();
    (1..=NAME_MAX)
        .contains(&name.chars().count())
        .then(|| name.to_string())
}

/// `categories` with `channel` moved to the end of category `category`,
/// and out of any other (each channel is listed once: Stoat refuses a
/// list with one twice). `None` if there's no such category any more.
pub fn with_channel_in(
    categories: &[Category],
    channel: &str,
    category: &str,
) -> Option<Vec<Category>> {
    if !categories.iter().any(|c| c.id == category) {
        return None;
    }
    let mut edited = categories.to_vec();
    for c in &mut edited {
        c.channels.retain(|id| id != channel);
        if c.id == category {
            c.channels.push(channel.to_string());
        }
    }
    listed_once(&mut edited);
    Some(edited)
}

/// A new category's ID: a ULID, as the web client makes them (the time,
/// then 80 random bits, so two made at once don't collide).
pub fn new_category_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// `categories` with a new, empty category `id` called `title` at the end.
pub fn with_category_added(categories: &[Category], id: &str, title: &str) -> Vec<Category> {
    let mut edited = categories.to_vec();
    edited.push(Category {
        id: id.to_string(),
        title: title.to_string(),
        channels: Vec::new(),
    });
    listed_once(&mut edited);
    edited
}

/// `categories` with category `id` called `title`, and nothing else
/// changed. `None` if there's no such category any more.
pub fn with_category_renamed(
    categories: &[Category],
    id: &str,
    title: &str,
) -> Option<Vec<Category>> {
    if !categories.iter().any(|c| c.id == id) {
        return None;
    }
    let mut edited = categories.to_vec();
    for c in edited.iter_mut().filter(|c| c.id == id) {
        c.title = title.to_string();
    }
    listed_once(&mut edited);
    Some(edited)
}

/// `categories` without category `id`; its channels become uncategorized
/// (none are deleted). It may leave none, which is sent as an empty list.
/// `None` if there's no such category any more.
pub fn without_category(categories: &[Category], id: &str) -> Option<Vec<Category>> {
    if !categories.iter().any(|c| c.id == id) {
        return None;
    }
    let mut edited = categories.to_vec();
    edited.retain(|c| c.id != id);
    listed_once(&mut edited);
    Some(edited)
}

/// What the channel header shows of a `description`: its first line
/// that isn't blank, trimmed, as typed (Markdown isn't rendered there).
/// `None` when there's no such line.
pub fn first_line(description: Option<&str>) -> Option<&str> {
    description?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
}

/// Keep each channel where it's first listed, and drop any later copy:
/// Stoat refuses a list with a channel twice.
fn listed_once(categories: &mut [Category]) {
    let mut seen = std::collections::HashSet::new();
    for c in categories {
        c.channels.retain(|id| seen.insert(id.clone()));
    }
}

/// Why `POST /servers/create` was refused.
pub fn create_server_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::TooManyServers { max }) => {
            format!("You're in {max} servers, the most Stoat allows. Leave one to create another.")
        }
        Some(ErrorType::CantCreateServers) => {
            "This Stoat instance doesn't let you create servers.".into()
        }
        _ => error_text(err, "Couldn't create the server. Try again."),
    }
}

/// Why `POST /servers/{id}/channels` was refused.
pub fn create_channel_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::TooManyChannels { max }) => {
            format!("This server already has {max} channels, the most Stoat allows.")
        }
        Some(ErrorType::MissingPermission { .. }) => {
            "You can't create channels in this server.".into()
        }
        _ => error_text(err, "Couldn't create the channel. Try again."),
    }
}

/// Why a change to a server's categories (`PATCH /servers/{id}`) was
/// refused.
pub fn categories_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => {
            "You can't manage channels in this server.".into()
        }
        Some(ErrorType::InvalidOperation) => {
            "Stoat refused that change to the categories. Try again.".into()
        }
        _ => error_text(err, "Couldn't change the categories. Try again."),
    }
}

/// Why `DELETE /channels/{id}` was refused. (`NotFound` isn't a refusal:
/// the channel is already gone.)
pub fn delete_channel_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => "You can't delete this channel.".into(),
        _ => error_text(err, "Couldn't delete the channel. Try again."),
    }
}

/// Why `DELETE /servers/{id}` (leaving) was refused. (`NotFound` isn't a
/// refusal: the user is already out.)
pub fn leave_error_text(err: &ApiError) -> String {
    error_text(err, "Couldn't leave the server. Try again.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_error(error_type: ErrorType) -> ApiError {
        ApiError::Api(revolt_result::Error {
            error_type,
            location: None,
        })
    }

    fn category(id: &str, channels: &[&str]) -> Category {
        Category {
            id: id.into(),
            title: id.to_uppercase(),
            channels: channels.iter().map(|c| c.to_string()).collect(),
        }
    }

    #[test]
    fn names_are_trimmed_and_1_to_32_characters() {
        assert_eq!(valid_name(""), None);
        assert_eq!(valid_name("   "), None);
        assert_eq!(valid_name(" Lounge ").as_deref(), Some("Lounge"));
        assert_eq!(valid_name(&"a".repeat(32)), Some("a".repeat(32)));
        assert_eq!(valid_name(&"a".repeat(33)), None);
        // Characters, not bytes: 32 "é" are 64 bytes.
        assert_eq!(valid_name(&"é".repeat(32)), Some("é".repeat(32)));
        assert_eq!(valid_name(&"é".repeat(33)), None);
    }

    #[test]
    fn create_server_refusals_say_why() {
        assert_eq!(
            create_server_error_text(&api_error(ErrorType::TooManyServers { max: 100 })),
            "You're in 100 servers, the most Stoat allows. Leave one to create another."
        );
        assert_eq!(
            create_server_error_text(&api_error(ErrorType::CantCreateServers)),
            "This Stoat instance doesn't let you create servers."
        );
        let limited = ApiError::RateLimited {
            retry_after_ms: Some(4200),
        };
        assert_eq!(
            create_server_error_text(&limited),
            "Stoat is limiting changes. Try again in 5 s."
        );
        assert_eq!(
            create_server_error_text(&api_error(ErrorType::InternalError)),
            "Couldn't create the server. Try again."
        );
    }

    #[test]
    fn delete_channel_refusals_say_why() {
        let missing = ErrorType::MissingPermission {
            permission: "ManageChannel".into(),
        };
        assert_eq!(
            delete_channel_error_text(&api_error(missing)),
            "You can't delete this channel."
        );
        let limited = ApiError::RateLimited {
            retry_after_ms: Some(4200),
        };
        assert_eq!(
            delete_channel_error_text(&limited),
            "Stoat is limiting changes. Try again in 5 s."
        );
        assert_eq!(
            delete_channel_error_text(&api_error(ErrorType::InternalError)),
            "Couldn't delete the channel. Try again."
        );
    }

    #[test]
    fn leave_refusals_say_why() {
        let limited = ApiError::RateLimited {
            retry_after_ms: Some(4200),
        };
        assert_eq!(
            leave_error_text(&limited),
            "Stoat is limiting changes. Try again in 5 s."
        );
        assert_eq!(
            leave_error_text(&api_error(ErrorType::InternalError)),
            "Couldn't leave the server. Try again."
        );
    }

    #[test]
    fn a_channel_moves_to_the_end_of_its_new_category() {
        let categories = [category("k1", &["c1", "c2"]), category("k2", &["c3"])];
        assert_eq!(
            with_channel_in(&categories, "c1", "k2").unwrap(),
            [category("k1", &["c2"]), category("k2", &["c3", "c1"])]
        );
    }

    #[test]
    fn a_new_channel_is_added_at_the_end_of_its_category() {
        let categories = [category("k1", &["c1"]), category("k2", &[])];
        assert_eq!(
            with_channel_in(&categories, "c9", "k1").unwrap(),
            [category("k1", &["c1", "c9"]), category("k2", &[])]
        );
        assert_eq!(
            with_channel_in(&categories, "c9", "k2").unwrap(),
            [category("k1", &["c1"]), category("k2", &["c9"])]
        );
    }

    #[test]
    fn a_channel_is_listed_once_however_often_it_was() {
        let categories = [category("k1", &["c1", "c2", "c1"]), category("k2", &["c1"])];
        assert_eq!(
            with_channel_in(&categories, "c1", "k1").unwrap(),
            [category("k1", &["c2", "c1"]), category("k2", &[])]
        );
    }

    #[test]
    fn a_missing_category_gives_nothing_to_send() {
        let categories = [category("k1", &["c1"])];
        assert_eq!(with_channel_in(&categories, "c1", "k-gone"), None);
        assert_eq!(with_channel_in(&[], "c1", "k1"), None);
    }

    #[test]
    fn a_new_category_is_appended_empty() {
        let categories = [category("k1", &["c1"])];
        let added = with_category_added(&categories, "k2", "Games");
        assert_eq!(added.len(), 2);
        assert_eq!(added[0], categories[0]);
        assert_eq!(
            added[1],
            Category {
                id: "k2".into(),
                title: "Games".into(),
                channels: Vec::new(),
            }
        );
        assert_eq!(with_category_added(&[], "k1", "Chat").len(), 1);
    }

    #[test]
    fn renaming_changes_only_that_title() {
        let categories = [category("k1", &["c1"]), category("k2", &["c2"])];
        let renamed = with_category_renamed(&categories, "k2", "Games").unwrap();
        assert_eq!(renamed[0], categories[0]);
        assert_eq!(renamed[1].id, "k2");
        assert_eq!(renamed[1].title, "Games");
        assert_eq!(renamed[1].channels, ["c2"]);
    }

    #[test]
    fn deleting_removes_only_that_category() {
        let categories = [
            category("k1", &["c1"]),
            category("k2", &["c2", "c3"]),
            category("k3", &[]),
        ];
        assert_eq!(
            without_category(&categories, "k2").unwrap(),
            [category("k1", &["c1"]), category("k3", &[])]
        );
        // The last one leaves an empty list, to send as it is.
        assert_eq!(
            without_category(&[category("k1", &["c1"])], "k1").unwrap(),
            []
        );
    }

    #[test]
    fn a_missing_category_changes_nothing() {
        let categories = [category("k1", &["c1"])];
        assert_eq!(with_category_renamed(&categories, "k-gone", "Games"), None);
        assert_eq!(without_category(&categories, "k-gone"), None);
        assert_eq!(without_category(&[], "k1"), None);
    }

    #[test]
    fn edits_list_each_channel_once() {
        let categories = [category("k1", &["c1", "c1"]), category("k2", &["c1", "c2"])];
        let once = [category("k1", &["c1"]), category("k2", &["c2"])];
        assert_eq!(with_category_added(&categories, "k3", "New")[..2], once);
        let renamed = with_category_renamed(&categories, "k1", "K1").unwrap();
        assert_eq!(renamed, once);
        assert_eq!(
            without_category(&categories, "k1").unwrap(),
            [category("k2", &["c1", "c2"])]
        );
    }

    #[test]
    fn category_ids_are_26_characters_and_differ() {
        let (a, b) = (new_category_id(), new_category_id());
        assert_eq!(a.len(), 26);
        assert_eq!(b.len(), 26);
        assert_ne!(a, b);
        // Crockford's base32: digits and capitals, without I, L, O or U.
        assert!(
            a.chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() && !"ILOU".contains(c))
        );
    }

    #[test]
    fn the_header_shows_the_first_line_that_isnt_blank() {
        assert_eq!(first_line(None), None);
        assert_eq!(first_line(Some("")), None);
        assert_eq!(first_line(Some("\n\n")), None);
        assert_eq!(first_line(Some("  \n\t\n")), None);
        assert_eq!(first_line(Some("Rules\nmore")), Some("Rules"));
        assert_eq!(first_line(Some("\n  Hi  \nx")), Some("Hi"));
        // As typed: Markdown stays, and Windows line ends go.
        assert_eq!(first_line(Some("**Be nice**\r\nx")), Some("**Be nice**"));
    }

    #[test]
    fn create_channel_refusals_say_why() {
        assert_eq!(
            create_channel_error_text(&api_error(ErrorType::TooManyChannels { max: 200 })),
            "This server already has 200 channels, the most Stoat allows."
        );
        let missing = ErrorType::MissingPermission {
            permission: "ManageChannel".into(),
        };
        assert_eq!(
            create_channel_error_text(&api_error(missing)),
            "You can't create channels in this server."
        );
        let limited = ApiError::RateLimited {
            retry_after_ms: Some(4200),
        };
        assert_eq!(
            create_channel_error_text(&limited),
            "Stoat is limiting changes. Try again in 5 s."
        );
        assert_eq!(
            create_channel_error_text(&api_error(ErrorType::InternalError)),
            "Couldn't create the channel. Try again."
        );
    }

    #[test]
    fn category_refusals_say_why() {
        let missing = ErrorType::MissingPermission {
            permission: "ManageChannel".into(),
        };
        assert_eq!(
            categories_error_text(&api_error(missing)),
            "You can't manage channels in this server."
        );
        assert_eq!(
            categories_error_text(&api_error(ErrorType::InvalidOperation)),
            "Stoat refused that change to the categories. Try again."
        );
        assert_eq!(
            categories_error_text(&api_error(ErrorType::InternalError)),
            "Couldn't change the categories. Try again."
        );
    }
}
