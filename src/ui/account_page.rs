//! The Account page, for one account: two-factor authentication, meaning
//! the authenticator app and recovery codes. Every change confirms it's the
//! user first (see [`crate::ui::confirm_dialog`]).
//!
//! The settings window keeps one per account, as it does the other
//! account pages.

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use image::Frame;

use crate::api::StoatClient;
use crate::api::models::MultiFactorStatus;
use crate::runtime::spawn_io;
use crate::security::{
    QR_QUIET_ZONE, RecoveryAction, check_totp_code, enable_totp_error_text, otpauth_uri, qr_image,
    qr_modules, recovery_actions, recovery_codes_text, secret_groups,
};
use crate::ui::chat::ChatView;
use crate::ui::confirm_dialog::{ConfirmRequest, confirm_then};

/// The recovery codes dialog's width.
const CODES_DIALOG_WIDTH: f32 = 400.;

/// Where loading the two-factor status is at.
enum Load {
    Loading { _task: Task<()> },
    Loaded(MultiFactorStatus),
    Failed,
}

/// One account's Account page.
pub struct AccountSecurityView {
    /// The account's chat: its client and store.
    chat: WeakEntity<ChatView>,
    status: Load,
}

impl AccountSecurityView {
    pub fn new(chat: Entity<ChatView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            chat: chat.downgrade(),
            status: Load::Failed,
        };
        this.refresh(window, cx);
        this
    }

    /// Whether this is the page for `chat` (an account signed in again has
    /// a new one).
    pub fn is_for(&self, chat: &Entity<ChatView>) -> bool {
        self.chat.entity_id() == chat.entity_id()
    }

    /// Fetch the two-factor status again.
    pub fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let client = chat.read(cx).client().clone();
            async move { client.mfa_status().await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(Ok(status)) => Load::Loaded(status),
                    Ok(Err(err)) => {
                        log::warn!("couldn't load the MFA status: {err}");
                        Load::Failed
                    }
                    Err(err) => {
                        log::warn!("MFA status task failed: {err:?}");
                        Load::Failed
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.status = Load::Loading { _task: task };
        cx.notify();
    }

    /// `@username#1234`, for the line saying which account this is.
    fn handle(&self, cx: &App) -> Option<String> {
        let chat = self.chat.upgrade()?;
        let store = chat.read(cx).store();
        let user = store.user(store.me())?;
        Some(format!("@{}#{}", user.username, user.discriminator))
    }

    /// Generate, view or regenerate recovery codes, after confirming it's
    /// the user, then show them.
    fn recovery_codes(
        &mut self,
        action: RecoveryAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let client = chat.read(cx).client().clone();
        let request = match action {
            RecoveryAction::Generate => ConfirmRequest {
                title: "Generate recovery codes?".into(),
                detail: "Each code signs you in once if you lose your authenticator app.".into(),
                confirm_label: "Generate".into(),
                failed_text: "Couldn't generate recovery codes.",
            },
            RecoveryAction::View => ConfirmRequest {
                title: "View recovery codes?".into(),
                detail: "Confirm it's you to see your recovery codes.".into(),
                confirm_label: "View".into(),
                failed_text: "Couldn't load your recovery codes.",
            },
            RecoveryAction::Regenerate => ConfirmRequest {
                title: "Regenerate recovery codes?".into(),
                detail: "Your current recovery codes will stop working.".into(),
                confirm_label: "Regenerate".into(),
                failed_text: "Couldn't regenerate recovery codes.",
            },
        };
        let view = cx.entity().downgrade();
        confirm_then(
            client,
            request,
            move |client, ticket| {
                Box::pin(async move {
                    match action {
                        RecoveryAction::View => client.fetch_recovery_codes(&ticket).await,
                        RecoveryAction::Generate | RecoveryAction::Regenerate => {
                            client.generate_recovery_codes(&ticket).await
                        }
                    }
                })
            },
            move |codes, window, cx| {
                log::debug!("showing {} recovery codes", codes.len());
                if action != RecoveryAction::View {
                    view.update(cx, |this, cx| this.refresh(window, cx)).ok();
                }
                open_recovery_codes(codes, view, window, cx);
            },
            window,
            cx,
        );
    }

    fn render_recovery(&self, status: &MultiFactorStatus, cx: &mut Context<Self>) -> AnyElement {
        let buttons = recovery_actions(status).into_iter().map(|action| {
            let label = match action {
                RecoveryAction::Generate => "Generate recovery codes",
                RecoveryAction::View => "View",
                RecoveryAction::Regenerate => "Regenerate",
            };
            Button::new(SharedString::from(format!("recovery-{action:?}")))
                .label(label)
                .outline()
                .small()
                .on_click(
                    cx.listener(move |this, _, window, cx| this.recovery_codes(action, window, cx)),
                )
        });
        row(
            "Recovery codes",
            if status.recovery_active {
                "Each code signs you in once if you lose your authenticator app."
            } else {
                "None yet. Generate some to sign in if you lose your authenticator app."
            },
            h_flex().gap_2().children(buttons).into_any_element(),
            cx,
        )
    }

    fn render_authenticator(
        &self,
        status: &MultiFactorStatus,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button = if status.totp_mfa {
            Button::new("authenticator-remove")
                .label("Remove")
                .outline()
                .small()
                .on_click(cx.listener(|this, _, window, cx| this.remove_authenticator(window, cx)))
        } else {
            Button::new("authenticator-set-up")
                .label("Set up")
                .primary()
                .small()
                .on_click(cx.listener(|this, _, window, cx| this.set_up_authenticator(window, cx)))
        };
        row(
            "Authenticator app",
            if status.totp_mfa {
                "On: signing in asks for a code from your authenticator app."
            } else {
                "Off. Turn it on so signing in also asks for a code from your phone."
            },
            button.into_any_element(),
            cx,
        )
    }

    /// Confirm it's the user, get a new secret, and show it to scan.
    fn set_up_authenticator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let (client, username) = {
            let store = chat.read(cx).store();
            let username = store
                .user(store.me())
                .map(|u| u.username.clone())
                .unwrap_or_default();
            (chat.read(cx).client().clone(), username)
        };
        let recovery_active =
            matches!(&self.status, Load::Loaded(status) if status.recovery_active);
        let view = cx.entity().downgrade();
        let setup_client = client.clone();
        confirm_then(
            client,
            ConfirmRequest {
                title: "Set up an authenticator app?".into(),
                detail: "Confirm it's you, then scan a QR code with your authenticator app.".into(),
                confirm_label: "Continue".into(),
                failed_text: "Couldn't start setting up the authenticator app.",
            },
            |client, ticket| Box::pin(async move { client.create_totp_secret(&ticket).await }),
            move |secret, window, cx| {
                let setup = cx.new(|cx| {
                    TotpSetup::new(
                        setup_client,
                        view,
                        &username,
                        secret,
                        recovery_active,
                        window,
                        cx,
                    )
                });
                let focus = setup.read(cx).input.read(cx).focus_handle(cx);
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog
                        .title("Set up an authenticator app")
                        .w(px(TOTP_DIALOG_WIDTH))
                        .child(setup.clone())
                });
                window.focus(&focus, cx);
            },
            window,
            cx,
        );
    }

    /// Confirm it's the user (with an authenticator or recovery code), then
    /// turn the authenticator off.
    fn remove_authenticator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let client = chat.read(cx).client().clone();
        let view = cx.entity().downgrade();
        confirm_then(
            client,
            ConfirmRequest {
                title: "Remove the authenticator app?".into(),
                detail: "Signing in will only ask for your password. Your recovery codes stay."
                    .into(),
                confirm_label: "Remove".into(),
                failed_text: "Couldn't remove the authenticator app.",
            },
            |client, ticket| Box::pin(async move { client.disable_totp(&ticket).await }),
            move |(), window, cx| {
                view.update(cx, |this, cx| this.refresh(window, cx)).ok();
                window.push_notification(
                    Notification::success("The authenticator app is removed."),
                    cx,
                );
            },
            window,
            cx,
        );
    }
}

