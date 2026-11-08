//! The accounts the user is signed in to, possibly on several Stoat instances.
//!
//! The list isn't secret, so it lives in a small JSON file
//! (`~/Library/Application Support/com.chrishultin.rustoat/accounts.json`). Each
//! account's session token is in the Keychain under the account's ID (see
//! [`crate::session`]).
//!
//! A session saved before multiple accounts existed was keyed by the bare API
//! URL. [`migrate_legacy`] adopts it as-is, with that URL as its ID, instead of
//! moving the token, which would need the user's ID and so fail offline.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::session::{self, TokenStore};

/// The Keychain name (and account ID) for `user_id` on `instance`.
pub fn keychain_name(instance: &str, user_id: &str) -> String {
    format!("{instance}#{user_id}")
}

/// One saved account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountEntry {
    /// Stable ID, and the Keychain name its token is saved under.
    pub id: String,
    /// API base URL, e.g. `https://api.stoat.chat`.
    pub instance: String,
    /// `None` only for a session saved before multiple accounts, until it's
    /// first restored.
    #[serde(default)]
    pub user_id: Option<String>,
    /// Display name, as last seen.
    #[serde(default)]
    pub name: Option<String>,
    /// The ID of this client's session (from its login), to tell it apart
    /// in the session list. `None` for sessions saved before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// The saved accounts, in the switcher's order (as added, unless dragged
/// since), and which one was shown last.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountList {
    #[serde(default)]
    pub accounts: Vec<AccountEntry>,
    #[serde(default)]
    pub active: Option<String>,
}

impl AccountList {
    pub fn get(&self, id: &str) -> Option<&AccountEntry> {
        self.accounts.iter().find(|a| a.id == id)
    }

    /// The account for `user_id` on `instance`, if saved.
    pub fn find(&self, instance: &str, user_id: &str) -> Option<&AccountEntry> {
        self.accounts
            .iter()
            .find(|a| a.instance == instance && a.user_id.as_deref() == Some(user_id))
    }

    /// The account to show: the active one, or else the first.
    pub fn active_entry(&self) -> Option<&AccountEntry> {
        self.active
            .as_deref()
            .and_then(|id| self.get(id))
            .or_else(|| self.accounts.first())
    }

    /// Record a login: add the account, or update the saved one for the same
    /// user on the same instance. Returns the account's ID.
    pub fn add_or_update(&mut self, instance: &str, user_id: &str, name: Option<String>) -> String {
        if let Some(entry) = self
            .accounts
            .iter_mut()
            .find(|a| a.instance == instance && a.user_id.as_deref() == Some(user_id))
        {
            if name.is_some() {
                entry.name = name;
            }
            return entry.id.clone();
        }
        let id = keychain_name(instance, user_id);
        self.accounts.push(AccountEntry {
            id: id.clone(),
            instance: instance.to_string(),
            user_id: Some(user_id.to_string()),
            name,
            session_id: None,
        });
        id
    }

    /// Remember the session an account just logged in with. Returns whether
    /// it changed.
    pub fn set_session(&mut self, id: &str, session_id: &str) -> bool {
        match self.accounts.iter_mut().find(|a| a.id == id) {
            Some(entry) if entry.session_id.as_deref() != Some(session_id) => {
                entry.session_id = Some(session_id.to_string());
                true
            }
            _ => false,
        }
    }

    /// Fill in who an account belongs to, once its session is checked.
    /// Returns whether anything changed.
    pub fn set_user(&mut self, id: &str, user_id: &str, name: &str) -> bool {
        let Some(entry) = self.accounts.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        let changed =
            entry.user_id.as_deref() != Some(user_id) || entry.name.as_deref() != Some(name);
        entry.user_id = Some(user_id.to_string());
        entry.name = Some(name.to_string());
        changed
    }

    /// Update an account's display name, as last seen. Returns whether it
    /// changed.
    pub fn set_name(&mut self, id: &str, name: &str) -> bool {
        match self.accounts.iter_mut().find(|a| a.id == id) {
            Some(entry) if entry.name.as_deref() != Some(name) => {
                entry.name = Some(name.to_string());
                true
            }
            _ => false,
        }
    }

    /// Show `id` next time. Ignored if there's no such account.
    pub fn set_active(&mut self, id: &str) {
        if self.get(id).is_some() {
            self.active = Some(id.to_string());
        }
    }

