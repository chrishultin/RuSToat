//! Acting on a server's members: a confirmation that runs one request
//! (Kick, End timeout), "Ban", with a reason and how much of their history
//! goes, and "Time out", for how long.
//! The options and error texts are in [`crate::moderation`]; who may do
//! what is [`crate::permissions::moderation`].

use futures::future::BoxFuture;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{ActiveTheme, Disableable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::models::DataBanCreate;
use crate::api::{ApiError, StoatClient};
use crate::moderation::{
    BAN_HISTORY, BAN_REASON_MAX, TIMEOUTS, moderation_error_text, timeout_edit, timeout_until,
};
use crate::runtime::spawn_io;
use crate::ui::server_dialogs::field_label;

/// These dialogs' width.
pub const MODERATION_DIALOG_WIDTH: f32 = 420.;

/// A request to Stoat about a member, ready to send.
pub type MemberRequest = BoxFuture<'static, Result<(), ApiError>>;

/// What a confirmed member action says about itself.
pub struct MemberActionRequest {
    /// The dialog's title: "Kick Ada?"
    pub title: SharedString,
    /// What will happen.
    pub detail: SharedString,
    /// The confirm button: "Kick".
    pub ok_label: &'static str,
    /// The button is drawn as destructive.
    pub danger: bool,
    /// The toast when Stoat refuses for no reason it gives.
    pub failed_text: String,
}

/// Ask `request`'s question, and send `action` once it's confirmed. The
/// dialog waits, busy, for Stoat's answer, then closes; a refusal toasts.
pub fn confirm_member_action(
    request: MemberActionRequest,
    action: MemberRequest,
    window: &mut Window,
    cx: &mut App,
) {
    let title = request.title.clone();
    let dialog = cx.new(|_| ConfirmMemberAction {
        request,
        action: Some(action),
        busy: false,
    });
    window.open_dialog(cx, move |modal, _, _| {
        modal
            .title(title.clone())
            .w(px(MODERATION_DIALOG_WIDTH))
            .child(dialog.clone())
    });
}

/// Send `action` without asking (Shift was held); a refusal toasts.
pub fn run_member_action<V: 'static>(
    action: MemberRequest,
    failed_text: String,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let request = spawn_io(action);
    cx.spawn_in(window, async move |_, cx| {
        let result = request.await;
        cx.update(|window, cx| {
            if let Some(text) = refusal_text(result, &failed_text) {
                window.push_notification(Notification::error(text), cx);
            }
        })
        .ok();
    })
    .detach();
}

/// What a toast says about a member action's outcome; `None` if it went
/// through.
fn refusal_text(
    result: Result<Result<(), ApiError>, impl std::fmt::Debug>,
    failed_text: &str,
) -> Option<String> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(err)) => {
            log::warn!("member action refused: {err:?}");
            Some(moderation_error_text(&err, failed_text))
        }
        Err(err) => {
            log::warn!("member action task failed: {err:?}");
            Some("Couldn't reach Stoat. Try again.".into())
        }
    }
}

/// The confirmation's content (see [`confirm_member_action`]).
pub struct ConfirmMemberAction {
    request: MemberActionRequest,
    action: Option<MemberRequest>,
    busy: bool,
}

