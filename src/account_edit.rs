//! Changing the account's login details: its username, email and
//! password. The checks before sending mirror Stoat's, and each refusal is
//! placed under the field it's about. Plain data with no GPUI, so it's
//! unit-testable.

use revolt_result::ErrorType;
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

use crate::api::ApiError;
use crate::api::models::MultiFactorStatus;
use crate::profile_edit::error_text;
use crate::security::{ConfirmMethod, confirm_methods};

/// Usernames: 2 to 32 characters (`delta`'s `DataChangeUsername`).
pub const USERNAME_MIN: usize = 2;
pub const USERNAME_MAX: usize = 32;
/// Stoat's shortest password, in bytes. It also refuses a username change
/// whose current password is shorter, as invalid rather than wrong.
pub const PASSWORD_MIN_BYTES: usize = 8;

/// Names Stoat never gives out, in any case.
const BLOCKED_USERNAMES: [&str; 3] = ["admin", "revolt", "stoat"];

/// Which part of a change dialog something is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeField {
    /// The new username, email or password.
    Value,
    /// The new password, typed again.
    Confirm,
    /// The current password.
    Current,
    /// The authenticator or recovery code.
    Code,
    /// The dialog as a whole, above its buttons (a rate limit).
    Form,
}

/// What to do when Stoat refuses a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeError {
    /// Say this at that field, and let them try again.
    Field(ChangeField, String),
    /// Give up, with this in a toast.
    Toast(String),
}

/// Check a new username as Stoat will: trimmed, 2–32 letters, digits, `_`,
/// `.` or `-`, and not a reserved name. `Err(None)` has nothing to say yet
/// (blank, or the current name unchanged); a change of case only is fine.
pub fn check_username<'a>(text: &'a str, current: &str) -> Result<&'a str, Option<&'static str>> {
    let name = text.trim();
    if name.is_empty() || name == current {
        return Err(None);
    }
    let count = name.chars().count();
    if !(USERNAME_MIN..=USERNAME_MAX).contains(&count) {
        return Err(Some("Use 2–32 characters."));
    }
    if !name.chars().all(username_char) {
        return Err(Some("Use only letters, numbers, _, . or -."));
    }
    if BLOCKED_USERNAMES.contains(&name.to_lowercase().as_str()) {
        return Err(Some("Stoat doesn't allow that username."));
    }
    Ok(name)
}

/// Stoat's `^(\p{L}|[\d_.-])+$`, one character at a time.
fn username_char(c: char) -> bool {
    c.general_category_group() == GeneralCategoryGroup::Letter
        || c.general_category() == GeneralCategory::DecimalNumber
        || matches!(c, '_' | '.' | '-')
}

/// What a refused username change says. `short_password` is whether the
/// current password typed is under Stoat's minimum, which makes its
/// `FailedValidation` about the password rather than the name.
pub fn username_error(err: &ApiError, short_password: bool) -> ChangeError {
    use ChangeField::{Current, Value};
    change_error(err, "Couldn't change your username. Try again.", |error| {
        Some(match error {
            ErrorType::FailedValidation { .. } if short_password => {
                (Current, "That password isn't right.")
            }
            ErrorType::FailedValidation { .. } => (Value, "Use 2–32 letters, numbers, _, . or -."),
            ErrorType::InvalidUsername => (Value, "Stoat doesn't allow that username."),
            ErrorType::UsernameTaken => (Value, "That username is taken. Try another."),
            ErrorType::DiscriminatorChangeRatelimited => (
                Value,
                "Your #number can only change once a day, and that name needs a new one. \
                 Try again tomorrow, or pick another name.",
            ),
            _ => return None,
        })
    })
}

/// What a refused email change says.
pub fn email_error(err: &ApiError) -> ChangeError {
    change_error(err, "Couldn't change your email. Try again.", |error| {
        Some(match error {
            ErrorType::IncorrectData { .. } => {
                (ChangeField::Value, "That isn't a valid email address.")
            }
            ErrorType::Blacklisted => (
                ChangeField::Value,
                "Stoat doesn't accept addresses from that provider.",
            ),
            _ => return None,
        })
    })
}

