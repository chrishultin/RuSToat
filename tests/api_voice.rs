//! Asking Stoat for a voice call, against a mock server.

use serde_json::json;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(api: &str) -> StoatClient {
    StoatClient::new(api).with_token("tok")
}

#[tokio::test]
async fn joining_asks_for_the_node_and_takes_over_the_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/c1/join_call"))
        .and(header("x-session-token", "tok"))
        .and(body_json(
            json!({ "node": "hel1", "force_disconnect": true }),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "token": "t", "url": "wss://x" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let call = client(&server.uri()).join_call("c1", "hel1").await.unwrap();
    assert_eq!(call.token, "t");
    assert_eq!(call.url, "wss://x");
}

#[tokio::test]
async fn stoat_refusals_stay_stoat_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/full/join_call"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "type": "CannotJoinCall" })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/channels/locked/join_call"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "MissingPermission", "permission": "Connect" })),
        )
        .mount(&server)
        .await;

    let client = client(&server.uri());
    for channel in ["full", "locked"] {
        let err = client.join_call(channel, "hel1").await.unwrap_err();
        assert!(matches!(err, ApiError::Api(_)), "{channel}: got {err:?}");
    }
}
