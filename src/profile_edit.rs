//! Editing the user's own profile: Stoat's rules for each field and
//! image, what a Save sends, and what a failed one says. Plain data with no
//! GPUI, so it's unit-testable.

use std::path::{Path, PathBuf};

use revolt_result::ErrorType;

use crate::api::models::{DataEditUser, DataMemberEdit, DataUserProfile, FieldsMember, FieldsUser};
use crate::api::{ApiError, RootConfig};
use crate::draft::decimal_size_label;

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

/// The image types Stoat takes for avatars and banners (by extension).
pub const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Why a picked image can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProblem {
    /// Not a PNG, JPEG, GIF or WebP.
    WrongType,
    /// Over the size limit (`limit` bytes).
    TooBig { limit: u64 },
}

impl std::fmt::Display for ImageProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageProblem::WrongType => write!(f, "Choose a PNG, JPEG, GIF or WebP image."),
            ImageProblem::TooBig { limit } => write!(
                f,
                "That image is too big. It can be up to {}.",
                decimal_size_label(*limit)
            ),
        }
    }
}

/// Check a picked image (`size` bytes) against its type and `limit`.
/// Returns its content type, to upload it with. Uploaded unchanged, so a
/// GIF stays animated.
pub fn check_image(path: &Path, size: u64, limit: u64) -> Result<&'static str, ImageProblem> {
    let guessed = mime_guess::from_path(path).first();
    let content_type = guessed
        .and_then(|mime| IMAGE_TYPES.into_iter().find(|t| *t == mime.essence_str()))
        .ok_or(ImageProblem::WrongType)?;
    if size > limit {
        return Err(ImageProblem::TooBig { limit });
    }
    Ok(content_type)
}

/// How big an avatar, a profile banner, a server icon and a server banner
/// may be (from `GET /`, see [`RootConfig::avatar_size_limit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageLimits {
    pub avatar: u64,
    pub background: u64,
    pub server_icon: u64,
    pub server_banner: u64,
}

impl Default for ImageLimits {
    fn default() -> Self {
        Self {
            avatar: crate::api::DEFAULT_AVATAR_SIZE_LIMIT,
            background: crate::api::DEFAULT_BACKGROUND_SIZE_LIMIT,
            server_icon: crate::api::DEFAULT_ICON_SIZE_LIMIT,
            server_banner: crate::api::DEFAULT_BANNER_SIZE_LIMIT,
        }
    }
}

impl ImageLimits {
    pub fn from_root(root: &RootConfig) -> Self {
        Self {
            avatar: root.avatar_size_limit(),
            background: root.background_size_limit(),
            server_icon: root.icon_size_limit(),
            server_banner: root.banner_size_limit(),
        }
    }
}

/// An image in a form: an avatar or banner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProfileImage {
    /// None, so the default shows.
    #[default]
    None,
    /// One on Stoat, by file ID.
    Current(String),
    /// A file picked to replace it, not uploaded yet.
    New(PathBuf),
}

impl ProfileImage {
    /// The file to upload before saving, if one was picked.
    pub fn new_file(&self) -> Option<&Path> {
        match self {
            ProfileImage::New(path) => Some(path),
            _ => None,
        }
    }
}

/// How an image changed, as [`changed`] says it. A picked file that isn't
/// uploaded yet is left out: Save uploads it and puts its ID in first.
fn image_changed(loaded: &ProfileImage, edited: &ProfileImage) -> Option<Option<String>> {
    match edited {
        ProfileImage::New(_) => None,
        ProfileImage::Current(id) => (loaded != edited).then(|| Some(id.clone())),
        ProfileImage::None => (*loaded != ProfileImage::None).then_some(None),
    }
}

/// The profile form: as loaded from Stoat, or as edited. A missing text
/// value is the empty string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileForm {
    pub display_name: String,
    pub pronouns: String,
    pub bio: String,
    pub avatar: ProfileImage,
    pub banner: ProfileImage,
}

impl ProfileForm {
    /// Whether every field follows Stoat's rules, so Save can send them.
    pub fn is_valid(&self) -> bool {
        check_display_name(&self.display_name).is_ok()
            && check_pronouns(&self.pronouns).is_ok()
            && check_bio(&self.bio).is_ok()
    }

    /// Whether `edited` differs from `loaded`, counting picked images that
    /// aren't uploaded yet.
    pub fn has_changes(loaded: &ProfileForm, edited: &ProfileForm) -> bool {
        edited.avatar.new_file().is_some()
            || edited.banner.new_file().is_some()
            || !is_empty_edit(&ProfileForm::changes(loaded, edited))
    }

