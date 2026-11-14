//! A server's custom emoji against a mock Stoat API, from Server Settings'
//! Emojis page: `/custom/emoji/{id}`.

use serde_json::json;
use stoat_gpui::api::StoatClient;
use stoat_gpui::custom_emoji::emoji_error_text;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn deleting_an_emoji_sends_a_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/custom/emoji/01ARYZ6S41TSV4RRFFQ69G5FAV"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .delete_emoji("01ARYZ6S41TSV4RRFFQ69G5FAV")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_refused_delete_reads_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/custom/emoji/e"))
        .respond_with(ResponseTemplate::new(403).set_body_json(
            json!({ "type": "MissingPermission", "permission": "ManageCustomisation" }),
        ))
        .mount(&server)
        .await;

    let err = client(&server).delete_emoji("e").await.unwrap_err();
    assert_eq!(
        emoji_error_text(&err),
        "You don't have permission to manage this server's emoji."
    );
}

#[tokio::test]
async fn creating_an_emoji_names_its_file_for_the_server() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/custom/emoji/01ARYZ6S41TSV4RRFFQ69G5FAV"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({
            "name": "stoat",
            "parent": { "type": "Server", "id": "srv" },
            "nsfw": false
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "01ARYZ6S41TSV4RRFFQ69G5FAV",
            "parent": { "type": "Server", "id": "srv" },
            "creator_id": "u-me", "name": "stoat", "animated": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let emoji = client(&server)
        .create_emoji("01ARYZ6S41TSV4RRFFQ69G5FAV", "srv", "stoat")
        .await
        .unwrap();
    assert_eq!((emoji.name.as_str(), emoji.animated), ("stoat", true));
}

#[tokio::test]
async fn renaming_an_emoji_sends_only_its_name() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/custom/emoji/e"))
        .and(body_json(json!({ "name": "otter" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "e", "parent": { "type": "Server", "id": "srv" },
            "creator_id": "u-me", "name": "otter"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let emoji = client(&server).rename_emoji("e", "otter").await.unwrap();
    assert_eq!(emoji.name, "otter");
}

#[tokio::test]
async fn too_many_or_badly_named_emoji_read_plainly() {
    for (body, text) in [
        (
            json!({ "type": "TooManyEmoji", "max": 100 }),
            "This server has 100 emoji, the most Stoat allows.",
        ),
        (
            json!({ "type": "FailedValidation", "error": "name: regex" }),
            "Emoji names are 1–32 lowercase letters, numbers and underscores.",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/custom/emoji/e"))
            .respond_with(ResponseTemplate::new(400).set_body_json(body))
            .mount(&server)
            .await;
        let err = client(&server)
            .create_emoji("e", "srv", "x")
            .await
            .unwrap_err();
        assert_eq!(emoji_error_text(&err), text);
    }
}
