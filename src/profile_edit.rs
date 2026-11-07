//! Editing the user's own profile: Stoat's rules for each field, what a
//! Save sends, and what a failed one says. Plain data with no GPUI, so it's
//! unit-testable.

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{DataEditUser, DataUserProfile, FieldsUser};

/// Display names: 2 to 32 characters (`revolt-models`' `DataEditUser`).
pub const DISPLAY_NAME_MIN: usize = 2;
pub const DISPLAY_NAME_MAX: usize = 32;
/// Pronouns, globally or on a server: up to 24 characters.
pub const PRONOUNS_MAX: usize = 24;
/// The bio: up to 2000 characters.
pub const BIO_MAX: usize = 2000;
/// A server nickname: up to 32 characters.
pub const NICKNAME_MAX: usize = 32;

/// What saving one field would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldCheck<'a> {
    /// Set it to this (trimmed, except a bio).
    Ok(&'a str),
    /// Blank: remove it, falling back to the default.
    Clear,
    /// Under the field's minimum (this many characters).
    TooShort(usize),
    /// Over the field's maximum (this many characters).
    TooLong(usize),
    /// Stoat won't take it, for this reason.
    Invalid(&'static str),
}

impl FieldCheck<'_> {
    /// Whether Save can send this field.
    pub fn is_ok(&self) -> bool {
        matches!(self, FieldCheck::Ok(_) | FieldCheck::Clear)
    }

    /// The value to compare and send: `None` for a blank field. Only
    /// meaningful when [`Self::is_ok`].
    fn value(&self) -> Option<&str> {
        match self {
            FieldCheck::Ok(text) => Some(text),
            _ => None,
        }
    }
}

/// Check a length in characters, between `min` and `max`.
fn check_length(text: &str, min: usize, max: usize) -> FieldCheck<'_> {
    match text.chars().count() {
        0 => FieldCheck::Clear,
        n if n < min => FieldCheck::TooShort(n),
        n if n > max => FieldCheck::TooLong(n),
        _ => FieldCheck::Ok(text),
    }
}

/// A display name: trimmed, 2–32 characters, on one line and without
/// zero-width spaces (Stoat's `RE_DISPLAY_NAME`).
pub fn check_display_name(text: &str) -> FieldCheck<'_> {
    let text = text.trim();
    if text.contains(['\n', '\r']) {
        return FieldCheck::Invalid("Display names can't have line breaks.");
    }
    if text.contains('\u{200B}') {
        return FieldCheck::Invalid("Display names can't have zero-width spaces.");
    }
    check_length(text, DISPLAY_NAME_MIN, DISPLAY_NAME_MAX)
}

/// Pronouns: trimmed, up to 24 characters.
pub fn check_pronouns(text: &str) -> FieldCheck<'_> {
    check_length(text.trim(), 1, PRONOUNS_MAX)
}

/// A bio: up to 2000 characters, kept as typed (the web client doesn't
/// trim it either). Only whitespace counts as blank.
pub fn check_bio(text: &str) -> FieldCheck<'_> {
    if text.trim().is_empty() {
        return FieldCheck::Clear;
    }
    check_length(text, 0, BIO_MAX)
}

/// A server nickname: trimmed, up to 32 characters.
pub fn check_nickname(text: &str) -> FieldCheck<'_> {
    check_length(text.trim(), 1, NICKNAME_MAX)
}

/// The text fields of the profile form: as loaded from Stoat, or as typed.
/// A missing value is the empty string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileForm {
    pub display_name: String,
    pub pronouns: String,
    pub bio: String,
}

impl ProfileForm {
    /// Whether every field follows Stoat's rules, so Save can send them.
    pub fn is_valid(&self) -> bool {
        check_display_name(&self.display_name).is_ok()
            && check_pronouns(&self.pronouns).is_ok()
            && check_bio(&self.bio).is_ok()
    }

    /// The `PATCH /users/@me` that turns `loaded` into `edited`: only the
    /// fields that changed, with blanked ones in `remove`. A field that
    /// breaks its rule is left out (Save is disabled meanwhile).
    pub fn changes(loaded: &ProfileForm, edited: &ProfileForm) -> DataEditUser {
        let mut edit = empty_edit();
        match changed(
            check_display_name,
            &loaded.display_name,
            &edited.display_name,
        ) {
            Some(Some(name)) => edit.display_name = Some(name),
            Some(None) => edit.remove.push(FieldsUser::DisplayName),
            None => {}
        }
        match changed(check_pronouns, &loaded.pronouns, &edited.pronouns) {
            Some(Some(pronouns)) => edit.pronouns = Some(pronouns),
            Some(None) => edit.remove.push(FieldsUser::Pronouns),
            None => {}
        }
        match changed(check_bio, &loaded.bio, &edited.bio) {
            Some(Some(bio)) => {
                edit.profile = Some(DataUserProfile {
                    content: Some(bio),
                    background: None,
                })
            }
            Some(None) => edit.remove.push(FieldsUser::ProfileContent),
            None => {}
        }
        edit
    }
}

