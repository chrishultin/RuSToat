//! The Sessions page, for one account: where it's signed in, newest first,
//! with this device first when RuSToat knows which session it is (see
//! [`crate::security`]). Any session can be renamed, and any other one
//! logged out (or all of them at once) after confirming it's the user.
//!
//! The settings window keeps one per account, as it does the Profile page.

use chrono::Local;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::profile_edit::{FieldCheck, error_text};
use crate::runtime::spawn_io;
use crate::security::{
    Platform, SESSION_NAME_MAX, SessionRow, check_session_name, created_label, session_rows,
};
use crate::ui::chat::ChatView;
use crate::ui::confirm_dialog::{ConfirmRequest, confirm_then};

/// The rename dialog's width.
const RENAME_DIALOG_WIDTH: f32 = 380.;

/// Where loading the session list is at.
enum Load {
    Loading { _task: Task<()> },
    Loaded(Vec<SessionRow>),
    Failed,
}

/// One account's Sessions page.
pub struct SessionsView {
    /// The account's chat: its client and store.
    chat: WeakEntity<ChatView>,
    /// This client's own session, if it's known.
    session_id: Option<String>,
    sessions: Load,
    /// The rename dialog's field, and the session it renames.
    rename_input: Entity<InputState>,
    renaming: Option<String>,
    rename_saving: bool,
    _rename_input: Subscription,
}

impl SessionsView {
    pub fn new(
        chat: Entity<ChatView>,
        session_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        let subscription = cx.subscribe(&rename_input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let mut this = Self {
            chat: chat.downgrade(),
            session_id,
            sessions: Load::Failed,
            rename_input,
            renaming: None,
            rename_saving: false,
            _rename_input: subscription,
        };
        this.refresh(window, cx);
        this
    }

    /// Whether this is the page for `chat` signed in with `session_id` (an
    /// account signed in again has a new chat and session).
    pub fn is_for(&self, chat: &Entity<ChatView>, session_id: Option<&str>) -> bool {
        self.chat.entity_id() == chat.entity_id() && self.session_id.as_deref() == session_id
    }

    /// Fetch the session list again.
    pub fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let client = chat.read(cx).client().clone();
            async move { client.fetch_sessions().await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.sessions = match result {
                    Ok(Ok(sessions)) => {
                        log::debug!("{} sessions", sessions.len());
                        Load::Loaded(session_rows(sessions, this.session_id.as_deref()))
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't load sessions: {err}");
                        Load::Failed
                    }
                    Err(err) => {
                        log::warn!("sessions task failed: {err:?}");
                        Load::Failed
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.sessions = Load::Loading { _task: task };
        cx.notify();
    }

    /// `@username#1234`, for the line saying which account this is.
    fn handle(&self, cx: &App) -> Option<String> {
        let chat = self.chat.upgrade()?;
        let store = chat.read(cx).store();
        let user = store.user(store.me())?;
        Some(format!("@{}#{}", user.username, user.discriminator))
    }

    /// Open "Rename session", filled with its name.
    fn open_rename(&mut self, row: &SessionRow, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = Some(row.id.clone());
        self.rename_saving = false;
        let name = row.name.clone();
        self.rename_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        let view = cx.entity().downgrade();
        let input = self.rename_input.clone();
        let first_build = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first_build.replace(false) {
                let focus = input.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            let saving = view
                .upgrade()
                .is_some_and(|view| view.read(cx).rename_saving);
            let save = {
                let view = view.clone();
                move |window: &mut Window, cx: &mut App| {
                    view.update(cx, |this, cx| this.save_rename(window, cx))
                        .ok();
                }
            };
            dialog
                .title("Rename session")
                .w(px(RENAME_DIALOG_WIDTH))
                .child(render_rename(&input, saving, save, cx))
        });
    }

    /// Save the rename dialog's name, then close it.
    fn save_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.rename_input.read(cx).value().to_string();
        let FieldCheck::Ok(name) = check_session_name(&typed) else {
            return;
        };
        let (Some(id), Some(chat)) = (self.renaming.clone(), self.chat.upgrade()) else {
            return;
        };
        if self.rename_saving {
            return;
        }
        self.rename_saving = true;
        window.refresh();
        let request = spawn_io({
            let (client, name) = (chat.read(cx).client().clone(), name.to_string());
            let id = id.clone();
            async move { client.rename_session(&id, &name).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.rename_saving = false;
                match result {
                    Ok(Ok(session)) => {
                        if let Load::Loaded(rows) = &mut this.sessions
                            && let Some(row) = rows.iter_mut().find(|r| r.id == session.id)
                        {
                            row.name = session.name;
                            row.platform = crate::security::platform_of(&row.name);
                        }
                        this.renaming = None;
                        window.close_dialog(cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't rename session {id}: {err:?}");
                        let text = error_text(&err, "Couldn't rename the session.");
                        window.push_notification(Notification::error(text), cx);
                    }
                    Err(err) => {
                        log::warn!("rename task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't rename the session."),
                            cx,
                        );
                    }
                }
                window.refresh();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_row(&self, row: &SessionRow, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let created = row
            .created_ms
            .and_then(|_| crate::time::ulid_local_time(&row.id))
            .map(|created| created_label(&created, &Local::now()));
        let icon = match row.platform {
            Platform::Phone => IconName::Smartphone,
            Platform::Desktop => IconName::Monitor,
        };
        let rename = {
            let row = row.clone();
            cx.listener(move |this, _, window, cx| this.open_rename(&row, window, cx))
        };
        h_flex()
            .w_full()
            .gap_3()
            .py_1()
            .child(Icon::new(icon).text_color(muted))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(row.name.clone()),
                    )
                    .children(
                        created.map(|created| div().text_xs().text_color(muted).child(created)),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("session-rename-{}", row.id)))
                    .label("Rename")
                    .ghost()
                    .small()
                    .on_click(rename),
            )
            // This device logs out from the Stoat menu, which also forgets
            // the account here.
            .when(!row.current, |this| {
                let row = row.clone();
                this.child(
                    Button::new(SharedString::from(format!("session-log-out-{}", row.id)))
                        .label("Log out")
                        .ghost()
                        .small()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.log_out(Some(&row), window, cx)
                        })),
                )
            })
            .into_any_element()
    }

    /// Log out one session (`Some`), or every other session (`None`),
    /// after confirming it's the user. The list is fetched again after.
    fn log_out(&mut self, row: Option<&SessionRow>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let client = chat.read(cx).client().clone();
        let request = match row {
            Some(row) => ConfirmRequest {
                title: "Log out this session?".into(),
                detail: format!("“{}” will be signed out.", row.name).into(),
                confirm_label: "Log out".into(),
                failed_text: "Couldn't log the session out.",
            },
            None => ConfirmRequest {
                title: "Log out other sessions?".into(),
                detail: "Every other session will be signed out. You'll stay signed in here."
                    .into(),
                confirm_label: "Log out others".into(),
                failed_text: "Couldn't log the other sessions out.",
            },
        };
        let target = row.map(|row| row.id.clone());
        let view = cx.entity().downgrade();
        confirm_then(
            client,
            request,
            move |client, ticket| {
                Box::pin(async move {
                    match target {
                        Some(id) => client.revoke_session(&ticket, &id).await,
                        None => client.revoke_other_sessions(&ticket).await,
                    }
                })
            },
            move |(), window, cx| {
                view.update(cx, |this, cx| this.refresh(window, cx)).ok();
            },
            window,
            cx,
        );
    }
}

