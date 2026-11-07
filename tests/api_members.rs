//! The user's own identity on a server against a mock Stoat API:
//! `PATCH /servers/{server}/members/{user}`.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::profile_edit::{IdentityForm, ProfileImage};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

/// Stoat's answer: the member, as changed.
fn member(extra: serde_json::Value) -> serde_json::Value {
    let mut member = json!({
        "_id": { "server": "srv", "user": "u-me" },
        "joined_at": "2026-01-01T00:00:00.000Z",
        "roles": []
    });
    member
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    member
}

#[tokio::test]
async fn a_new_nickname_sends_only_the_nickname() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-me"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "nickname": "Ada" })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(member(json!({ "nickname": "Ada" }))),
        )
        .expect(1)
        .mount(&server)
        .await;

    let edit = IdentityForm::changes(
        &IdentityForm::default(),
        &IdentityForm {
            nickname: " Ada ".into(),
            ..IdentityForm::default()
        },
    );
    let member = client(&server)
        .edit_member("srv", "u-me", &edit)
        .await
        .unwrap();
    assert_eq!(
        (member.id.server.as_str(), member.id.user.as_str()),
        ("srv", "u-me")
    );
    assert_eq!(member.nickname.as_deref(), Some("Ada"));
}

#[tokio::test]
async fn removing_the_server_avatar_sends_only_the_removal() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-me"))
        .and(body_json(json!({ "remove": ["Avatar"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(member(json!({}))))
        .expect(1)
        .mount(&server)
        .await;

    let loaded = IdentityForm {
        avatar: ProfileImage::Current("file-1".into()),
        ..IdentityForm::default()
    };
    let edit = IdentityForm::changes(&loaded, &IdentityForm::default());
    let member = client(&server)
        .edit_member("srv", "u-me", &edit)
        .await
        .unwrap();
    assert!(member.avatar.is_none());
}

#[tokio::test]
async fn a_change_without_permission_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-me"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "type": "MissingPermission", "permission": "ChangeNickname", "location": "x"
        })))
        .mount(&server)
        .await;

    let edit = IdentityForm::changes(
        &IdentityForm::default(),
        &IdentityForm {
            nickname: "Ada".into(),
            ..IdentityForm::default()
        },
    );
    let err = client(&server)
        .edit_member("srv", "u-me", &edit)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApiError::Api(e) if matches!(e.error_type, ErrorType::MissingPermission { .. })),
        "got {err:?}"
    );
}