/// What any refused change says: a wrong current password, a rate limit
/// (with the wait), what `specific` places at a field, or else a toast
/// with `fallback`.
fn change_error(
    err: &ApiError,
    fallback: &str,
    specific: impl Fn(&ErrorType) -> Option<(ChangeField, &'static str)>,
) -> ChangeError {
    match err {
        ApiError::RateLimited { .. } => ChangeError::Field(ChangeField::Form, error_text(err, "")),
        ApiError::Unauthorized | ApiError::Network(_) => ChangeError::Toast(err.to_string()),
        _ => match err.error_type() {
            Some(ErrorType::InvalidCredentials) => {
                ChangeError::Field(ChangeField::Current, "That password isn't right.".into())
            }
            Some(error) => match specific(error) {
                Some((field, text)) => ChangeError::Field(field, text.into()),
                None => ChangeError::Toast(error_text(err, fallback)),
            },
            None => ChangeError::Toast(error_text(err, fallback)),
        },
    }
}

/// Under the code field after a change failed with it: Stoat used the
/// code up on the way.
pub const CODE_USED: &str = "That code is used up. Enter a new one.";

/// The ways to give a code for a change that needs one: none without an
/// authenticator app (the current password is enough); with one, its
/// code, or a recovery code if any are left.
pub fn code_methods(status: &MultiFactorStatus) -> Vec<ConfirmMethod> {
    confirm_methods(status)
        .into_iter()
        .filter(|method| *method != ConfirmMethod::Password)
        .collect()
}

/// The email shown until it's revealed.
pub const MASKED_EMAIL: &str = "••••••••@••••••";

/// Check a new email roughly as Stoat will (it decides): trimmed, one `@`
/// with something before it, a domain with a dot after it, and no spaces.
/// `Err(None)` has nothing to say yet (blank).
pub fn check_email(text: &str) -> Result<&str, Option<&'static str>> {
    let email = text.trim();
    if email.is_empty() {
        return Err(None);
    }
    let valid = !email.contains(char::is_whitespace)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.contains('@')
                && domain
                    .split_once('.')
                    .is_some_and(|(name, rest)| !name.is_empty() && !rest.is_empty())
        });
    if valid {
        Ok(email)
    } else {
        Err(Some("That isn't a valid email address."))
    }
}