    /// Move an account to just before `before`, or to the end with `None`
    /// (dragging it in the switcher). Returns whether the order changed:
    /// moving it before itself, or to where it already is, doesn't; nor
    /// does an unknown account or `before`.
    pub fn move_account(&mut self, id: &str, before: Option<&str>) -> bool {
        let position = |id: &str| self.accounts.iter().position(|a| a.id == id);
        let Some(from) = position(id) else {
            return false;
        };
        // Where it goes once taken out.
        let to = match before.map(position) {
            None => self.accounts.len() - 1,
            Some(None) => return false,
            Some(Some(at)) if at > from => at - 1,
            Some(Some(at)) => at,
        };
        if to == from {
            return false;
        }
        let entry = self.accounts.remove(from);
        self.accounts.insert(to, entry);
        true
    }

    /// Forget an account. If it was the active one, the account after it (or
    /// else before it) becomes active.
    pub fn remove(&mut self, id: &str) -> Option<AccountEntry> {
        let index = self.accounts.iter().position(|a| a.id == id)?;
        let removed = self.accounts.remove(index);
        if self.active.as_deref() == Some(id) {
            let next = self
                .accounts
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|i| self.accounts.get(i)));
            self.active = next.map(|a| a.id.clone());
        }
        Some(removed)
    }
}

/// Where the account list is kept.
pub trait AccountsFile: Send + Sync + 'static {
    /// The saved list, or `None` if nothing was ever saved (first launch).
    fn load(&self) -> anyhow::Result<Option<AccountList>>;
    fn save(&self, list: &AccountList) -> anyhow::Result<()>;
}

/// The real list: JSON in the app's folder in Application Support.
pub struct FileAccounts {
    path: PathBuf,
}

impl FileAccounts {
    /// `accounts.json` in `~/Library/Application Support/com.chrishultin.rustoat`,
    /// beside `places.json` ([`crate::places::app_dir`], which also moves the
    /// folder over from its old name). `None` if there's no home directory.
    pub fn in_app_dir() -> Option<Self> {
        Some(Self::at(crate::places::app_dir()?.join("accounts.json")))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl AccountsFile for FileAccounts {
    fn load(&self) -> anyhow::Result<Option<AccountList>> {
        let json = match std::fs::read_to_string(&self.path) {
            Ok(json) => json,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| format!("reading {}", self.path.display()));
            }
        };
        serde_json::from_str(&json)
            .map(Some)
            .with_context(|| format!("parsing {}", self.path.display()))
    }

    /// Written to a temporary file and renamed into place, so a crash can't
    /// leave half a list.
    fn save(&self, list: &AccountList) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(list).context("serializing accounts")?;
        std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))
    }
}

/// An in-memory list for tests, and a fallback without a config directory.
#[derive(Default)]
pub struct MemoryAccounts {
    list: Mutex<Option<AccountList>>,
}

impl AccountsFile for MemoryAccounts {
    fn load(&self) -> anyhow::Result<Option<AccountList>> {
        Ok(self.list.lock().unwrap().clone())
    }

    fn save(&self, list: &AccountList) -> anyhow::Result<()> {
        *self.list.lock().unwrap() = Some(list.clone());
        Ok(())
    }
}