/// The authenticator setup dialog's width.
const TOTP_DIALOG_WIDTH: f32 = 420.;
/// Pixels per QR module in the image, and per module on screen: drawn at
/// half its pixel size, so it's crisp on a Retina display.
const QR_IMAGE_SCALE: u32 = 10;
const QR_SCREEN_SCALE: f32 = 5.;

/// "Set up an authenticator app": the QR code to scan, the secret to type
/// instead, and the first code, which turns it on.
pub struct TotpSetup {
    client: StoatClient,
    view: WeakEntity<AccountSecurityView>,
    secret: String,
    /// The QR code and its size on screen. `None` if it couldn't be made.
    qr: Option<(Arc<RenderImage>, f32)>,
    input: Entity<InputState>,
    error: Option<String>,
    busy: bool,
    /// Whether to suggest recovery codes once it's on.
    recovery_active: bool,
    _input: Subscription,
}

impl TotpSetup {
    fn new(
        client: StoatClient,
        view: WeakEntity<AccountSecurityView>,
        username: &str,
        secret: String,
        recovery_active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let qr = qr_modules(&otpauth_uri(username, &secret)).map(|modules| {
            let side = (modules.size + 2 * QR_QUIET_ZONE) as f32 * QR_SCREEN_SCALE;
            let image = qr_image(&modules, QR_IMAGE_SCALE);
            (Arc::new(RenderImage::new(vec![Frame::new(image)])), side)
        });
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("123456"));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.turn_on(window, cx),
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                _ => {}
            });
        Self {
            client,
            view,
            secret,
            qr,
            input,
            error: None,
            busy: false,
            recovery_active,
            _input: subscription,
        }
    }

    /// Send the typed code; it turns the authenticator on if it's right.
    fn turn_on(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(code) = check_totp_code(&self.input.read(cx).value()) else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let client = self.client.clone();
            async move { client.enable_totp(&code).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(Ok(())) => {
                        window.close_dialog(cx);
                        this.view
                            .update(cx, |view, cx| view.refresh(window, cx))
                            .ok();
                        let text = if this.recovery_active {
                            "The authenticator app is on."
                        } else {
                            "The authenticator app is on. Generate recovery codes too, in case you lose your device."
                        };
                        window.push_notification(Notification::success(text), cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't turn on TOTP: {err:?}");
                        this.error = Some(enable_totp_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("TOTP task failed: {err:?}");
                        this.error = Some("Couldn't turn on the authenticator app.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for TotpSetup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let ready = check_totp_code(&self.input.read(cx).value()).is_some();
        let secret = self.secret.clone();
        v_flex()
            .gap_3()
            .child(div().text_sm().child(
                "Scan this with your authenticator app (1Password, Google Authenticator, …), \
                 then enter the 6-digit code it shows.",
            ))
            .children(self.qr.clone().map(|(image, side)| {
                // Black on white in either theme, so cameras can read it.
                h_flex()
                    .w_full()
                    .justify_center()
                    .child(img(ImageSource::Render(image)).size(px(side)).flex_none())
            }))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Can't scan it? Enter this key instead:"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .font_family("Menlo")
                                    .text_sm()
                                    .child(secret_groups(&self.secret)),
                            )
                            .child(
                                Button::new("totp-copy-secret")
                                    .icon(IconName::Copy)
                                    .ghost()
                                    .small()
                                    .tooltip("Copy key")
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            secret.clone(),
                                        ));
                                        window.push_notification(
                                            Notification::success("Copied the key."),
                                            cx,
                                        );
                                    }),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(Input::new(&self.input).disabled(self.busy))
                    .children(
                        self.error
                            .clone()
                            .map(|error| div().text_xs().text_color(danger).child(error)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("totp-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("totp-turn-on")
                            .label("Turn on")
                            .primary()
                            .loading(self.busy)
                            .disabled(self.busy || !ready)
                            .on_click(cx.listener(|this, _, window, cx| this.turn_on(window, cx))),
                    ),
            )
    }
}

impl Render for AccountSecurityView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let account = match self.handle(cx) {
            Some(handle) => format!("Signed in as {handle}"),
            None => "Loading your account…".to_string(),
        };
        let header = h_flex()
            .w_full()
            .gap_2()
            .child(div().flex_1().text_sm().text_color(muted).child(account))
            .child(
                Button::new("account-refresh")
                    .icon(IconName::RefreshCw)
                    .ghost()
                    .small()
                    .tooltip("Refresh")
                    .disabled(matches!(self.status, Load::Loading { .. }))
                    .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
            );
        let body = match &self.status {
            Load::Loading { .. } => div()
                .text_sm()
                .text_color(muted)
                .child("Loading…")
                .into_any_element(),
            Load::Failed => h_flex()
                .gap_2()
                .text_sm()
                .text_color(danger)
                .child("Couldn't load your two-factor settings.")
                .child(
                    Button::new("account-retry")
                        .label("Retry")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                )
                .into_any_element(),
            Load::Loaded(status) => {
                let status = status.clone();
                v_flex()
                    .w_full()
                    .gap_4()
                    .child(self.render_authenticator(&status, cx))
                    .child(self.render_recovery(&status, cx))
                    .into_any_element()
            }
        };
        v_flex().w_full().gap_3().child(header).child(body)
    }
}

