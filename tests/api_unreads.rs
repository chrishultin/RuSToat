//! Read acknowledgements against a mock Stoat API.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn ack_message_puts_the_channel_and_message() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/channels/ch-general/ack/01JC0000000000000000000009"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    StoatClient::new(server.uri())
        .with_token("tok")
        .ack_message("ch-general", "01JC0000000000000000000009")
        .await
        .unwrap();
}

#[tokio::test]
async fn ack_server_puts_the_server() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv-alpha/ack"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    StoatClient::new(server.uri())
        .with_token("tok")
        .ack_server("srv-alpha")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_refused_ack_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/channels/ch-secret/ack/01JC0000000000000000000009"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "MissingPermission", "permission": "ViewChannel" })),
        )
        .mount(&server)
        .await;

    let err = StoatClient::new(server.uri())
        .with_token("tok")
        .ack_message("ch-secret", "01JC0000000000000000000009")
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApiError::Api(ref e) if matches!(e.error_type, ErrorType::MissingPermission { .. })),
        "got {err:?}"
    );
}
