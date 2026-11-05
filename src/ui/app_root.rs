use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::*;

use crate::accounts::{self, AccountList, AccountsFile};
use crate::api::StoatClient;
use crate::runtime::spawn_io;
use crate::session::{self, Restored, TokenStore};
use crate::store::display_name;
use crate::ui::chat::{ChatEvent, ChatView};
use crate::ui::login::{LoginEvent, LoginView};

/// What the main window is showing.
enum Screen {
    /// Loading the account list, then checking a saved session token.
    Restoring { _task: Task<()> },
    Login {
        view: Entity<LoginView>,
        _events: Subscription,
    },
    /// A saved token exists but couldn't be checked. Kept so a retry can work.
    Offline { token: String, error: SharedString },
    Chat {
        view: Entity<ChatView>,
        token: String,
        _events: Subscription,
    },
}

/// Top-level view for the main window: routes between login and chat.
pub struct AppRoot {
    /// The instance the login screen suggests (`STOAT_API_URL`, or the
    /// official one).
    default_instance: String,
    store: Arc<dyn TokenStore>,
    accounts_file: Arc<dyn AccountsFile>,
    accounts: AccountList,
    /// The account on screen (its ID), once there is one.
    current: Option<String>,
    screen: Screen,
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
                this.show_active(window, cx);
            })
            .ok();
        });

        Self {
            default_instance,
            store,
            accounts_file,
            accounts: saved,
            current: None,
            screen: Screen::Restoring { _task: task },
            _appearance: appearance,
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

    /// Show the active account (restoring its session), or the login screen
    /// if there are none.
    fn show_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.current = self.accounts.active_entry().map(|entry| entry.id.clone());
        self.screen = match self.current.clone() {
            Some(id) => self.restoring_screen(id, window, cx),
            None => self.login_screen(window, cx),
        };
        cx.notify();
    }

    fn restoring_screen(&self, id: String, window: &mut Window, cx: &mut Context<Self>) -> Screen {
        let client = self.client_for(&id);
        log::info!("restoring {id}");
        let request = spawn_io(session::restore(self.store.clone(), client, id.clone()));
        let task = cx.spawn_in(window, async move |this, cx| {
            let restored = request.await;
            this.update_in(cx, |this, window, cx| {
                this.finish_restore(id, restored, window, cx)
            })
            .ok();
        });
        Screen::Restoring { _task: task }
    }

    fn finish_restore(
        &mut self,
        id: String,
        restored: anyhow::Result<Restored>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.screen = match restored {
            Ok(Restored::SignedIn { token, user }) => {
                if self.accounts.set_user(&id, &user.id, display_name(&user)) {
                    self.save_accounts();
                }
                self.chat_screen(&id, token, user.id, window, cx)
            }
            Ok(Restored::NeedsLogin) => {
                // An adopted old session we never identified has nothing left
                // to show once its token is gone, so forget it.
                if self.accounts.get(&id).is_some_and(|a| a.user_id.is_none()) {
                    self.accounts.remove(&id);
                    self.save_accounts();
                }
                self.login_screen(window, cx)
            }
            Ok(Restored::Unverified { token, error }) => Screen::Offline {
                token,
                error: error.to_string().into(),
            },
            Err(err) => {
                log::error!("session restore task failed: {err:?}");
                self.login_screen(window, cx)
            }
        };
        cx.notify();
    }

    fn login_screen(&self, window: &mut Window, cx: &mut Context<Self>) -> Screen {
        let instance = self.default_instance.clone();
        let view = cx.new(|cx| LoginView::new(instance, false, window, cx));
        let events = cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            LoginEvent::LoggedIn { session, instance } => this.logged_in(
                instance.clone(),
                session.token.clone(),
                session.user_id.clone(),
                window,
                cx,
            ),
        });
        Screen::Login {
            view,
            _events: events,
        }
    }

    fn chat_screen(
        &self,
        id: &str,
        token: String,
        me: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Screen {
        let client = self.client_for(id).with_token(token.clone());
        let view = cx.new(|cx| ChatView::new(client, me, window, cx));
        let events = cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            ChatEvent::LogOutRequested => this.log_out(window, cx),
            ChatEvent::SessionRejected => {
                log::warn!("server rejected the session; logging out");
                this.log_out(window, cx);
                if let Screen::Login { view, .. } = &this.screen {
                    view.update(cx, |login, cx| {
                        login.show_message("Your session has ended. Please log in again.", cx)
                    });
                }
            }
        });
        Screen::Chat {
            view,
            token,
            _events: events,
        }
    }

    /// Fresh login: add the account, remember the token and open the chat.
    fn logged_in(
        &mut self,
        instance: String,
        token: String,
        me: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.accounts.add_or_update(&instance, &me, None);
        self.accounts.set_active(&id);
        self.save_accounts();
        self.current = Some(id.clone());

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

        self.screen = self.chat_screen(&id, token, me, window, cx);
        cx.notify();
    }

    fn retry_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else {
            return;
        };
        self.screen = self.restoring_screen(id, window, cx);
        cx.notify();
    }

    /// Forget the account and its saved session, end it on the server, and
    /// show the login screen.
    pub fn log_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = match &self.screen {
            Screen::Chat { token, .. } | Screen::Offline { token, .. } => token.clone(),
            Screen::Restoring { .. } | Screen::Login { .. } => return,
        };
        let Some(id) = self.current.take() else {
            return;
        };

        let request = spawn_io(session::log_out(
            self.store.clone(),
            self.client_for(&id).with_token(token),
            id.clone(),
        ));
        self.accounts.remove(&id);
        self.save_accounts();
        // Keep the IO alive after the screen changes; dropping it would cancel it.
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

        // Nothing to attend to once logged out.
        crate::dock::set_badge(0);
        // Replacing the chat screen drops its events connection.
        self.screen = self.login_screen(window, cx);
        cx.notify();
    }
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (status, detail, actions): (SharedString, Option<SharedString>, Option<Div>) =
            match &self.screen {
                Screen::Login { view, .. } => return view.clone().into_any_element(),
                Screen::Chat { view, .. } => return view.clone().into_any_element(),
                Screen::Restoring { .. } => ("Signing in…".into(), None, None),
                Screen::Offline { error, .. } => (
                    "Couldn't check your saved session".into(),
                    Some(error.clone()),
                    Some(
                        h_flex()
                            .gap_2()
                            .child(Button::new("retry").primary().label("Retry").on_click(
                                cx.listener(|this, _, window, cx| this.retry_restore(window, cx)),
                            ))
                            .child(Button::new("log-out").ghost().label("Log out").on_click(
                                cx.listener(|this, _, window, cx| this.log_out(window, cx)),
                            )),
                    ),
                ),
            };

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
}