    /// The `PATCH /users/@me` that turns `loaded` into `edited`: only the
    /// fields that changed, with blanked ones in `remove`. A field that
    /// breaks its rule is left out (Save is disabled meanwhile), and so is
    /// an image until it's uploaded ([`ProfileImage::Current`] with its new
    /// ID).
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
        let mut profile = DataUserProfile {
            content: None,
            background: None,
        };
        match changed(check_bio, &loaded.bio, &edited.bio) {
            Some(Some(bio)) => profile.content = Some(bio),
            Some(None) => edit.remove.push(FieldsUser::ProfileContent),
            None => {}
        }
        match image_changed(&loaded.avatar, &edited.avatar) {
            Some(Some(id)) => edit.avatar = Some(id),
            Some(None) => edit.remove.push(FieldsUser::Avatar),
            None => {}
        }
        match image_changed(&loaded.banner, &edited.banner) {
            Some(Some(id)) => profile.background = Some(id),
            Some(None) => edit.remove.push(FieldsUser::ProfileBackground),
            None => {}
        }
        if profile.content.is_some() || profile.background.is_some() {
            edit.profile = Some(profile);
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

/// The identity form for one server: as loaded from the user's membership,
/// or as edited. A blank field falls back to the global profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityForm {
    pub nickname: String,
    pub avatar: ProfileImage,
    pub pronouns: String,
}

impl IdentityForm {
    /// Whether every field follows Stoat's rules, so Save can send them.
    pub fn is_valid(&self) -> bool {
        check_nickname(&self.nickname).is_ok() && check_pronouns(&self.pronouns).is_ok()
    }

    /// Whether `edited` differs from `loaded`, counting a picked avatar
    /// that isn't uploaded yet.
    pub fn has_changes(loaded: &IdentityForm, edited: &IdentityForm) -> bool {
        edited.avatar.new_file().is_some()
            || IdentityForm::changes(loaded, edited) != empty_member_edit()
    }

    /// The `PATCH /servers/{server}/members/{user}` that turns `loaded`
    /// into `edited`, as [`ProfileForm::changes`] does for the profile.
    pub fn changes(loaded: &IdentityForm, edited: &IdentityForm) -> DataMemberEdit {
        let mut edit = empty_member_edit();
        match changed(check_nickname, &loaded.nickname, &edited.nickname) {
            Some(Some(nickname)) => edit.nickname = Some(nickname),
            Some(None) => edit.remove.push(FieldsMember::Nickname),
            None => {}
        }
        match image_changed(&loaded.avatar, &edited.avatar) {
            Some(Some(id)) => edit.avatar = Some(id),
            Some(None) => edit.remove.push(FieldsMember::Avatar),
            None => {}
        }
        match changed(check_pronouns, &loaded.pronouns, &edited.pronouns) {
            Some(Some(pronouns)) => edit.pronouns = Some(pronouns),
            Some(None) => edit.remove.push(FieldsMember::Pronouns),
            None => {}
        }
        edit
    }

    /// The `PATCH /servers/{server}/members/{user}` a moderator sends to
    /// turn someone else's `loaded` identity into `edited`: their nickname,
    /// and removing their server avatar. Stoat refuses anything else from
    /// another member (pronouns, or setting an avatar), so it's never sent.
    pub fn changes_for_other(loaded: &IdentityForm, edited: &IdentityForm) -> DataMemberEdit {
        let mut edit = empty_member_edit();
        match changed(check_nickname, &loaded.nickname, &edited.nickname) {
            Some(Some(nickname)) => edit.nickname = Some(nickname),
            Some(None) => edit.remove.push(FieldsMember::Nickname),
            None => {}
        }
        if edited.avatar == ProfileImage::None && loaded.avatar != ProfileImage::None {
            edit.remove.push(FieldsMember::Avatar);
        }
        edit
    }
}

/// A `DataMemberEdit` that changes nothing (it has no `Default`).
pub fn empty_member_edit() -> DataMemberEdit {
    DataMemberEdit {
        nickname: None,
        pronouns: None,
        avatar: None,
        roles: None,
        timeout: None,
        can_publish: None,
        can_receive: None,
        voice_channel: None,
        remove: Vec::new(),
    }
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
    error_text(err, "Couldn't save your profile.")
}

/// What a toast says when Stoat refuses a change to the user's identity on
/// a server.
pub fn identity_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to change that on this server.".into()
        }
        _ => error_text(err, "Couldn't change your identity on this server."),
    }
}

