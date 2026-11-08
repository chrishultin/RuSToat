//! Servers and channels against a mock Stoat API: `POST /servers/create`
//! (create a server), `POST /servers/{id}/channels` (create a channel),
//! `PATCH /servers/{id}` (its categories) and `DELETE /channels/{id}`
//! (delete a channel).

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::{Category, Channel};
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{body_json, body_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn creating_a_server_sends_only_its_name() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/servers/create"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "name": "Lounge" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "server": {
                "_id": "srv-new", "owner": "u-me", "name": "Lounge",
                "channels": ["ch-general"], "default_permissions": 0,
                "approximate_member_count": 1
            },
            "channels": [
                { "channel_type": "TextChannel", "_id": "ch-general", "server": "srv-new", "name": "General" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let created = client(&server).create_server("Lounge").await.unwrap();
    assert_eq!(created.server.id, "srv-new");
    assert_eq!(created.server.owner, "u-me");
    assert!(created.server.categories.is_none());
    let [Channel::TextChannel { id, name, .. }] = &created.channels[..] else {
        panic!("one text channel: {:?}", created.channels);
    };
    assert_eq!((id.as_str(), name.as_str()), ("ch-general", "General"));
}

#[tokio::test]
async fn too_many_servers_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/servers/create"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({ "type": "TooManyServers", "max": 100 })),
        )
        .mount(&server)
        .await;

    let err = client(&server).create_server("Lounge").await.unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(
        matches!(
            err.error_type(),
            Some(ErrorType::TooManyServers { max: 100 })
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn deleting_a_channel_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/channels/c1"))
        .and(header("x-session-token", "tok"))
        .and(body_string(""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).delete_channel("c1").await.unwrap();
}

#[tokio::test]
async fn deleting_a_channel_without_permission_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/channels/c1"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "type": "MissingPermission", "permission": "ManageChannel"
        })))
        .mount(&server)
        .await;

    let err = client(&server).delete_channel("c1").await.unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(
        matches!(
            err.error_type(),
            Some(ErrorType::MissingPermission { permission }) if permission == "ManageChannel"
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn creating_a_voice_channel_sends_its_type_and_name() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/servers/s1/channels"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "type": "Voice", "name": "Lobby" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "TextChannel", "_id": "c9", "server": "s1", "name": "Lobby",
            "voice": {}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let channel = client(&server)
        .create_channel("s1", "Lobby", true)
        .await
        .unwrap();
    let Channel::TextChannel {
        id, server, voice, ..
    } = channel
    else {
        panic!("a text channel: {channel:?}");
    };
    assert_eq!((id.as_str(), server.as_str()), ("c9", "s1"));
    assert!(voice.is_some());
}

#[tokio::test]
async fn creating_a_text_channel_says_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/servers/s1/channels"))
        .and(body_json(json!({ "type": "Text", "name": "news" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "TextChannel", "_id": "c8", "server": "s1", "name": "news"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let channel = client(&server)
        .create_channel("s1", "news", false)
        .await
        .unwrap();
    assert!(matches!(channel, Channel::TextChannel { voice: None, .. }));
}

#[tokio::test]
async fn too_many_channels_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/servers/s1/channels"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({ "type": "TooManyChannels", "max": 200 })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .create_channel("s1", "Lobby", false)
        .await
        .unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(
        matches!(
            err.error_type(),
            Some(ErrorType::TooManyChannels { max: 200 })
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn editing_categories_sends_only_the_whole_list() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/s1"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({
            "categories": [{ "id": "k1", "title": "Chat", "channels": ["c1"] }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "s1", "owner": "u-me", "name": "Lounge", "channels": ["c1"],
            "categories": [{ "id": "k1", "title": "Chat", "channels": ["c1"] }],
            "default_permissions": 0, "approximate_member_count": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    let categories = [Category {
        id: "k1".into(),
        title: "Chat".into(),
        channels: vec!["c1".into()],
    }];
    let kept = client(&server)
        .edit_categories("s1", &categories)
        .await
        .unwrap();
    assert_eq!(kept, categories);
}

#[tokio::test]
async fn an_answer_without_categories_means_none() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/s1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "s1", "owner": "u-me", "name": "Lounge", "channels": ["c1"],
            "default_permissions": 0, "approximate_member_count": 1
        })))
        .mount(&server)
        .await;

    let kept = client(&server).edit_categories("s1", &[]).await.unwrap();
    assert!(kept.is_empty());
}

#[tokio::test]
async fn deleting_the_last_category_sends_an_empty_list() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/s1"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "categories": [] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "s1", "owner": "u-me", "name": "Lounge", "channels": ["c1"],
            "default_permissions": 0, "approximate_member_count": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    let kept = client(&server).edit_categories("s1", &[]).await.unwrap();
    assert!(kept.is_empty());
}
