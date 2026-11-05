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
