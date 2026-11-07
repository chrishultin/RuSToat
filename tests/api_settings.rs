//! Writing synced user settings against a mock Stoat API.

use serde_json::{Value, json};
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::notify::{Level, NotificationSettings};
use wiremock::matchers::{header, method, path, query_param};
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
