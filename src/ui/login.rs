//! Email/password login, with a second step for two-factor codes.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::models::{MFAMethod, MFAResponse, ResponseLogin, Session};
use crate::api::{ApiError, StoatClient};
use crate::runtime::{IoTask, spawn_io};

/// Emitted once the server returns a session token.
pub enum LoginEvent {
    LoggedIn(Session),
}

enum Step {
    Credentials,
    /// The account has 2FA. `ticket` identifies this login attempt.
    Mfa {
        ticket: String,
        allow_totp: bool,
        allow_recovery: bool,
        use_recovery: bool,
    },
}

pub struct LoginView {
    client: StoatClient,
    email: Entity<InputState>,
    password: Entity<InputState>,
    code: Entity<InputState>,
    step: Step,
    error: Option<SharedString>,
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LoginEvent> for LoginView {}

impl LoginView {
    pub fn new(client: StoatClient, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("you@example.com"));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
        });
        let code = cx.new(|cx| InputState::new(window, cx).placeholder("123456"));

        let subscriptions = vec![
            // Enter in the email field moves on to the password field.
            cx.subscribe_in(&email, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.password
                        .update(cx, |input, cx| input.focus(window, cx));
                }
            }),
            cx.subscribe_in(
                &password,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit(window, cx);
                    }
                },
            ),
            cx.subscribe_in(&code, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit(window, cx);
                }
            }),
        ];

        email.update(cx, |input, cx| input.focus(window, cx));

        Self {
            client,
            email,
            password,
            code,
            step: Step::Credentials,
            error: None,
            pending: None,
            _subscriptions: subscriptions,
        }
    }

    fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }

        let request = match &self.step {
            Step::Credentials => {
                let email = self.email.read(cx).value().trim().to_string();
                let password = self.password.read(cx).value().to_string();
                if email.is_empty() || password.is_empty() {
                    self.show_error("Enter your email and password.", cx);
                    return;
                }
                let client = self.client.clone();
                spawn_io(async move { client.login(&email, &password).await })
            }
            Step::Mfa {
                ticket,
                use_recovery,
                ..
            } => {
                let code = self.code.read(cx).value().trim().to_string();
                if code.is_empty() {
                    self.show_error("Enter your code.", cx);
                    return;
                }
                let response = if *use_recovery {
                    MFAResponse::Recovery {
                        recovery_code: code,
                    }
                } else {
                    // Authenticator apps often show "123 456"; the API wants digits only.
                    MFAResponse::Totp {
                        totp_code: code.replace(' ', ""),
                    }
                };
                let client = self.client.clone();
                let ticket = ticket.clone();
                spawn_io(async move { client.login_mfa(&ticket, response).await })
            }
        };

        self.run(request, window, cx);
    }

    fn run(
        &mut self,
        request: IoTask<Result<ResponseLogin, ApiError>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        self.pending = Some(cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.pending = None;
                this.handle_result(result, window, cx);
            })
            .ok();
        }));
        cx.notify();
    }

    fn handle_result(
        &mut self,
        result: anyhow::Result<Result<ResponseLogin, ApiError>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Ok(ResponseLogin::Success(session))) => {
                log::info!("logged in as user {}", session.user_id);
                cx.emit(LoginEvent::LoggedIn(session));
            }
            Ok(Ok(ResponseLogin::MFA {
                ticket,
                allowed_methods,
            })) => {
                let allow_totp = allowed_methods.iter().any(|m| matches!(m, MFAMethod::Totp));
                let allow_recovery = allowed_methods
                    .iter()
                    .any(|m| matches!(m, MFAMethod::Recovery));
                if !allow_totp && !allow_recovery {
                    self.error = Some(
                        "This account's two-factor method isn't supported by this client yet."
                            .into(),
                    );
                } else {
                    log::info!("login needs MFA ({allowed_methods:?})");
                    self.step = Step::Mfa {
                        ticket,
                        allow_totp,
                        allow_recovery,
                        use_recovery: !allow_totp,
                    };
                    self.reset_code_input(!allow_totp, window, cx);
                }
            }
            Ok(Ok(ResponseLogin::Disabled { .. })) => {
                self.error = Some("This account has been disabled.".into());
            }
            Ok(Err(err)) => {
                log::warn!("login failed: {err:?}");
                self.error = Some(err.to_string().into());
            }
            Err(err) => {
                log::error!("login task failed: {err:?}");
                self.error = Some("Something went wrong. Please try again.".into());
            }
        }
        cx.notify();
    }

    fn reset_code_input(&self, recovery: bool, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = if recovery { "xxxx-xxxx" } else { "123456" };
        self.code.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_placeholder(placeholder, window, cx);
            input.focus(window, cx);
        });
    }

    fn toggle_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Step::Mfa { use_recovery, .. } = &mut self.step {
            *use_recovery = !*use_recovery;
            let recovery = *use_recovery;
            self.error = None;
            self.reset_code_input(recovery, window, cx);
            cx.notify();
        }
    }

    fn back_to_credentials(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending = None;
        self.step = Step::Credentials;
        self.error = None;
        self.password
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn show_error(&mut self, message: &'static str, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }
}

fn field_label(text: &'static str, cx: &App) -> Div {
    div()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

impl Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.is_busy();
        let theme = cx.theme();

        let (title, subtitle, submit_label) = match &self.step {
            Step::Credentials => ("Log in to Stoat", "Use your Stoat account.", "Log in"),
            Step::Mfa {
                use_recovery: false,
                ..
            } => (
                "Two-factor authentication",
                "Enter the 6-digit code from your authenticator app.",
                "Verify",
            ),
            Step::Mfa {
                use_recovery: true, ..
            } => (
                "Two-factor authentication",
                "Enter one of your recovery codes.",
                "Verify",
            ),
        };

        let fields = match &self.step {
            Step::Credentials => v_flex()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(field_label("Email", cx))
                        .child(Input::new(&self.email).disabled(busy)),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(field_label("Password", cx))
                        .child(Input::new(&self.password).mask_toggle().disabled(busy)),
                ),
            Step::Mfa {
                allow_totp,
                allow_recovery,
                use_recovery,
                ..
            } => {
                let label = if *use_recovery {
                    "Recovery code"
                } else {
                    "Authentication code"
                };
                let can_toggle = *allow_totp && *allow_recovery;
                v_flex()
                    .gap_1()
                    .child(field_label(label, cx))
                    .child(Input::new(&self.code).disabled(busy))
                    .when(can_toggle, |el| {
                        let toggle_label = if *use_recovery {
                            "Use authenticator app instead"
                        } else {
                            "Use a recovery code instead"
                        };
                        el.child(
                            Button::new("toggle-recovery")
                                .link()
                                .label(toggle_label)
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_recovery(window, cx)
                                })),
                        )
                    })
            }
        };

        let is_mfa = matches!(self.step, Step::Mfa { .. });

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                v_flex()
                    .w(px(380.))
                    .gap_4()
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xl().font_weight(FontWeight::BOLD).child(title))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(subtitle),
                            ),
                    )
                    .child(fields)
                    .when_some(self.error.clone(), |el, error| {
                        el.child(div().text_sm().text_color(theme.danger).child(error))
                    })
                    .child(
                        Button::new("submit")
                            .primary()
                            .w_full()
                            .label(submit_label)
                            .loading(busy)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    )
                    .when(is_mfa, |el| {
                        el.child(Button::new("back").ghost().w_full().label("Back").on_click(
                            cx.listener(|this, _, window, cx| this.back_to_credentials(window, cx)),
                        ))
                    }),
            )
    }
}
