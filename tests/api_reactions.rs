//! Adding and removing reactions against a mock Stoat API: the exact paths
//! (Unicode emoji percent-encoded), the session header, and refusals.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MSG: &str = "01JC0000000000000000000009";
const PARTY: &str = "01ARYZ6S41TSV4RRFFQ69G5FAV";

async fn expect(server: &MockServer, verb: &str, expected_path: &str) {
    Mock::given(method(verb))
        .and(path(expected_path))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(server)
        .await;
}

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn reacting_with_a_unicode_emoji_encodes_it_in_the_path() {
    let server = MockServer::start().await;
    expect(
        &server,
        "PUT",
        &format!("/channels/ch-general/messages/{MSG}/reactions/%F0%9F%91%8D"),
    )
    .await;
    client(&server)
        .react("ch-general", MSG, "👍")
        .await
        .unwrap();
}

#[tokio::test]
async fn reacting_with_a_custom_emoji_uses_its_id() {
    let server = MockServer::start().await;
    expect(
        &server,
        "PUT",
        &format!("/channels/ch-general/messages/{MSG}/reactions/{PARTY}"),
    )
    .await;
    client(&server)
        .react("ch-general", MSG, PARTY)
        .await
        .unwrap();
}

#[tokio::test]
async fn unreacting_deletes_the_same_paths() {
    let server = MockServer::start().await;
    // A multi-codepoint emoji (thumbs up, medium skin tone): every byte encoded.
    expect(
        &server,
        "DELETE",
        &format!("/channels/ch-general/messages/{MSG}/reactions/%F0%9F%91%8D%F0%9F%8F%BD"),
    )
    .await;
    expect(
        &server,
        "DELETE",
        &format!("/channels/ch-general/messages/{MSG}/reactions/{PARTY}"),
    )
    .await;
    let client = client(&server);
    client.unreact("ch-general", MSG, "👍🏽").await.unwrap();
    client.unreact("ch-general", MSG, PARTY).await.unwrap();
}

#[tokio::test]
async fn a_refused_reaction_is_a_missing_permission_error() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(format!(
            "/channels/ch-locked/messages/{MSG}/reactions/%F0%9F%91%8D"
        )))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "MissingPermission", "permission": "React" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .react("ch-locked", MSG, "👍")
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApiError::Api(e) if matches!(e.error_type, ErrorType::MissingPermission { .. })),
        "{err:?}"
    );
}
