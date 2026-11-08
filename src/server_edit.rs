//! Making and changing servers: Stoat's rules for a name, and why a
//! request was refused, in words. Plain data with no GPUI, so it's
//! unit-testable.

use revolt_result::ErrorType;

use crate::api::ApiError;
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

/// Why `DELETE /channels/{id}` was refused. (`NotFound` isn't a refusal:
/// the channel is already gone.)
pub fn delete_channel_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => "You can't delete this channel.".into(),
        _ => error_text(err, "Couldn't delete the channel. Try again."),
    }
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
}
