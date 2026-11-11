//! Settings' My Bots section, for one account: the user's bots, "Create
//! bot…", and each bot's page on the same page. The list is fetched the
//! first time the section shows; the bots (tokens and all) stay in this
//! view, and their users go to the store. What it lists and the texts are
//! [`crate::bots`].
//!
//! The settings window keeps one per account, as it does the Sessions page.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::models::{Bot, BotWithUserResponse, User};
use crate::bots::{
    BotRow, DEFAULT_BOT_LIMIT, at_limit, bot_error_text, bot_rows, check_bot_username, count_line,
    delete_prompt,
};
use crate::runtime::spawn_io;
use crate::ui::chat::{ChatView, avatar};
use crate::ui::server_dialogs::name_field;
use crate::ui::server_settings::open_focused_dialog;

/// How big an avatar shows in the list.
const LIST_AVATAR: f32 = 32.;

/// How big the avatar shows over a bot's page.
const PAGE_AVATAR: f32 = 48.;

/// Fetching the list.
enum Fetch {
    Idle,
    Loading {
        _task: Task<()>,
    },
    /// Why it couldn't be fetched.
    Failed(String),
    Loaded,
}

/// What the section shows.
enum PageMode {
    List,
    View(Entity<BotView>),
}

/// One account's My Bots section.
pub struct BotsPage {
    chat: WeakEntity<ChatView>,
    fetch: Fetch,
    /// The user's bots, with their tokens: never logged.
    bots: Vec<Bot>,
    mode: PageMode,
    _subscriptions: Vec<Subscription>,
}

impl BotsPage {
    pub fn new(chat: &Entity<ChatView>, cx: &mut Context<Self>) -> Self {
        Self {
            chat: chat.downgrade(),
            fetch: Fetch::Idle,
            bots: Vec::new(),
            mode: PageMode::List,
            // Names and avatars come from the store.
            _subscriptions: vec![cx.observe(chat, |_, _, cx| cx.notify())],
        }
    }

    /// Whether this is the section for `chat` (an account signed in again
    /// has a new chat).
    pub fn is_for(&self, chat: &Entity<ChatView>) -> bool {
        self.chat.entity_id() == chat.entity_id()
    }

