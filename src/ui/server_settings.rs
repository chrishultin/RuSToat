//! The Server Settings window: one server's pages, for its moderators and
//! owner, in a window of its own beside the main one. There's only ever one,
//! for one server of one account ([`ServerSettingsWindow`]).
//!
//! A page is a function returning it, or `None` when the user may not see
//! it ([`ServerPageFn`]), so a later page is one more function in
//! [`ServerSettingsView::PAGES`]. Who sees what is
//! [`crate::server_settings::access`].
//!
//! The window follows its account's [`ChatView`], and closes itself when
//! the server leaves the store (deleted, left, kicked or banned), when the
//! user may see no page of it any more, or when the account signs out.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::setting::{SelectIndex, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::models::Server;
use crate::runtime::spawn_io;
use crate::server_settings::{
    Access, DESCRIPTION_MAX, NAME_MAX, OverviewForm, access, save_error_text,
};
use crate::themes;
use crate::ui::chat::ChatView;
use crate::ui::profile_editor::field;

/// Rows the description field grows between before it scrolls.
const DESCRIPTION_ROWS: (usize, usize) = (3, 10);

/// Builds one page of the window, each time it renders: `None` hides it.
pub type ServerPageFn = fn(&ServerSettingsView, &mut App) -> Option<SettingPage>;

/// The Server Settings window and its content, while it's open.
#[derive(Default)]
pub struct ServerSettingsWindow(Option<(WindowHandle<Root>, WeakEntity<ServerSettingsView>)>);

/// gpui-component's window root, which hosts dialogs and toasts.
type Root = gpui_kit::component::Root;

impl Global for ServerSettingsWindow {}

impl ServerSettingsWindow {
    /// Open `server`'s settings for `chat`'s account, or bring them forward
    /// if they're already open. Another server's (or account's) settings
    /// close first, unsaved edits and all. Deferred: the click that asks
    /// arrives while GPUI is updating the main window.
    pub fn open(chat: Entity<ChatView>, server: String, cx: &mut App) {
        cx.defer(move |cx| {
            if let Some((window, view)) = cx.default_global::<Self>().0.clone() {
                let same = view
                    .upgrade()
                    .is_some_and(|view| view.read(cx).is_for(&chat, &server));
                let open = window
                    .update(cx, |_, window, _| {
                        if same {
                            window.activate_window();
                        } else {
                            window.remove_window();
                        }
                    })
                    .is_ok();
                if same && open {
                    cx.activate(true);
                    return;
                }
            }
            match Self::open_new(chat, server, cx) {
                Ok(opened) => cx.global_mut::<Self>().0 = Some(opened),
                Err(err) => log::error!("couldn't open the server settings window: {err:#}"),
            }
        });
    }

    fn open_new(
        chat: Entity<ChatView>,
        server: String,
        cx: &mut App,
    ) -> anyhow::Result<(WindowHandle<Root>, WeakEntity<ServerSettingsView>)> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(760.), px(560.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(window_title(chat.read(cx).store().server(&server)).into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (window, view) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| ServerSettingsView::new(chat, server, window, cx))
        })?;
        cx.activate(true);
        let window = window
            .downcast()
            .ok_or_else(|| anyhow::anyhow!("the server settings window isn't a Root"))?;
        Ok((window, view.downgrade()))
    }

    /// Close the window, if it's open.
    pub fn close(cx: &mut App) {
        let Some((window, _)) = cx.try_global::<Self>().and_then(|this| this.0.clone()) else {
            return;
        };
        window
            .update(cx, |_, window, _| window.remove_window())
            .ok();
    }
}

/// "Server Settings — {name}".
fn window_title(server: Option<&Server>) -> String {
    match server {
        Some(server) => format!("Server Settings — {}", server.name),
        None => "Server Settings".into(),
    }
}

