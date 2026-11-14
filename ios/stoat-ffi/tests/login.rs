//! Logging in, restoring and logging out through [`Core`], against a mock
//! Stoat API (wiremock), with in-memory token and account stores.

use std::sync::Arc;

use serde_json::{Value, json};
use stoat_ffi::session::{Core, LoginOutcome, MfaAnswer, RestoreOutcome, SESSION_NAME};
use stoat_gpui::accounts::{AccountList, AccountsFile, FileAccounts, MemoryAccounts};
use stoat_gpui::session::{MemoryStore, TokenStore};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const USER_ID: &str = "01JC0USER000000000000000AA";
const TOKEN: &str = "test-session-token-0000000000000000000000000000000000000000000000";

fn fixture(name: &str) -> Value {
    let path = format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// A mock instance that answers `GET /` like Stoat.
async fn instance() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("root.json")))
        .mount(&server)
        .await;
    server
}

async fn mount_me(server: &MockServer, status: u16) {
    let response = if status == 200 {
        ResponseTemplate::new(200).set_body_json(fixture("users_me.json"))
    } else {
        ResponseTemplate::new(status).set_body_json(json!({ "type": "InvalidSession" }))
    };
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(response)
        .mount(server)
        .await;
}

struct Stores {
    tokens: Arc<MemoryStore>,
    accounts: Arc<MemoryAccounts>,
}

impl Stores {
    fn new() -> Self {
        Self {
            tokens: Arc::new(MemoryStore::default()),
            accounts: Arc::new(MemoryAccounts::default()),
        }
    }

    fn core(&self) -> Core {
        Core::with_stores(self.tokens.clone(), self.accounts.clone())
    }
}

fn key(server: &MockServer) -> String {
    format!("{}#{USER_ID}", server.uri())
}

