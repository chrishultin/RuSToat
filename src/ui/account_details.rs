//! The Account page's "Account details", for one account: the username it
//! signs in with, changed in a dialog that confirms it's the user with
//! their current password.
//!
//! The settings window keeps one per account, as it does the other
//! account pages.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::account_edit::{
    ChangeError, ChangeField, PASSWORD_MIN_BYTES, USERNAME_MAX, check_username,
    username_changed_text, username_error,
};
use crate::api::StoatClient;
use crate::api::models::User;
use crate::runtime::spawn_io;
use crate::ui::account_page::row;
use crate::ui::chat::ChatView;

/// The change dialogs' width.
const CHANGE_DIALOG_WIDTH: f32 = 400.;

/// One account's "Account details".
pub struct AccountDetailsView {
    /// The account's chat: its client and store.
    chat: WeakEntity<ChatView>,
}

impl AccountDetailsView {
    pub fn new(chat: Entity<ChatView>) -> Self {
        Self {
            chat: chat.downgrade(),
        }
    }

    /// Whether this is the view for `chat` (an account signed in again has
    /// a new one).
    pub fn is_for(&self, chat: &Entity<ChatView>) -> bool {
        self.chat.entity_id() == chat.entity_id()
    }

    /// The user's username and `#tag`, as the store has them now.
    fn username(&self, cx: &App) -> Option<(String, String)> {
        let chat = self.chat.upgrade()?;
        let store = chat.read(cx).store();
        let user = store.user(store.me())?;
        Some((user.username.clone(), user.discriminator.clone()))
    }

    /// Open "Change your username".
    fn change_username(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let Some((username, tag)) = self.username(cx) else {
            return;
        };
        let dialog = cx.new(|cx| ChangeDialog::new(chat, username, tag, window, cx));
        let focus = dialog.read(cx).value.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |modal, _, _| {
            modal
                .title("Change your username")
                .w(px(CHANGE_DIALOG_WIDTH))
                .child(dialog.clone())
        });
        window.focus(&focus, cx);
    }
}

impl Render for AccountDetailsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let username = match self.username(cx) {
            Some((name, tag)) => format!("@{name}#{tag}"),
            None => "Loading your account…".into(),
        };
        v_flex().w_full().gap_4().child(row(
            "Username",
            username,
            Button::new("change-username")
                .label("Change")
                .outline()
                .small()
                .disabled(self.username(cx).is_none())
                .on_click(cx.listener(|this, _, window, cx| this.change_username(window, cx)))
                .into_any_element(),
            cx,
        ))
    }
}