/// One server's settings.
pub struct ServerSettingsView {
    /// The account's chat: its client and store.
    chat: WeakEntity<ChatView>,
    server: String,
    pages: Vec<ServerPageFn>,
    /// Which pages showed at the last render. When that changes, the
    /// sidebar goes back to its first page: gpui-component picks pages by
    /// position, so it would otherwise show the wrong one.
    shown: Vec<bool>,
    /// Bumped to go back to the first page: a new ID gives the sidebar new
    /// state, which starts there.
    page_key: usize,
    /// The window's title, as last set.
    title: String,
    overview: Entity<OverviewPage>,
    _subscriptions: Vec<Subscription>,
}

impl ServerSettingsView {
    /// The pages, in sidebar order.
    pub const PAGES: [ServerPageFn; 1] = [overview_page];

    pub fn new(
        chat: Entity<ChatView>,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            // Follow the server, and close once it's gone or there's
            // nothing left to show of it.
            cx.observe_in(&chat, window, |this, chat, window, cx| {
                let store = chat.read(cx).store();
                if store.server(&this.server).is_none() || !access(store, &this.server).any() {
                    log::debug!("closing server settings for {}", this.server);
                    window.remove_window();
                    return;
                }
                cx.notify();
            }),
            // The account signed out, or its window really closed.
            cx.observe_release_in(&chat, window, |_, _, window, _| window.remove_window()),
            // Follow macOS's light/dark setting even with the main window
            // closed.
            cx.observe_window_appearance(window, |_, _, cx| themes::apply_current(cx)),
        ];
        let title = window_title(chat.read(cx).store().server(&server));
        let overview = cx.new(|cx| OverviewPage::new(&chat, server.clone(), window, cx));
        Self {
            chat: chat.downgrade(),
            server,
            pages: Self::PAGES.to_vec(),
            shown: Vec::new(),
            page_key: 0,
            title,
            overview,
            _subscriptions: subscriptions,
        }
    }

    /// Whether these are `server`'s settings for `chat`'s account.
    pub fn is_for(&self, chat: &Entity<ChatView>, server: &str) -> bool {
        self.chat.entity_id() == chat.entity_id() && self.server == server
    }

    /// The account's chat, while it's signed in.
    pub fn chat(&self) -> Option<Entity<ChatView>> {
        self.chat.upgrade()
    }

    /// The server these settings are for.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// Which pages the user may see now.
    pub fn access(&self, cx: &App) -> Access {
        self.chat()
            .map(|chat| access(chat.read(cx).store(), &self.server))
            .unwrap_or_default()
    }
}

impl Render for ServerSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = window_title(
            self.chat()
                .as_ref()
                .and_then(|chat| chat.read(cx).store().server(&self.server)),
        );
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        let pages: Vec<Option<SettingPage>> =
            self.pages.iter().map(|page| page(self, cx)).collect();
        let shown: Vec<bool> = pages.iter().map(Option::is_some).collect();
        if shown != self.shown {
            if !self.shown.is_empty() {
                self.page_key += 1;
            }
            self.shown = shown;
        }
        div().size_full().bg(cx.theme().background).child(
            Settings::new(("server-settings", self.page_key))
                .default_selected_index(SelectIndex::default())
                .pages(pages.into_iter().flatten()),
        )
    }
}

/// Overview: the server's name and description.
fn overview_page(view: &ServerSettingsView, cx: &mut App) -> Option<SettingPage> {
    if !view.access(cx).overview {
        return None;
    }
    let overview = view.overview.clone();
    Some(
        SettingPage::new("Overview")
            .icon(IconName::Settings)
            .resettable(false)
            .group(SettingGroup::new().item(
                SettingItem::render(move |_, _, _| overview.clone()).keywords([
                    "overview",
                    "name",
                    "description",
                ]),
            )),
    )
}

/// The Overview form: the server's name and description, saved together
/// with one `PATCH /servers/{id}` (see [`OverviewForm::changes`]).
pub struct OverviewPage {
    chat: WeakEntity<ChatView>,
    server: String,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    /// The server's values the fields were last filled with. A field still
    /// showing its value is untouched, and follows changes made elsewhere;
    /// once it's edited, it keeps the edit.
    filled: OverviewForm,
    saving: bool,
    _subscriptions: Vec<Subscription>,
}

