//! Startup session restore and logout, against a mock Stoat API and an
//! in-memory token store (never the real Keychain).

use std::sync::Arc;

use serde_json::{Value, json};
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::session::{self, MemoryStore, MigratingStore, Restored, TokenStore};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn users_me() -> Value {
    let path = format!(
        "{}/tests/fixtures/users_me.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A store that already holds `token` for `client`'s instance.
fn store_with(client: &StoatClient, token: &str) -> Arc<MemoryStore> {
    let store = Arc::new(MemoryStore::default());
    store.save(client.base_url(), token).unwrap();
    store
}

#[tokio::test]
async fn no_saved_token_needs_login_without_calling_the_api() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(users_me()))
        .expect(0)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let restored = session::restore(Arc::new(MemoryStore::default()), client).await;
    assert!(matches!(restored, Restored::NeedsLogin), "got {restored:?}");
}

#[tokio::test]
async fn valid_saved_token_signs_in_and_is_kept() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .and(header("x-session-token", "saved-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(users_me()))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let store = store_with(&client, "saved-token");
    let restored = session::restore(store.clone(), client.clone()).await;

    let Restored::SignedIn { token, user } = restored else {
        panic!("expected SignedIn, got {restored:?}");
    };
    assert_eq!(token, "saved-token");
    assert_eq!(user.username, "stoatfan");
    assert_eq!(
        store.load(client.base_url()).unwrap().as_deref(),
        Some("saved-token")
    );
}

#[tokio::test]
async fn rejected_saved_token_is_cleared() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "type": "InvalidSession" })))
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let store = store_with(&client, "revoked-token");
    let restored = session::restore(store.clone(), client.clone()).await;

    assert!(matches!(restored, Restored::NeedsLogin), "got {restored:?}");
    assert_eq!(store.load(client.base_url()).unwrap(), None);
}

#[tokio::test]
async fn unreachable_server_keeps_saved_token() {
    // Port 1 is reserved and closed, so the connection is refused immediately.
    let client = StoatClient::new("http://127.0.0.1:1");
    let store = store_with(&client, "saved-token");
    let restored = session::restore(store.clone(), client.clone()).await;

    let Restored::Unverified { token, error } = restored else {
        panic!("expected Unverified, got {restored:?}");
    };
    assert_eq!(token, "saved-token");
    assert!(matches!(error, ApiError::Network(_)), "got {error:?}");
    assert_eq!(
        store.load(client.base_url()).unwrap().as_deref(),
        Some("saved-token")
    );
}

#[tokio::test]
async fn server_error_keeps_saved_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let store = store_with(&client, "saved-token");
    let restored = session::restore(store.clone(), client.clone()).await;

    assert!(
        matches!(restored, Restored::Unverified { .. }),
        "got {restored:?}"
    );
    assert!(store.load(client.base_url()).unwrap().is_some());
}

#[tokio::test]
async fn token_saved_for_another_instance_is_not_used() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(users_me()))
        .expect(0)
        .mount(&server)
        .await;

    let store = Arc::new(MemoryStore::default());
    store
        .save("https://some-other-stoat.example", "other-token")
        .unwrap();
    let restored = session::restore(store, StoatClient::new(server.uri())).await;
    assert!(matches!(restored, Restored::NeedsLogin), "got {restored:?}");
}

#[tokio::test]
async fn remembered_token_is_restored() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .and(header("x-session-token", "fresh-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(users_me()))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let store = Arc::new(MemoryStore::default());
    session::remember(
        store.clone(),
        client.base_url().to_string(),
        "fresh-token".into(),
    )
    .await
    .unwrap();

    let restored = session::restore(store, client).await;
    assert!(
        matches!(restored, Restored::SignedIn { .. }),
        "got {restored:?}"
    );
}

#[tokio::test]
async fn log_out_ends_server_session_and_clears_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/logout"))
        .and(header("x-session-token", "saved-token"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let store = store_with(&client, "saved-token");
    let (server_result, local_result) =
        session::log_out(store.clone(), client.with_token("saved-token")).await;

    server_result.unwrap();
    local_result.unwrap();
    assert_eq!(store.load(client.base_url()).unwrap(), None);
}

#[tokio::test]
async fn log_out_clears_token_even_when_offline() {
    let client = StoatClient::new("http://127.0.0.1:1");
    let store = store_with(&client, "saved-token");
    let (server_result, local_result) =
        session::log_out(store.clone(), client.with_token("saved-token")).await;

    assert!(matches!(server_result, Err(ApiError::Network(_))));
    local_result.unwrap();
    assert_eq!(store.load(client.base_url()).unwrap(), None);
}

#[tokio::test]
async fn log_out_with_already_revoked_token_counts_as_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/logout"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "type": "InvalidSession" })))
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let store = store_with(&client, "revoked-token");
    let (server_result, local_result) =
        session::log_out(store.clone(), client.with_token("revoked-token")).await;

    server_result.unwrap();
    local_result.unwrap();
    assert_eq!(store.load(client.base_url()).unwrap(), None);
}

#[test]
fn a_legacy_token_moves_to_the_new_store() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::default();
    legacy.save("https://a", "old-token").unwrap();

    let token = session::load_migrating(&current, &legacy, "https://a").unwrap();
    assert_eq!(token.as_deref(), Some("old-token"));
    assert_eq!(
        current.load("https://a").unwrap().as_deref(),
        Some("old-token")
    );
    assert_eq!(legacy.load("https://a").unwrap(), None);
}

#[test]
fn an_existing_new_token_wins_and_the_legacy_one_is_left_alone() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::default();
    current.save("https://a", "new-token").unwrap();
    legacy.save("https://a", "old-token").unwrap();

    let token = session::load_migrating(&current, &legacy, "https://a").unwrap();
    assert_eq!(token.as_deref(), Some("new-token"));
    assert_eq!(
        legacy.load("https://a").unwrap().as_deref(),
        Some("old-token")
    );
}

#[test]
fn no_token_in_either_store_gives_nothing() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::default();
    legacy.save("https://other", "other-token").unwrap();

    assert_eq!(
        session::load_migrating(&current, &legacy, "https://a").unwrap(),
        None
    );
    assert_eq!(current.load("https://a").unwrap(), None);
    assert_eq!(
        legacy.load("https://other").unwrap().as_deref(),
        Some("other-token")
    );
}

#[tokio::test]
async fn restore_signs_in_with_a_migrated_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .and(header("x-session-token", "old-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(users_me()))
        .expect(1)
        .mount(&server)
        .await;

    let client = StoatClient::new(server.uri());
    let legacy = MemoryStore::default();
    legacy.save(client.base_url(), "old-token").unwrap();
    let store = Arc::new(MigratingStore::new(MemoryStore::default(), legacy));

    let restored = session::restore(store.clone(), client.clone()).await;
    assert!(
        matches!(restored, Restored::SignedIn { .. }),
        "got {restored:?}"
    );
    // Now saved under the new store: loading again doesn't need the legacy one.
    assert_eq!(
        store.load(client.base_url()).unwrap().as_deref(),
        Some("old-token")
    );
}
