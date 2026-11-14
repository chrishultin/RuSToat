//! Logging in, staying logged in, and logging out.
//!
//! [`Core`] holds the app's one account (milestone 1) and its session. The
//! token goes to the iOS Keychain through Swift's [`TokenVault`], under the
//! same `{API URL}#{user ID}` name as desktop ([`accounts::keychain_name`]),
//! and the account list is the same `accounts.json` desktop keeps, in a
//! folder Swift chooses. The call sequence follows desktop's login screen
//! (`src/ui/login.rs`) and `AppRoot::logged_in` (`src/ui/app_root.rs`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use stoat_gpui::accounts::{AccountEntry, AccountList, AccountsFile, FileAccounts};
use stoat_gpui::api::instance::resolve_instance;
use stoat_gpui::api::models::{MFAMethod, MFAResponse, ResponseLogin};
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::runtime::spawn_io;
use stoat_gpui::session::{self, Restored, TokenStore};
use stoat_gpui::store::display_name;

use crate::CoreError;

/// What iOS sessions are called in Stoat's Settings → Sessions.
pub const SESSION_NAME: &str = "RuSToat (iPhone)";

/// Where Swift keeps session tokens (the iOS Keychain). Called from Tokio
/// threads, so implementations must be thread-safe.
#[uniffi::export(callback_interface)]
pub trait TokenVault: Send + Sync {
    fn load(&self, key: String) -> Result<Option<String>, CoreError>;
    fn save(&self, key: String, token: String) -> Result<(), CoreError>;
    /// Not an error if there's nothing saved.
    fn clear(&self, key: String) -> Result<(), CoreError>;
}

/// A [`TokenVault`] as the core's [`TokenStore`].
struct VaultStore(Box<dyn TokenVault>);

impl TokenStore for VaultStore {
    fn load(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self.0.load(key.into())?)
    }

    fn save(&self, key: &str, token: &str) -> anyhow::Result<()> {
        Ok(self.0.save(key.into(), token.into())?)
    }

    fn clear(&self, key: &str) -> anyhow::Result<()> {
        Ok(self.0.clear(key.into())?)
    }
}

/// A signed-in (or saved) account, as Swift shows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Account {
    /// `{API URL}#{user ID}`: the account's ID and its Keychain name.
    pub id: String,
    /// The API's address, e.g. `https://api.stoat.chat`.
    pub instance: String,
    pub user_id: String,
    /// Display name, as last seen.
    pub name: String,
}

impl Account {
    fn from_entry(entry: &AccountEntry) -> Self {
        Self {
            id: entry.id.clone(),
            instance: entry.instance.clone(),
            user_id: entry.user_id.clone().unwrap_or_default(),
            name: entry.name.clone().unwrap_or_default(),
        }
    }
}

/// How a login attempt (or its second step) ended.
#[derive(Debug, uniffi::Enum)]
pub enum LoginOutcome {
    LoggedIn {
        account: Account,
    },
    /// The account uses two-factor authentication: answer with
    /// [`Core::login_mfa`], passing `instance` and `ticket` back.
    NeedsMfa {
        instance: String,
        ticket: String,
        allows_totp: bool,
        allows_recovery: bool,
    },
    Disabled,
}

/// A second-factor answer.
#[derive(Debug, uniffi::Enum)]
pub enum MfaAnswer {
    /// From an authenticator app. Spaces ("123 456") are removed.
    Totp { code: String },
    Recovery { code: String },
}

/// What startup found.
#[derive(Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RestoreOutcome {
    SignedIn {
        account: Account,
    },
    /// Nothing saved, or Stoat rejected the saved session (which is then
    /// forgotten).
    NeedsLogin,
    /// The saved session couldn't be checked (offline, server error). It's
    /// kept, so a retry can still sign in.
    Offline {
        account: Account,
        message: String,
    },
}

/// The account currently signed in, with its token.
struct Session {
    account: Account,
    token: String,
}

/// The app's sessions and saved accounts.
#[derive(uniffi::Object)]
pub struct Core {
    tokens: Arc<dyn TokenStore>,
    accounts: Arc<dyn AccountsFile>,
    session: Mutex<Option<Session>>,
}

impl Core {
    /// A core over any token store and account file (for tests).
    pub fn with_stores(tokens: Arc<dyn TokenStore>, accounts: Arc<dyn AccountsFile>) -> Self {
        Self {
            tokens,
            accounts,
            session: Mutex::new(None),
        }
    }