/// "Change your username": the new name, and the current password.
pub struct ChangeDialog {
    chat: WeakEntity<ChatView>,
    client: StoatClient,
    /// The username and `#tag` when the dialog opened.
    username: String,
    tag: String,
    value: Entity<InputState>,
    current: Entity<InputState>,
    /// Why the last try was refused, and where to say so.
    error: Option<(ChangeField, String)>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl ChangeDialog {
    fn new(
        chat: Entity<ChatView>,
        username: String,
        tag: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let client = chat.read(cx).client().clone();
        let value = cx.new(|cx| InputState::new(window, cx).default_value(username.clone()));
        let current = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Current password")
        });
        let subscriptions = vec![
            cx.subscribe_in(&value, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let focus = this.current.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                }
                InputEvent::Change => this.edited(ChangeField::Value, cx),
                _ => {}
            }),
            cx.subscribe_in(&current, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => this.edited(ChangeField::Current, cx),
                _ => {}
            }),
        ];
        Self {
            chat: chat.downgrade(),
            client,
            username,
            tag,
            value,
            current,
            error: None,
            busy: false,
            _subscriptions: subscriptions,
        }
    }

    /// A field changed: what was said about it (or the whole form) is out
    /// of date.
    fn edited(&mut self, field: ChangeField, cx: &mut Context<Self>) {
        if self
            .error
            .as_ref()
            .is_some_and(|(at, _)| *at == field || *at == ChangeField::Form)
        {
            self.error = None;
        }
        cx.notify();
    }

    /// Whether everything needed has been typed, and passes the checks.
    fn ready(&self, cx: &App) -> bool {
        check_username(&self.value.read(cx).value(), &self.username).is_ok()
            && !self.current.read(cx).value().is_empty()
    }

    /// Send the change.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.ready(cx) {
            return;
        }
        let typed = self.value.read(cx).value().to_string();
        let Ok(name) = check_username(&typed, &self.username) else {
            return;
        };
        let name = name.to_string();
        let password = self.current.read(cx).value().to_string();
        let short_password = password.len() < PASSWORD_MIN_BYTES;
        self.busy = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let client = self.client.clone();
            async move { client.change_username(&name, &password).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(Ok(me)) => this.changed(me, window, cx),
                    Ok(Err(err)) => {
                        log::warn!("couldn't change the username: {err:?}");
                        this.refused(username_error(&err, short_password), window, cx);
                    }
                    Err(err) => {
                        log::warn!("username task failed: {err:?}");
                        this.refused(
                            ChangeError::Toast("Couldn't change your username. Try again.".into()),
                            window,
                            cx,
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Stoat took it: the account shows it at once.
    fn changed(&mut self, me: User, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        let text = username_changed_text(&self.tag, &me.username, &me.discriminator);
        if let Some(chat) = self.chat.upgrade() {
            chat.update(cx, |chat, cx| chat.own_profile_saved(me, None, false, cx));
        }
        window.push_notification(Notification::success(text), cx);
    }

    /// Stoat refused it: say why at the field, or give up with a toast.
    fn refused(&mut self, error: ChangeError, window: &mut Window, cx: &mut Context<Self>) {
        match error {
            ChangeError::Field(field, text) => {
                let input = match field {
                    ChangeField::Value | ChangeField::Confirm => Some(&self.value),
                    ChangeField::Current => Some(&self.current),
                    ChangeField::Code | ChangeField::Form => None,
                };
                if let Some(input) = input {
                    let focus = input.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                }
                self.error = Some((field, text));
            }
            ChangeError::Toast(text) => {
                window.close_dialog(cx);
                window.push_notification(Notification::error(text), cx);
            }
        }
    }

    /// What's said at `field`, if anything.
    fn error_at(&self, field: ChangeField) -> Option<String> {
        self.error
            .as_ref()
            .filter(|(at, _)| *at == field)
            .map(|(_, text)| text.clone())
    }
}

impl Render for ChangeDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let typed = self.value.read(cx).value();
        let count = typed.trim().chars().count();
        // What the checks say, unless Stoat has said something since.
        let value_error = self.error_at(ChangeField::Value).or_else(|| {
            check_username(&typed, &self.username)
                .err()
                .flatten()
                .map(str::to_string)
        });
        let username = v_flex()
            .gap_1()
            .child(label("New username"))
            .child(
                Input::new(&self.value)
                    .suffix(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("#{}", self.tag)),
                    )
                    .disabled(self.busy),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .text_color(danger)
                            .children(value_error.clone()),
                    )
                    .child(
                        div()
                            .text_color(if count > USERNAME_MAX { danger } else { muted })
                            .child(format!("{count}/{USERNAME_MAX}")),
                    ),
            );
        let current = v_flex()
            .gap_1()
            .child(label("Current password"))
            .child(Input::new(&self.current).disabled(self.busy))
            .children(error_line(self.error_at(ChangeField::Current), danger));
        v_flex()
            .gap_3()
            .child(div().text_sm().text_color(muted).child(
                "Your #number stays if it's free under the new name. If not, Stoat picks a new one.",
            ))
            .child(username)
            .child(current)
            .children(error_line(self.error_at(ChangeField::Form), danger))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("change-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("change-ok")
                            .label("Change")
                            .primary()
                            .loading(self.busy)
                            .disabled(self.busy || !self.ready(cx))
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

/// A field's name, above it.
fn label(text: &'static str) -> Div {
    div().text_sm().font_weight(FontWeight::MEDIUM).child(text)
}

/// What's wrong, under a field.
fn error_line(text: Option<String>, danger: Hsla) -> Option<Div> {
    text.map(|text| div().text_xs().text_color(danger).child(text))
}