    /// Fetch the list the first time the section shows.
    fn load_once(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.fetch, Fetch::Idle) {
            self.refresh(window, cx);
        }
    }

    /// Fetch the list again.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let client = chat.read(cx).client().clone();
            async move { client.fetch_owned_bots().await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.fetch = match result {
                    Ok(Ok(owned)) => {
                        log::debug!("{} bots", owned.bots.len());
                        this.chat
                            .update(cx, |chat, cx| chat.bot_users_loaded(owned.users, cx))
                            .ok();
                        this.bots = owned.bots;
                        Fetch::Loaded
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't list the bots: {err}");
                        Fetch::Failed(bot_error_text(&err, None))
                    }
                    Err(err) => {
                        log::warn!("bots task failed: {err:?}");
                        Fetch::Failed("Couldn't load your bots.".into())
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.fetch = Fetch::Loading { _task: task };
        cx.notify();
    }

    /// The account's limit, else Stoat's default.
    fn limit(&self, cx: &App) -> u32 {
        self.chat
            .upgrade()
            .and_then(|chat| chat.read(cx).bot_limit())
            .unwrap_or(DEFAULT_BOT_LIMIT)
    }

    fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(chat), Some(bot)) = (
            self.chat.upgrade(),
            self.bots.iter().find(|bot| bot.id == id).cloned(),
        ) else {
            return;
        };
        let view = cx.new(|cx| BotView::new(&chat, bot, window, cx));
        self._subscriptions
            .push(cx.subscribe(&view, |this, _, event: &BotEvent, cx| {
                match event {
                    BotEvent::Back => {}
                    BotEvent::Deleted(id) => this.bots.retain(|bot| &bot.id != id),
                }
                this.mode = PageMode::List;
                cx.notify();
            }));
        self.mode = PageMode::View(view);
        cx.notify();
    }

    /// "Create bot": a username, then Create, which opens the new bot.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let owned = self.bots.len();
        let dialog = cx.new(|cx| CreateBotDialog::new(chat, owned, window, cx));
        self._subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |this, _, created: &BotCreated, window, cx| {
                let id = created.0.bot.id.clone();
                if this.bots.iter().all(|bot| bot.id != id) {
                    this.bots.push(created.0.bot.clone());
                }
                this.open(&id, window, cx);
            },
        ));
        let focus = dialog.read(cx).input.read(cx).focus_handle(cx);
        open_focused_dialog("Create bot".into(), dialog, focus, window, cx);
    }

    fn render_row(&self, row: BotRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let id = row.id.clone();
        h_flex()
            .id(SharedString::from(format!("bot-{}", row.id)))
            .w_full()
            .gap_3()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|el| el.bg(theme.secondary))
            .on_click(cx.listener(move |this, _, window, cx| this.open(&id, window, cx)))
            .child(avatar(
                format!("bot-avatar-{}", row.id),
                row.avatar_url,
                &row.display_name,
                LIST_AVATAR,
                theme.secondary,
                theme.secondary_foreground,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(row.display_name),
                    )
                    .when(!row.username.is_empty(), |el| {
                        el.child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("@{}", row.username)),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(if row.public { "Public" } else { "Private" }),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(Icon::new(IconName::ChevronRight).small()),
            )
            .into_any_element()
    }

    fn render_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.load_once(window, cx);
        let muted = cx.theme().muted_foreground;
        let note = |text: String| div().px_2().text_sm().text_color(muted).child(text);
        let body = match &self.fetch {
            Fetch::Failed(text) => v_flex()
                .gap_2()
                .child(note(text.clone()))
                .child(
                    div().px_2().child(
                        Button::new("bots-retry")
                            .label("Try again")
                            .outline()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                    ),
                )
                .into_any_element(),
            Fetch::Idle | Fetch::Loading { .. } => note("Loading…".into()).into_any_element(),
            Fetch::Loaded => {
                let rows = match self.chat.upgrade() {
                    Some(chat) => {
                        let store = chat.read(cx).store();
                        let users: Vec<User> = self
                            .bots
                            .iter()
                            .filter_map(|bot| store.user(&bot.id).cloned())
                            .collect();
                        bot_rows(&self.bots, &users, store.media())
                    }
                    None => Vec::new(),
                };
                let empty = rows.is_empty();
                let rows: Vec<AnyElement> = rows
                    .into_iter()
                    .map(|row| self.render_row(row, cx))
                    .collect();
                v_flex()
                    .gap_1()
                    .when(empty, |el| {
                        el.child(note("You don't have any bots yet.".into()))
                    })
                    .children(rows)
                    .into_any_element()
            }
        };
        let loaded = matches!(self.fetch, Fetch::Loaded);
        let max = self.limit(cx);
        let full = loaded && at_limit(self.bots.len(), max);
        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(muted)
                    .child(Icon::new(IconName::Bot).small())
                    .child(div().flex_1().child(if loaded {
                        count_line(self.bots.len(), max)
                    } else {
                        "Bots post and answer as their own users, through their token.".into()
                    }))
                    .when(loaded, |el| {
                        el.child(
                            Button::new("create-bot")
                                .label("Create bot…")
                                .icon(IconName::Plus)
                                .outline()
                                .small()
                                .disabled(full)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.create(window, cx)),
                                ),
                        )
                    }),
            )
            .when(full, |el| {
                el.child(note(
                    "You have as many bots as Stoat allows. Delete one to make another.".into(),
                ))
            })
            .child(body)
            .into_any_element()
    }
}

impl Render for BotsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.mode {
            PageMode::List => self.render_list(window, cx),
            PageMode::View(view) => view.clone().into_any_element(),
        }
    }
}

/// "Create bot": its username, then Create.
pub struct CreateBotDialog {
    chat: WeakEntity<ChatView>,
    /// How many bots the user has, for the limit's refusal.
    owned: usize,
    input: Entity<InputState>,
    creating: bool,
    error: Option<String>,
    _input: Subscription,
}

/// A bot was created: Stoat's answer, token and all.
pub struct BotCreated(pub Box<BotWithUserResponse>);

impl EventEmitter<BotCreated> for CreateBotDialog {}

