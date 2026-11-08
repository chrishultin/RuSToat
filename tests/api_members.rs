//! Server members against a mock Stoat API: the user's own identity
//! (`PATCH /servers/{server}/members/{user}`), and moderation (kicks and
//! bans).

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::DataBanCreate;
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::moderation::{
    end_timeout_edit, moderation_error_text, timeout_edit, timeout_until,
};
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

#[tokio::test]
async fn a_kick_deletes_the_membership() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/servers/srv/members/u-ada"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).kick_member("srv", "u-ada").await.unwrap();
}

#[tokio::test]
async fn a_ban_sends_its_reason_and_history() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/bans/u-ada"))
        .and(header("x-session-token", "tok"))
        .and(body_json(
            json!({ "reason": "spam", "delete_message_seconds": 86400 }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": { "server": "srv", "user": "u-ada" }, "reason": "spam"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ban = DataBanCreate {
        reason: Some("spam".into()),
        delete_message_seconds: Some(86400),
    };
    let ban = client(&server)
        .ban_user("srv", "u-ada", &ban)
        .await
        .unwrap();
    assert_eq!(ban.id.user, "u-ada");
    assert_eq!(ban.reason.as_deref(), Some("spam"));
}

#[tokio::test]
async fn a_bare_ban_leaves_out_what_is_unset() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/bans/u-ada"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": { "server": "srv", "user": "u-ada" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ban = DataBanCreate {
        reason: None,
        delete_message_seconds: None,
    };
    client(&server)
        .ban_user("srv", "u-ada", &ban)
        .await
        .unwrap();
}

#[tokio::test]
async fn kicking_someone_who_outranks_you_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/servers/srv/members/u-ada"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "NotElevated", "location": "x" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .kick_member("srv", "u-ada")
        .await
        .unwrap_err();
    assert_eq!(
        moderation_error_text(&err, "Couldn't kick them."),
        "They rank too high for you to do that."
    );
}

#[tokio::test]
async fn a_timeout_sends_when_it_ends() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-ada"))
        .and(body_json(json!({ "timeout": "2026-11-08T12:05:00.000Z" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(member(json!({
            "_id": { "server": "srv", "user": "u-ada" },
            "timeout": "2026-11-08T12:05:00.000Z"
        }))))
        .expect(1)
        .mount(&server)
        .await;

    // 2026-11-08T12:00:00Z, plus 5 minutes.
    let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_794_139_200);
    let edit = timeout_edit(timeout_until(now, 300));
    let member = client(&server)
        .edit_member("srv", "u-ada", &edit)
        .await
        .unwrap();
    assert!(member.timeout.is_some());
}

#[tokio::test]
async fn ending_a_timeout_removes_it() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-ada"))
        .and(body_json(json!({ "remove": ["Timeout"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(member(json!({
            "_id": { "server": "srv", "user": "u-ada" }
        }))))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .edit_member("srv", "u-ada", &end_timeout_edit())
        .await
        .unwrap();
}

#[tokio::test]
async fn timing_out_a_moderator_says_they_rank_too_high() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-ada"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "IsElevated", "location": "x" })),
        )
        .mount(&server)
        .await;

    let now = std::time::SystemTime::now();
    let err = client(&server)
        .edit_member("srv", "u-ada", &timeout_edit(timeout_until(now, 60)))
        .await
        .unwrap_err();
    assert_eq!(
        moderation_error_text(&err, "x"),
        "They rank too high for you to do that."
    );
}
