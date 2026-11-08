//! People and memberships against a mock Stoat API.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::Channel;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{body_json, header, method, path};
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

#[tokio::test]
async fn fetching_mutuals_gets_the_friends_servers_and_channels() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/u1/mutual"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "users": ["u-ada", "u-bob"],
            "servers": ["srv"],
            "channels": ["grp", "dm"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mutual = client(&server).fetch_mutual("u1").await.unwrap();
    assert_eq!(mutual.users, ["u-ada", "u-bob"]);
    assert_eq!(mutual.servers, ["srv"]);
    assert_eq!(mutual.channels, ["grp", "dm"]);
}

#[tokio::test]
async fn mutuals_without_a_connection_are_missing_permission() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/u-blocked/mutual"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "type": "MissingPermission", "permission": "ViewProfile", "location": "x"
        })))
        .mount(&server)
        .await;

    let err = client(&server).fetch_mutual("u-blocked").await.unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::MissingPermission { .. })),
        "got {err:?}"
    );
}

#[tokio::test]
async fn opening_a_dm_gets_the_channel() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/u-bob/dm"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "DirectMessage", "_id": "dm-new", "active": true,
            "recipients": ["u-me", "u-bob"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let channel = client(&server).open_dm("u-bob").await.unwrap();
    assert!(matches!(
        channel,
        Channel::DirectMessage { ref id, active: true, ref recipients, .. }
            if id == "dm-new" && recipients.len() == 2
    ));
}

#[tokio::test]
async fn opening_a_dm_with_yourself_is_saved_notes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/u-me/dm"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "SavedMessages", "_id": "ch-saved", "user": "u-me"
        })))
        .mount(&server)
        .await;

    let channel = client(&server).open_dm("u-me").await.unwrap();
    assert!(matches!(channel, Channel::SavedMessages { ref id, .. } if id == "ch-saved"));
}

#[tokio::test]
async fn a_dm_with_a_non_friend_is_missing_send_message() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/u-stranger/dm"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "type": "MissingPermission", "permission": "SendMessage", "location": "x"
        })))
        .mount(&server)
        .await;

    let err = client(&server).open_dm("u-stranger").await.unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::MissingPermission { permission }) if permission == "SendMessage"),
        "got {err:?}"
    );
}

fn user_json(relationship: &str) -> serde_json::Value {
    json!({
        "_id": "u-bob", "username": "bob", "discriminator": "0001",
        "relationship": relationship, "online": true
    })
}

#[tokio::test]
async fn friend_and_block_routes_answer_with_the_user() {
    use stoat_gpui::api::models::RelationshipStatus;
    let server = MockServer::start().await;
    for (verb, route, relationship) in [
        ("PUT", "/users/u-bob/friend", "Outgoing"),
        ("DELETE", "/users/u-bob/friend", "None"),
        ("PUT", "/users/u-bob/block", "Blocked"),
        ("DELETE", "/users/u-bob/block", "None"),
    ] {
        Mock::given(method(verb))
            .and(path(route))
            .and(header("x-session-token", "tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_json(relationship)))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
    }
    let client = client(&server);
    assert_eq!(
        client.add_friend("u-bob").await.unwrap().relationship,
        RelationshipStatus::Outgoing
    );
    assert_eq!(
        client.remove_friend("u-bob").await.unwrap().relationship,
        RelationshipStatus::None
    );
    assert_eq!(
        client.block_user("u-bob").await.unwrap().relationship,
        RelationshipStatus::Blocked
    );
    assert_eq!(
        client.unblock_user("u-bob").await.unwrap().relationship,
        RelationshipStatus::None
    );
}

#[tokio::test]
async fn relationship_refusals_decode() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/users/u-friend/friend"))
        .respond_with(
            ResponseTemplate::new(409)
                .set_body_json(json!({ "type": "AlreadyFriends", "location": "x" })),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/users/u-blocker/friend"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "BlockedByOther", "location": "x" })),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/users/u-popular/friend"))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            json!({ "type": "TooManyPendingFriendRequests", "max": 100, "location": "x" }),
        ))
        .mount(&server)
        .await;
    let client = client(&server);

    let err = client.add_friend("u-friend").await.unwrap_err();
    assert!(matches!(err.error_type(), Some(ErrorType::AlreadyFriends)));
    assert!(err.relationship_already_done());

    let err = client.add_friend("u-blocker").await.unwrap_err();
    assert!(matches!(err.error_type(), Some(ErrorType::BlockedByOther)));
    assert!(!err.relationship_already_done());

    let err = client.add_friend("u-popular").await.unwrap_err();
    assert!(matches!(
        err.error_type(),
        Some(ErrorType::TooManyPendingFriendRequests { max: 100 })
    ));
}

