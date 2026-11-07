//! The Account page's "Account details", for one account: the username,
//! email and password it signs in with. Each is changed in a dialog that
//! confirms it's the user with their current password, plus an
//! authenticator or recovery code when Stoat wants one (an email or
//! password change with an authenticator app on).
//!
//! The settings window keeps one per account, as it does the other
//! account pages.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::account_edit::{
    CODE_USED, ChangeError, ChangeField, MASKED_EMAIL, PASSWORD_MIN_BYTES, USERNAME_MAX,
    check_email, check_new_password, check_username, code_methods, email_error, password_error,
    username_changed_text, username_error,
};
use crate::api::models::User;
use crate::api::{ApiError, StoatClient};
use crate::profile_edit::error_text;
use crate::runtime::spawn_io;
use crate::security::{ConfirmError, ConfirmMethod, mfa_response, ticket_error};
use crate::ui::account_page::row;
use crate::ui::chat::ChatView;

/// The change dialogs' width.
const CHANGE_DIALOG_WIDTH: f32 = 400.;

/// Where showing the email is at.
enum EmailShown {
    Hidden,
    Loading { _task: Task<()> },
    Shown(String),
}

/// One account's "Account details".
pub struct AccountDetailsView {
    /// The account's chat: its client and store.
    chat: WeakEntity<ChatView>,
    email: EmailShown,
    /// A new email waiting for the user to open the link Stoat sent it.
    pending_email: Option<String>,
}

impl AccountDetailsView {
    pub fn new(chat: Entity<ChatView>) -> Self {
        Self {
            chat: chat.downgrade(),
            email: EmailShown::Hidden,
            pending_email: None,
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

    /// Fetch the email and show it.
    fn reveal_email(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let client = chat.read(cx).client().clone();
            async move { client.fetch_account().await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.email = match result {
                    Ok(Ok(account)) => EmailShown::Shown(account.email),
                    Ok(Err(err)) => {
                        log::warn!("couldn't load the account's email: {err:?}");
                        let text = error_text(&err, "Couldn't load your email.");
                        window.push_notification(Notification::error(text), cx);
                        EmailShown::Hidden
                    }
                    Err(err) => {
                        log::warn!("account task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't load your email."),
                            cx,
                        );
                        EmailShown::Hidden
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.email = EmailShown::Loading { _task: task };
        cx.notify();
    }

    /// Open the dialog that changes `kind`.
    fn open_change(&mut self, kind: ChangeKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let Some((username, tag)) = self.username(cx) else {
            return;
        };
        let view = cx.entity().downgrade();
        let dialog = cx.new(|cx| ChangeDialog::new(kind, chat, view, username, tag, window, cx));
        let focus = dialog.read(cx).value.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |modal, _, _| {
            modal
                .title(kind.title())
                .w(px(CHANGE_DIALOG_WIDTH))
                .child(dialog.clone())
        });
        window.focus(&focus, cx);
    }

    /// Stoat took a new email: it's changed already (an instance that
    /// sends no email), or waits for its link to be opened.
    fn email_sent(
        &mut self,
        email: String,
        changed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = if changed {
            if matches!(self.email, EmailShown::Shown(_)) {
                self.email = EmailShown::Shown(email.clone());
            }
            self.pending_email = None;
            format!("Your email is now {email}.")
        } else {
            let text = format!("Check {email} for a link to confirm the change.");
            self.pending_email = Some(email);
            text
        };
        window.push_notification(Notification::success(text), cx);
        cx.notify();
    }

    fn render_email(&self, cx: &mut Context<Self>) -> AnyElement {
        let (detail, reveal) = match &self.email {
            EmailShown::Hidden => (MASKED_EMAIL.to_string(), true),
            EmailShown::Loading { .. } => ("Loading…".to_string(), true),
            EmailShown::Shown(email) => (email.clone(), false),
        };
        let loading = matches!(self.email, EmailShown::Loading { .. });
        let toggle = Button::new("email-reveal")
            .label(if reveal { "Reveal" } else { "Hide" })
            .ghost()
            .small()
            .loading(loading)
            .disabled(loading)
            .on_click(cx.listener(move |this, _, window, cx| {
                if reveal {
                    this.reveal_email(window, cx);
                } else {
                    this.email = EmailShown::Hidden;
                    cx.notify();
                }
            }));
        let actions = h_flex()
            .gap_2()
            .child(toggle)
            .child(change_button(ChangeKind::Email, cx));
        v_flex()
            .w_full()
            .gap_1()
            .child(row("Email", detail, actions.into_any_element(), cx))
            .children(self.pending_email.as_ref().map(|email| {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("Waiting for you to open the link sent to {email}."))
            }))
            .into_any_element()
    }
}

impl Render for AccountDetailsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let username = match self.username(cx) {
            Some((name, tag)) => format!("@{name}#{tag}"),
            None => "Loading your account…".into(),
        };
        v_flex()
            .w_full()
            .gap_4()
            .child(row(
                "Username",
                username,
                change_button(ChangeKind::Username, cx).into_any_element(),
                cx,
            ))
            .child(self.render_email(cx))
            .child(row(
                "Password",
                "Change the password you sign in with.",
                change_button(ChangeKind::Password, cx).into_any_element(),
                cx,
            ))
    }
}

