//! Remembering logins between launches.
//!
//! Session tokens live in the macOS Keychain, one item per account. The item's
//! name (the Keychain "account") is the account's ID in [`crate::accounts`]:
//! `{api_url}#{user_id}`, or just the API URL for a session saved before
//! multiple accounts. Each token is only ever sent to its own instance.
//! Startup and logout logic sits behind the [`TokenStore`] trait so it can be
//! tested without touching the real Keychain.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Context as _;

use crate::api::models::User;
use crate::api::{ApiError, StoatClient};

/// Keychain service name for this app's entries (the bundle identifier).
pub const KEYCHAIN_SERVICE: &str = "com.chrishultin.rustoat";

/// The service name used before RuSToat had its own bundle identifier.
/// Items found there are moved to [`KEYCHAIN_SERVICE`] on first load.
pub const LEGACY_KEYCHAIN_SERVICE: &str = "chat.stoat.gpui-poc";

/// Somewhere to keep one session token per account, by the account's ID.
///
/// Calls may block (Keychain), so async code runs them on a blocking thread.
pub trait TokenStore: Send + Sync + 'static {
    fn load(&self, key: &str) -> anyhow::Result<Option<String>>;
    fn save(&self, key: &str, token: &str) -> anyhow::Result<()>;
    /// Remove the token. Not an error if there wasn't one.
    fn clear(&self, key: &str) -> anyhow::Result<()>;
}

/// The real store: one generic-password item per account in the login Keychain.
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

    fn entry(&self, key: &str) -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key).context("opening Keychain entry")
    }
}

impl Default for KeychainStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenStore for KeychainStore {
    fn load(&self, key: &str) -> anyhow::Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err).context("reading session token from Keychain"),
        }
    }

    fn save(&self, key: &str, token: &str) -> anyhow::Result<()> {
        self.entry(key)?
            .set_password(token)
            .context("saving session token to Keychain")
    }

    fn clear(&self, key: &str) -> anyhow::Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err).context("deleting session token from Keychain"),
        }
    }
}

/// A store that moves tokens over from an older store the first time
/// they're loaded (see [`load_migrating`]). Saves and clears only touch the
/// current store.
pub struct MigratingStore<C, L> {
    current: C,
    legacy: L,
}

impl<C: TokenStore, L: TokenStore> MigratingStore<C, L> {
    pub fn new(current: C, legacy: L) -> Self {
        Self { current, legacy }
    }
}

impl MigratingStore<KeychainStore, KeychainStore> {
    /// The real store: [`KEYCHAIN_SERVICE`], taking over items from
    /// [`LEGACY_KEYCHAIN_SERVICE`].
    pub fn keychain() -> Self {
        Self::new(
            KeychainStore::new(),
            KeychainStore::with_service(LEGACY_KEYCHAIN_SERVICE),
        )
    }
}

impl<C: TokenStore, L: TokenStore> TokenStore for MigratingStore<C, L> {
    fn load(&self, key: &str) -> anyhow::Result<Option<String>> {
        load_migrating(&self.current, &self.legacy, key)
    }

    fn save(&self, key: &str, token: &str) -> anyhow::Result<()> {
        self.current.save(key, token)
    }

    fn clear(&self, key: &str) -> anyhow::Result<()> {
        self.current.clear(key)
    }
}

/// Load the token saved under `key` from `current`, or move it there from `legacy`.
///
/// A token already in `current` wins, and `legacy` isn't touched. Otherwise
/// a legacy token is saved to `current` and then deleted from `legacy`. If
/// the save fails the legacy item is kept, so the next launch tries again;
/// either way the token is returned so this launch can sign in.
pub fn load_migrating(
    current: &dyn TokenStore,
    legacy: &dyn TokenStore,
    key: &str,
) -> anyhow::Result<Option<String>> {
    if let Some(token) = current.load(key)? {
        return Ok(Some(token));
    }
    let token = match legacy.load(key) {
        Ok(Some(token)) => token,
        Ok(None) => return Ok(None),
        Err(err) => {
            log::warn!("couldn't read the legacy saved session: {err:#}");
            return Ok(None);
        }
    };
    match current.save(key, &token) {
        Ok(()) => {
            log::info!("moved the saved session to the new Keychain service");
            if let Err(err) = legacy.clear(key) {
                log::warn!("couldn't delete the legacy saved session: {err:#}");
            }
        }
        Err(err) => log::warn!("couldn't move the legacy saved session: {err:#}"),
    }
    Ok(Some(token))
}

/// An in-memory store for tests, and a fallback if the Keychain is unavailable.
#[derive(Default)]
pub struct MemoryStore {
    tokens: Mutex<HashMap<String, String>>,
}

impl TokenStore for MemoryStore {
    fn load(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self.tokens.lock().unwrap().get(key).cloned())
    }

    fn save(&self, key: &str, token: &str) -> anyhow::Result<()> {
        self.tokens
            .lock()
            .unwrap()
            .insert(key.to_string(), token.to_string());
        Ok(())
    }

    fn clear(&self, key: &str) -> anyhow::Result<()> {
        self.tokens.lock().unwrap().remove(key);
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

/// Look for the token saved under `key` and check it with `GET /users/@me` on
/// `client`'s instance.
///
/// Must run on the Tokio IO runtime (use [`crate::runtime::spawn_io`]).
pub async fn restore(store: Arc<dyn TokenStore>, client: StoatClient, key: String) -> Restored {
    let token = match blocking({
        let store = store.clone();
        let key = key.clone();
        move || store.load(&key)
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
            if let Err(err) = blocking(move || store.clear(&key)).await {
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

/// Save the token under `key`. Must run on the Tokio IO runtime.
pub async fn remember(
    store: Arc<dyn TokenStore>,
    key: String,
    token: String,
) -> anyhow::Result<()> {
    blocking(move || store.save(&key, &token)).await
}

/// Forget the token locally, then end the session on the server.
///
/// The local token is cleared first and always, even if the server can't be
/// reached: after "Log out" the app must not keep signing in. Clearing first
/// also means a slow server call can't delete a token from a quick re-login.
/// Both results are returned so the caller can report them. `client` must
/// carry the token, and `key` is where it's saved. Must run on the Tokio IO
/// runtime.
pub async fn log_out(
    store: Arc<dyn TokenStore>,
    client: StoatClient,
    key: String,
) -> (Result<(), ApiError>, anyhow::Result<()>) {
    let local = blocking(move || store.clear(&key)).await;
    let server = client.logout().await;
    (server, local)
}

/// Forget the token saved under `key`, without contacting the server (for
/// a session that's already over). Must run on the Tokio IO runtime.
pub async fn forget(store: Arc<dyn TokenStore>, key: String) -> anyhow::Result<()> {
    blocking(move || store.clear(&key)).await
}

/// Run a blocking store call off the async worker threads.
pub(crate) async fn blocking<T: Send + 'static>(
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
    fn memory_store_round_trips_per_key() {
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
    /// (service `com.chrishultin.rustoat.test`). Run with `cargo test -- --ignored`.
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
