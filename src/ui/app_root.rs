use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Theme, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::accounts::{self, AccountList, AccountsFile};
use crate::api::StoatClient;
use crate::notify;
use crate::runtime::spawn_io;
use crate::session::{self, Restored, TokenStore};
use crate::store::display_name;
use crate::ui::chat::{ChatEvent, ChatView};
use crate::ui::login::{LoginEvent, LoginView};
use crate::ui::switcher::{AccountAction, AccountStatus, SwitcherItem, host_of, render_switcher};

/// Shown when Stoat has ended an account's session.
const SESSION_ENDED: &str = "Your session has ended. Please log in again.";

/// Where one saved account's session is at.
enum AccountState {
    /// Checking its saved session token.
    Restoring { _task: Task<()> },
    /// A saved token exists but couldn't be checked. Kept so a retry can work.
    Offline { token: String, error: SharedString },
    /// No usable session: logging in again, on the account's own instance.
    SignedOut {
        login: Entity<LoginView>,
        _events: Subscription,
    },
    /// Signed in and connected, whether or not it's on screen.
    Chat {
        view: Entity<ChatView>,
        token: String,
        _events: Subscription,
        /// Keeps the Dock badge, banners and switcher in step with the view.
        _changes: Subscription,
    },
}

/// What the main window is showing, next to the account switcher.
enum Shown {
    /// Loading the account list at startup.
    Loading { _task: Task<()> },
    /// One account, whatever its state.
    Account(String),
    /// Logging in to another account. Cancel goes back to `back`.
    Adding {
        login: Entity<LoginView>,
        back: Option<String>,
        _events: Subscription,
    },
}

/// Top-level view for the main window: every saved account, each connected
/// on its own, with a switcher to pick the one on screen.
pub struct AppRoot {
    /// The instance the login screen suggests (`STOAT_API_URL`, or the
    /// official one).
    default_instance: String,
    store: Arc<dyn TokenStore>,
    accounts_file: Arc<dyn AccountsFile>,
    accounts: AccountList,
    /// Each account's state, by account ID.
    states: HashMap<String, AccountState>,
    shown: Shown,
    /// The count last shown on the Dock icon, to only touch AppKit on change.
    dock_badge: usize,
    _appearance: Subscription,
}

