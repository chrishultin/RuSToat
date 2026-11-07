//! "Join a server": paste an invite link or code, see what it leads to,
//! then join it, or open it if the user is already in. The rules for codes
//! and links, and the error texts, are in [`crate::invites`].

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::StoatClient;
use crate::invites::{InviteHosts, InvitePreview, already_joined, join_error_text, parse_input};
use crate::runtime::spawn_io;
use crate::ui::chat::{ChatView, avatar};

/// The dialog's width (as Add friend's).
pub const JOIN_DIALOG_WIDTH: f32 = 420.;
/// The invite's icon in the preview.
const PREVIEW_ICON: f32 = 48.;

/// What the dialog is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    LookingUp,
    Joining,
}

/// The Join dialog's content, for one account.
pub struct JoinDialog {
    chat: WeakEntity<ChatView>,
    client: StoatClient,
    hosts: InviteHosts,
    /// "@ada#1234": who joins.
    account: Option<String>,
    input: Entity<InputState>,
    /// The code last looked up; a lookup that comes back for another
    /// (the field was edited meanwhile) is dropped.
    looked_up: Option<String>,
    preview: Option<InvitePreview>,
    busy: Option<Busy>,
    error: Option<String>,
    _input: Subscription,
}

impl JoinDialog {
    /// The dialog for `chat`'s account, with `code` filled in and looked up
    /// when it's given (a clicked invite link).
    pub fn new(
        chat: &ChatView,
        weak: WeakEntity<ChatView>,
        code: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let hosts = chat.invite_hosts();
        let placeholder = hosts.placeholder();
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(placeholder);
            if let Some(code) = &code {
                input.set_value(code.clone(), window, cx);
            }
            input
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.edited(cx),
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => {}
            });
        let account = chat
            .store()
            .user(chat.store().me())
            .map(|me| format!("@{}#{}", me.username, me.discriminator));
        let mut this = Self {
            chat: weak,
            client: chat.client().clone(),
            hosts,
            account,
            input,
            looked_up: None,
            preview: None,
            busy: None,
            error: None,
            _input: subscription,
        };
        if code.is_some() {
            this.look_up(window, cx);
        }
        this
    }

    /// The field, to focus when the dialog opens.
    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// The field changed: what was looked up no longer applies.
    fn edited(&mut self, cx: &mut Context<Self>) {
        let typed = self.input.read(cx).value().to_string();
        let same = self
            .looked_up
            .as_deref()
            .is_some_and(|code| parse_input(&typed, &self.hosts).is_ok_and(|typed| typed == code));
        if same || self.busy == Some(Busy::Joining) {
            return;
        }
        self.looked_up = None;
        self.preview = None;
        self.error = None;
        self.busy = None;
        cx.notify();
    }

    /// Enter: look the invite up, or join it once it's shown.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.is_some() {
            self.join(window, cx);
        } else {
            self.look_up(window, cx);
        }
    }

    /// Whether the user is already in what `preview` leads to.
    fn is_member(&self, preview: &InvitePreview, cx: &App) -> bool {
        self.chat
            .upgrade()
            .is_some_and(|chat| chat.read(cx).is_invite_member(preview))
    }

    /// Look up what's typed: Stoat says what the invite leads to.
    fn look_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let typed = self.input.read(cx).value().to_string();
        let code = match parse_input(&typed, &self.hosts) {
            Ok(code) => code,
            Err(text) => {
                self.preview = None;
                self.error = Some(text.into());
                cx.notify();
                return;
            }
        };
        self.looked_up = Some(code.clone());
        self.preview = None;
        self.error = None;
        self.busy = Some(Busy::LookingUp);
        cx.notify();
        let request = spawn_io({
            let (client, code) = (self.client.clone(), code.clone());
            async move { client.fetch_invite(&code).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if this.looked_up.as_deref() != Some(code.as_str()) {
                    return;
                }
                this.busy = None;
                match result {
                    Ok(Ok(invite)) => this.preview = Some(InvitePreview::new(invite)),
                    Ok(Err(err)) => {
                        log::info!("couldn't look up invite {code}: {err:?}");
                        this.error = Some(join_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("invite lookup task failed: {err:?}");
                        this.error = Some("Couldn't reach Stoat. Try again.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Join what's shown, or open it if the user is already in. Success
    /// closes the dialog and shows it; a refusal says why and keeps it.
    fn join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.preview.clone() else {
            return;
        };
        if self.busy.is_some() {
            return;
        }
        if self.is_member(&preview, cx) {
            self.open(&preview, window, cx);
            return;
        }
        self.busy = Some(Busy::Joining);
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let (client, code) = (self.client.clone(), preview.code.clone());
            async move { client.join_invite(&code).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(Ok(joined)) => {
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.invite_joined(&preview, joined, cx));
                        }
                        window.close_dialog(cx);
                        window.push_notification(
                            Notification::success(format!("Joined {}.", preview.name)),
                            cx,
                        );
                    }
                    Ok(Err(err)) if already_joined(&err) => this.open(&preview, window, cx),
                    Ok(Err(err)) => {
                        log::info!("couldn't join invite {}: {err:?}", preview.code);
                        this.error = Some(join_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("invite join task failed: {err:?}");
                        this.error = Some("Couldn't reach Stoat. Try again.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Show what `preview` leads to, which the user is already in.
    fn open(&mut self, preview: &InvitePreview, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.upgrade() {
            chat.update(cx, |chat, cx| chat.open_invite(preview, cx));
        }
        window.close_dialog(cx);
    }
}

impl Render for JoinDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let looking_up = self.busy == Some(Busy::LookingUp);
        let joining = self.busy == Some(Busy::Joining);
        let member = self
            .preview
            .as_ref()
            .is_some_and(|preview| self.is_member(preview, cx));
        let icon_url = self.preview.as_ref().and_then(|preview| {
            let chat = self.chat.upgrade()?;
            chat.read(cx).store().media().file(preview.icon.as_ref()?)
        });
        let preview = self
            .preview
            .as_ref()
            .map(|preview| render_preview(preview, icon_url, cx).into_any_element());
        let entity = cx.entity().downgrade();
        let look_up = {
            let entity = entity.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                entity.update(cx, |this, cx| this.look_up(window, cx)).ok();
            }
        };
        let join = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            entity.update(cx, |this, cx| this.join(window, cx)).ok();
        };

        v_flex()
            .gap_3()
            .children(self.account.clone().map(|account| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("Joining as {account}"))
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Paste an invite link or code."),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&self.input).disabled(joining)),
                            )
                            .child(
                                Button::new("look-up-invite")
                                    .outline()
                                    .label("Look up")
                                    .loading(looking_up)
                                    .disabled(self.busy.is_some())
                                    .on_click(look_up),
                            ),
                    )
                    .children(
                        self.error
                            .clone()
                            .map(|text| div().text_xs().text_color(danger).child(text)),
                    ),
            )
            .children(preview)
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-join")
                            .outline()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("join-invite")
                            .primary()
                            .label(if member { "Open" } else { "Join" })
                            .loading(joining)
                            .disabled(self.preview.is_none() || self.busy.is_some())
                            .on_click(join),
                    ),
            )
    }
}

/// What an invite leads to: its icon, name, members and channel, and who
/// made it.
fn render_preview(preview: &InvitePreview, icon_url: Option<String>, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .gap_3()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .child(avatar(
            format!("invite-preview-{}", preview.code),
            icon_url,
            &preview.name,
            PREVIEW_ICON,
            theme.muted,
            theme.muted_foreground,
        ))
        .child(
            v_flex()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(preview.name.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(preview.detail()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(format!("Invited by {}", preview.inviter)),
                ),
        )
}
