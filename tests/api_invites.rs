//! Invites against a mock Stoat API: `GET /invites/{code}` (look one up),
//! `POST /invites/{code}` (join) and `POST /channels/{id}/invites` (create).

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::{Invite, InviteJoinResponse, InviteResponse};
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

fn server_json() -> serde_json::Value {
    json!({
        "_id": "srv-new", "owner": "u-ada", "name": "Rust",
        "channels": ["ch-lobby"], "default_permissions": 0, "approximate_member_count": 2
    })
}

#[tokio::test]
async fn a_server_invite_is_looked_up() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/invites/wVEJDGVs"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "Server", "code": "wVEJDGVs", "server_id": "srv-new",
            "server_name": "Rust", "channel_id": "ch-lobby", "channel_name": "lobby",
            "user_name": "ada", "member_count": 1234
        })))
        .expect(1)
        .mount(&server)
        .await;

    let invite = client(&server).fetch_invite("wVEJDGVs").await.unwrap();
    let InviteResponse::Server {
        server_id,
        server_icon,
        member_count,
        ..
    } = invite
    else {
        panic!("a server invite: {invite:?}");
    };
    assert_eq!(server_id, "srv-new");
    assert!(server_icon.is_none());
    assert_eq!(member_count, 1234);
}

#[tokio::test]
async fn a_group_invite_is_looked_up() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/invites/grp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "Group", "code": "grp", "channel_id": "g1",
            "channel_name": "Friends", "user_name": "ada"
        })))
        .mount(&server)
        .await;

    let invite = client(&server).fetch_invite("grp").await.unwrap();
    assert!(matches!(invite, InviteResponse::Group { channel_id, .. } if channel_id == "g1"));
}

#[tokio::test]
async fn an_unknown_invite_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/invites/gone"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "type": "NotFound" })))
        .mount(&server)
        .await;

    let err = client(&server).fetch_invite("gone").await.unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotFound)),
        "{err:?}"
    );
}

#[tokio::test]
async fn joining_a_server_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/invites/wVEJDGVs"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "Server",
            "server": server_json(),
            "channels": [
                { "channel_type": "TextChannel", "_id": "ch-lobby", "server": "srv-new", "name": "lobby" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let joined = client(&server).join_invite("wVEJDGVs").await.unwrap();
    let InviteJoinResponse::Server {
        server: joined,
        channels,
    } = joined
    else {
        panic!("a server join");
    };
    assert_eq!(joined.id, "srv-new");
    assert_eq!(channels.len(), 1);

    let requests = server.received_requests().await.unwrap();
    assert!(requests[0].body.is_empty(), "{:?}", requests[0].body);
}

#[tokio::test]
async fn joining_a_group_gives_the_group_and_its_people() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/invites/grp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "Group",
            "channel": {
                "channel_type": "Group", "_id": "g1", "name": "Friends",
                "owner": "u-ada", "recipients": ["u-ada", "u-me"]
            },
            "users": [{ "_id": "u-ada", "username": "ada", "discriminator": "0001", "relationship": "None", "online": false }]
        })))
        .mount(&server)
        .await;

    let joined = client(&server).join_invite("grp").await.unwrap();
    let InviteJoinResponse::Group { users, .. } = joined else {
        panic!("a group join");
    };
    assert_eq!(users.len(), 1);
}

#[tokio::test]
async fn already_in_the_server_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/invites/wVEJDGVs"))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(json!({ "type": "AlreadyInServer" })),
        )
        .mount(&server)
        .await;

    let err = client(&server).join_invite("wVEJDGVs").await.unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(stoat_gpui::invites::already_joined(&err));
}

fn created() -> serde_json::Value {
    json!({
        "type": "Server", "_id": "abcdEFGH", "server": "s1", "creator": "u-me",
        "channel": "c1", "uses": 0
    })
}

#[tokio::test]
async fn an_invite_without_options_sends_an_empty_object() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/c1/invites"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(created()))
        .expect(1)
        .mount(&server)
        .await;

    let invite = client(&server)
        .create_invite("c1", None, None)
        .await
        .unwrap();
    assert!(matches!(&invite, Invite::Server { code, .. } if code == "abcdEFGH"));
    assert_eq!(stoat_gpui::invites::invite_code(&invite), "abcdEFGH");
}

#[tokio::test]
async fn an_invite_with_options_sends_both() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/c1/invites"))
        .and(body_json(json!({
            "max_uses": 10,
            "expires": "2026-11-14T12:00:00.000Z"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(created()))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .create_invite("c1", Some(10), Some("2026-11-14T12:00:00.000Z"))
        .await
        .unwrap();
}

#[tokio::test]
async fn creating_without_permission_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/c1/invites"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(
                json!({ "type": "MissingPermission", "permission": "InviteOthers" }),
            ),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .create_invite("c1", None, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::MissingPermission { .. })),
        "{err:?}"
    );
}