/// The toast after a username change, from what Stoat stored (it may have
/// cleaned the name up, or picked a new number).
pub fn username_changed_text(before_tag: &str, username: &str, tag: &str) -> String {
    if tag == before_tag {
        format!("Your username is now {username}#{tag}.")
    } else {
        format!(
            "Your username is now {username}#{tag}. #{before_tag} was taken under that name, \
             so Stoat gave you a new number."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use revolt_result::Error as StoatError;

    fn api(error_type: ErrorType) -> ApiError {
        ApiError::Api(StoatError {
            error_type,
            location: None,
        })
    }

    #[test]
    fn usernames_follow_stoats_rules() {
        assert_eq!(check_username("ada", "bob"), Ok("ada"));
        assert_eq!(check_username(" ada ", "bob"), Ok("ada"));
        assert_eq!(check_username("a.d_a-1", "bob"), Ok("a.d_a-1"));
        assert_eq!(check_username("Ådå", "bob"), Ok("Ådå"));
        assert_eq!(check_username("名前", "bob"), Ok("名前"));
        assert_eq!(
            check_username("a", "bob"),
            Err(Some("Use 2–32 characters."))
        );
        assert_eq!(check_username(&"a".repeat(32), "bob").map(str::len), Ok(32));
        assert_eq!(
            check_username(&"a".repeat(33), "bob"),
            Err(Some("Use 2–32 characters."))
        );
        for bad in ["ada lovelace", "ada!", "ada🙂"] {
            assert_eq!(
                check_username(bad, "bob"),
                Err(Some("Use only letters, numbers, _, . or -.")),
                "{bad}"
            );
        }
        for reserved in ["Stoat", "ADMIN", "revolt"] {
            assert_eq!(
                check_username(reserved, "bob"),
                Err(Some("Stoat doesn't allow that username."))
            );
        }
    }

    #[test]
    fn a_blank_or_unchanged_username_says_nothing() {
        assert_eq!(check_username("", "ada"), Err(None));
        assert_eq!(check_username("   ", "ada"), Err(None));
        assert_eq!(check_username("ada", "ada"), Err(None));
        // A change of case is a change.
        assert_eq!(check_username("Ada", "ada"), Ok("Ada"));
    }

    #[test]
    fn username_refusals_land_on_their_field() {
        let at = |field, text: &str| ChangeError::Field(field, text.into());
        assert_eq!(
            username_error(&api(ErrorType::InvalidCredentials), false),
            at(ChangeField::Current, "That password isn't right.")
        );
        let invalid = api(ErrorType::FailedValidation {
            error: "username: Validation error: regex".into(),
        });
        assert_eq!(
            username_error(&invalid, false),
            at(ChangeField::Value, "Use 2–32 letters, numbers, _, . or -.")
        );
        assert_eq!(
            username_error(&invalid, true),
            at(ChangeField::Current, "That password isn't right.")
        );
        assert_eq!(
            username_error(&api(ErrorType::InvalidUsername), false),
            at(ChangeField::Value, "Stoat doesn't allow that username.")
        );
        assert_eq!(
            username_error(&api(ErrorType::UsernameTaken), false),
            at(ChangeField::Value, "That username is taken. Try another.")
        );
        assert!(matches!(
            username_error(&api(ErrorType::DiscriminatorChangeRatelimited), false),
            ChangeError::Field(ChangeField::Value, text) if text.contains("once a day")
        ));
        assert_eq!(
            username_error(
                &ApiError::RateLimited {
                    retry_after_ms: Some(2500)
                },
                false
            ),
            at(
                ChangeField::Form,
                "Stoat is limiting changes. Try again in 3 s."
            )
        );
        assert_eq!(
            username_error(
                &ApiError::Unexpected {
                    status: 500,
                    detail: "x".into()
                },
                false
            ),
            ChangeError::Toast("Couldn't change your username. Try again.".into())
        );
    }

    #[test]
    fn the_toast_says_when_the_number_changed() {
        assert_eq!(
            username_changed_text("1234", "ada", "1234"),
            "Your username is now ada#1234."
        );
        assert_eq!(
            username_changed_text("1234", "ada", "5678"),
            "Your username is now ada#5678. #1234 was taken under that name, so Stoat gave \
             you a new number."
        );
    }

    #[test]
    fn emails_are_roughly_checked() {
        assert_eq!(check_email("a@b.co"), Ok("a@b.co"));
        assert_eq!(check_email(" a@b.co "), Ok("a@b.co"));
        assert_eq!(
            check_email("a.b+c@mail.example.org"),
            Ok("a.b+c@mail.example.org")
        );
        assert_eq!(check_email(""), Err(None));
        assert_eq!(check_email("  "), Err(None));
        for bad in [
            "ab.co", "a@", "@b.co", "a@b", "a@.co", "a@b.", "a b@c.de", "a@b@c.de",
        ] {
            assert_eq!(
                check_email(bad),
                Err(Some("That isn't a valid email address.")),
                "{bad}"
            );
        }
    }

    #[test]
    fn email_refusals_land_on_their_field() {
        let at = |field, text: &str| ChangeError::Field(field, text.into());
        assert_eq!(
            email_error(&api(ErrorType::IncorrectData {
                with: "email".into()
            })),
            at(ChangeField::Value, "That isn't a valid email address.")
        );
        assert_eq!(
            email_error(&api(ErrorType::Blacklisted)),
            at(
                ChangeField::Value,
                "Stoat doesn't accept addresses from that provider."
            )
        );
        assert_eq!(
            email_error(&api(ErrorType::InvalidCredentials)),
            at(ChangeField::Current, "That password isn't right.")
        );
        assert_eq!(
            email_error(&ApiError::RateLimited {
                retry_after_ms: None
            }),
            at(
                ChangeField::Form,
                "Stoat is limiting changes. Try again in a moment."
            )
        );
        assert_eq!(
            email_error(&api(ErrorType::InternalError)),
            ChangeError::Toast("Couldn't change your email. Try again.".into())
        );
    }

    #[test]
    fn a_code_is_asked_for_only_with_an_authenticator() {
        let status = |json| serde_json::from_str::<MultiFactorStatus>(json).unwrap();
        assert_eq!(code_methods(&status("{}")), []);
        assert_eq!(code_methods(&status(r#"{"recovery_active":true}"#)), []);
        assert_eq!(
            code_methods(&status(r#"{"totp_mfa":true}"#)),
            [ConfirmMethod::Totp]
        );
        assert_eq!(
            code_methods(&status(r#"{"totp_mfa":true,"recovery_active":true}"#)),
            [ConfirmMethod::Totp, ConfirmMethod::Recovery]
        );
    }
}
