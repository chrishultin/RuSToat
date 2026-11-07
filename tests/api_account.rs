//! The account's login details against a mock Stoat API: its email, and
//! changing it, with an MFA ticket only when one is given.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn the_account_has_its_email() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth/account/"))
        .and(header("x-session-token", "tok"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "_id": "a1", "email": "ada@example.com" })),
        )
        .mount(&server)
        .await;

    let account = client(&server).fetch_account().await.unwrap();
    assert_eq!(account.id, "a1");
    assert_eq!(account.email, "ada@example.com");
}

#[tokio::test]
async fn changing_the_email_without_a_ticket_sends_none() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/auth/account/change/email"))
        .and(header("x-session-token", "tok"))
        .and(body_json(
            json!({ "email": "new@example.com", "current_password": "pw" }),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .change_email(None, "new@example.com", "pw")
        .await
        .unwrap();
    let request = &server.received_requests().await.unwrap()[0];
    assert!(!request.headers.contains_key("x-mfa-ticket"));
}

#[tokio::test]
async fn changing_the_email_with_a_ticket_sends_it() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/auth/account/change/email"))
        .and(header("x-mfa-ticket", "t1"))
        .and(body_json(
            json!({ "email": "new@example.com", "current_password": "pw" }),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .change_email(Some("t1"), "new@example.com", "pw")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_wrong_password_isnt_a_dead_session() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/auth/account/change/email"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({ "type": "InvalidCredentials" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .change_email(None, "new@example.com", "wrong")
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Api(_)));
    assert!(matches!(
        err.error_type(),
        Some(ErrorType::InvalidCredentials)
    ));
}