/// On the first launch with an account list, adopt a session saved by an
/// older version (keyed by the bare API URL of `instance`) as the first
/// account. Its user is filled in when it's restored. A Keychain error is
/// logged and treated as no saved session.
///
/// Must run on the Tokio IO runtime.
pub async fn migrate_legacy(tokens: Arc<dyn TokenStore>, instance: String) -> AccountList {
    let found = session::blocking({
        let instance = instance.clone();
        move || tokens.load(&instance)
    })
    .await;
    match found {
        Ok(Some(_)) => {
            log::info!("adopting the session saved for {instance} as the first account");
            AccountList {
                accounts: vec![AccountEntry {
                    id: instance.clone(),
                    instance: instance.clone(),
                    user_id: None,
                    name: None,
                    session_id: None,
                }],
                active: Some(instance),
            }
        }
        Ok(None) => AccountList::default(),
        Err(err) => {
            log::warn!("couldn't check for a saved session: {err:#}");
            AccountList::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(ids: &[&str]) -> AccountList {
        AccountList {
            accounts: ids
                .iter()
                .map(|id| AccountEntry {
                    id: id.to_string(),
                    instance: "https://a".into(),
                    user_id: Some(id.to_string()),
                    name: None,
                    session_id: None,
                })
                .collect(),
            active: None,
        }
    }

    #[test]
    fn keychain_name_joins_instance_and_user() {
        assert_eq!(
            keychain_name("https://api.stoat.chat", "01ABC"),
            "https://api.stoat.chat#01ABC"
        );
    }

    #[test]
    fn a_login_remembers_its_session() {
        let mut accounts = AccountList::default();
        let id = accounts.add_or_update("https://a", "u1", None);
        assert!(accounts.set_session(&id, "s1"));
        assert!(!accounts.set_session(&id, "s1"));
        // Signing in again replaces it.
        accounts.add_or_update("https://a", "u1", None);
        assert!(accounts.set_session(&id, "s2"));
        assert_eq!(accounts.get(&id).unwrap().session_id.as_deref(), Some("s2"));
        assert!(!accounts.set_session("nobody", "s3"));
    }

    #[test]
    fn add_or_update_dedupes_on_instance_and_user() {
        let mut accounts = AccountList::default();
        let a = accounts.add_or_update("https://a", "u1", None);
        let b = accounts.add_or_update("https://b", "u1", None);
        let again = accounts.add_or_update("https://a", "u1", Some("Name".into()));
        assert_eq!(a, "https://a#u1");
        assert_eq!(b, "https://b#u1");
        assert_eq!(again, a);
        assert_eq!(accounts.accounts.len(), 2);
        assert_eq!(accounts.get(&a).unwrap().name.as_deref(), Some("Name"));
        // Two users on the same instance are separate accounts.
        accounts.add_or_update("https://a", "u2", None);
        assert_eq!(accounts.accounts.len(), 3);
    }

    #[test]
    fn set_user_fills_in_a_migrated_entry() {
        let mut accounts = AccountList {
            accounts: vec![AccountEntry {
                id: "https://a".into(),
                instance: "https://a".into(),
                user_id: None,
                name: None,
                session_id: None,
            }],
            active: None,
        };
        assert!(accounts.set_user("https://a", "u1", "Ann"));
        assert!(!accounts.set_user("https://a", "u1", "Ann"));
        // Found by user from now on, and its ID (Keychain name) is unchanged.
        assert_eq!(accounts.find("https://a", "u1").unwrap().id, "https://a");
        assert_eq!(accounts.add_or_update("https://a", "u1", None), "https://a");
    }

    #[test]
    fn set_name_reports_changes() {
        let mut accounts = list(&["a"]);
        assert!(accounts.set_name("a", "Ann"));
        assert!(!accounts.set_name("a", "Ann"));
        assert!(!accounts.set_name("missing", "Ann"));
        assert_eq!(accounts.get("a").unwrap().name.as_deref(), Some("Ann"));
    }

    #[test]
    fn remove_moves_active_to_the_next_account() {
        let mut accounts = list(&["a", "b", "c"]);
        accounts.set_active("b");
        accounts.remove("b");
        assert_eq!(accounts.active.as_deref(), Some("c"));
        // Removing the last one falls back to the one before it.
        accounts.remove("c");
        assert_eq!(accounts.active.as_deref(), Some("a"));
        accounts.remove("a");
        assert_eq!(accounts.active, None);
        assert!(accounts.remove("a").is_none());
    }

    #[test]
    fn remove_keeps_active_when_another_account_goes() {
        let mut accounts = list(&["a", "b", "c"]);
        accounts.set_active("c");
        accounts.remove("a");
        assert_eq!(accounts.active.as_deref(), Some("c"));
    }

    fn ids(accounts: &AccountList) -> Vec<&str> {
        accounts.accounts.iter().map(|a| a.id.as_str()).collect()
    }

    #[test]
    fn move_account_to_the_front_and_the_end() {
        let mut accounts = list(&["a", "b", "c"]);
        accounts.set_active("b");
        assert!(accounts.move_account("c", Some("a")));
        assert_eq!(ids(&accounts), ["c", "a", "b"]);
        assert!(accounts.move_account("c", None));
        assert_eq!(ids(&accounts), ["a", "b", "c"]);
        assert!(accounts.move_account("a", Some("c")));
        assert_eq!(ids(&accounts), ["b", "a", "c"]);
        // Which account is shown doesn't change.
        assert_eq!(accounts.active.as_deref(), Some("b"));
    }

    #[test]
    fn move_account_where_it_is_changes_nothing() {
        let mut accounts = list(&["a", "b", "c"]);
        // Before itself, before the next one, and the last to the end.
        assert!(!accounts.move_account("b", Some("b")));
        assert!(!accounts.move_account("a", Some("b")));
        assert!(!accounts.move_account("c", None));
        assert_eq!(ids(&accounts), ["a", "b", "c"]);
    }

    #[test]
    fn move_account_ignores_unknown_ids() {
        let mut accounts = list(&["a", "b"]);
        assert!(!accounts.move_account("missing", Some("a")));
        assert!(!accounts.move_account("missing", None));
        assert!(!accounts.move_account("b", Some("missing")));
        assert_eq!(ids(&accounts), ["a", "b"]);
    }

    #[test]
    fn active_entry_falls_back_to_the_first() {
        let mut accounts = list(&["a", "b"]);
        assert_eq!(accounts.active_entry().unwrap().id, "a");
        accounts.set_active("b");
        assert_eq!(accounts.active_entry().unwrap().id, "b");
        accounts.set_active("missing");
        assert_eq!(accounts.active.as_deref(), Some("b"));
    }
}
