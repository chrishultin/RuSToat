//! Searching a channel against a mock Stoat API: the path, the session
//! header, the exact body (never `pinned`), both response shapes, and refusals.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::{BulkMessageResponse, MessageSort};
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::search::{SearchCursor, page_limit};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MSG: &str = "01JC0000000000000000000009";

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

fn found(content: &str) -> serde_json::Value {
    json!({ "_id": MSG, "channel": "ch-general", "author": "u-me", "content": content })
}

#[tokio::test]
async fn a_search_posts_the_query_with_users_and_no_pinned() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/search"))
        .and(header("x-session-token", "tok"))
        // Exactly this: Stoat refuses `pinned` next to `query`, even false.
        .and(body_json(json!({
            "query": "hello",
            "limit": 50,
            "sort": "Latest",
            "include_users": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [found("hello there")],
            "users": [{ "_id": "u-me", "username": "me", "discriminator": "0001", "relationship": "None", "online": false }],
            "members": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let page = client(&server)
        .search_messages("ch-general", "hello", MessageSort::Latest, 50, None)
        .await
        .unwrap();
    let BulkMessageResponse::MessagesAndUsers {
        messages,
        users,
        members,
    } = page
    else {
        panic!("expected users with the messages");
    };
    assert_eq!(messages[0].id, MSG);
    assert_eq!(users[0].username, "me");
    assert_eq!(members, Some(Vec::new()));
}

#[tokio::test]
async fn the_chosen_sort_is_sent() {
    for (sort, name) in [
        (MessageSort::Latest, "Latest"),
        (MessageSort::Oldest, "Oldest"),
        (MessageSort::Relevance, "Relevance"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/channels/ch-general/search"))
            .and(body_json(json!({
                "query": "x",
                "limit": 50,
                "sort": name,
                "include_users": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;
        client(&server)
            .search_messages("ch-general", "x", sort, 50, None)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_bare_array_of_messages_parses_too() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([found("one"), found("two")])))
        .mount(&server)
        .await;

    let page = client(&server)
        .search_messages("ch-general", "one", MessageSort::Latest, 50, None)
        .await
        .unwrap();
    assert!(matches!(page, BulkMessageResponse::JustMessages(ref m) if m.len() == 2));
}

#[tokio::test]
async fn a_search_without_matches_is_an_empty_page() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "messages": [], "users": [], "members": [] })),
        )
        .mount(&server)
        .await;

    let page = client(&server)
        .search_messages("ch-general", "nothing", MessageSort::Latest, 50, None)
        .await
        .unwrap();
    assert!(
        matches!(page, BulkMessageResponse::MessagesAndUsers { ref messages, .. } if messages.is_empty())
    );
}

#[tokio::test]
async fn a_refused_search_is_an_invalid_operation_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/search"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "type": "InvalidOperation" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .search_messages("ch-general", "hi", MessageSort::Latest, 50, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::InvalidOperation)),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_channel_that_cant_be_read_is_a_missing_permission_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-locked/search"))
        .respond_with(ResponseTemplate::new(403).set_body_json(
            json!({ "type": "MissingPermission", "permission": "ReadMessageHistory" }),
        ))
        .mount(&server)
        .await;

    let err = client(&server)
        .search_messages("ch-locked", "hi", MessageSort::Latest, 50, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::MissingPermission { .. })),
        "{err:?}"
    );
}

#[tokio::test]
async fn searching_too_fast_is_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/search"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({ "retry_after": 1200 })))
        .mount(&server)
        .await;

    let err = client(&server)
        .search_messages("ch-general", "hi", MessageSort::Latest, 50, None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            ApiError::RateLimited {
                retry_after_ms: Some(1200)
            }
        ),
        "{err:?}"
    );
}

/// Searches with `cursor` and `sort`, expecting exactly `body`.
async fn search_sends(sort: MessageSort, cursor: Option<SearchCursor>, body: serde_json::Value) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/search"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    let limit = page_limit(&sort);
    client(&server)
        .search_messages("ch-general", "x", sort, limit, cursor)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_next_latest_page_is_before_the_last_result() {
    search_sends(
        MessageSort::Latest,
        Some(SearchCursor::Before(MSG.into())),
        json!({
            "query": "x",
            "limit": 50,
            "before": MSG,
            "sort": "Latest",
            "include_users": true
        }),
    )
    .await;
}

#[tokio::test]
async fn the_next_oldest_page_is_after_the_last_result() {
    search_sends(
        MessageSort::Oldest,
        Some(SearchCursor::After(MSG.into())),
        json!({
            "query": "x",
            "limit": 50,
            "after": MSG,
            "sort": "Oldest",
            "include_users": true
        }),
    )
    .await;
}

#[tokio::test]
async fn a_relevance_search_asks_for_a_hundred() {
    search_sends(
        MessageSort::Relevance,
        None,
        json!({
            "query": "x",
            "limit": 100,
            "sort": "Relevance",
            "include_users": true
        }),
    )
    .await;
}