#[tokio::test]
async fn a_friend_request_by_username_answers_with_the_user() {
    use stoat_gpui::api::models::RelationshipStatus;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/users/friend"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "username": "bob#0001" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(user_json("Outgoing")))
        .expect(1)
        .mount(&server)
        .await;
    let bob = client(&server)
        .send_friend_request("bob#0001")
        .await
        .unwrap();
    assert_eq!(bob.id, "u-bob");
    assert_eq!(bob.relationship, RelationshipStatus::Outgoing);
}

#[tokio::test]
async fn friend_requests_by_username_can_be_refused() {
    let server = MockServer::start().await;
    for (username, status, error) in [
        ("nobody#0000", 404, "NotFound"),
        ("no-tag", 400, "InvalidProperty"),
        ("robot#0001", 400, "IsBot"),
    ] {
        Mock::given(method("POST"))
            .and(path("/users/friend"))
            .and(body_json(json!({ "username": username })))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(json!({ "type": error, "location": "x" })),
            )
            .mount(&server)
            .await;
    }
    let client = client(&server);
    let err = client.send_friend_request("nobody#0000").await.unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotFound)),
        "{err:?}"
    );
    let err = client.send_friend_request("no-tag").await.unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::InvalidProperty)),
        "{err:?}"
    );
    let err = client.send_friend_request("robot#0001").await.unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::IsBot)),
        "{err:?}"
    );
    assert_eq!(err.to_string(), "Bots can't be friends.");
}

/// Who Stoat answers a `PATCH /users/@me` with: the user, as changed.
fn me_with_status(status: serde_json::Value) -> serde_json::Value {
    json!({
        "_id": "u-me", "username": "me", "discriminator": "0001",
        "relationship": "User", "online": true, "status": status
    })
}

#[tokio::test]
async fn setting_a_presence_sends_only_the_presence() {
    use stoat_gpui::api::models::Presence;
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/users/@me"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "status": { "presence": "Busy" } })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(me_with_status(json!({ "text": "brb", "presence": "Busy" }))),
        )
        .expect(1)
        .mount(&server)
        .await;

    let me = client(&server).set_presence(Presence::Busy).await.unwrap();
    // Stoat merges the status: the text stays.
    let status = me.status.unwrap();
    assert_eq!(status.presence, Some(Presence::Busy));
    assert_eq!(status.text.as_deref(), Some("brb"));
}

#[tokio::test]
async fn setting_a_custom_status_sends_only_the_text() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/users/@me"))
        .and(body_json(json!({ "status": { "text": "brb" } })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(me_with_status(json!({ "text": "brb" }))),
        )
        .expect(1)
        .mount(&server)
        .await;

    let me = client(&server)
        .set_status_text(Some("  brb "))
        .await
        .unwrap();
    assert_eq!(me.status.unwrap().text.as_deref(), Some("brb"));
}

#[tokio::test]
async fn clearing_the_custom_status_removes_it() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/users/@me"))
        .and(body_json(json!({ "remove": ["StatusText"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "u-me", "username": "me", "discriminator": "0001",
            "relationship": "User", "online": true
        })))
        .expect(2)
        .mount(&server)
        .await;

    let client = client(&server);
    assert!(client.set_status_text(None).await.unwrap().status.is_none());
    // Blank text clears it too.
    client.set_status_text(Some("   ")).await.unwrap();
}