/// A row's Change button.
fn change_button(kind: ChangeKind, cx: &mut Context<AccountDetailsView>) -> Button {
    Button::new(SharedString::from(format!("change-{kind:?}")))
        .label("Change")
        .outline()
        .small()
        .on_click(cx.listener(move |this, _, window, cx| this.open_change(kind, window, cx)))
}

/// What a change dialog changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Username,
    Email,
    Password,
}

impl ChangeKind {
    fn title(self) -> &'static str {
        match self {
            ChangeKind::Username => "Change your username",
            ChangeKind::Email => "Change your email",
            ChangeKind::Password => "Change your password",
        }
    }

    /// What it says above the fields.
    fn intro(self) -> &'static str {
        match self {
            ChangeKind::Username => {
                "Your #number stays if it's free under the new name. If not, Stoat picks a new one."
            }
            ChangeKind::Email => {
                "Stoat sends a link to the new address. Your email changes once you open it."
            }
            ChangeKind::Password => {
                "Use at least 8 characters. Your other sessions stay signed in."
            }
        }
    }

    /// The new value's label and placeholder.
    fn value_label(self) -> (&'static str, &'static str) {
        match self {
            ChangeKind::Username => ("New username", ""),
            ChangeKind::Email => ("New email", "someone@example.com"),
            ChangeKind::Password => ("New password", "New password"),
        }
    }

    /// Whether Stoat wants a code too when the account has an
    /// authenticator app.
    fn needs_code(self) -> bool {
        match self {
            ChangeKind::Username => false,
            ChangeKind::Email | ChangeKind::Password => true,
        }
    }

    /// The toast when it fails for no reason Stoat gives.
    fn failed_text(self) -> &'static str {
        match self {
            ChangeKind::Username => "Couldn't change your username. Try again.",
            ChangeKind::Email => "Couldn't change your email. Try again.",
            ChangeKind::Password => "Couldn't change your password. Try again.",
        }
    }
}

/// A checked change, ready to send.
enum Request {
    Username { name: String, password: String },
    Email { email: String, password: String },
    Password { new: String, password: String },
}

/// What a change came back with.
enum Outcome {
    /// The user as Stoat stored them.
    Username(Box<User>),
    /// The new email, and whether it's changed already.
    Email {
        email: String,
        changed: bool,
    },
    Password,
}

/// Why a change didn't go through.
enum Refusal {
    /// Stoat wouldn't take the code.
    Ticket(ApiError),
    /// Stoat refused the change itself.
    Change(ApiError),
}

impl Request {
    async fn send(self, client: &StoatClient, ticket: Option<&str>) -> Result<Outcome, ApiError> {
        match self {
            Request::Username { name, password } => client
                .change_username(&name, &password)
                .await
                .map(|me| Outcome::Username(Box::new(me))),
            Request::Email { email, password } => {
                client.change_email(ticket, &email, &password).await?;
                // An instance that sends no email changes it at once.
                let changed = client
                    .fetch_account()
                    .await
                    .is_ok_and(|account| account.email == email);
                Ok(Outcome::Email { email, changed })
            }
            Request::Password { new, password } => client
                .change_password(ticket, &new, &password)
                .await
                .map(|()| Outcome::Password),
        }
    }
}

/// Asking Stoat whether a code is needed, and of which kinds.
enum Methods {
    Loading {
        _task: Task<()>,
    },
    /// Empty when no code is needed.
    Loaded(Vec<ConfirmMethod>),
    Failed(String),
}

