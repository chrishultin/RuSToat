//! "Confirm it's you": the dialog every protected action goes through
//! (logging sessions out, recovery codes, the authenticator app).
//!
//! It asks Stoat how the user may confirm (see
//! [`crate::security::confirm_methods`]), takes their password or code for
//! a single-use MFA ticket, and runs the action with it at once. Tickets
//! are used up even when the action fails, so a retry confirms again.

use futures::future::BoxFuture;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::{ApiError, StoatClient};
use crate::runtime::spawn_io;
use crate::security::{
    ConfirmError, ConfirmMethod, action_error_text, confirm_methods, mfa_response, ticket_error,
};

/// The dialog's width.
const CONFIRM_DIALOG_WIDTH: f32 = 400.;

/// What a protected action says about itself.
pub struct ConfirmRequest {
    /// The dialog's title: "Log out this session?"
    pub title: SharedString,
    /// What will happen, above the field.
    pub detail: SharedString,
    /// The confirm button: "Log out".
    pub confirm_label: SharedString,
    /// The toast when the action itself fails for no reason Stoat gives.
    pub failed_text: &'static str,
}

/// The action, given the client and a ticket's token.
type Action<T> = Box<dyn FnOnce(StoatClient, String) -> BoxFuture<'static, Result<T, ApiError>>>;
/// What to do with the action's result, once the dialog has closed.
type Done<T> = Box<dyn FnOnce(T, &mut Window, &mut App)>;

/// Confirm it's the user, then run `action` with a fresh ticket and hand
/// its result to `done`. The dialog opens in `window`.
pub fn confirm_then<T: Send + 'static>(
    client: StoatClient,
    request: ConfirmRequest,
    action: impl FnOnce(StoatClient, String) -> BoxFuture<'static, Result<T, ApiError>> + 'static,
    done: impl FnOnce(T, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let title = request.title.clone();
    let dialog = cx.new(|cx| {
        ConfirmDialog::new(
            client,
            request,
            Box::new(action),
            Box::new(done),
            window,
            cx,
        )
    });
    window.open_dialog(cx, move |modal, _, _| {
        modal
            .title(title.clone())
            .w(px(CONFIRM_DIALOG_WIDTH))
            .child(dialog.clone())
    });
}

/// Asking Stoat how the user may confirm.
enum Methods {
    Loading { _task: Task<()> },
    Loaded(Vec<ConfirmMethod>),
    Failed(String),
}

pub struct ConfirmDialog<T: 'static> {
    client: StoatClient,
    request: ConfirmRequest,
    methods: Methods,
    /// The way being asked for.
    method: ConfirmMethod,
    input: Entity<InputState>,
    /// Why the last try was refused, under the field.
    error: Option<String>,
    busy: bool,
    action: Option<Action<T>>,
    done: Option<Done<T>>,
    _input: Subscription,
}

