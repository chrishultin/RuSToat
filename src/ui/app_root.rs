use gpui_kit::component::{ActiveTheme, Theme, v_flex};
use gpui_kit::*;

use crate::api::models::{Session, User};
use crate::api::{ApiError, StoatClient};
use crate::runtime::spawn_io;
use crate::ui::login::{LoginEvent, LoginView};

/// What the main window is showing.
enum Screen {
    Login {
        view: Entity<LoginView>,
        _events: Subscription,
    },
    SignedIn(Box<SignedIn>),
}

/// Placeholder until the chat UI lands. Shows who we're logged in as.
struct SignedIn {
    session: Session,
    user: Option<User>,
    error: Option<SharedString>,
    _fetch: Task<()>,
}

/// Top-level view for the main window: routes between login and chat.
pub struct AppRoot {
    client: StoatClient,
    screen: Screen,
    _appearance: Subscription,
}

impl AppRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Follow the macOS light/dark setting, now and when it changes.
        Theme::sync_system_appearance(Some(window), cx);
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
        });

        let client = StoatClient::from_env();
        log::info!("using Stoat API at {}", client.base_url());
        let screen = Self::login_screen(&client, window, cx);

        Self {
            client,
            screen,
            _appearance: appearance,
        }
    }

    fn login_screen(client: &StoatClient, window: &mut Window, cx: &mut Context<Self>) -> Screen {
        let view = cx.new(|cx| LoginView::new(client.clone(), window, cx));
        let events = cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            LoginEvent::LoggedIn(session) => this.signed_in(session.clone(), window, cx),
        });
        Screen::Login {
            view,
            _events: events,
        }
    }

    fn signed_in(&mut self, session: Session, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.client.with_token(session.token.clone());
        let request = spawn_io(async move { client.fetch_self().await });
        let fetch = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if let Screen::SignedIn(state) = &mut this.screen {
                    match result {
                        Ok(Ok(me)) => state.user = Some(me),
                        Ok(Err(err)) => state.error = Some(describe_fetch_error(&err)),
                        Err(err) => state.error = Some(format!("{err}").into()),
                    }
                    cx.notify();
                }
            })
            .ok();
        });

        self.screen = Screen::SignedIn(Box::new(SignedIn {
            session,
            user: None,
            error: None,
            _fetch: fetch,
        }));
        cx.notify();
    }
}

fn describe_fetch_error(err: &ApiError) -> SharedString {
    log::warn!("fetching /users/@me failed: {err:?}");
    err.to_string().into()
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
        match &self.screen {
            Screen::Login { view, .. } => view.clone().into_any_element(),
            Screen::SignedIn(state) => {
                let theme = cx.theme();
                let status: SharedString = match (&state.user, &state.error) {
                    (Some(user), _) => format!("Logged in as {}", user_label(user)).into(),
                    (None, Some(error)) => error.clone(),
                    (None, None) => "Loading your profile…".into(),
                };
                v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .bg(theme.background)
                    .text_color(theme.foreground)
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::BOLD)
                            .child("Stoat"),
                    )
                    .child(div().text_lg().child(status))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(format!("Session: {}", state.session.name)),
                    )
                    .into_any_element()
            }
        }
    }
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
