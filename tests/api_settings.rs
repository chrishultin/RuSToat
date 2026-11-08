//! Writing synced user settings against a mock Stoat API.

use serde_json::{Value, json};
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::notify::{Level, NotificationSettings};
use stoat_gpui::rail::{self, Ordering, ServerFolders};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn notification_settings_are_written_whole_as_a_string() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sync/settings/set"))
        .and(query_param("timestamp", "1730000000123"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let mut settings = NotificationSettings::parse(r#"{"future_key": 1}"#);
    settings.set_server_level("srv", Level::All);
    settings.mute_channel("ch", None);
    client(&server)
        .set_settings(
            &[("notifications", settings.to_json(1730000000123))],
            1730000000123,
        )
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let object = body.as_object().unwrap();
    assert_eq!(object.len(), 1, "only the notifications key: {body}");
    // The value is a string of JSON, not an object.
    let value = object["notifications"].as_str().expect("a string");
    let written: Value = serde_json::from_str(value).unwrap();
    assert_eq!(
        written,
        json!({
            "future_key": 1,
            "server": { "srv": "all" },
            "channel_mutes": { "ch": {} }
        })
    );
    assert_eq!(NotificationSettings::parse(value), settings);
}

#[tokio::test]
async fn writing_settings_with_a_dead_session_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sync/settings/set"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({ "type": "NotAuthenticated", "location": "x" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .set_settings(&[("notifications", "{}".into())], 1)
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Unauthorized), "got {err:?}");
}

#[tokio::test]
async fn settings_are_fetched_by_key_with_when_they_were_written() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sync/settings/fetch"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "keys": ["ordering", "server-folders"] })))
        // `server-folders` was never written, so it's left out.
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ordering": [1730000000123i64, r#"{"servers":["a"],"serverSidebar":["b","a"]}"#]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut settings = client(&server)
        .fetch_settings(&[rail::ORDERING_KEY, rail::FOLDERS_KEY])
        .await
        .unwrap();
    let (updated, json) = settings.remove("ordering").unwrap();
    assert_eq!(updated, 1730000000123);
    assert_eq!(Ordering::parse(&json).sidebar(), ["b", "a"]);
    assert!(settings.is_empty());
}

#[tokio::test]
async fn the_order_and_folders_are_written_together_at_one_time() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sync/settings/set"))
        .and(query_param("timestamp", "1730000000123"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let mut ordering = Ordering::parse(r#"{"servers":["a","b"],"future":true}"#);
    ordering.set_server_sidebar(vec!["folder-1".into(), "b".into(), "a".into()]);
    let folders = ServerFolders::parse(
        r#"{"folders":[{"id":"folder-1","name":"Pair","servers":["b","a"],"icon":"x"}]}"#,
    );
    client(&server)
        .set_settings(
            &[
                (rail::ORDERING_KEY, ordering.to_json()),
                (rail::FOLDERS_KEY, folders.to_json()),
            ],
            1730000000123,
        )
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let written =
        |key: &str| -> Value { serde_json::from_str(body[key].as_str().unwrap()).unwrap() };
    assert_eq!(
        written("ordering"),
        json!({ "servers": ["a", "b"], "serverSidebar": ["folder-1", "b", "a"], "future": true })
    );
    assert_eq!(
        written("server-folders"),
        json!({ "folders": [{ "id": "folder-1", "name": "Pair", "servers": ["b", "a"], "icon": "x" }] })
    );
}
