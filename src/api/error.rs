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
    /// Classify a non-success HTTP response.
    pub fn from_response(status: StatusCode, body: &[u8]) -> Self {
        if status == StatusCode::TOO_MANY_REQUESTS {
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
    fn unknown_body_maps_to_unexpected() {
        let err = ApiError::from_response(StatusCode::BAD_GATEWAY, b"<html>bad gateway</html>");
        assert!(matches!(err, ApiError::Unexpected { status: 502, .. }));
    }

    #[test]
    fn unmapped_error_types_fall_back_to_variant_name() {
        let err = ApiError::from_response(StatusCode::FORBIDDEN, br#"{"type":"Blacklisted"}"#);
        assert_eq!(err.to_string(), "Stoat returned an error: Blacklisted");
    }
}
