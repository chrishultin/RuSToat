//! Startup session restore, logout and adopting an old saved session,
//! against a mock Stoat API and an in-memory token store (never the real
//! Keychain).

use std::sync::Arc;

use serde_json::{Value, json};
use stoat_gpui::accounts;
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

/// The Keychain name tests save the token under (any string works).
const KEY: &str = "https://stoat.test#01USER";

/// A store that already holds `token` under [`KEY`].
fn store_with(token: &str) -> Arc<MemoryStore> {
    let store = Arc::new(MemoryStore::default());
    store.save(KEY, token).unwrap();
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
    let restored = session::restore(Arc::new(MemoryStore::default()), client, KEY.into()).await;
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
    let store = store_with("saved-token");
    let restored = session::restore(store.clone(), client.clone(), KEY.into()).await;

    let Restored::SignedIn { token, user } = restored else {
        panic!("expected SignedIn, got {restored:?}");
    };
    assert_eq!(token, "saved-token");
    assert_eq!(user.username, "stoatfan");
    assert_eq!(store.load(KEY).unwrap().as_deref(), Some("saved-token"));
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
    let store = store_with("revoked-token");
    let restored = session::restore(store.clone(), client.clone(), KEY.into()).await;

    assert!(matches!(restored, Restored::NeedsLogin), "got {restored:?}");
    assert_eq!(store.load(KEY).unwrap(), None);
}

#[tokio::test]
async fn unreachable_server_keeps_saved_token() {
    // Port 1 is reserved and closed, so the connection is refused immediately.
    let client = StoatClient::new("http://127.0.0.1:1");
    let store = store_with("saved-token");
    let restored = session::restore(store.clone(), client.clone(), KEY.into()).await;

    let Restored::Unverified { token, error } = restored else {
        panic!("expected Unverified, got {restored:?}");
    };
    assert_eq!(token, "saved-token");
    assert!(matches!(error, ApiError::Network(_)), "got {error:?}");
    assert_eq!(store.load(KEY).unwrap().as_deref(), Some("saved-token"));
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
    let store = store_with("saved-token");
    let restored = session::restore(store.clone(), client.clone(), KEY.into()).await;

    assert!(
        matches!(restored, Restored::Unverified { .. }),
        "got {restored:?}"
    );
    assert!(store.load(KEY).unwrap().is_some());
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
        .save("https://some-other-stoat.example#01USER", "other-token")
        .unwrap();
    let restored = session::restore(store, StoatClient::new(server.uri()), KEY.into()).await;
    assert!(matches!(restored, Restored::NeedsLogin), "got {restored:?}");
}

#[tokio::test]
async fn two_accounts_on_one_instance_keep_their_own_tokens() {
    let server = MockServer::start().await;
    for token in ["token-a", "token-b"] {
        Mock::given(method("GET"))
            .and(path("/users/@me"))
            .and(header("x-session-token", token))
            .respond_with(ResponseTemplate::new(200).set_body_json(users_me()))
            .expect(1)
            .mount(&server)
            .await;
    }

    let client = StoatClient::new(server.uri());
    let a = accounts::keychain_name(client.base_url(), "01A");
    let b = accounts::keychain_name(client.base_url(), "01B");
    let store = Arc::new(MemoryStore::default());
    session::remember(store.clone(), a.clone(), "token-a".into())
        .await
        .unwrap();
    session::remember(store.clone(), b.clone(), "token-b".into())
        .await
        .unwrap();

    for (key, expected) in [(a, "token-a"), (b, "token-b")] {
        let restored = session::restore(store.clone(), client.clone(), key).await;
        let Restored::SignedIn { token, .. } = restored else {
            panic!("expected SignedIn, got {restored:?}");
        };
        assert_eq!(token, expected);
    }
}

