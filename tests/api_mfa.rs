//! Two-factor authentication against a mock Stoat API: the status, MFA
//! tickets, recovery codes and the authenticator app.

use serde_json::json;
use stoat_gpui::api::StoatClient;
use stoat_gpui::api::models::MFAResponse;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn the_status_leaves_out_what_is_off() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth/mfa/"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/auth/mfa/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "totp_mfa": true, "recovery_active": true })),
        )
        .mount(&server)
        .await;

    let client = client(&server);
    let off = client.mfa_status().await.unwrap();
    assert!(!off.totp_mfa && !off.recovery_active);
    let on = client.mfa_status().await.unwrap();
    assert!(on.totp_mfa && on.recovery_active);
}

#[tokio::test]
async fn a_ticket_is_asked_for_with_the_password_alone() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/auth/mfa/ticket"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "password": "pw" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "01K00000000000000000000000", "account_id": "u-me", "token": "tok-ticket",
            "validated": true, "authorised": false, "last_totp_code": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ticket = client(&server)
        .create_mfa_ticket(&MFAResponse::Password {
            password: "pw".into(),
        })
        .await
        .unwrap();
    assert_eq!(ticket.token, "tok-ticket");
    assert!(ticket.validated);
    // Sending a ticket with this call makes Stoat fail it.
    let request = &server.received_requests().await.unwrap()[0];
    assert!(!request.headers.contains_key("x-mfa-ticket"));
}

#[tokio::test]
async fn a_ticket_can_be_asked_for_with_a_code() {
    let server = MockServer::start().await;
    for body in [
        json!({ "totp_code": "123456" }),
        json!({ "recovery_code": "abcde-fghjk" }),
    ] {
        Mock::given(method("PUT"))
            .and(path("/auth/mfa/ticket"))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": "01K00000000000000000000000", "account_id": "u-me", "token": "t",
                "validated": true, "authorised": false
            })))
            .expect(1)
            .mount(&server)
            .await;
    }

    let client = client(&server);
    client
        .create_mfa_ticket(&MFAResponse::Totp {
            totp_code: "123456".into(),
        })
        .await
        .unwrap();
    client
        .create_mfa_ticket(&MFAResponse::Recovery {
            recovery_code: "abcde-fghjk".into(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn recovery_codes_are_fetched_and_regenerated_with_a_ticket() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/mfa/recovery"))
        .and(header("x-session-token", "tok"))
        .and(header("x-mfa-ticket", "t1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(["abcde-fghjk"])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/auth/mfa/recovery"))
        .and(header("x-mfa-ticket", "t2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!(["mnpqr-stvwx", "yz234-56789"])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = client(&server);
    assert_eq!(
        client.fetch_recovery_codes("t1").await.unwrap(),
        ["abcde-fghjk"]
    );
    assert_eq!(
        client.generate_recovery_codes("t2").await.unwrap(),
        ["mnpqr-stvwx", "yz234-56789"]
    );
}

#[tokio::test]
async fn setting_up_the_authenticator_gets_a_secret_with_a_ticket() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/mfa/totp"))
        .and(header("x-mfa-ticket", "t1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "secret": "JBSWY3DPEHPK3PXP" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let secret = client(&server).create_totp_secret("t1").await.unwrap();
    assert_eq!(secret, "JBSWY3DPEHPK3PXP");
}

#[tokio::test]
async fn turning_the_authenticator_on_sends_only_the_code() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/auth/mfa/totp"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "totp_code": "123456" })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).enable_totp("123456").await.unwrap();
    // It takes no ticket.
    let request = &server.received_requests().await.unwrap()[0];
    assert!(!request.headers.contains_key("x-mfa-ticket"));
}

#[tokio::test]
async fn a_wrong_first_code_is_an_api_error() {
    use revolt_result::ErrorType;
    use stoat_gpui::api::ApiError;
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/auth/mfa/totp"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "type": "InvalidToken" })))
        .mount(&server)
        .await;

    let err = client(&server).enable_totp("000000").await.unwrap_err();
    assert!(
        matches!(&err, ApiError::Api(e) if matches!(e.error_type, ErrorType::InvalidToken)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn removing_the_authenticator_sends_the_ticket() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/auth/mfa/totp"))
        .and(header("x-mfa-ticket", "t2"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).disable_totp("t2").await.unwrap();
}