/// One setting: its name and what it's doing, with its buttons on the right.
pub(crate) fn row(
    title: &'static str,
    detail: impl Into<SharedString>,
    actions: AnyElement,
    cx: &App,
) -> AnyElement {
    h_flex()
        .w_full()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail.into()),
                ),
        )
        .child(actions)
        .into_any_element()
}

/// "Your recovery codes": the codes in two columns, with Copy all,
/// Regenerate (which confirms again) and Done.
fn open_recovery_codes(
    codes: Vec<String>,
    view: WeakEntity<AccountSecurityView>,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme();
        let grid = if codes.is_empty() {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("You have no recovery codes left. Regenerate them for a new set.")
                .into_any_element()
        } else {
            div()
                .grid()
                .grid_cols(2)
                .gap_x_6()
                .gap_y_1()
                .p_3()
                .rounded_md()
                .bg(theme.secondary)
                .font_family("Menlo")
                .text_sm()
                .children(codes.iter().map(|code| div().child(code.clone())))
                .into_any_element()
        };
        let copy = {
            let text = recovery_codes_text(&codes);
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                window.push_notification(Notification::success("Copied your recovery codes."), cx);
            }
        };
        let regenerate = {
            let view = view.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                window.close_dialog(cx);
                view.update(cx, |this, cx| {
                    this.recovery_codes(RecoveryAction::Regenerate, window, cx)
                })
                .ok();
            }
        };
        dialog
            .title("Your recovery codes")
            .w(px(CODES_DIALOG_WIDTH))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Save these somewhere safe. Each works once."),
                    )
                    .child(grid)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("recovery-copy")
                                    .label("Copy all")
                                    .icon(IconName::Copy)
                                    .outline()
                                    .disabled(codes.is_empty())
                                    .on_click(copy),
                            )
                            .child(
                                Button::new("recovery-regenerate")
                                    .label("Regenerate")
                                    .outline()
                                    .on_click(regenerate),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("recovery-done")
                                    .label("Done")
                                    .primary()
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            ),
                    ),
            )
    });
}
