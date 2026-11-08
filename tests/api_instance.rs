//! Finding an instance's API from a typed address, against a mock server.

use serde_json::{Value, json};
use stoat_gpui::api::{InstanceError, RootConfig, resolve_instance};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root() -> Value {
    let path = format!("{}/tests/fixtures/root.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn html() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/html")
        .set_body_string("<!doctype html><title>Stoat</title>")
}

#[tokio::test]
async fn api_at_the_typed_address() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root()))
        .mount(&server)
        .await;

    let (client, config) = resolve_instance(&format!("{}/", server.uri()))
        .await
        .unwrap();
    assert_eq!(client.base_url(), server.uri());
    assert!(config.version.starts_with("0.16."));
}

#[tokio::test]
async fn api_under_slash_api() {
    // A self-hosted instance: the web app at `/`, the API at `/api`.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(html())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root()))
        .mount(&server)
        .await;

    let (client, _) = resolve_instance(&server.uri()).await.unwrap();
    assert_eq!(client.base_url(), format!("{}/api", server.uri()));
}

#[tokio::test]
async fn typed_api_address_is_not_doubled() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root()))
        .expect(0)
        .mount(&server)
        .await;

    let (client, _) = resolve_instance(&format!("{}/api", server.uri()))
        .await
        .unwrap();
    assert_eq!(client.base_url(), format!("{}/api", server.uri()));
}

#[tokio::test]
async fn neither_address_is_stoat() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(html())
        .mount(&server)
        .await;
    // `/api/` isn't mocked, so wiremock answers 404.

    let err = resolve_instance(&server.uri()).await.unwrap_err();
    let InstanceError::NotStoat { url } = &err else {
        panic!("expected NotStoat, got {err:?}");
    };
    assert_eq!(*url, server.uri());
    assert!(err.to_string().contains(&server.uri()));
}

#[tokio::test]
async fn unreachable_address_doesnt_try_slash_api() {
    // Port 1 is reserved and closed, so the connection is refused immediately.
    let err = resolve_instance("http://127.0.0.1:1").await.unwrap_err();
    assert!(
        matches!(err, InstanceError::Unreachable { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn other_api_versions_still_resolve() {
    let server = MockServer::start().await;
    let mut config = root();
    config["revolt"] = "0.17.0".into();
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(config))
        .mount(&server)
        .await;

    let (_, config) = resolve_instance(&server.uri()).await.unwrap();
    assert_eq!(config.version, "0.17.0");
}

#[tokio::test]
async fn junk_is_rejected_without_a_request() {
    let err = resolve_instance("   ").await.unwrap_err();
    assert!(matches!(err, InstanceError::Invalid), "got {err:?}");
}

#[test]
fn the_first_voice_node_is_offered() {
    let config: RootConfig = serde_json::from_value(root()).unwrap();
    assert_eq!(config.voice_node(), Some("hel1"));
}

#[test]
fn a_voice_node_is_found_by_name() {
    let config: RootConfig = serde_json::from_value(root()).unwrap();
    assert_eq!(
        config.voice_node_url("hel1"),
        Some("wss://livekit.hel1.voice.stoat.chat")
    );
    assert_eq!(config.voice_node_url("nowhere"), None);
    // Voice off: no node is known.
    let mut disabled = root();
    disabled["features"]["livekit"]["enabled"] = false.into();
    let config: RootConfig = serde_json::from_value(disabled).unwrap();
    assert_eq!(config.voice_node_url("hel1"), None);
}

#[test]
fn no_voice_node_when_voice_is_off_or_missing() {
    let mut disabled = root();
    disabled["features"]["livekit"]["enabled"] = false.into();
    let mut no_nodes = root();
    no_nodes["features"]["livekit"]["nodes"] = json!([]);
    let mut missing = root();
    missing["features"]
        .as_object_mut()
        .unwrap()
        .remove("livekit");

    for value in [disabled, no_nodes, missing] {
        let config: RootConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.voice_node(), None);
    }
}