impl Render for SessionsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let account = match self.handle(cx) {
            Some(handle) => format!("Sessions of {handle}"),
            None => "Loading your account…".to_string(),
        };
        // Stoat spares this session, so it's offered whenever there's
        // another one, whether or not this device is known.
        let can_log_out_others = matches!(&self.sessions, Load::Loaded(rows) if rows.len() > 1);
        let header = h_flex()
            .w_full()
            .gap_2()
            .child(div().flex_1().text_sm().text_color(muted).child(account))
            .when(can_log_out_others, |this| {
                this.child(
                    Button::new("sessions-log-out-others")
                        .label("Log out other sessions")
                        .icon(IconName::LogOut)
                        .outline()
                        .small()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.log_out(None, window, cx)),
                        ),
                )
            })
            .child(
                Button::new("sessions-refresh")
                    .icon(IconName::RefreshCw)
                    .ghost()
                    .small()
                    .tooltip("Refresh")
                    .disabled(matches!(self.sessions, Load::Loading { .. }))
                    .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
            );
        let body = match &self.sessions {
            Load::Loading { .. } => div()
                .text_sm()
                .text_color(muted)
                .child("Loading sessions…")
                .into_any_element(),
            Load::Failed => h_flex()
                .gap_2()
                .text_sm()
                .text_color(danger)
                .child("Couldn't load your sessions.")
                .child(
                    Button::new("sessions-retry")
                        .label("Retry")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                )
                .into_any_element(),
            Load::Loaded(rows) => {
                let rows = rows.clone();
                let (current, others): (Vec<_>, Vec<_>) = rows.iter().partition(|r| r.current);
                v_flex()
                    .w_full()
                    .gap_4()
                    .when(!current.is_empty(), |this| {
                        this.child(section("This device", cx))
                            .children(current.iter().map(|row| self.render_row(row, cx)))
                    })
                    .child(section(
                        if current.is_empty() {
                            "Sessions"
                        } else {
                            "Other sessions"
                        },
                        cx,
                    ))
                    .map(|this| {
                        if others.is_empty() {
                            this.child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .child("You aren't signed in anywhere else."),
                            )
                        } else {
                            this.children(others.iter().map(|row| self.render_row(row, cx)))
                        }
                    })
                    .into_any_element()
            }
        };
        v_flex().w_full().gap_3().child(header).child(body)
    }
}

/// A heading inside the page.
fn section(title: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

/// The rename dialog: one field with a character count, and Cancel/Save.
fn render_rename(
    input: &Entity<InputState>,
    saving: bool,
    on_save: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let typed = input.read(cx).value();
    let check = check_session_name(&typed);
    let count = typed.trim().chars().count();
    v_flex()
        .gap_3()
        .child(
            v_flex()
                .gap_1()
                .child(Input::new(input).disabled(saving))
                .child(
                    div()
                        .text_xs()
                        .text_right()
                        .text_color(if matches!(check, FieldCheck::TooLong(_)) {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(format!("{count}/{SESSION_NAME_MAX}")),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("cancel-rename")
                        .outline()
                        .label("Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("save-rename")
                        .primary()
                        .label("Save")
                        .loading(saving)
                        .disabled(saving || !matches!(check, FieldCheck::Ok(_)))
                        .on_click(move |_, window, cx| on_save(window, cx)),
                ),
        )
        .into_any_element()
}
