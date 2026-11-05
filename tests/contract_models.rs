//! Contract tests: JSON shaped like the live Stoat API must parse into the
//! pinned backend types (`revolt-models`, `revolt-result`). If the server moves
//! ahead of the pinned crates, these fail first.

use revolt_result::ErrorType;
use stoat_gpui::api::RootConfig;
use stoat_gpui::api::models::{MFAMethod, RelationshipStatus, ResponseLogin, User};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

#[test]
fn root_config_reads_version_and_ws() {
    let root: RootConfig = serde_json::from_str(&fixture("root.json")).unwrap();
    assert_eq!(root.version, "0.16.2");
    assert_eq!(root.ws, "wss://events.stoat.chat");
}

#[test]
fn login_success_parses() {
    let login: ResponseLogin = serde_json::from_str(&fixture("login_success.json")).unwrap();
    let ResponseLogin::Success(session) = login else {
        panic!("expected Success, got {login:?}");
    };
    assert_eq!(session.user_id, "01JC0USER000000000000000AA");
    assert!(session.token.starts_with("test-session-token"));
}

#[test]
fn login_mfa_parses() {
    let login: ResponseLogin = serde_json::from_str(&fixture("login_mfa.json")).unwrap();
    let ResponseLogin::MFA {
        ticket,
        allowed_methods,
    } = login
    else {
        panic!("expected MFA, got {login:?}");
    };
    assert_eq!(ticket, "test-mfa-ticket");
    assert!(matches!(
        allowed_methods.as_slice(),
        [MFAMethod::Totp, MFAMethod::Recovery]
    ));
}

#[test]
fn login_disabled_parses() {
    let login: ResponseLogin = serde_json::from_str(&fixture("login_disabled.json")).unwrap();
    assert!(matches!(login, ResponseLogin::Disabled { .. }));
}

#[test]
fn error_body_parses_into_revolt_result_error() {
    let err: revolt_result::Error =
        serde_json::from_str(&fixture("error_invalid_credentials.json")).unwrap();
    assert!(matches!(err.error_type, ErrorType::InvalidCredentials));
    assert!(err.location.is_some());
}

#[test]
fn users_me_parses() {
    let user: User = serde_json::from_str(&fixture("users_me.json")).unwrap();
    assert_eq!(user.username, "stoatfan");
    assert_eq!(user.discriminator, "1234");
    assert_eq!(user.display_name.as_deref(), Some("Stoat Fan"));
    assert!(matches!(user.relationship, RelationshipStatus::User));
}
