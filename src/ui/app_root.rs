use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::*;

use crate::api::StoatClient;
use crate::api::models::User;
use crate::runtime::spawn_io;
use crate::session::{self, Restored, TokenStore};
use crate::ui::login::{LoginEvent, LoginView};

/// What the main window is showing.
enum Screen {
    /// Checking a saved session token at startup.
    Restoring {
        _task: Task<()>,
    },
    Login {
        view: Entity<LoginView>,
        _events: Subscription,
    },
    /// A saved token exists but couldn't be checked. Kept so a retry can work.
    Offline(Box<Offline>),
    SignedIn(Box<SignedIn>),
}

struct Offline {
    token: String,
    error: SharedString,
}

/// Placeholder until the chat UI lands. Shows who we're logged in as.
struct SignedIn {
    token: String,
    user: Option<User>,
    error: Option<SharedString>,
    _fetch: Option<Task<()>>,
}

/// Top-level view for the main window: routes between login and chat.
pub struct AppRoot {
    client: StoatClient,
    store: Arc<dyn TokenStore>,
    screen: Screen,
    _appearance: Subscription,
}

impl AppRoot {
    pub fn new(store: Arc<dyn TokenStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Follow the macOS light/dark setting, now and when it changes.
        Theme::sync_system_appearance(Some(window), cx);
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
        });

        let client = StoatClient::from_env();
        log::info!("using Stoat API at {}", client.base_url());
        let screen = Self::restoring_screen(&client, &store, window, cx);

        Self {
            client,
            store,
            screen,
            _appearance: appearance,
        }
    }

    fn restoring_screen(
        client: &StoatClient,
        store: &Arc<dyn TokenStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Screen {
        let request = spawn_io(session::restore(store.clone(), client.clone()));
        let task = cx.spawn_in(window, async move |this, cx| {
            let restored = request.await;
            this.update_in(cx, |this, window, cx| {
                this.finish_restore(restored, window, cx)
            })
            .ok();
        });
        Screen::Restoring { _task: task }
    }

    fn finish_restore(
        &mut self,
        restored: anyhow::Result<Restored>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.screen = match restored {
            Ok(Restored::SignedIn { token, user }) => Screen::SignedIn(Box::new(SignedIn {
                token,
                user: Some(*user),
                error: None,
                _fetch: None,
            })),
            Ok(Restored::NeedsLogin) => Self::login_screen(&self.client, window, cx),
            Ok(Restored::Unverified { token, error }) => Screen::Offline(Box::new(Offline {
                token,
                error: error.to_string().into(),
            })),
            Err(err) => {
                log::error!("session restore task failed: {err:?}");
                Self::login_screen(&self.client, window, cx)
            }
        };
        cx.notify();
    }

    fn login_screen(client: &StoatClient, window: &mut Window, cx: &mut Context<Self>) -> Screen {
        let view = cx.new(|cx| LoginView::new(client.clone(), window, cx));
        let events = cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            LoginEvent::LoggedIn(session) => this.logged_in(session.token.clone(), window, cx),
        });
        Screen::Login {
            view,
            _events: events,
        }
    }

    /// Fresh login: remember the token, then load the profile.
    fn logged_in(&mut self, token: String, window: &mut Window, cx: &mut Context<Self>) {
        let save = spawn_io(session::remember(
            self.store.clone(),
            self.client.base_url().to_string(),
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

        let client = self.client.with_token(token.clone());
        let request = spawn_io(async move { client.fetch_self().await });
        let fetch = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if let Screen::SignedIn(state) = &mut this.screen {
                    match result {
                        Ok(Ok(me)) => state.user = Some(me),
                        Ok(Err(err)) => {
                            log::warn!("fetching /users/@me failed: {err:?}");
                            state.error = Some(err.to_string().into());
                        }
                        Err(err) => state.error = Some(err.to_string().into()),
                    }
                    cx.notify();
                }
            })
            .ok();
        });

        self.screen = Screen::SignedIn(Box::new(SignedIn {
            token,
            user: None,
            error: None,
            _fetch: Some(fetch),
        }));
        cx.notify();
    }

    fn retry_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.screen = Self::restoring_screen(&self.client, &self.store, window, cx);
        cx.notify();
    }

    /// Forget the saved session, end it on the server, and show the login screen.
    pub fn log_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = match &self.screen {
            Screen::SignedIn(state) => state.token.clone(),
            Screen::Offline(state) => state.token.clone(),
            Screen::Restoring { .. } | Screen::Login { .. } => return,
        };

        let request = spawn_io(session::log_out(
            self.store.clone(),
            self.client.with_token(token),
        ));
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

        self.screen = Self::login_screen(&self.client, window, cx);
        cx.notify();
    }
}

/// "Display Name (username#1234)", or just "username#1234".
fn user_label(user: &User) -> String {
    let tag = format!("{}#{}", user.username, user.discriminator);
    match &user.display_name {
        Some(display) if !display.is_empty() => format!("{display} ({tag})"),
        _ => tag,
    }
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (status, detail, actions): (SharedString, Option<SharedString>, Option<Div>) =
            match &self.screen {
                Screen::Login { view, .. } => return view.clone().into_any_element(),
                Screen::Restoring { .. } => ("Signing in…".into(), None, None),
                Screen::Offline(state) => (
                    "Couldn't check your saved session".into(),
                    Some(state.error.clone()),
                    Some(
                        h_flex()
                            .gap_2()
                            .child(Button::new("retry").primary().label("Retry").on_click(
                                cx.listener(|this, _, window, cx| this.retry_restore(window, cx)),
                            ))
                            .child(log_out_button(cx)),
                    ),
                ),
                Screen::SignedIn(state) => {
                    let status = match (&state.user, &state.error) {
                        (Some(user), _) => format!("Logged in as {}", user_label(user)).into(),
                        (None, Some(error)) => error.clone(),
                        (None, None) => "Loading your profile…".into(),
                    };
                    (status, None, Some(h_flex().child(log_out_button(cx))))
                }
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

fn log_out_button(cx: &mut Context<AppRoot>) -> Button {
    Button::new("log-out")
        .ghost()
        .label("Log out")
        .on_click(cx.listener(|this, _, window, cx| this.log_out(window, cx)))
}

#[cfg(test)]
mod tests {
    // Explicit imports: `use super::*` would bring in GPUI's own `test` macro
    // through the `gpui_kit::*` glob and shadow Rust's `#[test]`.
    use super::user_label;
    use crate::api::models::User;

    fn user(display_name: Option<&str>) -> User {
        serde_json::from_value(serde_json::json!({
            "_id": "01JC0USER000000000000000AA",
            "username": "stoatfan",
            "discriminator": "1234",
            "display_name": display_name,
            "relationship": "User",
            "online": true,
        }))
        .unwrap()
    }

    #[test]
    fn label_prefers_display_name() {
        assert_eq!(
            user_label(&user(Some("Stoat Fan"))),
            "Stoat Fan (stoatfan#1234)"
        );
    }

    #[test]
    fn label_falls_back_to_username_tag() {
        assert_eq!(user_label(&user(None)), "stoatfan#1234");
        assert_eq!(user_label(&user(Some(""))), "stoatfan#1234");
    }
}
