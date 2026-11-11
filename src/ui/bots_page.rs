//! Settings' My Bots section, for one account: the user's bots, "Create
//! bot…", and each bot's page on the same page. The list is fetched the
//! first time the section shows; the bots (tokens and all) stay in this
//! view, and their users go to the store. What it lists and the texts are
//! [`crate::bots`].
//!
//! The settings window keeps one per account, as it does the Sessions page.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::models::{Bot, BotWithUserResponse, DataEditBot, User};
use crate::bots::{
    BotForm, BotRow, DEFAULT_BOT_LIMIT, INTERACTIONS_URL_MAX, at_limit, bot_error_text, bot_rows,
    check_bot_username, count_line, delete_prompt, invite_url, rename_edit, reset_prompt,
    token_reset_edit, username_change,
};
use crate::permission_matrix::with_value;
use crate::runtime::spawn_io;
use crate::ui::chat::{ChatView, avatar};
use crate::ui::permission_matrix::{Mode, PermissionMatrix};
use crate::ui::profile_editor::{ProfileEditor, field};
use crate::ui::server_dialogs::name_field;
use crate::ui::server_settings::{open_focused_dialog, section_heading};

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
                    BotEvent::Back => this.mode = PageMode::List,
                    BotEvent::Deleted(id) => {
                        this.bots.retain(|bot| &bot.id != id);
                        this.mode = PageMode::List;
                    }
                    BotEvent::Saved(saved) => {
                        if let Some(bot) = this.bots.iter_mut().find(|bot| bot.id == saved.id) {
                            *bot = (**saved).clone();
                        }
                    }
                }
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
    /// Stoat answered a change with the bot (token and all).
    Saved(Box<Bot>),
}

/// Which change is under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// The bar's Save: public, URL, requested permissions.
    Settings,
    Rename,
    Token,
}

/// One bot's page: its profile (the Profile page's form), its username
/// (with its own Save), whether it's public, its interactions URL and the
/// permissions it asks for (saved together), its token to copy or reset,
/// its ID and invite link to copy, and Delete.
pub struct BotView {
    chat: WeakEntity<ChatView>,
    /// The bot as Stoat last sent it, token and all: never logged.
    bot: Bot,
    /// Its avatar, banner, display name, pronouns and bio: the Profile
    /// page's form, for the bot.
    profile: Entity<ProfileEditor>,
    username: Entity<InputState>,
    url: Entity<InputState>,
    public: bool,
    permissions: u64,
    /// The bot's settings the fields were last filled with.
    filled: BotForm,
    /// The change waiting for Stoat, if any.
    busy: Option<Change>,
    deleting: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BotEvent> for BotView {}

impl BotView {
    fn new(chat: &Entity<ChatView>, bot: Bot, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let username = cx.new(|cx| InputState::new(window, cx));
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://"));
        let subscriptions = vec![
            // Its name and avatar come from the store.
            cx.observe(chat, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &username,
                window,
                |this, _, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.rename(window, cx),
                    _ => cx.notify(),
                },
            ),
            cx.subscribe_in(&url, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.save(window, cx),
                _ => cx.notify(),
            }),
        ];
        let profile = cx.new(|cx| ProfileEditor::for_bot(chat.clone(), bot.id.clone(), window, cx));
        let mut this = Self {
            chat: chat.downgrade(),
            bot,
            profile,
            username,
            url,
            public: false,
            permissions: 0,
            filled: BotForm::default(),
            busy: None,
            deleting: false,
            _subscriptions: subscriptions,
        };
        this.reset(window, cx);
        this.reset_username(window, cx);
        this
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

    /// The settings as edited.
    fn edited(&self, cx: &App) -> BotForm {
        BotForm {
            public: self.public,
            interactions_url: self.url.read(cx).value().to_string(),
            default_permissions: self.permissions,
        }
    }

