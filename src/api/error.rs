use reqwest::StatusCode;
use revolt_result::{Error as StoatError, ErrorType};
use serde::Deserialize;

/// Anything that can go wrong talking to the Stoat API.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The server rejected the request with a Stoat error body
    /// (`{"type": "InvalidCredentials", ...}`), parsed with the backend's own
    /// `revolt_result::Error` type.
    #[error("{}", describe(&.0.error_type))]
    Api(StoatError),

    /// The session token is missing, invalid, or revoked.
    #[error("Your session has expired. Please log in again.")]
    Unauthorized,

    /// Too many requests. `retry_after_ms` comes from the response body when present.
    #[error("Stoat is rate-limiting this client. Wait a moment and try again.")]
    RateLimited { retry_after_ms: Option<u64> },

    /// No response at all: offline, DNS, TLS, or a timeout.
    #[error("Couldn't reach Stoat. Check your connection and try again.")]
    Network(#[source] reqwest::Error),

    /// A file to upload couldn't be read: missing, unreadable, or it got
    /// shorter while it was being sent.
    #[error("Couldn't read the file ({0}).")]
    File(#[source] std::io::Error),

    /// The server answered, but not in a shape this client understands.
    #[error("Unexpected response from Stoat (HTTP {status}): {detail}")]
    Unexpected { status: u16, detail: String },
}

impl ApiError {
    /// Classify a non-success HTTP response. A 429 is `RateLimited`, except
    /// a slowmode refusal (`{"type":"InSlowmode","retry_after":N}`, in
    /// seconds) and the once-a-day limit on a new `#tag`
    /// (`{"type":"DiscriminatorChangeRatelimited"}`), which are the Stoat
    /// errors they are.
    pub fn from_response(status: StatusCode, body: &[u8]) -> Self {
        if status == StatusCode::TOO_MANY_REQUESTS {
            if let Ok(err) = serde_json::from_slice::<StoatError>(body)
                && matches!(
                    err.error_type,
                    ErrorType::InSlowmode { .. } | ErrorType::DiscriminatorChangeRatelimited
                )
            {
                return ApiError::Api(err);
            }
            #[derive(Deserialize)]
            struct RateLimit {
                retry_after: Option<u64>,
            }
            let retry_after_ms = serde_json::from_slice::<RateLimit>(body)
                .ok()
                .and_then(|r| r.retry_after);
            return ApiError::RateLimited { retry_after_ms };
        }

        match serde_json::from_slice::<StoatError>(body) {
            Ok(err) => match err.error_type {
                ErrorType::InvalidSession | ErrorType::NotAuthenticated => ApiError::Unauthorized,
                _ => ApiError::Api(err),
            },
            Err(_) if status == StatusCode::UNAUTHORIZED => ApiError::Unauthorized,
            Err(_) => ApiError::Unexpected {
                status: status.as_u16(),
                detail: snippet(body),
            },
        }
    }

    /// Stoat says the message is already in the state a pin or unpin asked
    /// for (`AlreadyPinned`, `NotPinned`): someone else got there first.
    pub fn pin_already_done(&self) -> bool {
        matches!(
            self.error_type(),
            Some(ErrorType::AlreadyPinned | ErrorType::NotPinned)
        )
    }

    /// Stoat says a friend request or block is already as asked
    /// (`AlreadyFriends`, `AlreadySentRequest`, `NoEffect`): another client
    /// got there first.
    pub fn relationship_already_done(&self) -> bool {
        matches!(
            self.error_type(),
            Some(ErrorType::AlreadyFriends | ErrorType::AlreadySentRequest | ErrorType::NoEffect)
        )
    }

    /// Seconds left before the user may send again, if Stoat refused a
    /// message because of the channel's slowmode.
    pub fn slowmode_retry_after(&self) -> Option<u64> {
        match self.error_type() {
            Some(ErrorType::InSlowmode { retry_after }) => Some(*retry_after),
            _ => None,
        }
    }

    /// Stoat already has a message with this send's idempotency key: an
    /// earlier attempt got through, though its answer was lost.
    pub fn is_duplicate_nonce(&self) -> bool {
        matches!(self.error_type(), Some(ErrorType::DuplicateNonce))
    }

    /// What a message that failed to send says under it.
    pub fn send_failure_text(&self) -> String {
        match self {
            ApiError::Network(_) => "Couldn't reach Stoat.".into(),
            ApiError::RateLimited { .. } => "Sending too fast. Wait a moment, then retry.".into(),
            _ => match self.error_type() {
                Some(ErrorType::MissingPermission { .. }) => "You can't send messages here.".into(),
                Some(ErrorType::PayloadTooLarge) => "This message is too long.".into(),
                _ => self.to_string(),
            },
        }
    }

    /// What a failed pin (`pin`) or unpin says.
    pub fn pin_failure_text(&self, pin: bool) -> String {
        let (done, doing) = if pin {
            ("pin", "pinning")
        } else {
            ("unpin", "unpinning")
        };
        match self {
            ApiError::RateLimited { .. } => {
                format!("Too many requests while {doing}. Wait a moment, then try again.")
            }
            _ => match self.error_type() {
                Some(ErrorType::MissingPermission { .. }) => {
                    format!("You can't {done} messages here.")
                }
                _ => format!("Couldn't {done} the message: {self}"),
            },
        }
    }

    /// The Stoat error type, if the server sent one.
    pub fn error_type(&self) -> Option<&ErrorType> {
        match self {
            ApiError::Api(err) => Some(&err.error_type),
            _ => None,
        }
    }
}

/// Human-readable text for the errors a user is likely to see while logging
/// in. Everything else falls back to the backend's variant name.
fn describe(error: &ErrorType) -> String {
    match error {
        ErrorType::InvalidCredentials => "Incorrect email or password.".into(),
        ErrorType::InvalidToken => "That code isn't valid. Check it and try again.".into(),
        ErrorType::UnverifiedAccount => "Verify your email address before logging in.".into(),
        ErrorType::LockedOut => {
            "Too many failed attempts. This account is temporarily locked.".into()
        }
        ErrorType::DisallowedMFAMethod => {
            "That verification method isn't enabled on this account.".into()
        }
        ErrorType::CaptchaFailed => {
            "Stoat asked for a captcha, which this client can't show yet.".into()
        }
        ErrorType::AlreadyPinned => "That message is already pinned.".into(),
        ErrorType::NotPinned => "That message isn't pinned.".into(),
        ErrorType::AlreadyFriends => "You're already friends.".into(),
        ErrorType::AlreadySentRequest => "You've already sent them a friend request.".into(),
        ErrorType::Blocked => "You've blocked them. Unblock them first.".into(),
        ErrorType::BlockedByOther => "They've blocked you.".into(),
        ErrorType::NotFriends => "You aren't friends.".into(),
        ErrorType::IsBot => "Bots can't be friends.".into(),
        ErrorType::TooManyPendingFriendRequests { max } => {
            format!("You have too many pending friend requests (at most {max}).")
        }
        ErrorType::NoEffect => "Nothing changed.".into(),
        ErrorType::InSlowmode { retry_after } => {
            format!(
                "Slowmode: try again in {}",
                crate::time::format_countdown(*retry_after)
            )
        }
        other => {
            let name: &'static str = other.into();
            format!("Stoat returned an error: {name}")
        }
    }
}

fn snippet(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    if text.is_empty() {
        return "(empty body)".into();
    }
    text.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_credentials_maps_to_api_error_with_readable_text() {
        let err = ApiError::from_response(
            StatusCode::UNAUTHORIZED,
            br#"{"type":"InvalidCredentials","location":"crates/core/database/src/models/accounts.rs:42"}"#,
        );
        assert!(matches!(
            err.error_type(),
            Some(ErrorType::InvalidCredentials)
        ));
        assert_eq!(err.to_string(), "Incorrect email or password.");
    }

    #[test]
    fn invalid_session_maps_to_unauthorized() {
        let err =
            ApiError::from_response(StatusCode::UNAUTHORIZED, br#"{"type":"InvalidSession"}"#);
        assert!(matches!(err, ApiError::Unauthorized));
    }

    #[test]
    fn unparseable_401_maps_to_unauthorized() {
        let err = ApiError::from_response(StatusCode::UNAUTHORIZED, b"Unauthorized");
        assert!(matches!(err, ApiError::Unauthorized));
    }

    #[test]
    fn rate_limit_reads_retry_after() {
        let err =
            ApiError::from_response(StatusCode::TOO_MANY_REQUESTS, br#"{"retry_after":1500}"#);
        assert!(matches!(
            err,
            ApiError::RateLimited {
                retry_after_ms: Some(1500)
            }
        ));
    }

    #[test]
    fn a_new_tag_too_soon_isnt_a_rate_limit() {
        let err = ApiError::from_response(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"type":"DiscriminatorChangeRatelimited"}"#,
        );
        assert!(matches!(
            err.error_type(),
            Some(ErrorType::DiscriminatorChangeRatelimited)
        ));
    }

    #[test]
    fn unknown_body_maps_to_unexpected() {
        let err = ApiError::from_response(StatusCode::BAD_GATEWAY, b"<html>bad gateway</html>");
        assert!(matches!(err, ApiError::Unexpected { status: 502, .. }));
    }

    #[test]
    fn unmapped_error_types_fall_back_to_variant_name() {
        let err = ApiError::from_response(StatusCode::FORBIDDEN, br#"{"type":"Blacklisted"}"#);
        assert_eq!(err.to_string(), "Stoat returned an error: Blacklisted");
    }

    #[test]
    fn pin_errors_are_told_apart_from_the_rest() {
        let pinned =
            ApiError::from_response(StatusCode::BAD_REQUEST, br#"{"type":"AlreadyPinned"}"#);
        let unpinned = ApiError::from_response(StatusCode::BAD_REQUEST, br#"{"type":"NotPinned"}"#);
        let other =
            ApiError::from_response(StatusCode::BAD_REQUEST, br#"{"type":"InvalidOperation"}"#);
        assert!(pinned.pin_already_done());
        assert!(unpinned.pin_already_done());
        assert!(!other.pin_already_done());
        assert!(!ApiError::Unauthorized.pin_already_done());
        assert_eq!(pinned.to_string(), "That message is already pinned.");
    }

    #[test]
    fn relationship_errors_read_well_and_some_count_as_done() {
        let friends =
            ApiError::from_response(StatusCode::CONFLICT, br#"{"type":"AlreadyFriends"}"#);
        let blocked =
            ApiError::from_response(StatusCode::FORBIDDEN, br#"{"type":"BlockedByOther"}"#);
        let too_many = ApiError::from_response(
            StatusCode::BAD_REQUEST,
            br#"{"type":"TooManyPendingFriendRequests","max":100}"#,
        );
        assert!(friends.relationship_already_done());
        assert!(!blocked.relationship_already_done());
        assert!(!too_many.relationship_already_done());
        assert_eq!(blocked.to_string(), "They've blocked you.");
        assert_eq!(
            too_many.to_string(),
            "You have too many pending friend requests (at most 100)."
        );
    }
}