impl OverviewPage {
    fn new(
        chat: &Entity<ChatView>,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(DESCRIPTION_ROWS.0, DESCRIPTION_ROWS.1)
                .placeholder("This server is about…")
        });
        let subscriptions = vec![
            // Counters and Save follow what's typed.
            cx.subscribe(&name, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&description, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.observe_in(chat, window, |this, _, window, cx| this.follow(window, cx)),
        ];
        let mut this = Self {
            chat: chat.downgrade(),
            server,
            name,
            description,
            filled: OverviewForm::default(),
            saving: false,
            _subscriptions: subscriptions,
        };
        this.reset(window, cx);
        this
    }

    /// The server as the store has it now.
    fn current(&self, cx: &App) -> Option<Server> {
        let chat = self.chat.upgrade()?;
        chat.read(cx).store().server(&self.server).cloned()
    }

    /// The form as edited.
    fn edited(&self, cx: &App) -> OverviewForm {
        OverviewForm {
            name: self.name.read(cx).value().to_string(),
            description: self.description.read(cx).value().to_string(),
        }
    }

    /// Fill every field with the server's values.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(server) = self.current(cx) else {
            return;
        };
        let form = OverviewForm::of(&server);
        self.name.update(cx, |input, cx| {
            input.set_value(form.name.clone(), window, cx)
        });
        self.description.update(cx, |input, cx| {
            input.set_value(form.description.clone(), window, cx)
        });
        self.filled = form;
        cx.notify();
    }

    /// The server changed (here or elsewhere): untouched fields take its new
    /// values, edited ones keep theirs.
    fn follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(server) = self.current(cx) else {
            return;
        };
        let now = OverviewForm::of(&server);
        if now == self.filled {
            return;
        }
        let edited = self.edited(cx);
        if edited.name == self.filled.name && now.name != self.filled.name {
            let name = now.name.clone();
            self.name
                .update(cx, |input, cx| input.set_value(name, window, cx));
        }
        if edited.description == self.filled.description
            && now.description != self.filled.description
        {
            let description = now.description.clone();
            self.description
                .update(cx, |input, cx| input.set_value(description, window, cx));
        }
        self.filled = now;
        cx.notify();
    }

    /// Send what changed. Stoat's answer shows at once, in the main window
    /// too; a refusal says why in a toast, and the form keeps the edit.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let (Some(chat), Some(server)) = (self.chat.upgrade(), self.current(cx)) else {
            return;
        };
        let Some(edit) = self.edited(cx).changes(&server) else {
            return;
        };
        self.saving = true;
        cx.notify();
        let request = spawn_io({
            let (client, id) = (chat.read(cx).client().clone(), self.server.clone());
            async move { client.edit_server(&id, &edit).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(Ok(saved)) => {
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.server_edited(&saved, cx));
                        }
                        this.reset(window, cx);
                        window.push_notification(Notification::success("Saved."), cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't save server {}: {err:?}", this.server);
                        window.push_notification(Notification::error(save_error_text(&err)), cx);
                    }
                    Err(err) => {
                        log::warn!("server save task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't save the server's settings."),
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
}

impl Render for OverviewPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let edited = self.edited(cx);
        let server = self.current(cx);
        let changed = edited != self.filled;
        let can_save = !self.saving
            && server
                .as_ref()
                .is_some_and(|server| edited.changes(server).is_some());
        let name_count = edited.name.trim().chars().count();
        let name = field(
            "Server name",
            Input::new(&self.name)
                .disabled(self.saving)
                .into_any_element(),
            name_count,
            NAME_MAX,
            (name_count == 0).then(|| "A server needs a name.".to_string()),
            cx,
        );
        let description = field(
            "Server description",
            Textarea::new(&self.description)
                .disabled(self.saving)
                .into_any_element(),
            edited.description.trim().chars().count(),
            DESCRIPTION_MAX,
            None,
            cx,
        );
        v_flex()
            .w_full()
            .gap_4()
            .child(name)
            .child(description)
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("overview-reset")
                            .label("Reset")
                            .outline()
                            .small()
                            .disabled(!changed || self.saving)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    )
                    .child(
                        Button::new("overview-save")
                            .label("Save")
                            .primary()
                            .small()
                            .loading(self.saving)
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}