#[tokio::test]
async fn legacy_session_is_adopted_and_restores() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .and(header("x-session-token", "old-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(users_me()))
        .expect(1)
        .mount(&server)
        .await;

    // Saved by an older version: keyed by the bare API URL.
    let client = StoatClient::new(server.uri());
    let store = Arc::new(MemoryStore::default());
    store.save(client.base_url(), "old-token").unwrap();

    let list = accounts::migrate_legacy(store.clone(), client.base_url().into()).await;
    let entry = list.active_entry().expect("adopted entry").clone();
    assert_eq!(entry.id, client.base_url());
    assert_eq!(entry.user_id, None);

    let restored = session::restore(store.clone(), client, entry.id.clone()).await;
    assert!(
        matches!(restored, Restored::SignedIn { .. }),
        "got {restored:?}"
    );
    // The token stays where it was.
    assert_eq!(store.load(&entry.id).unwrap().as_deref(), Some("old-token"));
}

#[tokio::test]
async fn nothing_to_adopt_gives_an_empty_list() {
    let store = Arc::new(MemoryStore::default());
    store.save("https://elsewhere.example", "other").unwrap();
    let list = accounts::migrate_legacy(store, "https://stoat.test".into()).await;
    assert!(list.accounts.is_empty());
    assert_eq!(list.active, None);
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
    session::remember(store.clone(), KEY.into(), "fresh-token".into())
        .await
        .unwrap();

    let restored = session::restore(store, client, KEY.into()).await;
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
    let store = store_with("saved-token");
    let (server_result, local_result) =
        session::log_out(store.clone(), client.with_token("saved-token"), KEY.into()).await;

    server_result.unwrap();
    local_result.unwrap();
    assert_eq!(store.load(KEY).unwrap(), None);
}

#[tokio::test]
async fn log_out_clears_token_even_when_offline() {
    let client = StoatClient::new("http://127.0.0.1:1");
    let store = store_with("saved-token");
    let (server_result, local_result) =
        session::log_out(store.clone(), client.with_token("saved-token"), KEY.into()).await;

    assert!(matches!(server_result, Err(ApiError::Network(_))));
    local_result.unwrap();
    assert_eq!(store.load(KEY).unwrap(), None);
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
    let store = store_with("revoked-token");
    let (server_result, local_result) = session::log_out(
        store.clone(),
        client.with_token("revoked-token"),
        KEY.into(),
    )
    .await;

    server_result.unwrap();
    local_result.unwrap();
    assert_eq!(store.load(KEY).unwrap(), None);
}

/// Another account's key, for the Keychain migration tests.
const OTHER_KEY: &str = "https://stoat.test#02OTHER";

#[test]
fn a_legacy_token_moves_to_the_new_store() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::default();
    legacy.save(KEY, "old-token").unwrap();

    let token = session::load_migrating(&current, &legacy, KEY).unwrap();
    assert_eq!(token.as_deref(), Some("old-token"));
    assert_eq!(current.load(KEY).unwrap().as_deref(), Some("old-token"));
    assert_eq!(legacy.load(KEY).unwrap(), None);
}

#[test]
fn an_existing_new_token_wins_and_the_legacy_one_is_left_alone() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::default();
    current.save(KEY, "new-token").unwrap();
    legacy.save(KEY, "old-token").unwrap();

    let token = session::load_migrating(&current, &legacy, KEY).unwrap();
    assert_eq!(token.as_deref(), Some("new-token"));
    assert_eq!(legacy.load(KEY).unwrap().as_deref(), Some("old-token"));
}

#[test]
fn no_token_in_either_store_gives_nothing() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::default();
    legacy.save(OTHER_KEY, "other-token").unwrap();

    assert_eq!(
        session::load_migrating(&current, &legacy, KEY).unwrap(),
        None
    );
    assert_eq!(current.load(KEY).unwrap(), None);
    assert_eq!(
        legacy.load(OTHER_KEY).unwrap().as_deref(),
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
    legacy.save(KEY, "old-token").unwrap();
    let store = Arc::new(MigratingStore::new(MemoryStore::default(), legacy));

    let restored = session::restore(store.clone(), client.clone(), KEY.into()).await;
    assert!(
        matches!(restored, Restored::SignedIn { .. }),
        "got {restored:?}"
    );
    // Now saved under the new store: loading again doesn't need the legacy one.
    assert_eq!(store.load(KEY).unwrap().as_deref(), Some("old-token"));
}