    /// Fill the settings with the bot's values.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = BotForm::of(&self.bot);
        let url = form.interactions_url.clone();
        self.url
            .update(cx, |input, cx| input.set_value(url, window, cx));
        self.public = form.public;
        self.permissions = form.default_permissions;
        self.filled = form;
        cx.notify();
    }

    /// Fill the username field with the bot's username.
    fn reset_username(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.user(cx).map(|user| user.username).unwrap_or_default();
        self.username
            .update(cx, |input, cx| input.set_value(name, window, cx));
    }

    /// The rename the username field asks for, if any.
    fn rename_to(&self, cx: &App) -> Option<String> {
        let current = self.user(cx).map(|user| user.username).unwrap_or_default();
        username_change(&self.username.read(cx).value(), &current)
    }

    /// The bar's Save: send what changed in the settings.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        if let Some(edit) = self.edited(cx).changes(&self.bot) {
            self.send(Change::Settings, edit, window, cx);
        }
    }

    /// The username's own Save.
    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        if let Some(name) = self.rename_to(cx) {
            self.send(Change::Rename, rename_edit(name), window, cx);
        }
    }

    /// Send `edit`. Stoat's answer (the bot and its user) shows at once; a
    /// failure says why, and the fields keep their edits.
    fn send(
        &mut self,
        change: Change,
        edit: DataEditBot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.busy = Some(change);
        window.refresh();
        cx.notify();
        let request = spawn_io({
            let (client, id) = (chat.read(cx).client().clone(), self.bot.id.clone());
            async move { client.edit_bot(&id, &edit).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(Ok(saved)) => {
                        log::info!("saved bot {} ({change:?})", this.bot.id);
                        let BotWithUserResponse { bot, user } = saved;
                        this.bot = bot;
                        this.chat
                            .update(cx, |chat, cx| chat.bot_users_loaded(vec![user], cx))
                            .ok();
                        cx.emit(BotEvent::Saved(Box::new(this.bot.clone())));
                        let text = match change {
                            Change::Settings => {
                                this.reset(window, cx);
                                "Saved."
                            }
                            Change::Rename => {
                                this.reset_username(window, cx);
                                "Renamed."
                            }
                            Change::Token => {
                                window.close_dialog(cx);
                                "Token reset."
                            }
                        };
                        window.push_notification(Notification::success(text), cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't save bot {} ({change:?}): {err}", this.bot.id);
                        window
                            .push_notification(Notification::error(bot_error_text(&err, None)), cx);
                    }
                    Err(err) => {
                        log::warn!("bot save task failed: {err:?}");
                        window
                            .push_notification(Notification::error("Couldn't change the bot."), cx);
                    }
                }
                window.refresh();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Copy the token. It's never shown.
    fn copy_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.bot.token.clone()));
        window.push_notification(Notification::success("Token copied."), cx);
    }

    /// Ask before giving the bot a new token.
    fn confirm_reset_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (title, detail) = reset_prompt(&self.name(cx));
        let view = cx.entity().downgrade();
        let reset = move |window: &mut Window, cx: &mut App| {
            view.update(cx, |this, cx| {
                if this.busy.is_none() {
                    this.send(Change::Token, token_reset_edit(), window, cx);
                }
            })
            .ok();
        };
        let busy = {
            let view = cx.entity().downgrade();
            move |cx: &App| {
                view.upgrade()
                    .is_some_and(|view| view.read(cx).busy == Some(Change::Token))
            }
        };
        open_confirm(
            ConfirmText {
                title: title.into(),
                detail,
                confirm: "Reset token",
                id: "reset-token",
            },
            busy,
            reset,
            window,
            cx,
        );
    }

    /// Ask before deleting the bot.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (title, detail) = delete_prompt(&self.name(cx));
        let view = cx.entity().downgrade();
        let delete = move |window: &mut Window, cx: &mut App| {
            view.update(cx, |this, cx| this.delete(window, cx)).ok();
        };
        let busy = {
            let view = cx.entity().downgrade();
            move |cx: &App| view.upgrade().is_some_and(|view| view.read(cx).deleting)
        };
        open_confirm(
            ConfirmText {
                title: title.into(),
                detail,
                confirm: "Delete",
                id: "delete-bot",
            },
            busy,
            delete,
            window,
            cx,
        );
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

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let user = self.user(cx);
        let name = self.name(cx);
        let avatar_url = self.chat.upgrade().and_then(|chat| {
            let store = chat.read(cx).store();
            user.as_ref()
                .and_then(|user| user.avatar.as_ref())
                .and_then(|file| store.media().file(file))
        });
        let theme = cx.theme();
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
                            .text_color(theme.muted_foreground)
                            .child(format!("@{}#{}", user.username, user.discriminator))
                    })),
            )
            .into_any_element()
    }

    /// The username, with its own Save.
    fn render_username(&self, cx: &mut Context<Self>) -> AnyElement {
        let typed = self.username.read(cx).value();
        let problem = check_bot_username(&typed)
            .err()
            .map(|problem| problem.to_string());
        let count = typed.trim().chars().count();
        let renaming = self.busy == Some(Change::Rename);
        let can_rename = self.busy.is_none() && self.rename_to(cx).is_some();
        let input = h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&self.username).disabled(renaming)),
            )
            .child(
                Button::new("bot-rename")
                    .label("Save")
                    .outline()
                    .small()
                    .loading(renaming)
                    .disabled(!can_rename)
                    .on_click(cx.listener(|this, _, window, cx| this.rename(window, cx))),
            )
            .into_any_element();
        field(
            "Username",
            input,
            count,
            crate::bots::BOT_USERNAME_MAX,
            problem,
            cx,
        )
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let saving = self.busy == Some(Change::Settings);
        let muted = cx.theme().muted_foreground;
        let url_count = self.url.read(cx).value().trim().chars().count();
        let url = field(
            "Interactions URL",
            Input::new(&self.url).disabled(saving).into_any_element(),
            url_count,
            INTERACTIONS_URL_MAX,
            None,
            cx,
        );
        let public = h_flex()
            .gap_3()
            .child(
                v_flex()
                    .flex_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Public"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Anyone can add this bot to their servers and groups."),
                    ),
            )
            .child(
                Switch::new("bot-public")
                    .checked(self.public)
                    .disabled(saving)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.public = *on;
                        cx.notify();
                    })),
            );
        let view = cx.entity().downgrade();
        let matrix = PermissionMatrix::new("bot-permissions", Mode::Values(self.permissions))
            .read_only(saving)
            .on_change(move |bit, setting, _, cx| {
                view.update(cx, |this, cx| {
                    this.permissions = with_value(this.permissions, bit, setting);
                    cx.notify();
                })
                .ok();
            });
        v_flex()
            .gap_4()
            .child(public)
            .child(url)
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Requested permissions"),
                    )
                    .child(div().text_xs().text_color(muted).child(
                        "What it asks for when it's added to a server. Whoever adds it \
                         grants only those they hold themselves.",
                    ))
                    .child(matrix),
            )
            .into_any_element()
    }

    fn render_token(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let resetting = self.busy == Some(Change::Token);
        v_flex()
            .gap_2()
            .child(section_heading("Token"))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Anything with the token can act as the bot. Keep it secret."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme.secondary)
                            .text_sm()
                            .font_family("Menlo")
                            .child("••••••"),
                    )
                    .child(
                        Button::new("bot-copy-token")
                            .label("Copy token")
                            .icon(IconName::Copy)
                            .outline()
                            .small()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.copy_token(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("bot-reset-token")
                            .label("Reset token…")
                            .icon(IconName::RefreshCw)
                            .outline()
                            .small()
                            .loading(resetting)
                            .disabled(self.busy.is_some())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_reset_token(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_share(&self, cx: &mut Context<Self>) -> AnyElement {
        let id = self.bot.id.clone();
        let app = self
            .chat
            .upgrade()
            .and_then(|chat| chat.read(cx).web_app().map(str::to_string));
        let link = app.map(|app| invite_url(&app, &self.bot.id));
        v_flex()
            .gap_2()
            .child(section_heading("Share"))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.bot.id.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
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
                    )
                    .child(
                        Button::new("bot-add")
                            .label("Add to a server or group…")
                            .icon(IconName::Plus)
                            .outline()
                            .small()
                            .on_click({
                                let (chat, id) = (self.chat.clone(), self.bot.id.clone());
                                move |_, _, cx| add_in_main_window(chat.clone(), id.clone(), cx)
                            }),
                    )
                    .children(link.map(|link| {
                        Button::new("bot-copy-invite")
                            .label("Copy invite URL")
                            .icon(IconName::Link)
                            .outline()
                            .small()
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                                window.push_notification(Notification::success("Copied."), cx);
                            })
                    })),
            )
            .into_any_element()
    }
}