#[tokio::test]
async fn a_plain_login_saves_the_token_and_the_account() {
    let server = instance().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .and(body_partial_json(json!({
            "email": "me@example.com",
            "password": "hunter2",
            "friendly_name": SESSION_NAME,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_success.json")))
        .expect(1)
        .mount(&server)
        .await;
    mount_me(&server, 200).await;

    let stores = Stores::new();
    let core = stores.core();
    let outcome = core
        .login(server.uri(), " me@example.com ".into(), "hunter2".into())
        .await
        .unwrap();

    let LoginOutcome::LoggedIn { account } = outcome else {
        panic!("expected LoggedIn, got {outcome:?}");
    };
    assert_eq!(account.id, key(&server));
    assert_eq!(account.instance, server.uri());
    assert_eq!(account.user_id, USER_ID);
    assert_eq!(account.name, "Stoat Fan");
    assert_eq!(core.account(), Some(account.clone()));
    assert_eq!(stores.tokens.load(&key(&server)).unwrap().as_deref(), Some(TOKEN));

    let list = stores.accounts.load().unwrap().unwrap();
    assert_eq!(list.active.as_deref(), Some(account.id.as_str()));
    assert_eq!(
        list.get(&account.id).unwrap().session_id.as_deref(),
        Some("01JC0SESS10N000000000000AA")
    );
    assert_eq!(core.saved_account(), Some(account));
    assert_eq!(
        core.client().unwrap().token(),
        Some(TOKEN),
        "the signed-in client carries the token"
    );
}

#[tokio::test]
async fn a_login_needing_mfa_then_succeeds() {
    let server = instance().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .and(body_partial_json(json!({ "email": "me@example.com" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_mfa.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .and(body_partial_json(json!({
            "mfa_ticket": "test-mfa-ticket",
            "mfa_response": { "totp_code": "123456" },
            "friendly_name": SESSION_NAME,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_success.json")))
        .expect(1)
        .mount(&server)
        .await;
    mount_me(&server, 200).await;

    let stores = Stores::new();
    let core = stores.core();
    let outcome = core
        .login(server.uri(), "me@example.com".into(), "hunter2".into())
        .await
        .unwrap();
    let LoginOutcome::NeedsMfa {
        instance,
        ticket,
        allows_totp,
        allows_recovery,
    } = outcome
    else {
        panic!("expected NeedsMfa, got {outcome:?}");
    };
    assert_eq!(instance, server.uri());
    assert!(allows_totp && allows_recovery);
    assert!(core.account().is_none());

    let outcome = core
        .login_mfa(
            instance,
            ticket,
            MfaAnswer::Totp {
                code: "123 456".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(outcome, LoginOutcome::LoggedIn { .. }), "{outcome:?}");
    assert_eq!(stores.tokens.load(&key(&server)).unwrap().as_deref(), Some(TOKEN));
}

#[tokio::test]
async fn a_wrong_password_gives_desktops_error_text() {
    let server = instance().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(fixture("error_invalid_credentials.json")),
        )
        .mount(&server)
        .await;

    let stores = Stores::new();
    let err = stores
        .core()
        .login(server.uri(), "me@example.com".into(), "wrong".into())
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "Incorrect email or password.");
    assert!(stores.accounts.load().unwrap().is_none(), "nothing saved");
}

#[tokio::test]
async fn a_disabled_account_says_so() {
    let server = instance().await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_disabled.json")))
        .mount(&server)
        .await;

    let outcome = Stores::new()
        .core()
        .login(server.uri(), "me@example.com".into(), "hunter2".into())
        .await
        .unwrap();
    assert!(matches!(outcome, LoginOutcome::Disabled), "{outcome:?}");
}

#[tokio::test]
async fn blank_fields_are_refused_without_a_request() {
    let core = Stores::new().core();
    let err = core
        .login("http://127.0.0.1:9".into(), " ".into(), "x".into())
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "Enter your email and password.");
    let err = core
        .login_mfa(
            "http://127.0.0.1:9".into(),
            "t".into(),
            MfaAnswer::Recovery { code: " ".into() },
        )
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "Enter your code.");
}

/// Log in once, so the stores hold a session to restore.
async fn logged_in_stores(server: &MockServer) -> Stores {
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_success.json")))
        .mount(server)
        .await;
    let stores = Stores::new();
    stores
        .core()
        .login(server.uri(), "me@example.com".into(), "hunter2".into())
        .await
        .unwrap();
    stores
}

#[tokio::test]
async fn restore_signs_in_with_the_saved_token() {
    let server = instance().await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .and(header("x-session-token", TOKEN))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("users_me.json")))
        .mount(&server)
        .await;
    let stores = logged_in_stores(&server).await;

    let core = stores.core();
    let RestoreOutcome::SignedIn { account } = core.restore().await else {
        panic!("expected SignedIn");
    };
    assert_eq!(account.id, key(&server));
    assert_eq!(account.name, "Stoat Fan");
    assert_eq!(core.account(), Some(account));
}

#[tokio::test]
async fn restore_with_a_rejected_token_clears_the_vault() {
    let server = instance().await;
    mount_me(&server, 401).await;
    let stores = logged_in_stores(&server).await;

    let core = stores.core();
    assert_eq!(core.restore().await, RestoreOutcome::NeedsLogin);
    assert_eq!(stores.tokens.load(&key(&server)).unwrap(), None);
    assert!(core.account().is_none());
}

#[tokio::test]
async fn restore_while_offline_keeps_the_token() {
    // A saved account on an instance where nothing listens.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let instance = format!("http://127.0.0.1:{port}");
    let stores = Stores::new();
    let mut list = AccountList::default();
    let id = list.add_or_update(&instance, USER_ID, Some("Stoat Fan".into()));
    list.set_active(&id);
    stores.accounts.save(&list).unwrap();
    stores.tokens.save(&id, TOKEN).unwrap();

    let core = stores.core();
    let RestoreOutcome::Offline { account, message } = core.restore().await else {
        panic!("expected Offline");
    };
    assert_eq!(account.id, id);
    assert_eq!(account.name, "Stoat Fan");
    assert_eq!(message, "Couldn't reach Stoat. Check your connection and try again.");
    assert_eq!(stores.tokens.load(&id).unwrap().as_deref(), Some(TOKEN));
    assert!(core.account().is_none());
}

#[tokio::test]
async fn restore_with_nothing_saved_needs_login() {
    assert_eq!(Stores::new().core().restore().await, RestoreOutcome::NeedsLogin);
}

#[tokio::test]
async fn log_out_forgets_the_session_everywhere() {
    let server = instance().await;
    mount_me(&server, 200).await;
    Mock::given(method("POST"))
        .and(path("/auth/session/logout"))
        .and(header("x-session-token", TOKEN))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let stores = logged_in_stores(&server).await;

    let core = stores.core();
    assert!(matches!(core.restore().await, RestoreOutcome::SignedIn { .. }));
    core.log_out().await;

    assert!(core.account().is_none());
    assert!(core.saved_account().is_none());
    assert_eq!(stores.tokens.load(&key(&server)).unwrap(), None);
    assert_eq!(core.restore().await, RestoreOutcome::NeedsLogin);
}

#[tokio::test]
async fn the_account_list_is_a_file_in_the_folder_swift_gives() {
    let dir = tempfile::tempdir().unwrap();
    let server = instance().await;
    mount_me(&server, 200).await;
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("login_success.json")))
        .mount(&server)
        .await;

    let tokens: Arc<MemoryStore> = Arc::new(MemoryStore::default());
    let file = Arc::new(FileAccounts::at(dir.path().join("accounts.json")));
    let core = Core::with_stores(tokens.clone(), file.clone());
    core.login(server.uri(), "me@example.com".into(), "hunter2".into())
        .await
        .unwrap();

    let reopened = Core::with_stores(tokens, file);
    assert_eq!(reopened.saved_account().unwrap().id, key(&server));
}