impl CreateBotDialog {
    fn new(
        chat: Entity<ChatView>,
        owned: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.create(window, cx),
                _ => {}
            });
        Self {
            chat: chat.downgrade(),
            owned,
            input,
            creating: false,
            error: None,
            _input: subscription,
        }
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating {
            return;
        }
        let Ok(name) = check_bot_username(&self.input.read(cx).value()) else {
            return;
        };
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.creating = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let client = chat.read(cx).client().clone();
            async move { client.create_bot(&name).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match result {
                    Ok(Ok(created)) => {
                        log::info!("created bot {}", created.bot.id);
                        let user = created.user.clone();
                        this.chat
                            .update(cx, |chat, cx| chat.bot_users_loaded(vec![user], cx))
                            .ok();
                        window.close_dialog(cx);
                        cx.emit(BotCreated(Box::new(created)));
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't create a bot: {err}");
                        this.error = Some(bot_error_text(&err, Some(this.owned)));
                    }
                    Err(err) => {
                        log::warn!("bot create task failed: {err:?}");
                        this.error = Some("Couldn't create the bot.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for CreateBotDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.input.read(cx).value();
        let check = check_bot_username(&typed);
        // A problem shows once something's typed; a request's refusal wins.
        let error = self.error.clone().or_else(|| {
            check
                .as_ref()
                .err()
                .filter(|_| !typed.trim().is_empty())
                .map(ToString::to_string)
        });
        v_flex()
            .gap_3()
            .child(name_field(
                "Username",
                &self.input,
                self.creating,
                error,
                cx,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("You can give it a display name and avatar after."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("create-bot-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.creating)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("create-bot-ok")
                            .primary()
                            .label("Create")
                            .loading(self.creating)
                            .disabled(self.creating || check.is_err())
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
    }
}

/// What a bot's page tells the section.
pub enum BotEvent {
    /// "‹ My Bots" was clicked.
    Back,
    /// The bot was deleted, by its ID.
    Deleted(String),
}

/// One bot's page: its name, Copy ID, and Delete.
pub struct BotView {
    chat: WeakEntity<ChatView>,
    /// The bot as Stoat last sent it, token and all: never logged.
    bot: Bot,
    deleting: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BotEvent> for BotView {}

impl BotView {
    fn new(
        chat: &Entity<ChatView>,
        bot: Bot,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            chat: chat.downgrade(),
            bot,
            deleting: false,
            // Its name and avatar come from the store.
            _subscriptions: vec![cx.observe(chat, |_, _, cx| cx.notify())],
        }
    }

    /// The bot's user, as the store has it.
    fn user(&self, cx: &App) -> Option<User> {
        let chat = self.chat.upgrade()?;
        chat.read(cx).store().user(&self.bot.id).cloned()
    }

    /// Its display name, else its username, else its ID.
    fn name(&self, cx: &App) -> String {
        self.user(cx)
            .map(|user| user.display_name.unwrap_or(user.username))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| self.bot.id.clone())
    }

    /// Ask before deleting the bot.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (title, detail) = delete_prompt(&self.name(cx));
        let title = SharedString::from(title);
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let busy = view.upgrade().is_some_and(|view| view.read(cx).deleting);
            let delete = {
                let view = view.clone();
                move |window: &mut Window, cx: &mut App| {
                    view.update(cx, |this, cx| this.delete(window, cx)).ok();
                }
            };
            alert
                .title(title.clone())
                .description(detail)
                .keyboard(!busy)
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("delete-bot-cancel")
                                .label("Cancel")
                                .disabled(busy)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("delete-bot-ok")
                                .with_variant(ButtonVariant::Danger)
                                .label("Delete")
                                .loading(busy)
                                .disabled(busy)
                                .on_click({
                                    let delete = delete.clone();
                                    move |_, window, cx| delete(window, cx)
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    if !busy {
                        delete(window, cx);
                    }
                    false
                })
        });
    }

    /// Delete the bot (the confirmation's Delete).
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.deleting = true;
        window.refresh();
        let request = spawn_io({
            let (client, id) = (chat.read(cx).client().clone(), self.bot.id.clone());
            async move { client.delete_bot(&id).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.deleting = false;
                match result {
                    Ok(Ok(())) => {
                        log::info!("deleted bot {}", this.bot.id);
                        window.close_dialog(cx);
                        window.push_notification(Notification::success("Bot deleted."), cx);
                        cx.emit(BotEvent::Deleted(this.bot.id.clone()));
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't delete bot {}: {err}", this.bot.id);
                        window
                            .push_notification(Notification::error(bot_error_text(&err, None)), cx);
                    }
                    Err(err) => {
                        log::warn!("bot delete task failed: {err:?}");
                        window
                            .push_notification(Notification::error("Couldn't delete the bot."), cx);
                    }
                }
                window.refresh();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for BotView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let user = self.user(cx);
        let name = self.name(cx);
        let avatar_url = self.chat.upgrade().and_then(|chat| {
            let store = chat.read(cx).store();
            user.as_ref()
                .and_then(|user| user.avatar.as_ref())
                .and_then(|file| store.media().file(file))
        });
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let id = self.bot.id.clone();
        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex().w_full().gap_2().child(
                    Button::new("bots-back")
                        .label("My Bots")
                        .icon(IconName::ChevronLeft)
                        .ghost()
                        .small()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(BotEvent::Back))),
                ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(avatar(
                        format!("bot-page-avatar-{}", self.bot.id),
                        avatar_url,
                        &name,
                        PAGE_AVATAR,
                        theme.secondary,
                        theme.secondary_foreground,
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .children(user.map(|user| {
                                div()
                                    .truncate()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(format!("@{}#{}", user.username, user.discriminator))
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(self.bot.id.clone()),
                    )
                    .child(
                        Button::new("bot-copy-id")
                            .label("Copy ID")
                            .icon(IconName::Copy)
                            .outline()
                            .small()
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                                window.push_notification(Notification::success("Copied."), cx);
                            }),
                    ),
            )
            .child(
                div().pt_4().border_t_1().border_color(theme.border).child(
                    Button::new("bot-delete")
                        .label("Delete bot…")
                        .icon(IconName::Trash)
                        .with_variant(ButtonVariant::Danger)
                        .outline()
                        .small()
                        .disabled(self.deleting)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.confirm_delete(window, cx)),
                        ),
                ),
            )
    }
}