/// How a field changed: `None` if it didn't (or breaks its rule),
/// `Some(None)` if it was blanked, `Some(Some(value))` for a new value.
fn changed(
    check: fn(&str) -> FieldCheck<'_>,
    loaded: &str,
    edited: &str,
) -> Option<Option<String>> {
    let edited = check(edited);
    if !edited.is_ok() {
        return None;
    }
    let after = edited.value();
    // What Stoat has, seen the same way (as it is, if it breaks today's rule).
    let before = match check(loaded) {
        FieldCheck::Clear => None,
        FieldCheck::Ok(text) => Some(text),
        _ => Some(loaded),
    };
    (before != after).then(|| after.map(str::to_string))
}

/// Whether `edit` would change anything.
pub fn is_empty_edit(edit: &DataEditUser) -> bool {
    *edit == empty_edit()
}

/// A `DataEditUser` that changes nothing (it has no `Default`).
pub fn empty_edit() -> DataEditUser {
    DataEditUser {
        display_name: None,
        pronouns: None,
        avatar: None,
        status: None,
        profile: None,
        badges: None,
        flags: None,
        remove: Vec::new(),
    }
}

/// What a toast says when Stoat refuses a profile change.
pub fn save_error_text(err: &ApiError) -> String {
    match err {
        ApiError::RateLimited {
            retry_after_ms: Some(ms),
        } => format!(
            "Stoat is limiting changes. Try again in {} s.",
            ms.div_ceil(1000).max(1)
        ),
        ApiError::RateLimited {
            retry_after_ms: None,
        } => "Stoat is limiting changes. Try again in a moment.".into(),
        ApiError::Api(e) => match &e.error_type {
            ErrorType::FailedValidation { error } => validation_text(error),
            _ => SAVE_FAILED_TEXT.into(),
        },
        _ => SAVE_FAILED_TEXT.into(),
    }
}

/// Shown when a save fails for any other reason.
const SAVE_FAILED_TEXT: &str = "Couldn't save your profile.";