/// Why a change was refused, else `fallback`.
pub(crate) fn error_text(err: &ApiError, fallback: &str) -> String {
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
            // The file server's refusals of an image.
            ErrorType::FileTooLarge { max } => {
                ImageProblem::TooBig { limit: *max as u64 }.to_string()
            }
            ErrorType::FileTypeNotAllowed | ErrorType::ImageProcessingFailed => {
                "Stoat couldn't use that image. Try a PNG or JPEG.".into()
            }
            _ => fallback.into(),
        },
        // An image that couldn't be read for its upload.
        ApiError::File(_) => err.to_string(),
        _ => fallback.into(),
    }
}

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
            ..ProfileForm::default()
        }
    }

    fn images(avatar: ProfileImage, banner: ProfileImage) -> ProfileForm {
        ProfileForm {
            avatar,
            banner,
            ..ProfileForm::default()
        }
    }

    #[test]
    fn images_are_png_jpeg_gif_or_webp_whatever_the_case() {
        let limit = 4_000_000;
        for (name, content_type) in [
            ("a.png", "image/png"),
            ("photo.JPG", "image/jpeg"),
            ("b.jpeg", "image/jpeg"),
            ("party.gif", "image/gif"),
            ("c.webp", "image/webp"),
        ] {
            assert_eq!(
                check_image(Path::new(name), 1000, limit),
                Ok(content_type),
                "{name}"
            );
        }
        for name in ["a.bmp", "IMG_0001.heic", "notes.txt", "noextension"] {
            assert_eq!(
                check_image(Path::new(name), 1000, limit),
                Err(ImageProblem::WrongType),
                "{name}"
            );
        }
    }

    #[test]
    fn an_image_over_its_limit_is_too_big() {
        let limits = ImageLimits::default();
        let path = Path::new("big.png");
        assert_eq!(
            check_image(path, 4_000_001, limits.avatar),
            Err(ImageProblem::TooBig { limit: 4_000_000 })
        );
        assert_eq!(check_image(path, 4_000_000, limits.avatar), Ok("image/png"));
        assert_eq!(
            check_image(path, 4_000_001, limits.background),
            Ok("image/png")
        );
        assert_eq!(
            ImageProblem::TooBig { limit: 4_000_000 }.to_string(),
            "That image is too big. It can be up to 4 MB."
        );
    }

    #[test]
    fn uploaded_images_are_sent_by_id() {
        let loaded = images(ProfileImage::Current("old".into()), ProfileImage::None);
        let edited = images(
            ProfileImage::Current("file-1".into()),
            ProfileImage::Current("file-2".into()),
        );
        let edit = ProfileForm::changes(&loaded, &edited);
        assert_eq!(edit.avatar.as_deref(), Some("file-1"));
        assert_eq!(
            edit.profile,
            Some(DataUserProfile {
                content: None,
                background: Some("file-2".into()),
            })
        );
        assert!(edit.remove.is_empty());
    }

    #[test]
    fn removed_images_are_removed_and_kept_ones_left_out() {
        let current = images(
            ProfileImage::Current("a".into()),
            ProfileImage::Current("b".into()),
        );
        let edit = ProfileForm::changes(&current, &ProfileForm::default());
        assert_eq!(
            edit.remove,
            [FieldsUser::Avatar, FieldsUser::ProfileBackground]
        );
        assert!(is_empty_edit(&ProfileForm::changes(&current, &current)));
        // Removing what isn't there is no change.
        assert!(!ProfileForm::has_changes(
            &ProfileForm::default(),
            &ProfileForm::default()
        ));
    }

    #[test]
    fn a_picked_image_is_a_change_but_waits_for_its_upload() {
        let loaded = images(ProfileImage::Current("a".into()), ProfileImage::None);
        let edited = images(ProfileImage::New("cat.gif".into()), ProfileImage::None);
        assert!(ProfileForm::has_changes(&loaded, &edited));
        assert!(is_empty_edit(&ProfileForm::changes(&loaded, &edited)));
        assert_eq!(edited.avatar.new_file(), Some(Path::new("cat.gif")));
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
    fn a_refused_image_says_why() {
        let err = ApiError::Api(StoatError {
            error_type: ErrorType::FileTooLarge { max: 6_000_000 },
            location: None,
        });
        assert_eq!(
            save_error_text(&err),
            "That image is too big. It can be up to 6 MB."
        );
        let err = ApiError::File(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert_eq!(save_error_text(&err), "Couldn't read the file (gone).");
    }

    #[test]
    fn nicknames_and_server_pronouns_follow_the_identity_rules() {
        let loaded = IdentityForm {
            nickname: "Ada".into(),
            ..IdentityForm::default()
        };
        let edited = IdentityForm {
            nickname: "a".repeat(33),
            pronouns: "she/her".into(),
            ..IdentityForm::default()
        };
        assert!(!edited.is_valid());
        // The nickname that's too long is left out; the pronouns go.
        let edit = IdentityForm::changes(&loaded, &edited);
        assert_eq!(edit.nickname, None);
        assert_eq!(edit.pronouns.as_deref(), Some("she/her"));
        assert!(edit.remove.is_empty());
    }

    #[test]
    fn identity_changes_send_only_what_changed() {
        let loaded = IdentityForm {
            nickname: "Ada".into(),
            avatar: ProfileImage::Current("a".into()),
            pronouns: "she/her".into(),
        };
        assert!(!IdentityForm::has_changes(&loaded, &loaded));
        let edited = IdentityForm {
            nickname: " Ada L. ".into(),
            ..loaded.clone()
        };
        assert_eq!(
            IdentityForm::changes(&loaded, &edited),
            DataMemberEdit {
                nickname: Some("Ada L.".into()),
                ..empty_member_edit()
            }
        );
        let uploaded = IdentityForm {
            avatar: ProfileImage::Current("b".into()),
            ..loaded.clone()
        };
        assert_eq!(
            IdentityForm::changes(&loaded, &uploaded).avatar.as_deref(),
            Some("b")
        );
        let picked = IdentityForm {
            avatar: ProfileImage::New("me.png".into()),
            ..loaded.clone()
        };
        assert!(IdentityForm::has_changes(&loaded, &picked));
    }

    #[test]
    fn clearing_the_identity_falls_back_to_the_global_profile() {
        let loaded = IdentityForm {
            nickname: "Ada".into(),
            avatar: ProfileImage::Current("a".into()),
            pronouns: "she/her".into(),
        };
        let cleared = IdentityForm {
            nickname: "  ".into(),
            ..IdentityForm::default()
        };
        assert_eq!(
            IdentityForm::changes(&loaded, &cleared).remove,
            [
                FieldsMember::Nickname,
                FieldsMember::Avatar,
                FieldsMember::Pronouns
            ]
        );
    }

    #[test]
    fn someone_elses_identity_edit_never_sends_pronouns_or_an_avatar() {
        let loaded = IdentityForm {
            nickname: "Ada".into(),
            avatar: ProfileImage::Current("file-1".into()),
            pronouns: "she/her".into(),
        };
        // Changed pronouns and a picked or swapped avatar are left out.
        for avatar in [
            ProfileImage::New("/tmp/a.png".into()),
            ProfileImage::Current("file-2".into()),
        ] {
            let edited = IdentityForm {
                nickname: " Countess ".into(),
                avatar,
                pronouns: "they/them".into(),
            };
            let edit = IdentityForm::changes_for_other(&loaded, &edited);
            assert_eq!(edit.nickname.as_deref(), Some("Countess"));
            assert!(edit.pronouns.is_none() && edit.avatar.is_none());
            assert!(edit.remove.is_empty());
        }
        // Clearing the nickname and removing the avatar are sent as removals.
        let cleared = IdentityForm {
            pronouns: String::new(),
            ..IdentityForm::default()
        };
        assert_eq!(
            IdentityForm::changes_for_other(&loaded, &cleared).remove,
            [FieldsMember::Nickname, FieldsMember::Avatar]
        );
        // Nothing changed, nothing sent.
        assert_eq!(
            IdentityForm::changes_for_other(&loaded, &loaded),
            empty_member_edit()
        );
    }

    #[test]
    fn identity_errors_say_why() {
        let err = ApiError::Api(StoatError {
            error_type: ErrorType::MissingPermission {
                permission: "ChangeNickname".into(),
            },
            location: None,
        });
        assert_eq!(
            identity_error_text(&err),
            "You don't have permission to change that on this server."
        );
        let err = ApiError::RateLimited {
            retry_after_ms: Some(1500),
        };
        assert_eq!(
            identity_error_text(&err),
            "Stoat is limiting changes. Try again in 2 s."
        );
        let err = ApiError::Unexpected {
            status: 500,
            detail: "oops".into(),
        };
        assert_eq!(
            identity_error_text(&err),
            "Couldn't change your identity on this server."
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
