//! Remembering the login between launches.
//!
//! The session token lives in the macOS Keychain, keyed by API URL, so a
//! token for one Stoat instance is never sent to another. Startup and logout
//! logic sits behind the [`TokenStore`] trait so it can be tested without
//! touching the real Keychain.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Context as _;

use crate::api::models::User;
use crate::api::{ApiError, StoatClient};

/// Keychain service name for this app's entries.
pub const KEYCHAIN_SERVICE: &str = "chat.stoat.gpui-poc";

/// Somewhere to keep one session token per Stoat instance (API base URL).
///
/// Calls may block (Keychain), so async code runs them on a blocking thread.
pub trait TokenStore: Send + Sync + 'static {
    fn load(&self, instance: &str) -> anyhow::Result<Option<String>>;
    fn save(&self, instance: &str, token: &str) -> anyhow::Result<()>;
    /// Remove the token. Not an error if there wasn't one.
    fn clear(&self, instance: &str) -> anyhow::Result<()>;
}

/// The real store: one generic-password item per instance in the login Keychain.
pub struct KeychainStore {
    service: String,
}

impl KeychainStore {
    pub fn new() -> Self {
        Self::with_service(KEYCHAIN_SERVICE)
    }

    /// A store under a different service name (used by the opt-in Keychain test).
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, instance: &str) -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(&self.service, instance).context("opening Keychain entry")
    }
}

impl Default for KeychainStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenStore for KeychainStore {
    fn load(&self, instance: &str) -> anyhow::Result<Option<String>> {
        match self.entry(instance)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err).context("reading session token from Keychain"),
        }
    }

    fn save(&self, instance: &str, token: &str) -> anyhow::Result<()> {
        self.entry(instance)?
            .set_password(token)
            .context("saving session token to Keychain")
    }

    fn clear(&self, instance: &str) -> anyhow::Result<()> {
        match self.entry(instance)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err).context("deleting session token from Keychain"),
        }
    }
}

/// An in-memory store for tests, and a fallback if the Keychain is unavailable.
#[derive(Default)]
pub struct MemoryStore {
    tokens: Mutex<HashMap<String, String>>,
}

impl TokenStore for MemoryStore {
    fn load(&self, instance: &str) -> anyhow::Result<Option<String>> {
        Ok(self.tokens.lock().unwrap().get(instance).cloned())
    }

    fn save(&self, instance: &str, token: &str) -> anyhow::Result<()> {
        self.tokens
            .lock()
            .unwrap()
            .insert(instance.to_string(), token.to_string());
        Ok(())
    }

    fn clear(&self, instance: &str) -> anyhow::Result<()> {
        self.tokens.lock().unwrap().remove(instance);
        Ok(())
    }
}

/// What startup found.
#[derive(Debug)]
pub enum Restored {
    /// A saved token that the server accepted.
    SignedIn { token: String, user: Box<User> },
    /// No saved token, or the server rejected it (in which case it was cleared).
    NeedsLogin,
    /// A saved token we couldn't check (offline, server error, rate limit).
    /// The token is kept, so a retry can still succeed.
    Unverified { token: String, error: ApiError },
}

/// Look for a saved token for `client`'s instance and check it with `GET /users/@me`.
///
/// Must run on the Tokio IO runtime (use [`crate::runtime::spawn_io`]).
pub async fn restore(store: Arc<dyn TokenStore>, client: StoatClient) -> Restored {
    let instance = client.base_url().to_string();
    let token = match blocking({
        let store = store.clone();
        let instance = instance.clone();
        move || store.load(&instance)
    })
    .await
    {
        Ok(Some(token)) => token,
        Ok(None) => return Restored::NeedsLogin,
        Err(err) => {
            log::warn!("couldn't read saved session: {err:#}");
            return Restored::NeedsLogin;
        }
    };

    match client.with_token(token.clone()).fetch_self().await {
        Ok(user) => {
            log::info!("restored saved session for {}", user.username);
            Restored::SignedIn {
                token,
                user: Box::new(user),
            }
        }
        Err(ApiError::Unauthorized) => {
            log::info!("saved session was rejected; clearing it");
            if let Err(err) = blocking(move || store.clear(&instance)).await {
                log::warn!("couldn't clear rejected session: {err:#}");
            }
            Restored::NeedsLogin
        }
        Err(error) => {
            log::warn!("couldn't verify saved session: {error:?}");
            Restored::Unverified { token, error }
        }
    }
}

/// Save the token for `instance`. Must run on the Tokio IO runtime.
pub async fn remember(
    store: Arc<dyn TokenStore>,
    instance: String,
    token: String,
) -> anyhow::Result<()> {
    blocking(move || store.save(&instance, &token)).await
}

/// Forget the token locally, then end the session on the server.
///
/// The local token is cleared first and always, even if the server can't be
/// reached: after "Log out" the app must not keep signing in. Clearing first
/// also means a slow server call can't delete a token from a quick re-login.
/// Both results are returned so the caller can report them. `client` must
/// carry the token. Must run on the Tokio IO runtime.
pub async fn log_out(
    store: Arc<dyn TokenStore>,
    client: StoatClient,
) -> (Result<(), ApiError>, anyhow::Result<()>) {
    let instance = client.base_url().to_string();
    let local = blocking(move || store.clear(&instance)).await;
    let server = client.logout().await;
    (server, local)
}

/// Run a blocking store call off the async worker threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .context("token store task failed")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trips_per_instance() {
        let store = MemoryStore::default();
        store.save("https://a", "tok-a").unwrap();
        store.save("https://b", "tok-b").unwrap();
        assert_eq!(store.load("https://a").unwrap().as_deref(), Some("tok-a"));
        store.clear("https://a").unwrap();
        assert_eq!(store.load("https://a").unwrap(), None);
        assert_eq!(store.load("https://b").unwrap().as_deref(), Some("tok-b"));
        // Clearing something that isn't there is fine.
        store.clear("https://a").unwrap();
    }

    /// Writes, reads and deletes a throwaway item in the real login Keychain
    /// (service `chat.stoat.gpui-poc.test`). Run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "touches the real macOS Keychain"]
    fn keychain_store_round_trips() {
        let store = KeychainStore::with_service(format!("{KEYCHAIN_SERVICE}.test"));
        let instance = "https://keychain-test.invalid";
        store.clear(instance).unwrap();
        assert_eq!(store.load(instance).unwrap(), None);
        store.save(instance, "test-token").unwrap();
        assert_eq!(store.load(instance).unwrap().as_deref(), Some("test-token"));
        store.clear(instance).unwrap();
        assert_eq!(store.load(instance).unwrap(), None);
    }
}
