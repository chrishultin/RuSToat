//! The account's security: its sessions (where it's signed in) and
//! two-factor authentication. Plain data with no GPUI, so it's
//! unit-testable.

use chrono::{DateTime, TimeZone};
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{MFAResponse, MultiFactorStatus, SessionInfo};
use crate::profile_edit::{FieldCheck, error_text};

/// The longest session name RuSToat sends. Stoat sets no limit; this one
/// keeps a slip from storing a huge name.
pub const SESSION_NAME_MAX: usize = 64;

/// What kind of device a session is on, from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Phone,
    Desktop,
}

/// A session's platform, read from its name as the web client reads it
/// ("Stoat for Web (Chrome on Android)"). Anything else is a desktop.
pub fn platform_of(name: &str) -> Platform {
    let name = name.to_lowercase();
    let phone = ["android", "ios", "ipad", "iphone"]
        .iter()
        .any(|word| name.contains(word))
        // "macOS" and "Mac OS" aren't iOS.
        && !name.contains("macos")
        && !name.contains("mac os");
    if phone {
        Platform::Phone
    } else {
        Platform::Desktop
    }
}

/// One session in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    /// When it was created (ms since the epoch), from its ULID.
    pub created_ms: Option<u64>,
    /// It's this client's own session.
    pub current: bool,
}

/// The sessions as the page lists them: this device's first (if `current`
/// is known), then the newest first.
pub fn session_rows(sessions: Vec<SessionInfo>, current: Option<&str>) -> Vec<SessionRow> {
    let mut rows: Vec<SessionRow> = sessions
        .into_iter()
        .map(|session| SessionRow {
            current: current == Some(session.id.as_str()),
            platform: platform_of(&session.name),
            created_ms: crate::time::ulid_millis(&session.id),
            id: session.id,
            name: session.name,
        })
        .collect();
    rows.sort_by_key(|row| (!row.current, std::cmp::Reverse(row.created_ms)));
    rows
}

/// "Created 3 days ago", from a creation time and now.
pub fn created_label<Tz: TimeZone>(created: &DateTime<Tz>, now: &DateTime<Tz>) -> String {
    format!("Created {}", crate::time::relative_time(created, now))
}

/// Check a new session name: trimmed, 1–64 characters. Blank is `Clear`,
/// which can't be saved (a session always has a name).
pub fn check_session_name(text: &str) -> FieldCheck<'_> {
    let text = text.trim();
    match text.chars().count() {
        0 => FieldCheck::Clear,
        n if n > SESSION_NAME_MAX => FieldCheck::TooLong(n),
        _ => FieldCheck::Ok(text),
    }
}

/// How the user confirms it's them for a protected action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmMethod {
    Password,
    Totp,
    Recovery,
}

impl ConfirmMethod {
    /// What the dialog asks for.
    pub fn prompt(self) -> &'static str {
        match self {
            ConfirmMethod::Password => "Enter your password.",
            ConfirmMethod::Totp => "Enter the 6-digit code from your authenticator app.",
            ConfirmMethod::Recovery => "Enter one of your recovery codes. It can't be used again.",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            ConfirmMethod::Password => "Password",
            ConfirmMethod::Totp => "123456",
            ConfirmMethod::Recovery => "xxxxx-xxxxx",
        }
    }

    /// The link that switches to this method.
    pub fn switch_label(self) -> &'static str {
        match self {
            ConfirmMethod::Password => "Use your password instead",
            ConfirmMethod::Totp => "Use your authenticator app instead",
            ConfirmMethod::Recovery => "Use a recovery code instead",
        }
    }
}

/// The ways Stoat lets the user confirm, as it decides them: the password
/// without an authenticator app; with one, its code, or a recovery code if
/// any are left. The first is the one asked for.
pub fn confirm_methods(status: &MultiFactorStatus) -> Vec<ConfirmMethod> {
    if !status.totp_mfa {
        return vec![ConfirmMethod::Password];
    }
    let mut methods = vec![ConfirmMethod::Totp];
    if status.recovery_active {
        methods.push(ConfirmMethod::Recovery);
    }
    methods
}

