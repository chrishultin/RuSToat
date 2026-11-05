//! People and memberships against a mock Stoat API.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn fetching_a_member_gets_the_bare_membership() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/servers/srv-new/members/u-me"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": { "server": "srv-new", "user": "u-me" },
            "joined_at": "2026-11-05T12:00:00.000Z",
            "nickname": "Me Here",
            "roles": ["staff"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let member = client(&server)
        .fetch_member("srv-new", "u-me")
        .await
        .unwrap();
    assert_eq!(
        (member.id.server.as_str(), member.id.user.as_str()),
        ("srv-new", "u-me")
    );
    assert_eq!(member.nickname.as_deref(), Some("Me Here"));
    assert_eq!(member.roles, ["staff"]);
}

#[tokio::test]
async fn fetching_a_membership_that_isnt_there_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/servers/srv-new/members/u-me"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({ "type": "NotFound", "location": "x" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .fetch_member("srv-new", "u-me")
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApiError::Api(e) if matches!(e.error_type, ErrorType::NotFound)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn fetching_a_profile_gets_the_bio_and_banner() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/u-ada/profile"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": "Hello, **world**",
            "background": {
                "_id": "file-banner", "tag": "backgrounds", "filename": "b.png",
                "metadata": { "type": "Image", "width": 1000, "height": 300 },
                "content_type": "image/png", "size": 1000
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let profile = client(&server).fetch_profile("u-ada").await.unwrap();
    assert_eq!(profile.content.as_deref(), Some("Hello, **world**"));
    assert_eq!(profile.background.unwrap().id, "file-banner");
}

#[tokio::test]
async fn an_empty_profile_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/u-ada/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let profile = client(&server).fetch_profile("u-ada").await.unwrap();
    assert_eq!((profile.content, profile.background), (None, None));
}

#[tokio::test]
async fn a_profile_without_a_connection_is_missing_permission() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/u-stranger/profile"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "type": "MissingPermission", "permission": "ViewProfile", "location": "x"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .fetch_profile("u-stranger")
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::MissingPermission { permission }) if permission == "ViewProfile"),
        "got {err:?}"
    );
}
