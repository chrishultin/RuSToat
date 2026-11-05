//! `StoatClient` against a mock Stoat API (wiremock).

use revolt_result::ErrorType;
use serde_json::{Value, json};
use stoat_gpui::api::models::{MFAResponse, ResponseLogin};
use stoat_gpui::api::{ApiError, SESSION_FRIENDLY_NAME, StoatClient};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

#[tokio::test]
async fn login_success_returns_session_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .and(body_partial_json(json!({
            "email": "me@example.com",
            "password": "hunter2",
            "friendly_name": SESSION_FRIENDLY_NAME,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_success.json")))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let login = client.login("me@example.com", "hunter2").await.unwrap();

    let ResponseLogin::Success(session) = login else {
        panic!("expected Success, got {login:?}");
    };
    assert!(session.token.starts_with("test-session-token"));
}

#[tokio::test]
async fn mfa_login_completes_with_totp_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .and(body_partial_json(json!({ "email": "me@example.com" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_mfa.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .and(body_partial_json(json!({
            "mfa_ticket": "test-mfa-ticket",
            "mfa_response": { "totp_code": "123456" },
            "friendly_name": SESSION_FRIENDLY_NAME,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_success.json")))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let ResponseLogin::MFA { ticket, .. } =
        client.login("me@example.com", "hunter2").await.unwrap()
    else {
        panic!("expected MFA step");
    };
    let login = client
        .login_mfa(
            &ticket,
            MFAResponse::Totp {
                totp_code: "123456".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(login, ResponseLogin::Success(_)));
}

#[tokio::test]
async fn mfa_login_sends_recovery_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .and(body_partial_json(json!({
            "mfa_ticket": "test-mfa-ticket",
            "mfa_response": { "recovery_code": "abcd-efgh" },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_success.json")))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let login = client
        .login_mfa(
            "test-mfa-ticket",
            MFAResponse::Recovery {
                recovery_code: "abcd-efgh".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(login, ResponseLogin::Success(_)));
}

#[tokio::test]
async fn invalid_credentials_is_a_readable_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(fixture("error_invalid_credentials.json")),
        )
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let err = client.login("me@example.com", "wrong").await.unwrap_err();

    assert!(
        matches!(err.error_type(), Some(ErrorType::InvalidCredentials)),
        "got {err:?}"
    );
    assert_eq!(err.to_string(), "Incorrect email or password.");
}

#[tokio::test]
async fn fetch_self_sends_session_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .and(header("x-session-token", "tok-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("users_me.json")))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri()).with_token("tok-123");
    let user = client.fetch_self().await.unwrap();
    assert_eq!(user.username, "stoatfan");
}

#[tokio::test]
async fn fetch_self_with_revoked_token_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "type": "InvalidSession" })))
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri()).with_token("revoked");
    let err = client.fetch_self().await.unwrap_err();
    assert!(matches!(err, ApiError::Unauthorized), "got {err:?}");
}

#[tokio::test]
async fn root_config_reads_ws_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("root.json")))
        .mount(&server)
        .await;

    let root = StoatClient::new(server.uri()).root_config().await.unwrap();
    assert_eq!(root.ws, "wss://events.stoat.chat");
}

#[tokio::test]
async fn unreachable_server_is_a_network_error_not_unauthorized() {
    // Port 1 is reserved and closed, so the connection is refused immediately.
    let client = StoatClient::new("http://127.0.0.1:1");
    let err = client.login("me@example.com", "hunter2").await.unwrap_err();
    assert!(matches!(err, ApiError::Network(_)), "got {err:?}");
}

#[tokio::test]
async fn trailing_slash_in_base_url_is_ignored() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("root.json")))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(format!("{}/", server.uri()));
    client.root_config().await.unwrap();
}

/// Talks to the real API (no login needed). Run with `cargo test -- --ignored`.
/// It doubles as a drift check: if Stoat moves past 0.16.x, bump the pinned
/// `revolt-*` crates and re-run the contract tests.
#[tokio::test]
#[ignore = "talks to the live Stoat API"]
async fn live_root_config_parses_and_matches_pinned_models() {
    let root = StoatClient::new(stoat_gpui::api::DEFAULT_API_URL)
        .root_config()
        .await
        .unwrap();
    assert!(root.ws.starts_with("wss://"), "ws = {}", root.ws);
    assert!(
        root.version.starts_with("0.16."),
        "live API is {}, but revolt-models is pinned to 0.16.2",
        root.version
    );
    // The attachment limits are read from the live shape, not the fallbacks.
    let limits = &root.features.limits;
    for account in [&limits.default, &limits.new_user] {
        assert!(account.file_upload_size_limits.attachments.is_some(), "{limits:?}");
        assert!(account.message_attachments.is_some(), "{limits:?}");
    }
}