/// What's typed, as Stoat takes it: a password as it is, a TOTP code
/// without spaces, a recovery code trimmed and in lower case. `None` if
/// it's blank.
pub fn mfa_response(method: ConfirmMethod, typed: &str) -> Option<MFAResponse> {
    match method {
        ConfirmMethod::Password => (!typed.is_empty()).then(|| MFAResponse::Password {
            password: typed.to_string(),
        }),
        ConfirmMethod::Totp => {
            let code: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
            (!code.is_empty()).then_some(MFAResponse::Totp { totp_code: code })
        }
        ConfirmMethod::Recovery => {
            let code = typed.trim().to_lowercase();
            (!code.is_empty()).then_some(MFAResponse::Recovery {
                recovery_code: code,
            })
        }
    }
}

/// What to do when Stoat refuses to confirm it's the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmError {
    /// Say this under the field, and let them try again.
    Retry(String),
    /// The ways to confirm have changed (an authenticator was turned on or
    /// off elsewhere): ask Stoat again.
    Methods,
    /// Give up, with this in a toast.
    Fail(String),
}

/// Why a ticket was refused, and what to do about it.
pub fn ticket_error(err: &ApiError) -> ConfirmError {
    match err {
        ApiError::RateLimited { .. } => ConfirmError::Retry(error_text(err, "")),
        _ => match err.error_type() {
            Some(ErrorType::InvalidCredentials) => {
                ConfirmError::Retry("That password isn't right.".into())
            }
            Some(ErrorType::InvalidToken) => ConfirmError::Retry("That code isn't right.".into()),
            Some(ErrorType::DisallowedMFAMethod) => ConfirmError::Methods,
            _ => ConfirmError::Fail(match err {
                ApiError::Unauthorized | ApiError::Network(_) => err.to_string(),
                _ => error_text(err, "Couldn't confirm it's you. Try again."),
            }),
        },
    }
}

/// Why a protected action failed after its ticket was made. A refused
/// ticket (expired, or used already) reads as such, not as a dead session.
pub fn action_error_text(err: &ApiError, fallback: &str) -> String {
    match (err, err.error_type()) {
        (ApiError::Unauthorized, _) | (_, Some(ErrorType::InvalidToken)) => {
            "That confirmation expired. Try again.".into()
        }
        _ => error_text(err, fallback),
    }
}

/// What the Recovery codes row offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// None yet: make the first set.
    Generate,
    View,
    /// Replace them with a new set (the old ones stop working).
    Regenerate,
}

/// The Recovery codes row's buttons, for the account's status.
pub fn recovery_actions(status: &MultiFactorStatus) -> Vec<RecoveryAction> {
    if status.recovery_active {
        vec![RecoveryAction::View, RecoveryAction::Regenerate]
    } else {
        vec![RecoveryAction::Generate]
    }
}

/// Recovery codes as Copy all puts them on the clipboard: one per line.
pub fn recovery_codes_text(codes: &[String]) -> String {
    codes.join("\n")
}

/// The URI an authenticator app scans, as the web client writes it:
/// `otpauth://totp/Stoat:{username}?secret={secret}&issuer=Stoat`. The
/// username is percent-encoded (the same text for ASCII ones). SHA-1, six
/// digits and 30 s are the defaults, and what Stoat uses.
pub fn otpauth_uri(username: &str, secret: &str) -> String {
    format!(
        "otpauth://totp/Stoat:{}?secret={}&issuer=Stoat",
        percent_encode(username),
        percent_encode(secret)
    )
}

/// Everything but RFC 3986's unreserved characters, percent-encoded.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A QR code's modules: `size` × `size`, dark ones `true`, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrModules {
    pub size: usize,
    pub dark: Vec<bool>,
}

/// The light border a QR code needs around it, in modules.
pub const QR_QUIET_ZONE: usize = 4;

/// Encode `text` as a QR code at medium error correction (as the web
/// client does). `None` if it's too long for a QR code.
pub fn qr_modules(text: &str) -> Option<QrModules> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    let size = qr.size();
    let dark = (0..size)
        .flat_map(|y| (0..size).map(move |x| (x, y)))
        .map(|(x, y)| qr.get_module(x, y))
        .collect();
    Some(QrModules {
        size: size as usize,
        dark,
    })
}

