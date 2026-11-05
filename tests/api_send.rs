//! Sending messages against a mock Stoat API: the path, the session and
//! `Idempotency-Key` headers, the exact body, and refusals.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::ReplyIntent;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MSG: &str = "01JC0000000000000000000009";
const KEY: &str = "rustoat-1793700000000-0-0123456789abcdef";

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

fn created(content: &str) -> serde_json::Value {
    json!({
        "_id": MSG, "nonce": KEY, "channel": "ch-general", "author": "u-me",
        "content": content
    })
}

#[tokio::test]
async fn sending_posts_the_content_with_the_idempotency_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .and(header("x-session-token", "tok"))
        .and(header("Idempotency-Key", KEY))
        // Exactly this: no `replies` (or other `null` fields) without replies.
        .and(body_json(json!({ "content": "hello\nworld" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(created("hello\nworld")))
        .expect(1)
        .mount(&server)
        .await;

    let message = client(&server)
        .send_message("ch-general", "hello\nworld", &[], KEY)
        .await
        .unwrap();
    assert_eq!(message.id, MSG);
    assert_eq!(message.nonce.as_deref(), Some(KEY));
    assert_eq!(message.content.as_deref(), Some("hello\nworld"));
}

#[tokio::test]
async fn replies_are_sent_with_their_mention_choice() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .and(body_json(json!({
            "content": "yes",
            "replies": [
                { "id": "01JC0000000000000000000001", "mention": true },
                { "id": "01JC0000000000000000000002", "mention": false }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(created("yes")))
        .expect(1)
        .mount(&server)
        .await;

    let reply = |id: &str, mention| ReplyIntent {
        id: id.into(),
        mention,
        fail_if_not_exists: None,
    };
    // The second with its "@" toggled off.
    let replies = [
        reply("01JC0000000000000000000001", true),
        reply("01JC0000000000000000000002", false),
    ];
    client(&server)
        .send_message("ch-general", "yes", &replies, KEY)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_refused_send_is_a_missing_permission_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-locked/messages"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "MissingPermission", "permission": "SendMessage" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .send_message("ch-locked", "hi", &[], KEY)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApiError::Api(e) if matches!(e.error_type, ErrorType::MissingPermission { .. })),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_reused_key_is_a_duplicate_nonce_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({ "type": "DuplicateNonce" })))
        .mount(&server)
        .await;

    let err = client(&server)
        .send_message("ch-general", "hi", &[], KEY)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::DuplicateNonce)),
        "{err:?}"
    );
}

#[tokio::test]
async fn sending_too_fast_is_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({ "retry_after": 900 })))
        .mount(&server)
        .await;

    let err = client(&server)
        .send_message("ch-general", "hi", &[], KEY)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            ApiError::RateLimited {
                retry_after_ms: Some(900)
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_reply_source_is_fetched_with_its_neighbours_and_authors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .and(header("x-session-token", "tok"))
        .and(query_param("nearby", MSG))
        .and(query_param("limit", "1"))
        .and(query_param("include_users", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [created("the original")],
            "users": [{ "_id": "u-me", "username": "me", "discriminator": "0001", "relationship": "None", "online": false }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let page = client(&server)
        .fetch_messages_around("ch-general", MSG, 1)
        .await
        .unwrap();
    let stoat_gpui::api::models::BulkMessageResponse::MessagesAndUsers { messages, users, .. } =
        page
    else {
        panic!("expected users with the messages");
    };
    assert_eq!(messages[0].id, MSG);
    assert_eq!(users[0].username, "me");
}
