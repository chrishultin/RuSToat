//! Pinned messages against a mock Stoat API: listing them (a search with
//! `pinned` and no `query`), pinning and unpinning, and the refusals.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MSG: &str = "01JC0000000000000000000009";

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn the_pins_list_is_a_pinned_search_with_no_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/search"))
        .and(header("x-session-token", "tok"))
        // Exactly this: Stoat refuses `pinned` next to `query`.
        .and(body_json(json!({
            "pinned": true,
            "limit": 100,
            "sort": "Latest",
            "include_users": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{
                "_id": MSG, "channel": "ch-general", "author": "u-me",
                "content": "remember this", "pinned": true
            }],
            "users": [{ "_id": "u-me", "username": "me", "discriminator": "0001", "relationship": "None", "online": false }],
            "members": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let page = client(&server).fetch_pins("ch-general", 100).await.unwrap();
    let BulkMessageResponse::MessagesAndUsers {
        messages, users, ..
    } = page
    else {
        panic!("expected users with the messages");
    };
    assert_eq!(messages[0].id, MSG);
    assert_eq!(messages[0].pinned, Some(true));
    assert_eq!(users[0].username, "me");
}

#[tokio::test]
async fn a_channel_without_pins_is_an_empty_page() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "messages": [], "users": [], "members": [] })),
        )
        .mount(&server)
        .await;

    let page = client(&server).fetch_pins("ch-general", 100).await.unwrap();
    assert!(
        matches!(page, BulkMessageResponse::MessagesAndUsers { ref messages, .. } if messages.is_empty())
    );
}

#[tokio::test]
async fn pinning_posts_to_the_message_and_expects_no_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/channels/ch-general/messages/{MSG}/pin")))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .pin_message("ch-general", MSG)
        .await
        .unwrap();
}

#[tokio::test]
async fn unpinning_deletes_the_pin_and_expects_no_content() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/channels/ch-general/messages/{MSG}/pin")))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .unpin_message("ch-general", MSG)
        .await
        .unwrap();
}

#[tokio::test]
async fn pinning_a_pinned_message_is_told_apart() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/channels/ch-general/messages/{MSG}/pin")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "type": "AlreadyPinned" })))
        .mount(&server)
        .await;

    let err = client(&server)
        .pin_message("ch-general", MSG)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::AlreadyPinned)),
        "{err:?}"
    );
    assert!(err.pin_already_done());
}

#[tokio::test]
async fn unpinning_an_unpinned_message_is_told_apart() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/channels/ch-general/messages/{MSG}/pin")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "type": "NotPinned" })))
        .mount(&server)
        .await;

    let err = client(&server)
        .unpin_message("ch-general", MSG)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotPinned)),
        "{err:?}"
    );
    assert!(err.pin_already_done());
}

#[tokio::test]
async fn pinning_without_permission_is_a_missing_permission_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/channels/ch-locked/messages/{MSG}/pin")))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(
                json!({ "type": "MissingPermission", "permission": "ManageMessages" }),
            ),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .pin_message("ch-locked", MSG)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::MissingPermission { .. })),
        "{err:?}"
    );
    assert!(!err.pin_already_done());
}

#[tokio::test]
async fn pinning_too_fast_is_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/channels/ch-general/messages/{MSG}/pin")))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({ "retry_after": 800 })))
        .mount(&server)
        .await;

    let err = client(&server)
        .pin_message("ch-general", MSG)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            ApiError::RateLimited {
                retry_after_ms: Some(800)
            }
        ),
        "{err:?}"
    );
}