/// The rule a `FailedValidation` broke, from the field Stoat names in it
/// (`display_name: Validation error: length […]`).
fn validation_text(error: &str) -> String {
    let rule = if error.contains("display_name") {
        format!("Display names are {DISPLAY_NAME_MIN}–{DISPLAY_NAME_MAX} characters, on one line.")
    } else if error.contains("pronouns") {
        format!("Pronouns are at most {PRONOUNS_MAX} characters.")
    } else if error.contains("nickname") {
        format!("Nicknames are at most {NICKNAME_MAX} characters.")
    } else if error.contains("content") {
        format!("Bios are at most {BIO_MAX} characters.")
    } else {
        return format!("Stoat didn't accept that change ({error}).");
    };
    format!("Stoat didn't accept that change. {rule}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use revolt_result::Error as StoatError;

    fn form(display_name: &str, pronouns: &str, bio: &str) -> ProfileForm {
        ProfileForm {
            display_name: display_name.into(),
            pronouns: pronouns.into(),
            bio: bio.into(),
        }
    }

    #[test]
    fn display_names_are_2_to_32_characters() {
        assert_eq!(check_display_name("a"), FieldCheck::TooShort(1));
        assert_eq!(check_display_name("ab"), FieldCheck::Ok("ab"));
        let at = "a".repeat(32);
        assert_eq!(check_display_name(&at), FieldCheck::Ok(at.as_str()));
        assert_eq!(check_display_name(&"a".repeat(33)), FieldCheck::TooLong(33));
    }

    #[test]
    fn display_names_count_characters_not_bytes() {
        let emoji = "🦦".repeat(32);
        assert_eq!(check_display_name(&emoji), FieldCheck::Ok(emoji.as_str()));
        assert_eq!(check_display_name("é"), FieldCheck::TooShort(1));
    }

    #[test]
    fn display_names_are_one_line_without_zero_width_spaces() {
        assert!(matches!(
            check_display_name("Ada\nLovelace"),
            FieldCheck::Invalid(_)
        ));
        assert!(matches!(
            check_display_name("Ada\u{200B}"),
            FieldCheck::Invalid(_)
        ));
        // Surrounding line breaks are trimmed away.
        assert_eq!(check_display_name("\n Ada \n"), FieldCheck::Ok("Ada"));
    }

    #[test]
    fn blank_fields_clear() {
        for blank in ["", "   ", " \n\t "] {
            assert_eq!(check_display_name(blank), FieldCheck::Clear);
            assert_eq!(check_pronouns(blank), FieldCheck::Clear);
            assert_eq!(check_bio(blank), FieldCheck::Clear);
            assert_eq!(check_nickname(blank), FieldCheck::Clear);
        }
    }

    #[test]
    fn pronouns_are_at_most_24_characters() {
        let at = "a".repeat(24);
        assert_eq!(check_pronouns(&at), FieldCheck::Ok(at.as_str()));
        assert_eq!(check_pronouns(&"a".repeat(25)), FieldCheck::TooLong(25));
        assert_eq!(check_pronouns(" she/her "), FieldCheck::Ok("she/her"));
    }

    #[test]
    fn bios_are_at_most_2000_characters_and_kept_as_typed() {
        let at = "a".repeat(2000);
        assert_eq!(check_bio(&at), FieldCheck::Ok(at.as_str()));
        assert_eq!(check_bio(&"a".repeat(2001)), FieldCheck::TooLong(2001));
        assert_eq!(
            check_bio("  two\nlines\n"),
            FieldCheck::Ok("  two\nlines\n")
        );
    }

    #[test]
    fn nicknames_are_at_most_32_characters() {
        assert_eq!(check_nickname("A"), FieldCheck::Ok("A"));
        let at = "a".repeat(32);
        assert_eq!(check_nickname(&at), FieldCheck::Ok(at.as_str()));
        assert_eq!(check_nickname(&"a".repeat(33)), FieldCheck::TooLong(33));
    }

    #[test]
    fn nothing_changed_sends_nothing() {
        let loaded = form("Ada", "she/her", "hi");
        assert!(is_empty_edit(&ProfileForm::changes(&loaded, &loaded)));
        // Spaces around a name or pronouns aren't a change.
        let edited = form(" Ada ", "she/her ", "hi");
        assert!(is_empty_edit(&ProfileForm::changes(&loaded, &edited)));
        // Nor is blank where there was nothing.
        let empty = ProfileForm::default();
        assert!(is_empty_edit(&ProfileForm::changes(
            &empty,
            &form(" ", "", "\n")
        )));
    }

    #[test]
    fn only_the_changed_field_is_sent() {
        let loaded = form("Ada", "she/her", "hi");
        let edit = ProfileForm::changes(&loaded, &form("Ada L.", "she/her", "hi"));
        assert_eq!(
            edit,
            DataEditUser {
                display_name: Some("Ada L.".into()),
                ..empty_edit()
            }
        );
    }

    #[test]
    fn a_cleared_display_name_is_removed() {
        let edit = ProfileForm::changes(&form("Ada", "", ""), &form("  ", "", ""));
        assert_eq!(edit.remove, [FieldsUser::DisplayName]);
        assert_eq!(edit.display_name, None);
    }

    #[test]
    fn a_cleared_bio_is_removed_and_a_new_one_is_profile_content() {
        let edit = ProfileForm::changes(&form("", "", "hi"), &form("", "", ""));
        assert_eq!(edit.remove, [FieldsUser::ProfileContent]);
        assert_eq!(edit.profile, None);

        let edit = ProfileForm::changes(&form("", "", "hi"), &form("", "", "hi\nthere"));
        assert_eq!(
            edit.profile,
            Some(DataUserProfile {
                content: Some("hi\nthere".into()),
                background: None,
            })
        );
        assert!(edit.remove.is_empty());
    }

    #[test]
    fn a_field_breaking_its_rule_is_left_out() {
        let loaded = form("Ada", "", "");
        let edited = form("A", "she/her", "");
        assert!(!edited.is_valid());
        let edit = ProfileForm::changes(&loaded, &edited);
        assert_eq!(edit.display_name, None);
        assert_eq!(edit.pronouns.as_deref(), Some("she/her"));
    }

    #[test]
    fn a_rate_limit_says_how_long_to_wait_rounded_up() {
        let err = ApiError::RateLimited {
            retry_after_ms: Some(4090),
        };
        assert_eq!(
            save_error_text(&err),
            "Stoat is limiting changes. Try again in 5 s."
        );
        let err = ApiError::RateLimited {
            retry_after_ms: Some(10),
        };
        assert!(save_error_text(&err).contains("in 1 s."));
    }

    #[test]
    fn a_failed_validation_names_the_rule() {
        let err = ApiError::Api(StoatError {
            error_type: ErrorType::FailedValidation {
                error: "display_name: Validation error: length [{\"min\": Number(2)}]".into(),
            },
            location: None,
        });
        assert_eq!(
            save_error_text(&err),
            "Stoat didn't accept that change. Display names are 2–32 characters, on one line."
        );
    }

    #[test]
    fn anything_else_couldnt_save() {
        let err = ApiError::Unexpected {
            status: 500,
            detail: "oops".into(),
        };
        assert_eq!(save_error_text(&err), "Couldn't save your profile.");
    }
}