impl AppRoot {
    pub fn new(
        store: Arc<dyn TokenStore>,
        accounts_file: Arc<dyn AccountsFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Follow the macOS light/dark setting, now and when it changes.
        Theme::sync_system_appearance(Some(window), cx);
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
        });
        // Banners: ask for permission up front. GPUI keeps one click handler
        // for the whole app, so it's registered here and routed by account.
        crate::banners::request_permission();
        if crate::banners::available() {
            let root = cx.weak_entity();
            let handle = window.window_handle();
            cx.on_system_notification_response(move |response, cx| {
                let tag = response.tag.to_string();
                log::debug!("banner: clicked {tag}");
                cx.activate(true);
                handle
                    .update(cx, |_, window, cx| {
                        window.activate_window();
                        root.update(cx, |this, cx| this.open_notice(&tag, cx)).ok();
                    })
                    .ok();
            });
        }

        let default_instance = StoatClient::from_env().base_url().to_string();
        log::info!("default Stoat API: {default_instance}");
        // Read here (it's a small file). Only a first launch needs the
        // Keychain, to adopt a session saved by an older version. An
        // unreadable list is left as it is until something is saved.
        let (saved, first_launch) = match accounts_file.load() {
            Ok(Some(list)) => (list, false),
            Ok(None) => (AccountList::default(), true),
            Err(err) => {
                log::warn!("couldn't read the account list; starting without one: {err:#}");
                (AccountList::default(), false)
            }
        };
        let migrate = first_launch.then(|| {
            spawn_io(accounts::migrate_legacy(
                store.clone(),
                default_instance.clone(),
            ))
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let adopted = match migrate {
                Some(migrate) => Some(migrate.await.unwrap_or_default()),
                None => None,
            };
            this.update_in(cx, |this, window, cx| {
                if let Some(accounts) = adopted {
                    this.accounts = accounts;
                    this.save_accounts();
                }
                this.start(window, cx);
            })
            .ok();
        });

        Self {
            default_instance,
            store,
            accounts_file,
            accounts: saved,
            states: HashMap::new(),
            shown: Shown::Loading { _task: task },
            dock_badge: 0,
            _appearance: appearance,
        }
    }

    /// Restore every saved account at once, and show the one shown last
    /// (or the login screen, if there are none).
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .accounts
            .accounts
            .iter()
            .map(|a| a.id.clone())
            .collect();
        for id in ids {
            self.restore(id, window, cx);
        }
        self.show_next(window, cx);
    }

    /// Show the active account, or the login screen if there are none.
    fn show_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.accounts.active_entry().map(|a| a.id.clone()) {
            Some(id) => self.show_account(&id, cx),
            None => self.add_account(window, cx),
        }
    }

    /// Write the account list. It's tiny, so this runs here rather than on the
    /// IO runtime, which also keeps writes in order. A failure is logged: the
    /// accounts work for now but won't be restored next launch.
    fn save_accounts(&self) {
        if let Err(err) = self.accounts_file.save(&self.accounts) {
            log::warn!("couldn't save the account list: {err:#}");
        }
    }

    /// A client for an account's instance, without a token.
    fn client_for(&self, id: &str) -> StoatClient {
        match self.accounts.get(id) {
            Some(entry) => StoatClient::new(entry.instance.clone()),
            None => StoatClient::new(self.default_instance.clone()),
        }
    }

    /// The account on screen, if any.
    fn shown_account(&self) -> Option<&str> {
        match &self.shown {
            Shown::Account(id) => Some(id),
            Shown::Loading { .. } | Shown::Adding { .. } => None,
        }
    }

    fn chat_view(&self, id: &str) -> Option<&Entity<ChatView>> {
        match self.states.get(id) {
            Some(AccountState::Chat { view, .. }) => Some(view),
            _ => None,
        }
    }

    /// Put `id` on screen. Only the account on screen marks things read.
    pub fn show_account(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.accounts.get(id).is_none() {
            return;
        }
        if let Some(previous) = self.shown_account().map(str::to_string)
            && previous != id
            && let Some(view) = self.chat_view(&previous)
        {
            view.update(cx, |view, cx| view.set_active(false, cx));
        }
        if let Some(view) = self.chat_view(id) {
            view.update(cx, |view, cx| view.set_active(true, cx));
        }
        self.shown = Shown::Account(id.to_string());
        if self.accounts.active.as_deref() != Some(id) {
            self.accounts.set_active(id);
            self.save_accounts();
        }
        cx.notify();
    }

    /// Show the login screen for another account (the only screen when there
    /// are no accounts, so then without Cancel).
    pub fn add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.shown, Shown::Adding { .. }) {
            return;
        }
        let back = self.shown_account().map(str::to_string);
        if let Some(view) = back.as_deref().and_then(|id| self.chat_view(id)) {
            view.update(cx, |view, cx| view.set_active(false, cx));
        }
        let instance = self.default_instance.clone();
        let cancellable = !self.accounts.accounts.is_empty();
        let login =
            cx.new(|cx| LoginView::new(instance, false, window, cx).cancellable(cancellable));
        let events = cx.subscribe_in(&login, window, |this, _, event, window, cx| match event {
            LoginEvent::LoggedIn { session, instance } => this.logged_in(
                instance.clone(),
                session.token.clone(),
                session.user_id.clone(),
                window,
                cx,
            ),
            LoginEvent::Cancelled => this.cancel_adding(window, cx),
        });
        self.shown = Shown::Adding {
            login,
            back,
            _events: events,
        };
        cx.notify();
    }

    fn cancel_adding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = match &self.shown {
            Shown::Adding { back, .. } => back.clone(),
            _ => return,
        };
        match back {
            Some(id) if self.accounts.get(&id).is_some() => self.show_account(&id, cx),
            _ => {
                // Nothing to go back to: show whichever account there is.
                self.shown = Shown::Loading {
                    _task: Task::ready(()),
                };
                self.show_next(window, cx);
            }
        }
    }

    fn restore(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.client_for(&id);
        log::info!("restoring {id}");
        let request = spawn_io(session::restore(self.store.clone(), client, id.clone()));
        let task = cx.spawn_in(window, {
            let id = id.clone();
            async move |this, cx| {
                let restored = request.await;
                this.update_in(cx, |this, window, cx| {
                    this.finish_restore(id, restored, window, cx)
                })
                .ok();
            }
        });
        self.states
            .insert(id, AccountState::Restoring { _task: task });
        cx.notify();
    }

    fn finish_restore(
        &mut self,
        id: String,
        restored: anyhow::Result<Restored>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.accounts.get(&id).is_none() {
            return; // Logged out meanwhile.
        }
        match restored {
            Ok(Restored::SignedIn { token, user }) => {
                if self.accounts.set_user(&id, &user.id, display_name(&user)) {
                    self.save_accounts();
                }
                self.start_chat(&id, token, user.id, window, cx);
            }
            Ok(Restored::NeedsLogin) => {
                if self.accounts.get(&id).is_some_and(|a| a.user_id.is_none()) {
                    // An adopted old session we never identified has nothing
                    // left to show once its token is gone, so forget it.
                    self.forget_account(&id, window, cx);
                } else {
                    self.sign_out(&id, None, window, cx);
                }
            }
            Ok(Restored::Unverified { token, error }) => {
                self.states.insert(
                    id,
                    AccountState::Offline {
                        token,
                        error: error.to_string().into(),
                    },
                );
            }
            Err(err) => {
                log::error!("session restore task failed: {err:?}");
                self.sign_out(&id, None, window, cx);
            }
        }
        cx.notify();
    }

    /// Connect a signed-in account. It starts hidden unless it's on screen.
    fn start_chat(
        &mut self,
        id: &str,
        token: String,
        me: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let client = self.client_for(id).with_token(token.clone());
        let account = id.to_string();
        let active = self.shown_account() == Some(id);
        let view = cx.new(|cx| {
            let mut view = ChatView::new(client, me, account, window, cx);
            view.set_active(active, cx);
            view
        });
        let changes = cx.observe(&view, |this, _, cx| this.sync_outside(cx));
        let events = cx.subscribe_in(&view, window, {
            let id = id.to_string();
            move |this, _, event, window, cx| match event {
                ChatEvent::Activate => this.show_account(&id, cx),
                ChatEvent::LogOutRequested => this.log_out(&id, window, cx),
                ChatEvent::SessionRejected => this.session_ended(&id, window, cx),
            }
        });
        self.states.insert(
            id.to_string(),
            AccountState::Chat {
                view,
                token,
                _events: events,
                _changes: changes,
            },
        );
        self.sync_outside(cx);
    }

    /// Mark an account signed out: it stays in the switcher, and shows a
    /// login screen for its own instance.
    fn sign_out(
        &mut self,
        id: &str,
        message: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let instance = self.client_for(id).base_url().to_string();
        let login = cx.new(|cx| {
            let mut login = LoginView::new(instance, true, window, cx);
            if let Some(message) = message {
                login.show_message(message, cx);
            }
            login
        });
        let events = cx.subscribe_in(&login, window, |this, _, event, window, cx| match event {
            LoginEvent::LoggedIn { session, instance } => this.logged_in(
                instance.clone(),
                session.token.clone(),
                session.user_id.clone(),
                window,
                cx,
            ),
            LoginEvent::Cancelled => {}
        });
        self.states.insert(
            id.to_string(),
            AccountState::SignedOut {
                login,
                _events: events,
            },
        );
        self.sync_outside(cx);
        cx.notify();
    }

    /// Stoat rejected an account's session: forget its token, and keep the
    /// account, signed out, so logging back in is one step.
    fn session_ended(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        log::warn!("server rejected the session for {id}; signing it out");
        let forget = spawn_io(session::forget(self.store.clone(), id.to_string()));
        cx.spawn(async move |_, _| match forget.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => log::warn!("couldn't remove the rejected session: {err:#}"),
            Err(err) => log::warn!("forget task failed: {err:?}"),
        })
        .detach();
        self.sign_out(id, Some(SESSION_ENDED), window, cx);
    }

    /// A login finished, from the add-account screen or a signed-out account.
    fn logged_in(
        &mut self,
        instance: String,
        token: String,
        me: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self.accounts.find(&instance, &me).map(|a| a.id.clone()) {
            match self.states.get(&existing) {
                Some(AccountState::Chat { .. }) => {
                    // Already here and connected: keep that session, and end
                    // the one just made so it doesn't linger in Settings.
                    log::info!("{existing} is already signed in; ending the new session");
                    self.end_session(StoatClient::new(instance).with_token(token), cx);
                    self.show_account(&existing, cx);
                    window.push_notification(
                        Notification::new().message("You're already signed in to that account."),
                        cx,
                    );
                    return;
                }
                Some(AccountState::Offline { token: old, .. }) => {
                    // The old session couldn't be checked; the new one replaces it.
                    let old = old.clone();
                    self.end_session(self.client_for(&existing).with_token(old), cx);
                }
                _ => {}
            }
        }

        let id = self.accounts.add_or_update(&instance, &me, None);
        self.save_accounts();
        let save = spawn_io(session::remember(
            self.store.clone(),
            id.clone(),
            token.clone(),
        ));
        cx.spawn(async move |_, _| match save.await {
            Ok(Ok(())) => log::info!("session saved to Keychain"),
            Ok(Err(err)) => {
                log::warn!("couldn't save session (you'll log in again next launch): {err:#}")
            }
            Err(err) => log::warn!("session save task failed: {err:?}"),
        })
        .detach();

        self.shown = Shown::Account(id.clone());
        self.start_chat(&id, token, me, window, cx);
        self.show_account(&id, cx);
    }

    /// End a session on the server, in the background, logging the result.
    fn end_session(&self, client: StoatClient, cx: &mut Context<Self>) {
        let request = spawn_io(async move { client.logout().await });
        cx.spawn(async move |_, _| match request.await {
            Ok(Ok(())) => log::debug!("ended a session that wasn't needed"),
            Ok(Err(err)) => log::warn!("couldn't end an unneeded session: {err:?}"),
            Err(err) => log::warn!("logout task failed: {err:?}"),
        })
        .detach();
    }

    fn retry_restore(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.restore(id.to_string(), window, cx);
    }

    /// Log out of the account on screen (the Stoat menu's Log Out).
    pub fn log_out_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.shown_account().map(str::to_string) {
            self.log_out(&id, window, cx);
        }
    }

    /// Log out of an account: end its session on Stoat, forget its token,
    /// and remove it. Another account is shown if this one was on screen.
    pub fn log_out(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let token = match self.states.get(id) {
            Some(AccountState::Chat { token, .. } | AccountState::Offline { token, .. }) => {
                Some(token.clone())
            }
            _ => None,
        };
        let key = id.to_string();
        let request = match token {
            Some(token) => spawn_io(session::log_out(
                self.store.clone(),
                self.client_for(id).with_token(token),
                key,
            )),
            // No live session: just forget whatever token is saved.
            None => {
                let store = self.store.clone();
                spawn_io(async move { (Ok(()), session::forget(store, key).await) })
            }
        };
        // Keep the IO alive after the account goes; dropping it would cancel it.
        cx.spawn(async move |_, _| match request.await {
            Ok((server, local)) => {
                if let Err(err) = local {
                    log::warn!("couldn't remove saved session: {err:#}");
                }
                match server {
                    Ok(()) => log::info!("logged out"),
                    Err(err) => log::warn!(
                        "logged out locally, but couldn't end the session on Stoat \
                         (revoke it in Settings → Sessions): {err:?}"
                    ),
                }
            }
            Err(err) => log::warn!("logout task failed: {err:?}"),
        })
        .detach();

        self.forget_account(id, window, cx);
    }

    /// Remove an account and its state (dropping its events connection).
    fn forget_account(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.states.remove(id);
        self.accounts.remove(id);
        self.save_accounts();
        let back_gone = matches!(&self.shown, Shown::Adding { back: Some(back), .. } if back == id);
        if back_gone && let Shown::Adding { back, .. } = &mut self.shown {
            *back = None;
        }
        if self.shown_account() == Some(id) {
            self.shown = Shown::Loading {
                _task: Task::ready(()),
            };
            self.show_next(window, cx);
        }
        self.sync_outside(cx);
        cx.notify();
    }

    /// A banner was clicked: show its channel, in its account. A banner
    /// from before multiple accounts names no account and opens in the one
    /// on screen; one for an account that's gone is ignored.
    fn open_notice(&mut self, tag: &str, cx: &mut Context<Self>) {
        let (account, channel) = notify::parse_banner_tag(tag);
        let Some(id) = account
            .map(str::to_string)
            .or_else(|| self.shown_account().map(str::to_string))
        else {
            return;
        };
        let Some(view) = self.chat_view(&id).cloned() else {
            log::debug!("banner: {tag} is for an account that isn't signed in here");
            return;
        };
        self.show_account(&id, cx);
        view.update(cx, |view, cx| view.open_channel(channel, cx));
    }

    /// After any account changes: show the sum of their attention counts on
    /// the Dock icon, take away banners for what's been read, remember
    /// display names, and redraw the switcher.
    fn sync_outside(&mut self, cx: &mut Context<Self>) {
        let views: Vec<(String, Entity<ChatView>)> = self
            .accounts
            .accounts
            .iter()
            .filter_map(|a| Some((a.id.clone(), self.chat_view(&a.id)?.clone())))
            .collect();
        let mut attention = 0;
        let mut renamed = false;
        for (id, view) in &views {
            attention += view.update(cx, |view, cx| view.sync_outside(cx));
            if let Some((name, _)) = view.read(cx).me_summary() {
                renamed |= self.accounts.set_name(id, &name);
            }
        }
        if renamed {
            self.save_accounts();
        }
        if attention != self.dock_badge {
            log::debug!("dock badge: {attention}");
            for (_, view) in &views {
                view.read(cx).log_attention();
            }
            crate::dock::set_badge(attention);
            self.dock_badge = attention;
        }
        cx.notify();
    }

    fn render_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.shown_account();
        let items = self
            .accounts
            .accounts
            .iter()
            .map(|entry| {
                let host = host_of(&entry.instance);
                let summary = self
                    .chat_view(&entry.id)
                    .and_then(|view| view.read(cx).me_summary());
                let (name, avatar) = match summary {
                    Some((name, avatar)) => (name, Some(avatar)),
                    None => (entry.name.clone().unwrap_or_else(|| host.clone()), None),
                };
                let status = match self.states.get(&entry.id) {
                    Some(AccountState::Chat { .. }) => AccountStatus::SignedIn,
                    Some(AccountState::Offline { .. }) => AccountStatus::Offline,
                    Some(AccountState::SignedOut { .. }) => AccountStatus::SignedOut,
                    Some(AccountState::Restoring { .. }) | None => AccountStatus::Loading,
                };
                SwitcherItem {
                    id: entry.id.clone(),
                    name,
                    host,
                    avatar,
                    status,
                    selected: shown == Some(entry.id.as_str()),
                    unread: false,
                    mentions: 0,
                }
            })
            .collect();
        let root = cx.weak_entity();
        let on_select: AccountAction = Rc::new({
            let root = root.clone();
            move |id, _, cx| {
                root.update(cx, |this, cx| this.show_account(id, cx)).ok();
            }
        });
        let on_log_out: AccountAction = Rc::new({
            let root = root.clone();
            move |id, window, cx| {
                root.update(cx, |this, cx| this.log_out(id, window, cx))
                    .ok();
            }
        });
        render_switcher(
            items,
            matches!(self.shown, Shown::Adding { .. }),
            on_select,
            on_log_out,
            cx.listener(|this, _, window, cx| this.add_account(window, cx)),
            cx,
        )
    }

    /// The main area: the shown account's chat or login, or a status page.
    fn render_main(&self, cx: &mut Context<Self>) -> AnyElement {
        let id = match &self.shown {
            Shown::Loading { .. } => return status_page("Signing in…", None, None, cx),
            Shown::Adding { login, .. } => return login.clone().into_any_element(),
            Shown::Account(id) => id.clone(),
        };
        match self.states.get(&id) {
            Some(AccountState::Chat { view, .. }) => view.clone().into_any_element(),
            Some(AccountState::SignedOut { login, .. }) => login.clone().into_any_element(),
            Some(AccountState::Restoring { .. }) | None => {
                status_page("Signing in…", None, None, cx)
            }
            Some(AccountState::Offline { error, .. }) => {
                let actions = h_flex()
                    .gap_2()
                    .child(Button::new("retry").primary().label("Retry").on_click({
                        let id = id.clone();
                        cx.listener(move |this, _, window, cx| this.retry_restore(&id, window, cx))
                    }))
                    .child(Button::new("log-out").ghost().label("Log out").on_click(
                        cx.listener(move |this, _, window, cx| this.log_out(&id, window, cx)),
                    ));
                status_page(
                    "Couldn't check your saved session",
                    Some(error.clone()),
                    Some(actions),
                    cx,
                )
            }
        }
    }
}

/// A centred page with a status line, an optional detail and buttons.
fn status_page(
    status: &'static str,
    detail: Option<SharedString>,
    actions: Option<Div>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(theme.background)
        .text_color(theme.foreground)
        .child(
            div()
                .text_3xl()
                .font_weight(FontWeight::BOLD)
                .child("Stoat"),
        )
        .child(div().text_lg().child(status))
        .children(detail.map(|detail| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(detail)
        }))
        .children(actions)
        .into_any_element()
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // No switcher before the first account exists.
        let has_accounts = !self.accounts.accounts.is_empty();
        h_flex()
            .size_full()
            .when(has_accounts, |el| el.child(self.render_switcher(cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_main(cx)),
            )
    }
}
