//! A server's custom emoji against a mock Stoat API, from Server Settings'
//! Emojis page: `/custom/emoji/{id}`.

use serde_json::json;
use stoat_gpui::api::StoatClient;
use stoat_gpui::custom_emoji::emoji_error_text;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn deleting_an_emoji_sends_a_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/custom/emoji/01ARYZ6S41TSV4RRFFQ69G5FAV"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .delete_emoji("01ARYZ6S41TSV4RRFFQ69G5FAV")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_refused_delete_reads_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/custom/emoji/e"))
        .respond_with(ResponseTemplate::new(403).set_body_json(
            json!({ "type": "MissingPermission", "permission": "ManageCustomisation" }),
        ))
        .mount(&server)
        .await;

    let err = client(&server).delete_emoji("e").await.unwrap_err();
    assert_eq!(
        emoji_error_text(&err),
        "You don't have permission to manage this server's emoji."
    );
}