/// The QR code as an image: black modules on white, `scale` pixels to a
/// module, with the quiet zone around it. Black and white read the same in
/// RGBA and GPUI's BGRA.
pub fn qr_image(modules: &QrModules, scale: u32) -> image::RgbaImage {
    let side = (modules.size + 2 * QR_QUIET_ZONE) as u32 * scale;
    image::RgbaImage::from_fn(side, side, |px, py| {
        let module = |p: u32| (p / scale) as usize;
        let (x, y) = (module(px), module(py));
        let inside = |m: usize| (QR_QUIET_ZONE..QR_QUIET_ZONE + modules.size).contains(&m);
        let dark = inside(x)
            && inside(y)
            && modules.dark[(y - QR_QUIET_ZONE) * modules.size + (x - QR_QUIET_ZONE)];
        if dark {
            image::Rgba([0, 0, 0, 255])
        } else {
            image::Rgba([255, 255, 255, 255])
        }
    })
}

/// A secret as it's shown to type in: groups of four, "JBSW Y3DP …".
pub fn secret_groups(secret: &str) -> String {
    secret
        .chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|group| group.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A typed authenticator code, as Stoat takes it: six digits, spaces
/// removed. `None` if it isn't that.
pub fn check_totp_code(text: &str) -> Option<String> {
    let code: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (code.len() == 6 && code.chars().all(|c| c.is_ascii_digit())).then_some(code)
}

/// Why turning the authenticator on failed. Stoat takes only the current
/// 30-second code, so a wrong one is most often the clock or a stale code.
pub fn enable_totp_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::InvalidToken) => {
            "That code isn't right. Check your device's clock and try the newest code.".into()
        }
        _ => error_text(err, "Couldn't turn on the authenticator app."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use revolt_result::Error as StoatError;

    fn api(error_type: ErrorType) -> ApiError {
        ApiError::Api(StoatError {
            error_type,
            location: None,
        })
    }

    #[test]
    fn without_an_authenticator_only_the_password_confirms() {
        let status: MultiFactorStatus = serde_json::from_str("{}").unwrap();
        assert_eq!(confirm_methods(&status), [ConfirmMethod::Password]);
        // Recovery codes alone don't change that.
        let status: MultiFactorStatus =
            serde_json::from_str(r#"{"recovery_active":true}"#).unwrap();
        assert_eq!(confirm_methods(&status), [ConfirmMethod::Password]);
    }

    #[test]
    fn with_an_authenticator_its_code_or_a_recovery_code_confirms() {
        let status: MultiFactorStatus = serde_json::from_str(r#"{"totp_mfa":true}"#).unwrap();
        assert_eq!(confirm_methods(&status), [ConfirmMethod::Totp]);
        let status: MultiFactorStatus =
            serde_json::from_str(r#"{"totp_mfa":true,"recovery_active":true}"#).unwrap();
        assert_eq!(
            confirm_methods(&status),
            [ConfirmMethod::Totp, ConfirmMethod::Recovery]
        );
    }

    #[test]
    fn typed_answers_are_cleaned_up_as_stoat_takes_them() {
        assert_eq!(
            mfa_response(ConfirmMethod::Totp, " 123 456 "),
            Some(MFAResponse::Totp {
                totp_code: "123456".into()
            })
        );
        assert_eq!(
            mfa_response(ConfirmMethod::Recovery, " ABCDE-12345 "),
            Some(MFAResponse::Recovery {
                recovery_code: "abcde-12345".into()
            })
        );
        // A password is sent as typed, spaces and all.
        assert_eq!(
            mfa_response(ConfirmMethod::Password, " pw "),
            Some(MFAResponse::Password {
                password: " pw ".into()
            })
        );
        for method in [
            ConfirmMethod::Password,
            ConfirmMethod::Totp,
            ConfirmMethod::Recovery,
        ] {
            assert_eq!(mfa_response(method, ""), None);
        }
        assert_eq!(mfa_response(ConfirmMethod::Totp, "   "), None);
    }

    #[test]
    fn a_wrong_password_or_code_asks_again() {
        assert_eq!(
            ticket_error(&api(ErrorType::InvalidCredentials)),
            ConfirmError::Retry("That password isn't right.".into())
        );
        assert_eq!(
            ticket_error(&api(ErrorType::InvalidToken)),
            ConfirmError::Retry("That code isn't right.".into())
        );
        assert_eq!(
            ticket_error(&api(ErrorType::DisallowedMFAMethod)),
            ConfirmError::Methods
        );
        assert_eq!(
            ticket_error(&ApiError::RateLimited {
                retry_after_ms: Some(2500)
            }),
            ConfirmError::Retry("Stoat is limiting changes. Try again in 3 s.".into())
        );
        assert!(matches!(
            ticket_error(&ApiError::Unexpected {
                status: 500,
                detail: "x".into()
            }),
            ConfirmError::Fail(_)
        ));
    }

    #[test]
    fn the_recovery_row_offers_what_the_status_allows() {
        let none: MultiFactorStatus = serde_json::from_str("{}").unwrap();
        assert_eq!(recovery_actions(&none), [RecoveryAction::Generate]);
        let some: MultiFactorStatus = serde_json::from_str(r#"{"recovery_active":true}"#).unwrap();
        assert_eq!(
            recovery_actions(&some),
            [RecoveryAction::View, RecoveryAction::Regenerate]
        );
    }

    #[test]
    fn copied_recovery_codes_are_one_per_line() {
        let codes = vec!["abcde-fghjk".to_string(), "mnpqr-stvwx".to_string()];
        assert_eq!(recovery_codes_text(&codes), "abcde-fghjk\nmnpqr-stvwx");
        assert_eq!(recovery_codes_text(&[]), "");
    }

    #[test]
    fn the_otpauth_uri_matches_the_web_clients() {
        assert_eq!(
            otpauth_uri("ada", "JBSWY3DPEHPK3PXP"),
            "otpauth://totp/Stoat:ada?secret=JBSWY3DPEHPK3PXP&issuer=Stoat"
        );
        assert_eq!(
            otpauth_uri("zoë.k_1", "JBSWY3DPEHPK3PXP"),
            "otpauth://totp/Stoat:zo%C3%AB.k_1?secret=JBSWY3DPEHPK3PXP&issuer=Stoat"
        );
    }

    #[test]
    fn a_qr_code_is_square_with_a_quiet_zone() {
        let uri = otpauth_uri("ada", "JBSWY3DPEHPK3PXP");
        let modules = qr_modules(&uri).unwrap();
        assert!(
            modules.size >= 21 && modules.size % 2 == 1,
            "{}",
            modules.size
        );
        assert_eq!(modules.dark.len(), modules.size * modules.size);
        // A finder pattern's corner module is always dark.
        assert!(modules.dark[0]);

        let image = qr_image(&modules, 3);
        let side = (modules.size as u32 + 8) * 3;
        assert_eq!(image.dimensions(), (side, side));
        let white = image::Rgba([255, 255, 255, 255]);
        let black = image::Rgba([0, 0, 0, 255]);
        // The quiet zone is white at every corner.
        for (x, y) in [(0, 0), (side - 1, 0), (0, side - 1), (side - 1, side - 1)] {
            assert_eq!(*image.get_pixel(x, y), white);
        }
        // The first module after the quiet zone is the finder's dark corner,
        // all `scale` pixels of it.
        for d in 0..3 {
            assert_eq!(*image.get_pixel(12 + d, 12 + d), black);
        }
        assert_eq!(*image.get_pixel(11, 12), white);
    }

    #[test]
    fn the_key_is_shown_in_fours() {
        assert_eq!(secret_groups("JBSWY3DPEHPK3PXP"), "JBSW Y3DP EHPK 3PXP");
        assert_eq!(secret_groups("ABCDEF"), "ABCD EF");
        assert_eq!(secret_groups(""), "");
    }

    #[test]
    fn authenticator_codes_are_six_digits() {
        assert_eq!(check_totp_code("123456").as_deref(), Some("123456"));
        assert_eq!(check_totp_code(" 123 456 ").as_deref(), Some("123456"));
        for bad in ["12345", "1234567", "abcdef", "12345a", ""] {
            assert_eq!(check_totp_code(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_wrong_first_code_points_at_the_clock() {
        assert!(enable_totp_error_text(&api(ErrorType::InvalidToken)).contains("clock"));
        assert_eq!(
            enable_totp_error_text(&api(ErrorType::OperationFailed)),
            "Couldn't turn on the authenticator app."
        );
    }

    #[test]
    fn a_refused_ticket_isnt_a_dead_session() {
        assert_eq!(
            action_error_text(&ApiError::Unauthorized, "Couldn't log it out."),
            "That confirmation expired. Try again."
        );
        assert_eq!(
            action_error_text(&api(ErrorType::InvalidToken), "Couldn't log it out."),
            "That confirmation expired. Try again."
        );
        assert_eq!(
            action_error_text(&api(ErrorType::NotFound), "Couldn't log it out."),
            "Couldn't log it out."
        );
    }

    fn session(id: &str, name: &str) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            name: name.into(),
        }
    }

    // ULIDs in time order: OLD < MID < NEW.
    const OLD: &str = "01H00000000000000000000000";
    const MID: &str = "01J00000000000000000000000";
    const NEW: &str = "01K00000000000000000000000";

    #[test]
    fn this_device_comes_first_then_the_newest() {
        let rows = session_rows(
            vec![
                session(MID, "RuSToat"),
                session(NEW, "Stoat for Web (Firefox on Linux)"),
                session(OLD, "Stoat for Web (Safari on iOS)"),
            ],
            Some(OLD),
        );
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, [OLD, NEW, MID]);
        assert!(rows[0].current);
        assert!(!rows[1].current && !rows[2].current);
        assert_eq!(rows[0].platform, Platform::Phone);
    }

    #[test]
    fn without_this_devices_id_none_is_marked() {
        let rows = session_rows(vec![session(OLD, "a"), session(NEW, "b")], None);
        assert!(rows.iter().all(|r| !r.current));
        assert_eq!(rows[0].id, NEW);
        // An ID this account doesn't have marks none either.
        let rows = session_rows(vec![session(OLD, "a")], Some("gone"));
        assert!(!rows[0].current);
    }

    #[test]
    fn creation_time_comes_from_the_ulid() {
        let rows = session_rows(vec![session(NEW, "x"), session("not-a-ulid", "y")], None);
        assert_eq!(rows[0].created_ms, crate::time::ulid_millis(NEW));
        assert!(rows[0].created_ms.is_some());
        assert_eq!(rows[1].created_ms, None);
        let now = Utc::now();
        let created = now - chrono::Duration::days(3);
        assert_eq!(created_label(&created, &now), "Created 3 days ago");
    }

    #[test]
    fn platforms_follow_the_name() {
        for (name, platform) in [
            ("Stoat for Web (Chrome on Android)", Platform::Phone),
            ("Stoat for Web (Safari on iOS)", Platform::Phone),
            ("Stoat for Web (Safari on iPadOS)", Platform::Phone),
            ("Stoat for Web (Safari on Mac OS)", Platform::Desktop),
            ("Stoat for Web (Firefox on macOS)", Platform::Desktop),
            ("Stoat for Web (Firefox on Windows)", Platform::Desktop),
            ("RuSToat", Platform::Desktop),
            ("Unknown", Platform::Desktop),
        ] {
            assert_eq!(platform_of(name), platform, "{name}");
        }
    }

    #[test]
    fn session_names_are_1_to_64_characters_trimmed() {
        assert_eq!(
            check_session_name("  Work Mac "),
            FieldCheck::Ok("Work Mac")
        );
        assert_eq!(check_session_name("   "), FieldCheck::Clear);
        let at = "a".repeat(64);
        assert_eq!(check_session_name(&at), FieldCheck::Ok(at.as_str()));
        assert_eq!(check_session_name(&"é".repeat(65)), FieldCheck::TooLong(65));
    }
}