impl Render for BotView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let edited = self.edited(cx);
        let saving = self.busy == Some(Change::Settings);
        let changed = edited != self.filled;
        let can_save = self.busy.is_none() && edited.changes(&self.bot).is_some();
        let border = cx.theme().border;
        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex().w_full().child(
                    Button::new("bots-back")
                        .label("My Bots")
                        .icon(IconName::ChevronLeft)
                        .ghost()
                        .small()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(BotEvent::Back))),
                ),
            )
            .child(self.render_header(cx))
            .child(
                v_flex()
                    .gap_4()
                    .child(section_heading("Profile"))
                    .child(self.profile.clone()),
            )
            .child(
                v_flex()
                    .gap_4()
                    .child(section_heading("Settings"))
                    .child(self.render_username(cx))
                    .child(self.render_settings(cx)),
            )
            // The settings' own Reset and Save (the profile has its own).
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("bot-reset")
                            .label("Reset")
                            .outline()
                            .small()
                            .disabled(!changed || saving)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    )
                    .child(
                        Button::new("bot-save")
                            .label("Save")
                            .primary()
                            .small()
                            .loading(saving)
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
            .child(self.render_token(cx))
            .child(self.render_share(cx))
            .child(
                div().pt_4().border_t_1().border_color(border).child(
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

/// A confirmation's words.
struct ConfirmText {
    title: SharedString,
    detail: &'static str,
    /// The danger button's label.
    confirm: &'static str,
    /// Its buttons' IDs start with this.
    id: &'static str,
}

/// An alert asking before something that can't be undone. Its buttons wait
/// for the answer (`busy`); `act` closes it when it's done.
fn open_confirm(
    text: ConfirmText,
    busy: impl Fn(&App) -> bool + 'static,
    act: impl Fn(&mut Window, &mut App) + Clone + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let busy = busy(cx);
        let act = act.clone();
        alert
            .title(text.title.clone())
            .description(text.detail)
            .keyboard(!busy)
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new(SharedString::from(format!("{}-cancel", text.id)))
                            .label("Cancel")
                            .disabled(busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{}-ok", text.id)))
                            .with_variant(ButtonVariant::Danger)
                            .label(text.confirm)
                            .loading(busy)
                            .disabled(busy)
                            .on_click({
                                let act = act.clone();
                                move |_, window, cx| act(window, cx)
                            }),
                    ),
            )
            .on_ok(move |_, window, cx| {
                if !busy {
                    act(window, cx);
                }
                false
            })
    });
}

/// Open the Add bot dialog for `bot` in the account's own window (the main
/// window), and bring it forward. Deferred: the click arrives while GPUI is
/// updating the settings window.
fn add_in_main_window(chat: WeakEntity<ChatView>, bot: String, cx: &mut App) {
    cx.defer(move |cx| {
        let Some(chat) = chat.upgrade() else {
            return;
        };
        let handle = chat.read(cx).window_handle();
        handle
            .update(cx, |_, window, cx| {
                window.activate_window();
                chat.update(cx, |chat, cx| chat.open_add_bot(bot, window, cx));
            })
            .ok();
        cx.activate(true);
    });
}