/// A change dialog: the new value, the current password and, when Stoat
/// wants one, an authenticator or recovery code.
pub struct ChangeDialog {
    kind: ChangeKind,
    chat: WeakEntity<ChatView>,
    view: WeakEntity<AccountDetailsView>,
    client: StoatClient,
    /// The username and `#tag` when the dialog opened.
    username: String,
    tag: String,
    value: Entity<InputState>,
    /// The new password, typed again.
    confirm: Entity<InputState>,
    current: Entity<InputState>,
    code: Entity<InputState>,
    methods: Methods,
    /// The kind of code being asked for.
    method: ConfirmMethod,
    /// The code field was emptied because a failed change used its code.
    code_used: bool,
    /// Why the last try was refused, and where to say so.
    error: Option<(ChangeField, String)>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl ChangeDialog {
    fn new(
        kind: ChangeKind,
        chat: Entity<ChatView>,
        view: WeakEntity<AccountDetailsView>,
        username: String,
        tag: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let client = chat.read(cx).client().clone();
        let (_, placeholder) = kind.value_label();
        let initial = match kind {
            ChangeKind::Username => username.clone(),
            ChangeKind::Email | ChangeKind::Password => String::new(),
        };
        let masked = kind == ChangeKind::Password;
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(masked)
                .default_value(initial)
                .placeholder(placeholder)
        });
        let confirm = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("New password again")
        });
        let current = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Current password")
        });
        let code = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&value, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let next = if this.kind == ChangeKind::Password {
                        &this.confirm
                    } else {
                        &this.current
                    };
                    let focus = next.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                }
                InputEvent::Change => this.edited(ChangeField::Value, cx),
                _ => {}
            }),
            cx.subscribe_in(&confirm, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let focus = this.current.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                }
                InputEvent::Change => this.edited(ChangeField::Confirm, cx),
                _ => {}
            }),
            cx.subscribe_in(&current, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } if this.asks_for_code() => {
                    let focus = this.code.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                }
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => this.edited(ChangeField::Current, cx),
                _ => {}
            }),
            cx.subscribe_in(&code, window, |this, code, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                // Emptying it (as a used code is) leaves what's said about it.
                InputEvent::Change if code.read(cx).value().is_empty() => cx.notify(),
                InputEvent::Change => {
                    this.code_used = false;
                    this.edited(ChangeField::Code, cx);
                }
                _ => {}
            }),
        ];
        let mut this = Self {
            kind,
            chat: chat.downgrade(),
            view,
            client,
            username,
            tag,
            value,
            confirm,
            current,
            code,
            methods: Methods::Loaded(Vec::new()),
            method: ConfirmMethod::Totp,
            code_used: false,
            error: None,
            busy: false,
            _subscriptions: subscriptions,
        };
        if kind.needs_code() {
            this.load_methods(window, cx);
        }
        this
    }

    /// Ask Stoat whether the account has an authenticator app, which makes
    /// the change need a code.
    fn load_methods(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = spawn_io({
            let client = self.client.clone();
            async move { client.mfa_status().await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.methods = match result {
                    Ok(Ok(status)) => {
                        let methods = code_methods(&status);
                        if let Some(first) = methods.first() {
                            this.set_method(*first, window, cx);
                        }
                        Methods::Loaded(methods)
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't load the MFA status: {err:?}");
                        Methods::Failed(error_text(&err, "Couldn't check your account."))
                    }
                    Err(err) => {
                        log::warn!("MFA status task failed: {err:?}");
                        Methods::Failed("Couldn't check your account.".into())
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.methods = Methods::Loading { _task: task };
        cx.notify();
    }

    /// Whether a code field is shown.
    fn asks_for_code(&self) -> bool {
        matches!(&self.methods, Methods::Loaded(methods) if !methods.is_empty())
    }

    /// Ask for a `method` code: the field is emptied.
    fn set_method(&mut self, method: ConfirmMethod, window: &mut Window, cx: &mut Context<Self>) {
        self.method = method;
        self.code_used = false;
        if self
            .error
            .as_ref()
            .is_some_and(|(at, _)| *at == ChangeField::Code)
        {
            self.error = None;
        }
        self.code.update(cx, |input, cx| {
            input.set_placeholder(method.placeholder(), window, cx);
            input.set_value("", window, cx);
        });
        cx.notify();
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

    /// The new value, if it passes the checks; else what they say, if
    /// anything, and at which field.
    fn checked_value(&self, cx: &App) -> Result<String, Option<(ChangeField, &'static str)>> {
        let typed = self.value.read(cx).value();
        let at_value = |text: Option<&'static str>| text.map(|text| (ChangeField::Value, text));
        match self.kind {
            ChangeKind::Username => check_username(&typed, &self.username)
                .map(str::to_string)
                .map_err(at_value),
            ChangeKind::Email => check_email(&typed).map(str::to_string).map_err(at_value),
            ChangeKind::Password => check_new_password(&typed, &self.confirm.read(cx).value())
                .map(|()| typed.to_string()),
        }
    }

    /// What the checks say at `field`, unless Stoat has said something
    /// there since.
    fn message_at(&self, field: ChangeField, cx: &App) -> Option<String> {
        self.error_at(field)
            .or_else(|| match self.checked_value(cx) {
                Err(Some((at, text))) if at == field => Some(text.to_string()),
                _ => None,
            })
    }

    /// The code, as Stoat takes it: `Some(None)` when none is needed, and
    /// `None` when one is but it isn't typed (or Stoat hasn't said yet).
    fn code_response(&self, cx: &App) -> Option<Option<crate::api::models::MFAResponse>> {
        match &self.methods {
            Methods::Loaded(methods) if methods.is_empty() => Some(None),
            Methods::Loaded(_) => mfa_response(self.method, &self.code.read(cx).value()).map(Some),
            _ => None,
        }
    }

    /// Whether everything needed has been typed, and passes the checks.
    fn ready(&self, cx: &App) -> bool {
        self.checked_value(cx).is_ok()
            && !self.current.read(cx).value().is_empty()
            && self.code_response(cx).is_some()
    }

    /// Send the change, after getting a ticket for the code if there is one.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.ready(cx) {
            return;
        }
        let (Ok(value), Some(response)) = (self.checked_value(cx), self.code_response(cx)) else {
            return;
        };
        let password = self.current.read(cx).value().to_string();
        let short_password = password.len() < PASSWORD_MIN_BYTES;
        let request = match self.kind {
            ChangeKind::Username => Request::Username {
                name: value,
                password,
            },
            ChangeKind::Email => Request::Email {
                email: value,
                password,
            },
            ChangeKind::Password => Request::Password {
                new: value,
                password,
            },
        };
        let used_code = response.is_some();
        self.busy = true;
        self.error = None;
        cx.notify();
        let task = spawn_io({
            let client = self.client.clone();
            async move {
                // The ticket's token, and the code, are never logged.
                let ticket = match response {
                    Some(response) => Some(
                        client
                            .create_mfa_ticket(&response)
                            .await
                            .map_err(Refusal::Ticket)?
                            .token,
                    ),
                    None => None,
                };
                request
                    .send(&client, ticket.as_deref())
                    .await
                    .map_err(Refusal::Change)
            }
        });
        let kind = self.kind;
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(Ok(outcome)) => this.changed(outcome, window, cx),
                    Ok(Err(Refusal::Ticket(err))) => this.code_refused(&err, window, cx),
                    Ok(Err(Refusal::Change(err))) => {
                        log::warn!("couldn't change the account's {kind:?}: {err:?}");
                        if used_code {
                            this.use_up_code(window, cx);
                        }
                        let error = match kind {
                            ChangeKind::Username => username_error(&err, short_password),
                            ChangeKind::Email => email_error(&err),
                            ChangeKind::Password => password_error(&err),
                        };
                        this.refused(error, window, cx);
                    }
                    Err(err) => {
                        log::warn!("{kind:?} change task failed: {err:?}");
                        this.refused(ChangeError::Toast(kind.failed_text().into()), window, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Stoat took it: the account shows it at once.
    fn changed(&mut self, outcome: Outcome, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        match outcome {
            Outcome::Username(me) => {
                let text = username_changed_text(&self.tag, &me.username, &me.discriminator);
                if let Some(chat) = self.chat.upgrade() {
                    chat.update(cx, |chat, cx| chat.own_profile_saved(*me, None, false, cx));
                }
                window.push_notification(Notification::success(text), cx);
            }
            Outcome::Email { email, changed } => {
                self.view
                    .update(cx, |view, cx| view.email_sent(email, changed, window, cx))
                    .ok();
            }
            Outcome::Password => window.push_notification(
                Notification::success(
                    "Password changed. Your other sessions stay signed in; you can log them out \
                     in Settings → Sessions.",
                ),
                cx,
            ),
        }
    }

    /// Stoat wouldn't take the code: ask again, or give up.
    fn code_refused(&mut self, err: &ApiError, window: &mut Window, cx: &mut Context<Self>) {
        match ticket_error(err) {
            ConfirmError::Retry(text) => {
                self.refused(ChangeError::Field(ChangeField::Code, text), window, cx)
            }
            // The authenticator was turned off or on elsewhere.
            ConfirmError::Methods => self.load_methods(window, cx),
            ConfirmError::Fail(text) => {
                log::warn!("couldn't make an MFA ticket: {err:?}");
                self.refused(ChangeError::Toast(text), window, cx);
            }
        }
    }

    /// A failed change used up its ticket, and with it the code: empty the
    /// field so it isn't sent again.
    fn use_up_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.code_used = true;
        self.code
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    /// Stoat refused it: say why at the field, or give up with a toast.
    fn refused(&mut self, error: ChangeError, window: &mut Window, cx: &mut Context<Self>) {
        match error {
            ChangeError::Field(field, text) => {
                let input = match field {
                    ChangeField::Value => Some(&self.value),
                    ChangeField::Confirm => Some(&self.confirm),
                    ChangeField::Current => Some(&self.current),
                    ChangeField::Code => Some(&self.code),
                    ChangeField::Form => None,
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

    fn render_value(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let error = self.message_at(ChangeField::Value, cx);
        let (label_text, _) = self.kind.value_label();
        let input = Input::new(&self.value).disabled(self.busy);
        match self.kind {
            ChangeKind::Username => {
                let count = self.value.read(cx).value().trim().chars().count();
                v_flex()
                    .gap_1()
                    .child(label(label_text))
                    .child(
                        input.suffix(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(format!("#{}", self.tag)),
                        ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .child(div().flex_1().text_color(danger).children(error))
                            .child(
                                div()
                                    .text_color(if count > USERNAME_MAX { danger } else { muted })
                                    .child(format!("{count}/{USERNAME_MAX}")),
                            ),
                    )
            }
            ChangeKind::Email => v_flex()
                .gap_1()
                .child(label(label_text))
                .child(input)
                .children(error_line(error, danger)),
            ChangeKind::Password => v_flex()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(label(label_text))
                        .child(input)
                        .children(error_line(error, danger)),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(label("Confirm new password"))
                        .child(Input::new(&self.confirm).disabled(self.busy))
                        .children(error_line(
                            self.message_at(ChangeField::Confirm, cx),
                            danger,
                        )),
                ),
        }
    }

    fn render_code(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        Some(match &self.methods {
            Methods::Loaded(methods) if methods.is_empty() => return None,
            Methods::Loading { .. } => div()
                .text_sm()
                .text_color(muted)
                .child("Checking your account…")
                .into_any_element(),
            Methods::Failed(text) => h_flex()
                .gap_2()
                .text_sm()
                .text_color(danger)
                .child(text.clone())
                .child(
                    Button::new("change-methods-retry")
                        .label("Retry")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.load_methods(window, cx))),
                )
                .into_any_element(),
            Methods::Loaded(methods) => {
                let error = self
                    .error_at(ChangeField::Code)
                    .or_else(|| self.code_used.then(|| CODE_USED.to_string()));
                let others = methods.iter().copied().filter(|m| *m != self.method);
                v_flex()
                    .gap_1()
                    .child(label("Two-factor code"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.method.prompt()),
                    )
                    .child(Input::new(&self.code).disabled(self.busy))
                    .children(error_line(error, danger))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Each code works once, even if the change fails."),
                    )
                    .children(others.map(|method| {
                        Button::new(SharedString::from(format!("change-use-{method:?}")))
                            .label(method.switch_label())
                            .link()
                            .small()
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_method(method, window, cx);
                                let focus = this.code.read(cx).focus_handle(cx);
                                window.focus(&focus, cx);
                            }))
                    }))
                    .into_any_element()
            }
        })
    }
}

impl Render for ChangeDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let current = v_flex()
            .gap_1()
            .child(label("Current password"))
            .child(Input::new(&self.current).disabled(self.busy))
            .children(error_line(self.error_at(ChangeField::Current), danger));
        v_flex()
            .gap_3()
            .child(div().text_sm().text_color(muted).child(self.kind.intro()))
            .child(self.render_value(cx))
            .child(current)
            .children(self.render_code(cx))
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
