//! A channel's or group's settings against a mock Stoat API, from the
//! Channel Settings window: `PATCH /channels/{id}`.

use revolt_permissions::Override;
use serde_json::json;
use stoat_gpui::api::StoatClient;
use stoat_gpui::api::models::{Channel, FieldsChannel};
use stoat_gpui::channel_permissions::channel_permissions_error_text;
use stoat_gpui::channel_settings::{empty_channel_edit, mature_edit, save_error_text};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn an_edit_sends_only_what_is_set() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/channels/ch-a"))
        .and(header("x-session-token", "tok"))
        .and(body_json(
            json!({ "name": "general", "remove": ["Description"] }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "TextChannel", "_id": "ch-a", "server": "srv",
            "name": "general", "slowmode": 10
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut edit = empty_channel_edit();
    edit.name = Some("general".into());
    edit.remove.push(FieldsChannel::Description);
    let saved = client(&server).edit_channel("ch-a", &edit).await.unwrap();
    let Channel::TextChannel {
        name,
        description,
        slowmode,
        ..
    } = saved
    else {
        panic!("a text channel");
    };
    assert_eq!(name, "general");
    assert_eq!(description, None);
    assert_eq!(slowmode, Some(10));
}

#[tokio::test]
async fn an_edit_without_removals_sends_no_remove() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/channels/grp"))
        .and(body_json(
            json!({ "icon": "ic-new", "description": "Pals" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "Group", "_id": "grp", "name": "Friends",
            "owner": "u-owner", "recipients": ["u-owner", "u-plain"],
            "description": "Pals",
            "icon": {
                "_id": "ic-new", "tag": "icons", "filename": "i.png",
                "metadata": { "type": "Image", "width": 64, "height": 64 },
                "content_type": "image/png", "size": 100
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut edit = empty_channel_edit();
    edit.icon = Some("ic-new".into());
    edit.description = Some("Pals".into());
    let saved = client(&server).edit_channel("grp", &edit).await.unwrap();
    let Channel::Group {
        description, icon, ..
    } = saved
    else {
        panic!("a group");
    };
    assert_eq!(description.as_deref(), Some("Pals"));
    assert_eq!(icon.map(|icon| icon.id).as_deref(), Some("ic-new"));
}

#[tokio::test]
async fn a_refusal_reads_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/channels/ch-a"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(
                json!({ "type": "MissingPermission", "permission": "ManageChannel" }),
            ),
        )
        .mount(&server)
        .await;

    let mut edit = empty_channel_edit();
    edit.name = Some("x".into());
    let err = client(&server)
        .edit_channel("ch-a", &edit)
        .await
        .unwrap_err();
    assert_eq!(
        save_error_text(&err),
        "You don't have permission to change this channel."
    );
}

#[tokio::test]
async fn marking_mature_sends_nsfw_alone() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/channels/ch-a"))
        .and(body_json(json!({ "nsfw": true })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "TextChannel", "_id": "ch-a", "server": "srv",
            "name": "a", "nsfw": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let saved = client(&server)
        .edit_channel("ch-a", &mature_edit(true))
        .await
        .unwrap();
    assert!(matches!(saved, Channel::TextChannel { nsfw: true, .. }));
}

#[tokio::test]
async fn slowmode_off_is_sent_as_zero() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/channels/ch-a"))
        .and(body_json(json!({ "slowmode": 0 })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "TextChannel", "_id": "ch-a", "server": "srv",
            "name": "a", "slowmode": 0
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut edit = empty_channel_edit();
    edit.slowmode = Some(0);
    client(&server).edit_channel("ch-a", &edit).await.unwrap();
}

#[tokio::test]
async fn everyones_override_is_sent_as_allow_and_deny() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/channels/ch-a/permissions/default"))
        .and(header("x-session-token", "tok"))
        .and(body_json(
            json!({ "permissions": { "allow": 2, "deny": 4 } }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "TextChannel", "_id": "ch-a", "server": "srv", "name": "a",
            "default_permissions": { "a": 2, "d": 4 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let saved = client(&server)
        .set_channel_default_permissions("ch-a", &Override { allow: 2, deny: 4 })
        .await
        .unwrap();
    let Channel::TextChannel {
        default_permissions: Some(field),
        ..
    } = saved
    else {
        panic!("a text channel with a default override");
    };
    assert_eq!((field.a, field.d), (2, 4));
}

#[tokio::test]
async fn an_override_stoat_wont_grant_reads_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/channels/ch-a/permissions/default"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "CannotGiveMissingPermissions" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .set_channel_default_permissions("ch-a", &Override { allow: 2, deny: 0 })
        .await
        .unwrap_err();
    assert_eq!(
        channel_permissions_error_text(&err),
        "You can't grant permissions you don't have."
    );
}
