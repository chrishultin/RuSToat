//! Server members against a mock Stoat API: the user's own identity
//! (`PATCH /servers/{server}/members/{user}`), moderation (kicks and
//! bans), and Server Settings' ban list and unban.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::{DataBanCreate, DataMemberEdit, FieldsMember};
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::moderation::{
    end_timeout_edit, moderation_error_text, timeout_edit, timeout_until,
};
use stoat_gpui::profile_edit::{IdentityForm, ProfileImage, empty_member_edit};
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

#[tokio::test]
async fn a_moderator_renames_someone_and_removes_their_avatar() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-ada"))
        .and(body_json(
            json!({ "nickname": "Countess", "remove": ["Avatar"] }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(member(json!({
            "_id": { "server": "srv", "user": "u-ada" }, "nickname": "Countess"
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let loaded = IdentityForm {
        nickname: "Ada".into(),
        avatar: ProfileImage::Current("file-1".into()),
        pronouns: String::new(),
    };
    let edited = IdentityForm {
        nickname: "Countess".into(),
        ..IdentityForm::default()
    };
    let edit = IdentityForm::changes_for_other(&loaded, &edited);
    let member = client(&server)
        .edit_member("srv", "u-ada", &edit)
        .await
        .unwrap();
    assert_eq!(member.nickname.as_deref(), Some("Countess"));
}

#[tokio::test]
async fn new_roles_are_sent_whole() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-ada"))
        .and(body_json(json!({ "roles": ["mod", "staff"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(member(json!({
            "_id": { "server": "srv", "user": "u-ada" }, "roles": ["mod", "staff"]
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let edit = DataMemberEdit {
        roles: Some(vec!["mod".into(), "staff".into()]),
        ..empty_member_edit()
    };
    let member = client(&server)
        .edit_member("srv", "u-ada", &edit)
        .await
        .unwrap();
    assert_eq!(member.roles, ["mod", "staff"]);
}

#[tokio::test]
async fn taking_every_role_away_sends_an_empty_list() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-ada"))
        .and(body_json(json!({ "roles": [] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(member(json!({
            "_id": { "server": "srv", "user": "u-ada" }
        }))))
        .expect(1)
        .mount(&server)
        .await;

    let edit = DataMemberEdit {
        roles: Some(Vec::new()),
        ..empty_member_edit()
    };
    client(&server)
        .edit_member("srv", "u-ada", &edit)
        .await
        .unwrap();
}

/// Stoat's answer to a voice edit of `u-ada`.
fn ada() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(member(json!({
        "_id": { "server": "srv", "user": "u-ada" }
    })))
}

#[tokio::test]
async fn voice_moderation_sends_one_field_each() {
    let cases = [
        (
            DataMemberEdit {
                can_publish: Some(false),
                ..empty_member_edit()
            },
            json!({ "can_publish": false }),
        ),
        (
            DataMemberEdit {
                can_receive: Some(true),
                ..empty_member_edit()
            },
            json!({ "can_receive": true }),
        ),
        (
            DataMemberEdit {
                voice_channel: Some("ch-lounge".into()),
                ..empty_member_edit()
            },
            json!({ "voice_channel": "ch-lounge" }),
        ),
        (
            DataMemberEdit {
                remove: vec![FieldsMember::VoiceChannel],
                ..empty_member_edit()
            },
            json!({ "remove": ["VoiceChannel"] }),
        ),
    ];
    for (edit, body) in cases {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/servers/srv/members/u-ada"))
            .and(body_json(body))
            .respond_with(ada())
            .expect(1)
            .mount(&server)
            .await;
        client(&server)
            .edit_member("srv", "u-ada", &edit)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn moving_someone_who_left_the_call_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/members/u-ada"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({ "type": "NotConnected", "location": "x" })),
        )
        .mount(&server)
        .await;

    let edit = DataMemberEdit {
        voice_channel: Some("ch-lounge".into()),
        ..empty_member_edit()
    };
    let err = client(&server)
        .edit_member("srv", "u-ada", &edit)
        .await
        .unwrap_err();
    assert_eq!(
        moderation_error_text(&err, "x"),
        "They're no longer in a call."
    );
}

#[tokio::test]
async fn the_ban_list_parses_users_and_bans() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/servers/srv/bans"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "users": [{ "_id": "u-ada", "username": "ada", "discriminator": "0001" }],
            "bans": [
                { "_id": { "server": "srv", "user": "u-ada" }, "reason": "Spam" },
                { "_id": { "server": "srv", "user": "u-gone" } }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let list = client(&server).fetch_bans("srv").await.unwrap();
    assert_eq!(list.users.len(), 1);
    assert_eq!(list.bans[0].reason.as_deref(), Some("Spam"));
    assert_eq!(list.bans[1].id.user, "u-gone");
    assert_eq!(list.bans[1].reason, None);
}

#[tokio::test]
async fn unbanning_sends_one_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/servers/srv/bans/u-ada"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).unban("srv", "u-ada").await.unwrap();
}

#[tokio::test]
async fn unbanning_someone_no_longer_banned_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/servers/srv/bans/u-ada"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "type": "NotFound" })))
        .mount(&server)
        .await;

    let err = client(&server).unban("srv", "u-ada").await.unwrap_err();
    assert_eq!(
        stoat_gpui::server_settings::unban_error_text(&err),
        "They aren't banned any more."
    );
}