impl ConfirmMemberAction {
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.action.take() else {
            return;
        };
        self.busy = true;
        cx.notify();
        let request = spawn_io(action);
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                window.close_dialog(cx);
                if let Some(text) = refusal_text(result, &this.request.failed_text) {
                    window.push_notification(Notification::error(text), cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Render for ConfirmMemberAction {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.request.detail.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("member-action-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("member-action-ok")
                            .with_variant(if self.request.danger {
                                ButtonVariant::Danger
                            } else {
                                ButtonVariant::Primary
                            })
                            .label(self.request.ok_label)
                            .loading(self.busy)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
                    ),
            )
    }
}

/// Ban `user` (called `name`) from `server` (called `server_name`): a
/// dialog asks for a reason and how much of their history goes with them.
pub fn open_ban_dialog(
    client: StoatClient,
    server: String,
    server_name: &str,
    user: String,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let title: SharedString = format!("Ban {name}").into();
    let detail: SharedString = format!(
        "{name} will be removed from {server_name} and won't be able to rejoin unless \
         they're unbanned."
    )
    .into();
    let dialog = cx.new(|cx| BanDialog::new(client, server, user, detail, window, cx));
    let first_build = std::cell::Cell::new(true);
    window.open_dialog(cx, move |modal, window, cx| {
        if first_build.replace(false) {
            let input = dialog.read(cx).reason.read(cx).focus_handle(cx);
            window.focus(&input, cx);
        }
        modal
            .title(title.clone())
            .w(px(MODERATION_DIALOG_WIDTH))
            .child(dialog.clone())
    });
}

/// The Ban dialog's content (see [`open_ban_dialog`]).
pub struct BanDialog {
    client: StoatClient,
    server: String,
    user: String,
    detail: SharedString,
    reason: Entity<TextareaState>,
    /// The chosen [`BAN_HISTORY`] option.
    history: usize,
    busy: bool,
    _reason: Subscription,
}

impl BanDialog {
    fn new(
        client: StoatClient,
        server: String,
        user: String,
        detail: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let reason = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 5)
                .placeholder("Reason (optional)")
        });
        let subscription = cx.observe(&reason, |_, _, cx| cx.notify());
        Self {
            client,
            server,
            user,
            detail,
            reason,
            history: 0,
            busy: false,
            _reason: subscription,
        }
    }

    /// The typed reason, if it's short enough: `Some(None)` for none.
    fn reason(&self, cx: &App) -> Option<Option<String>> {
        let text = self.reason.read(cx).value().trim().to_string();
        (text.chars().count() <= BAN_REASON_MAX).then(|| Some(text).filter(|t| !t.is_empty()))
    }

    /// Send the ban. Success closes the dialog (`ServerMemberLeave` takes
    /// them out of the member list); a refusal toasts and keeps it.
    fn ban(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(reason) = self.reason(cx) else {
            return;
        };
        let seconds = BAN_HISTORY[self.history].1;
        let ban = DataBanCreate {
            reason,
            delete_message_seconds: (seconds > 0).then_some(seconds),
        };
        self.busy = true;
        cx.notify();
        let request = spawn_io({
            let (client, server, user) =
                (self.client.clone(), self.server.clone(), self.user.clone());
            async move { client.ban_user(&server, &user, &ban).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                let error = match result {
                    Ok(Ok(_)) => {
                        log::info!("banned {} from {}", this.user, this.server);
                        window.close_dialog(cx);
                        None
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't ban {} from {}: {err:?}", this.user, this.server);
                        Some(moderation_error_text(&err, "Couldn't ban them. Try again."))
                    }
                    Err(err) => {
                        log::warn!("ban task failed: {err:?}");
                        Some("Couldn't reach Stoat. Try again.".into())
                    }
                };
                if let Some(error) = error {
                    window.push_notification(Notification::error(error), cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for BanDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let count = self.reason.read(cx).value().trim().chars().count();
        let valid = count <= BAN_REASON_MAX;
        let entity = cx.entity().downgrade();
        v_flex()
            .gap_3()
            .child(div().text_sm().text_color(muted).child(self.detail.clone()))
            .child(
                v_flex()
                    .gap_1()
                    .child(field_label("Reason", cx))
                    .child(Textarea::new(&self.reason).disabled(self.busy))
                    .child(
                        div()
                            .text_xs()
                            .text_right()
                            .text_color(if valid { muted } else { danger })
                            .child(format!("{count}/{BAN_REASON_MAX}")),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(field_label("Delete message history", cx))
                    .child(
                        RadioGroup::vertical("ban-history")
                            .flex_none()
                            .children(BAN_HISTORY.iter().map(|(label, _)| {
                                Radio::new(SharedString::from(*label)).label(*label)
                            }))
                            .selected_index(Some(self.history))
                            .disabled(self.busy)
                            .on_change(cx.listener(|this, index: &usize, _, cx| {
                                this.history = *index;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("ban-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("ban-ok")
                            .with_variant(ButtonVariant::Danger)
                            .label("Ban")
                            .loading(self.busy)
                            .disabled(self.busy || !valid)
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| this.ban(window, cx)).ok();
                            }),
                    ),
            )
    }
}

/// Time `user` (called `name`) out on `server`: a dialog asks for how long.
pub fn open_timeout_dialog(
    client: StoatClient,
    server: String,
    user: String,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let title: SharedString = format!("Time out {name}").into();
    let detail: SharedString = format!(
        "While timed out, {name} can read the server but can't send messages, react or join \
         calls."
    )
    .into();
    let dialog = cx.new(|_| TimeoutDialog {
        client,
        server,
        user,
        detail,
        length: 0,
        busy: false,
    });
    window.open_dialog(cx, move |modal, _, _| {
        modal
            .title(title.clone())
            .w(px(MODERATION_DIALOG_WIDTH))
            .child(dialog.clone())
    });
}

/// The Time out dialog's content (see [`open_timeout_dialog`]).
pub struct TimeoutDialog {
    client: StoatClient,
    server: String,
    user: String,
    detail: SharedString,
    /// The chosen [`TIMEOUTS`] option.
    length: usize,
    busy: bool,
}

impl TimeoutDialog {
    /// Send the timeout, ending the chosen length from now. Success closes
    /// the dialog (`ServerMemberUpdate` brings the timeout); a refusal
    /// toasts and keeps it.
    fn time_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let until = timeout_until(std::time::SystemTime::now(), TIMEOUTS[self.length].1);
        let edit = timeout_edit(until);
        self.busy = true;
        cx.notify();
        let request = spawn_io({
            let (client, server, user) =
                (self.client.clone(), self.server.clone(), self.user.clone());
            async move { client.edit_member(&server, &user, &edit).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                let error = match result {
                    Ok(Ok(_)) => {
                        log::info!("timed out {} on {}", this.user, this.server);
                        window.close_dialog(cx);
                        None
                    }
                    Ok(Err(err)) => {
                        log::warn!(
                            "couldn't time out {} on {}: {err:?}",
                            this.user,
                            this.server
                        );
                        Some(moderation_error_text(
                            &err,
                            "Couldn't time them out. Try again.",
                        ))
                    }
                    Err(err) => {
                        log::warn!("timeout task failed: {err:?}");
                        Some("Couldn't reach Stoat. Try again.".into())
                    }
                };
                if let Some(error) = error {
                    window.push_notification(Notification::error(error), cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for TimeoutDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.detail.clone()),
            )
            .child(
                v_flex().gap_1().child(field_label("Duration", cx)).child(
                    RadioGroup::vertical("timeout-length")
                        .flex_none()
                        .children(
                            TIMEOUTS.iter().map(|(label, _)| {
                                Radio::new(SharedString::from(*label)).label(*label)
                            }),
                        )
                        .selected_index(Some(self.length))
                        .disabled(self.busy)
                        .on_change(cx.listener(|this, index: &usize, _, cx| {
                            this.length = *index;
                            cx.notify();
                        })),
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("timeout-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("timeout-ok")
                            .with_variant(ButtonVariant::Danger)
                            .label("Time out")
                            .loading(self.busy)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.time_out(window, cx))),
                    ),
            )
    }
}