    fn load_accounts(&self) -> AccountList {
        match self.accounts.load() {
            Ok(list) => list.unwrap_or_default(),
            Err(err) => {
                log::warn!("couldn't read the account list: {err:#}");
                AccountList::default()
            }
        }
    }

    fn save_accounts(&self, list: &AccountList) {
        if let Err(err) = self.accounts.save(list) {
            log::warn!("couldn't save the account list: {err:#}");
        }
    }

    /// The signed-in account's client, carrying its token.
    pub fn client(&self) -> Option<StoatClient> {
        let session = self.session.lock().unwrap();
        let session = session.as_ref()?;
        Some(StoatClient::new(session.account.instance.clone()).with_token(session.token.clone()))
    }

    /// Record a successful login: save the token, add the account to the
    /// list as the active one, and sign it in.
    async fn logged_in(
        &self,
        instance: String,
        login: stoat_gpui::api::models::Session,
        name: Option<String>,
    ) -> Account {
        let mut list = self.load_accounts();
        let id = list.add_or_update(&instance, &login.user_id, name);
        list.set_session(&id, &login.id);
        list.set_active(&id);
        self.save_accounts(&list);
        let entry = list.get(&id).expect("just added").clone();

        let save = spawn_io(session::remember(
            self.tokens.clone(),
            id.clone(),
            login.token.clone(),
        ));
        match save.await {
            Ok(Ok(())) => log::info!("session saved to the Keychain"),
            Ok(Err(err)) => log::warn!("couldn't save the session: {err:#}"),
            Err(err) => log::warn!("session save task failed: {err:#}"),
        }

        let account = Account::from_entry(&entry);
        *self.session.lock().unwrap() = Some(Session {
            account: account.clone(),
            token: login.token,
        });
        account
    }

    /// Turn a login response from `client`'s instance into an outcome.
    async fn finish(
        &self,
        client: StoatClient,
        response: ResponseLogin,
    ) -> Result<LoginOutcome, CoreError> {
        match response {
            ResponseLogin::Success(login) => {
                // The login response has no name; fetch it so the account
                // reads well from the start. Not fatal if it fails.
                let me = client.with_token(login.token.clone());
                let name = match spawn_io(async move { me.fetch_self().await }).await {
                    Ok(Ok(user)) => Some(display_name(&user).to_string()),
                    _ => None,
                };
                let instance = client.base_url().to_string();
                log::info!("logged in as user {} on {instance}", login.user_id);
                Ok(LoginOutcome::LoggedIn {
                    account: self.logged_in(instance, login, name).await,
                })
            }
            ResponseLogin::MFA {
                ticket,
                allowed_methods,
            } => {
                let allows_totp = allowed_methods.iter().any(|m| matches!(m, MFAMethod::Totp));
                let allows_recovery = allowed_methods
                    .iter()
                    .any(|m| matches!(m, MFAMethod::Recovery));
                if !allows_totp && !allows_recovery {
                    return Err(CoreError::failed(
                        "This account's two-factor method isn't supported by this client yet.",
                    ));
                }
                Ok(LoginOutcome::NeedsMfa {
                    instance: client.base_url().to_string(),
                    ticket,
                    allows_totp,
                    allows_recovery,
                })
            }
            ResponseLogin::Disabled { .. } => Ok(LoginOutcome::Disabled),
        }
    }
}

#[uniffi::export]
impl Core {
    /// The real core: tokens in `vault`, the account list in
    /// `{data_dir}/accounts.json`.
    #[uniffi::constructor]
    pub fn new(vault: Box<dyn TokenVault>, data_dir: String) -> Arc<Self> {
        let file = FileAccounts::at(PathBuf::from(data_dir).join("accounts.json"));
        Arc::new(Self::with_stores(Arc::new(VaultStore(vault)), Arc::new(file)))
    }

    /// The account that's signed in, if any.
    pub fn account(&self) -> Option<Account> {
        let session = self.session.lock().unwrap();
        session.as_ref().map(|s| s.account.clone())
    }

    /// The saved account the app restores at launch, if any.
    pub fn saved_account(&self) -> Option<Account> {
        self.load_accounts().active_entry().map(Account::from_entry)
    }