#[tokio::test]
async fn a_status_stoat_refuses_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "type": "FailedValidation", "error": "status.text: length", "location": "x"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .set_status_text(Some(&"a".repeat(129)))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApiError::Api(e) if matches!(e.error_type, ErrorType::FailedValidation { .. })),
        "got {err:?}"
    );
}

/// A profile edit as the Profile page builds it, sent as exactly `body`.
async fn assert_profile_edit_sends(
    loaded: stoat_gpui::profile_edit::ProfileForm,
    edited: stoat_gpui::profile_edit::ProfileForm,
    body: serde_json::Value,
) {
    use stoat_gpui::profile_edit::ProfileForm;
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/users/@me"))
        .and(header("x-session-token", "tok"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "u-me", "username": "me", "discriminator": "0001",
            "display_name": "Ada", "relationship": "User", "online": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let me = client(&server)
        .edit_user(&ProfileForm::changes(&loaded, &edited))
        .await
        .unwrap();
    assert_eq!(me.display_name.as_deref(), Some("Ada"));
}

fn profile(display_name: &str, pronouns: &str, bio: &str) -> stoat_gpui::profile_edit::ProfileForm {
    stoat_gpui::profile_edit::ProfileForm {
        display_name: display_name.into(),
        pronouns: pronouns.into(),
        bio: bio.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_new_display_name_sends_only_the_name() {
    assert_profile_edit_sends(
        profile("", "they/them", "hello"),
        profile(" Ada ", "they/them", "hello"),
        json!({ "display_name": "Ada" }),
    )
    .await;
}

#[tokio::test]
async fn new_pronouns_and_bio_send_the_bio_as_profile_content() {
    assert_profile_edit_sends(
        profile("Ada", "", ""),
        profile("Ada", "she/her", "hi"),
        json!({ "pronouns": "she/her", "profile": { "content": "hi" } }),
    )
    .await;
}

#[tokio::test]
async fn clearing_the_name_and_bio_removes_them() {
    assert_profile_edit_sends(
        profile("Ada", "she/her", "hi"),
        profile("", "she/her", "  "),
        json!({ "remove": ["DisplayName", "ProfileContent"] }),
    )
    .await;
}

#[tokio::test]
async fn a_new_avatar_and_banner_send_their_upload_ids() {
    use stoat_gpui::profile_edit::{ProfileForm, ProfileImage};
    assert_profile_edit_sends(
        ProfileForm::default(),
        ProfileForm {
            avatar: ProfileImage::Current("file-1".into()),
            banner: ProfileImage::Current("file-2".into()),
            ..ProfileForm::default()
        },
        json!({ "avatar": "file-1", "profile": { "background": "file-2" } }),
    )
    .await;
}

#[tokio::test]
async fn changing_the_username_sends_the_name_and_password() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/users/@me/username"))
        .and(header("x-session-token", "tok"))
        .and(body_json(
            json!({ "username": "ada", "password": "pw123456" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "u-me", "username": "ada", "discriminator": "5678",
            "relationship": "User", "online": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let me = client(&server)
        .change_username("ada", "pw123456")
        .await
        .unwrap();
    assert_eq!(me.username, "ada");
    assert_eq!(me.discriminator, "5678");
}

#[tokio::test]
async fn a_new_tag_too_soon_isnt_a_rate_limit() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/users/@me/username"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(json!({ "type": "DiscriminatorChangeRatelimited" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .change_username("ada", "pw123456")
        .await
        .unwrap_err();
    assert!(matches!(
        err.error_type(),
        Some(ErrorType::DiscriminatorChangeRatelimited)
    ));
}

#[tokio::test]
async fn a_plain_429_on_a_username_change_is_still_a_rate_limit() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/users/@me/username"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({ "retry_after": 2500 })))
        .mount(&server)
        .await;

    let err = client(&server)
        .change_username("ada", "pw123456")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ApiError::RateLimited {
            retry_after_ms: Some(2500)
        }
    ));
}
