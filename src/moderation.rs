//! Acting on a server's members (kicks and bans), and why Stoat refused,
//! in words. Plain data with no GPUI, so it's unit-testable. Who may do
//! what is [`crate::permissions::moderation`]; the dialogs are in
//! [`crate::ui::moderation_dialogs`].

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::profile_edit::error_text;

/// The longest ban reason Stoat takes.
pub const BAN_REASON_MAX: usize = 1024;

/// How much of a banned person's message history goes with them: the web
/// client's choices, as labels and `delete_message_seconds`.
pub const BAN_HISTORY: [(&str, i64); 6] = [
    ("None", 0),
    ("1 hour", 3600),
    ("6 hours", 6 * 3600),
    ("1 day", 86_400),
    ("3 days", 3 * 86_400),
    ("7 days", 7 * 86_400),
];

/// What a toast says when Stoat refuses a moderation action, else
/// `fallback` (or why a change was refused, such as a rate limit).
pub fn moderation_error_text(err: &ApiError, fallback: &str) -> String {
    match err.error_type() {
        Some(ErrorType::NotElevated | ErrorType::IsElevated) => {
            "They rank too high for you to do that.".into()
        }
        Some(ErrorType::MissingPermission { .. }) => "You don't have permission to do that.".into(),
        Some(ErrorType::CannotReportYourself) => "You can't report yourself.".into(),
        Some(ErrorType::NotConnected) => "They're no longer in a call.".into(),
        _ => error_text(err, fallback),
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
    fn ban_history_runs_from_none_to_stoats_most() {
        let seconds: Vec<i64> = BAN_HISTORY.iter().map(|(_, s)| *s).collect();
        assert_eq!(seconds, [0, 3600, 21_600, 86_400, 259_200, 604_800]);
    }

    #[test]
    fn stoats_refusals_read_plainly() {
        let rank = "They rank too high for you to do that.";
        assert_eq!(
            moderation_error_text(&api_error(ErrorType::NotElevated), "x"),
            rank
        );
        assert_eq!(
            moderation_error_text(&api_error(ErrorType::IsElevated), "x"),
            rank
        );
        let missing = api_error(ErrorType::MissingPermission {
            permission: "KickMembers".into(),
        });
        assert_eq!(
            moderation_error_text(&missing, "x"),
            "You don't have permission to do that."
        );
        assert_eq!(
            moderation_error_text(&api_error(ErrorType::CannotReportYourself), "x"),
            "You can't report yourself."
        );
        assert_eq!(
            moderation_error_text(&api_error(ErrorType::NotConnected), "x"),
            "They're no longer in a call."
        );
    }

    #[test]
    fn rate_limits_and_the_rest_fall_back() {
        let limited = ApiError::RateLimited {
            retry_after_ms: Some(1500),
        };
        assert_eq!(
            moderation_error_text(&limited, "x"),
            "Stoat is limiting changes. Try again in 2 s."
        );
        assert_eq!(
            moderation_error_text(&api_error(ErrorType::InternalError), "Couldn't kick them."),
            "Couldn't kick them."
        );
    }
}