    /// Log in with an email and password, on the instance at `server` (as
    /// typed: `https://` is added, and `/api` tried, as on desktop).
    pub async fn login(
        &self,
        server: String,
        email: String,
        password: String,
    ) -> Result<LoginOutcome, CoreError> {
        let email = email.trim().to_string();
        if email.is_empty() || password.is_empty() {
            return Err(CoreError::failed("Enter your email and password."));
        }
        let attempt = spawn_io(async move {
            let client = resolve_instance(&server)
                .await
                .map_err(CoreError::failed)?
                .0
                .with_session_name(SESSION_NAME);
            let response = client.login(&email, &password).await?;
            Ok::<_, CoreError>((client, response))
        });
        let (client, response) = attempt.await.map_err(CoreError::failed)??;
        self.finish(client, response).await
    }

    /// Finish a login that [`LoginOutcome::NeedsMfa`] asked a code for.
    pub async fn login_mfa(
        &self,
        instance: String,
        ticket: String,
        answer: MfaAnswer,
    ) -> Result<LoginOutcome, CoreError> {
        let response = match answer {
            MfaAnswer::Totp { code } => MFAResponse::Totp {
                totp_code: code.replace(' ', "").trim().to_string(),
            },
            MfaAnswer::Recovery { code } => MFAResponse::Recovery {
                recovery_code: code.trim().to_string(),
            },
        };
        if matches!(&response, MFAResponse::Totp { totp_code } if totp_code.is_empty())
            || matches!(&response, MFAResponse::Recovery { recovery_code } if recovery_code.is_empty())
        {
            return Err(CoreError::failed("Enter your code."));
        }
        let client = StoatClient::new(instance).with_session_name(SESSION_NAME);
        let attempt = spawn_io({
            let client = client.clone();
            async move { client.login_mfa(&ticket, response).await }
        });
        let response = attempt.await.map_err(CoreError::failed)??;
        self.finish(client, response).await
    }

    /// Sign in to the saved account, checking its token with Stoat.
    pub async fn restore(&self) -> RestoreOutcome {
        let Some(entry) = self.load_accounts().active_entry().cloned() else {
            return RestoreOutcome::NeedsLogin;
        };
        let client = StoatClient::new(entry.instance.clone());
        let restored = spawn_io(session::restore(
            self.tokens.clone(),
            client,
            entry.id.clone(),
        ))
        .await;
        match restored {
            Ok(Restored::SignedIn { token, user }) => {
                let mut list = self.load_accounts();
                if list.set_user(&entry.id, &user.id, display_name(&user)) {
                    self.save_accounts(&list);
                }
                let entry = list.get(&entry.id).cloned().unwrap_or(entry);
                let account = Account::from_entry(&entry);
                *self.session.lock().unwrap() = Some(Session {
                    account: account.clone(),
                    token,
                });
                RestoreOutcome::SignedIn { account }
            }
            Ok(Restored::NeedsLogin) => RestoreOutcome::NeedsLogin,
            Ok(Restored::Unverified { error, .. }) => RestoreOutcome::Offline {
                account: Account::from_entry(&entry),
                message: error.to_string(),
            },
            Err(err) => RestoreOutcome::Offline {
                account: Account::from_entry(&entry),
                message: format!("Something went wrong ({err:#}). Please try again."),
            },
        }
    }

    /// Stoat ended the session (the events service rejected it): forget
    /// its token without contacting Stoat. The account stays in the list,
    /// so logging in again updates it.
    pub async fn forget_session(&self) {
        let Some(session) = self.session.lock().unwrap().take() else {
            return;
        };
        match spawn_io(session::forget(self.tokens.clone(), session.account.id)).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => log::warn!("couldn't forget the ended session: {err:#}"),
            Err(err) => log::warn!("forget task failed: {err:#}"),
        }
    }

    /// Log out: forget the token here (always), end the session on Stoat
    /// (if it can be reached), and remove the account from the list.
    pub async fn log_out(&self) {
        let Some(session) = self.session.lock().unwrap().take() else {
            return;
        };
        let client = StoatClient::new(session.account.instance.clone()).with_token(session.token);
        let (server, local) = match spawn_io(session::log_out(
            self.tokens.clone(),
            client,
            session.account.id.clone(),
        ))
        .await
        {
            Ok(results) => results,
            Err(err) => {
                log::warn!("log out task failed: {err:#}");
                return;
            }
        };
        if let Err(err) = server {
            log::warn!("couldn't end the session on Stoat: {err}");
        }
        if let Err(err) = local {
            log::warn!("couldn't forget the session token: {err:#}");
        }
        let mut list = self.load_accounts();
        list.remove(&session.account.id);
        self.save_accounts(&list);
    }
}

impl From<ApiError> for CoreError {
    fn from(err: ApiError) -> Self {
        CoreError::failed(err)
    }
}
