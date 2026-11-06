//! Gifbox (the GIF picker's service) against a mock server: the session
//! header, the locale and query parameters, lenient parsing of what the
//! live service sends, and errors.

use serde_json::json;
use stoat_gpui::api::ApiError;
use stoat_gpui::api::gifbox::{GifCategory, GifboxClient};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> GifboxClient {
    GifboxClient::new(server.uri(), "tok")
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

#[tokio::test]
async fn categories_send_the_token_and_locale() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/categories"))
        .and(header("x-session-token", "tok"))
        .and(query_param("locale", "en_US"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "title": "happy", "image": "https://media.gifbox.me/media/post/a/gif_high" },
            { "title": "no image" },
            { "title": "sad", "image": "https://media.gifbox.me/media/post/b/gif_low", "extra": 1 }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let categories = client(&server).categories().await.unwrap();
    assert_eq!(
        categories,
        vec![
            GifCategory {
                title: "happy".into(),
                image: "https://media.gifbox.me/media/post/a/gif_high".into()
            },
            GifCategory {
                title: "sad".into(),
                image: "https://media.gifbox.me/media/post/b/gif_low".into()
            },
        ]
    );
}

#[tokio::test]
async fn categories_that_arent_a_list_are_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/categories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(&server)
        .await;
    assert!(client(&server).categories().await.unwrap().is_empty());
}

#[tokio::test]
async fn trending_asks_for_a_limit_and_parses_the_live_shape() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/trending"))
        .and(header("x-session-token", "tok"))
        .and(query_param("locale", "en_US"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("gifbox_trending.json")))
        .expect(1)
        .mount(&server)
        .await;

    let page = client(&server).trending(50).await.unwrap();
    // The one with no URL is skipped; the one with no formats is kept.
    let ids: Vec<&str> = page.results.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "cXQwxRj5VVmb4q_qmUAg6",
            "smGy30kWfDtNbJyWRnq8a",
            "noPreviewHere"
        ]
    );
    assert_eq!(page.next.as_deref(), Some("eyJvZmZzZXQiOjUwfQ"));

    let animated = &page.results[0];
    assert_eq!(animated.url, "https://gifbox.me/view/cXQwxRj5VVmb4q_qmUAg6");
    assert_eq!(
        animated.still().map(|m| m.url.as_str()),
        Some("https://media.gifbox.me/media/post/cXQwxRj5VVmb4q_qmUAg6/poster")
    );
    assert_eq!(
        animated.animated().map(|m| m.url.as_str()),
        Some("https://media.gifbox.me/media/post/cXQwxRj5VVmb4q_qmUAg6/gif_high")
    );
    assert_eq!(animated.size(), Some((498, 280)));

    let video_only = &page.results[1];
    assert!(video_only.still().is_some());
    assert!(video_only.animated().is_none());
    assert_eq!(video_only.size(), Some((640, 360)));

    assert!(page.results[2].still().is_none());
}

#[tokio::test]
async fn a_search_encodes_its_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(header("x-session-token", "tok"))
        .and(query_param("locale", "en_US"))
        // Decoded by the matcher: sent as `cats+%26+dogs`.
        .and(query_param("query", "cats & dogs"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let page = client(&server).search("cats & dogs", 50).await.unwrap();
    assert!(page.results.is_empty());
    assert_eq!(page.next, None);
    let requests = server.received_requests().await.unwrap();
    let query = requests[0].url.query().unwrap_or_default().to_string();
    assert!(
        query.contains("query=cats+%26+dogs"),
        "the & must be escaped: {query}"
    );
}

#[tokio::test]
async fn a_rejected_token_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/trending"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({ "type": "NotAuthenticated" })),
        )
        .mount(&server)
        .await;
    let err = client(&server).trending(50).await.unwrap_err();
    assert!(matches!(err, ApiError::Unauthorized), "{err:?}");
}

#[tokio::test]
async fn too_many_searches_are_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({ "retry_after": 4000 })))
        .mount(&server)
        .await;
    let err = client(&server).search("cat", 50).await.unwrap_err();
    assert!(
        matches!(
            err,
            ApiError::RateLimited {
                retry_after_ms: Some(4000)
            }
        ),
        "{err:?}"
    );
}
