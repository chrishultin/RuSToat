//! A channel's webhooks against a mock Stoat API, from Channel Settings'
//! Webhooks page: `/channels/{id}/webhooks` and `/webhooks/{id}`.

use serde_json::json;
use stoat_gpui::api::StoatClient;
use stoat_gpui::webhooks::{list_error_text, webhook_error_text};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn the_list_comes_with_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/channels/ch-a/webhooks"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": "w1", "name": "CI", "creator_id": "u-me", "channel_id": "ch-a",
            "permissions": 1, "token": "secret"
        }])))
        .expect(1)
        .mount(&server)
        .await;

    let list = client(&server).fetch_webhooks("ch-a").await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].token.as_deref(), Some("secret"));
}

#[tokio::test]
async fn an_instance_without_webhooks_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/channels/ch-a/webhooks"))
        .respond_with(ResponseTemplate::new(404).set_body_string("<html>404</html>"))
        .mount(&server)
        .await;

    let err = client(&server).fetch_webhooks("ch-a").await.unwrap_err();
    assert_eq!(
        list_error_text(&err),
        "This Stoat instance doesn't have webhooks."
    );
}

#[tokio::test]
async fn creating_a_webhook_sends_only_its_name() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-a/webhooks"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "name": "CI" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "w1", "name": "CI", "creator_id": "u-me", "channel_id": "ch-a",
            "permissions": 1, "token": "secret"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let made = client(&server).create_webhook("ch-a", "CI").await.unwrap();
    assert_eq!(
        (made.id.as_str(), made.token.as_deref()),
        ("w1", Some("secret"))
    );
}

#[tokio::test]
async fn deleting_a_webhook_sends_a_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/webhooks/w1"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).delete_webhook("w1").await.unwrap();
}

#[tokio::test]
async fn refusals_read_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/dm/webhooks"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "type": "InvalidOperation" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .create_webhook("dm", "CI")
        .await
        .unwrap_err();
    assert_eq!(webhook_error_text(&err), "Webhooks can't be made here.");
}

#[tokio::test]
async fn editing_a_webhook_sends_only_what_is_set() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/webhooks/w1"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "name": "Builds", "remove": ["Avatar"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "w1", "name": "Builds", "creator_id": "u-me", "channel_id": "ch-a",
            "permissions": 1, "token": "secret"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut edit = stoat_gpui::webhooks::empty_webhook_edit();
    edit.name = Some("Builds".into());
    edit.remove
        .push(stoat_gpui::api::models::FieldsWebhook::Avatar);
    let saved = client(&server).edit_webhook("w1", &edit).await.unwrap();
    assert_eq!(saved.name, "Builds");
}

#[tokio::test]
async fn a_new_avatar_is_sent_by_file_id() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/webhooks/w1"))
        .and(body_json(json!({ "avatar": "av2" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "w1", "name": "CI", "creator_id": "u-me", "channel_id": "ch-a",
            "permissions": 1,
            "avatar": {
                "_id": "av2", "tag": "avatars", "filename": "a.png",
                "metadata": { "type": "Image", "width": 64, "height": 64 },
                "content_type": "image/png", "size": 100
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut edit = stoat_gpui::webhooks::empty_webhook_edit();
    edit.avatar = Some("av2".into());
    let saved = client(&server).edit_webhook("w1", &edit).await.unwrap();
    assert_eq!(saved.avatar.map(|file| file.id).as_deref(), Some("av2"));
}