impl<T: Send + 'static> ConfirmDialog<T> {
    fn new(
        client: StoatClient,
        request: ConfirmRequest,
        action: Action<T>,
        done: Done<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).masked(true));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                _ => {}
            });
        let mut this = Self {
            client,
            request,
            methods: Methods::Failed(String::new()),
            method: ConfirmMethod::Password,
            input,
            error: None,
            busy: false,
            action: Some(action),
            done: Some(done),
            _input: subscription,
        };
        this.load_methods(window, cx);
        this
    }

    /// Ask Stoat how the user may confirm, then ask for the first way.
    fn load_methods(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = spawn_io({
            let client = self.client.clone();
            async move { client.mfa_status().await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(Ok(status)) => {
                        let methods = confirm_methods(&status);
                        this.set_method(methods[0], window, cx);
                        this.methods = Methods::Loaded(methods);
                        let focus = this.input.read(cx).focus_handle(cx);
                        window.focus(&focus, cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't load the MFA status: {err:?}");
                        this.methods = Methods::Failed(action_error_text(
                            &err,
                            "Couldn't check how to confirm it's you.",
                        ));
                    }
                    Err(err) => {
                        log::warn!("MFA status task failed: {err:?}");
                        this.methods =
                            Methods::Failed("Couldn't check how to confirm it's you.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        });
        self.methods = Methods::Loading { _task: task };
        cx.notify();
    }

    /// Ask for `method`: the field is emptied, and masked for a password.
    fn set_method(&mut self, method: ConfirmMethod, window: &mut Window, cx: &mut Context<Self>) {
        self.method = method;
        self.error = None;
        self.input.update(cx, |input, cx| {
            input.set_masked(method == ConfirmMethod::Password, window, cx);
            input.set_placeholder(method.placeholder(), window, cx);
            input.set_value("", window, cx);
        });
        let focus = self.input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Take what's typed for a ticket, and run the action with it.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !matches!(self.methods, Methods::Loaded(_)) {
            return;
        }
        let typed = self.input.read(cx).value().to_string();
        let Some(response) = mfa_response(self.method, &typed) else {
            return;
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        let client = self.client.clone();
        let ticket = spawn_io({
            let client = client.clone();
            async move { client.create_mfa_ticket(&response).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let ticket = match ticket.await {
                Ok(Ok(ticket)) => ticket,
                Ok(Err(err)) => {
                    this.update_in(cx, |this, window, cx| this.refused(&err, window, cx))
                        .ok();
                    return;
                }
                Err(err) => {
                    log::warn!("ticket task failed: {err:?}");
                    this.update_in(cx, |this, window, cx| {
                        this.fail("Couldn't confirm it's you. Try again.".into(), window, cx)
                    })
                    .ok();
                    return;
                }
            };
            let Ok(Some(action)) = this.update(cx, |this, _| this.action.take()) else {
                return;
            };
            // The ticket's token is never logged.
            let result = spawn_io(action(client, ticket.token)).await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(value)) => {
                    this.busy = false;
                    window.close_dialog(cx);
                    if let Some(done) = this.done.take() {
                        done(value, window, cx);
                    }
                }
                Ok(Err(err)) => {
                    log::warn!("protected action failed: {err:?}");
                    let text = action_error_text(&err, this.request.failed_text);
                    this.fail(text, window, cx);
                }
                Err(err) => {
                    log::warn!("protected action task failed: {err:?}");
                    this.fail(this.request.failed_text.into(), window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Stoat wouldn't make a ticket: ask again, or give up.
    fn refused(&mut self, err: &ApiError, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = false;
        match ticket_error(err) {
            ConfirmError::Retry(text) => {
                self.error = Some(text);
                let focus = self.input.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            ConfirmError::Methods => self.load_methods(window, cx),
            ConfirmError::Fail(text) => {
                log::warn!("couldn't make an MFA ticket: {err:?}");
                self.fail(text, window, cx);
            }
        }
        cx.notify();
    }

    /// Close, and say why in a toast.
    fn fail(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = false;
        window.close_dialog(cx);
        window.push_notification(Notification::error(text), cx);
    }
}

impl<T: Send + 'static> Render for ConfirmDialog<T> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let body = match &self.methods {
            Methods::Loading { .. } => div()
                .text_sm()
                .text_color(muted)
                .child("Checking how to confirm it's you…")
                .into_any_element(),
            Methods::Failed(text) => h_flex()
                .gap_2()
                .text_sm()
                .text_color(danger)
                .child(text.clone())
                .child(
                    Button::new("confirm-retry")
                        .label("Retry")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.load_methods(window, cx))),
                )
                .into_any_element(),
            Methods::Loaded(methods) => {
                let others: Vec<ConfirmMethod> = methods
                    .iter()
                    .copied()
                    .filter(|m| *m != self.method)
                    .collect();
                v_flex()
                    .gap_2()
                    .child(div().text_sm().child(self.method.prompt()))
                    .child(Input::new(&self.input).disabled(self.busy))
                    .children(
                        self.error
                            .clone()
                            .map(|error| div().text_xs().text_color(danger).child(error)),
                    )
                    .children(others.into_iter().map(|method| {
                        Button::new(SharedString::from(format!("confirm-use-{method:?}")))
                            .label(method.switch_label())
                            .link()
                            .small()
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_method(method, window, cx)
                            }))
                    }))
                    .into_any_element()
            }
        };
        let typed = mfa_response(self.method, &self.input.read(cx).value()).is_some();
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.request.detail.clone()),
            )
            .child(body)
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("confirm-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("confirm-ok")
                            .label(self.request.confirm_label.clone())
                            .primary()
                            .loading(self.busy)
                            .disabled(
                                self.busy || !matches!(self.methods, Methods::Loaded(_)) || !typed,
                            )
                            .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
                    ),
            )
    }
}
