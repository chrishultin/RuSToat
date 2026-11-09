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

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::setting::{SelectIndex, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::ApiError;
use crate::api::models::Server;
use crate::profile_edit::{ImageLimits, ProfileImage};
use crate::runtime::spawn_io;
use crate::server_settings::{
    Access, DESCRIPTION_MAX, NAME_MAX, OverviewForm, ServerImage, access, save_error_text,
};
use crate::themes;
use crate::ui::chat::ChatView;
use crate::ui::profile_editor::{
    ImageSlot, Preview, avatar_preview, banner_preview, field, prompt_for_image, render_image_row,
    upload_image,
};

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

/// Overview: the server's icon, banner, name and description.
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
                    "icon",
                    "banner",
                ]),
            )),
    )
}

/// What a Save is doing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Saving {
    Uploading,
    Sending,
}

/// The Overview form: the server's icon, banner, name and description,
/// saved together with one `PATCH /servers/{id}` (see
/// [`OverviewForm::changes`]). New images are uploaded first, unchanged.
pub struct OverviewPage {
    chat: WeakEntity<ChatView>,
    server: String,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    icon: ImageSlot,
    banner: ImageSlot,
    /// The server's values the fields were last filled with. A field still
    /// showing its value is untouched, and follows changes made elsewhere;
    /// once it's edited, it keeps the edit.
    filled: OverviewForm,
    saving: Option<Saving>,
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
            icon: ImageSlot::default(),
            banner: ImageSlot::default(),
            filled: OverviewForm::default(),
            saving: None,
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
            icon: self.icon.image.clone(),
            banner: self.banner.image.clone(),
        }
    }

    /// Whether Save can send the form: every field and image fine,
    /// something changed, and not already saving.
    fn can_save(&self, edited: &OverviewForm, cx: &App) -> bool {
        self.saving.is_none()
            && edited.is_valid()
            && self.icon.problem.is_none()
            && self.banner.problem.is_none()
            && self
                .current(cx)
                .is_some_and(|server| edited.has_changes(&server))
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
        self.icon = slot(form.icon.clone());
        self.banner = slot(form.banner.clone());
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
        if edited.icon == self.filled.icon && self.icon.problem.is_none() {
            self.icon = slot(now.icon.clone());
        }
        if edited.banner == self.filled.banner && self.banner.problem.is_none() {
            self.banner = slot(now.banner.clone());
        }
        self.filled = now;
        cx.notify();
    }

    fn slot(&mut self, kind: ServerImage) -> &mut ImageSlot {
        match kind {
            ServerImage::Icon => &mut self.icon,
            ServerImage::Banner => &mut self.banner,
        }
    }

    /// Change… on an image row.
    fn pick_image(&mut self, kind: ServerImage, window: &mut Window, cx: &mut Context<Self>) {
        let limits = self
            .chat
            .upgrade()
            .map(|chat| chat.read(cx).image_limits())
            .unwrap_or_default();
        prompt_for_image(window, cx, move |this: &mut Self, path, _| {
            this.slot(kind).pick(path, kind.limit(limits));
        });
    }

    /// Send what changed: upload any new images first, then one `PATCH`
    /// with their IDs, only if every upload worked. Stoat's answer shows at
    /// once, in the main window too; a failure says why in a toast, and the
    /// form keeps the edit.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edited = self.edited(cx);
        if !self.can_save(&edited, cx) {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let (client, file_server) = {
            let chat = chat.read(cx);
            let file_server = chat.store().media().file_server().map(str::to_string);
            (chat.client().clone(), file_server)
        };
        let uploads: Vec<(ServerImage, PathBuf)> = edited
            .new_files()
            .into_iter()
            .map(|(kind, path)| (kind, path.to_path_buf()))
            .collect();
        let file_server = match (file_server, uploads.is_empty()) {
            (Some(server), _) => server,
            (None, true) => String::new(),
            (None, false) => {
                window.push_notification(
                    Notification::error("Couldn't upload: Stoat's file server isn't known yet."),
                    cx,
                );
                return;
            }
        };
        self.saving = Some(if uploads.is_empty() {
            Saving::Sending
        } else {
            Saving::Uploading
        });
        cx.notify();
        let id = self.server.clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut edited = edited;
            for (kind, path) in uploads {
                let upload = spawn_io(upload_image(
                    client.clone(),
                    file_server.clone(),
                    kind.tag(),
                    path,
                ));
                let uploaded = match upload.await {
                    Ok(Ok(uploaded)) => uploaded,
                    failed => {
                        let failed = failed.map(|result| result.map(|_| ()));
                        this.update_in(cx, |this, window, cx| this.save_failed(failed, window, cx))
                            .ok();
                        return;
                    }
                };
                match kind {
                    ServerImage::Icon => edited.icon = ProfileImage::Current(uploaded),
                    ServerImage::Banner => edited.banner = ProfileImage::Current(uploaded),
                }
            }
            // Against the server as it is now, which may have changed while
            // the images uploaded.
            let Ok(edit) = this.update(cx, |this, cx| {
                this.saving = Some(Saving::Sending);
                cx.notify();
                this.current(cx).and_then(|server| edited.changes(&server))
            }) else {
                return;
            };
            let Some(edit) = edit else {
                this.update(cx, |this, cx| {
                    this.saving = None;
                    cx.notify();
                })
                .ok();
                return;
            };
            let request = spawn_io(async move { client.edit_server(&id, &edit).await });
            let result = request.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(saved)) => {
                    this.saving = None;
                    if let Some(chat) = this.chat.upgrade() {
                        chat.update(cx, |chat, cx| chat.server_edited(&saved, cx));
                    }
                    this.reset(window, cx);
                    window.push_notification(Notification::success("Saved."), cx);
                }
                failed => this.save_failed(failed.map(|result| result.map(|_| ())), window, cx),
            })
            .ok();
        })
        .detach();
    }

    /// An upload or the `PATCH` failed: say why, and keep what was edited.
    fn save_failed(
        &mut self,
        result: Result<Result<(), ApiError>, impl std::fmt::Debug>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving = None;
        let text = match result {
            Ok(Err(err)) => {
                log::warn!("couldn't save server {}: {err:?}", self.server);
                save_error_text(&err)
            }
            Ok(Ok(())) => return,
            Err(err) => {
                log::warn!("server save task failed: {err:?}");
                "Couldn't save the server's settings.".into()
            }
        };
        window.push_notification(Notification::error(text), cx);
        cx.notify();
    }

    /// The icon's and banner's rows.
    fn render_images(&self, cx: &mut Context<Self>) -> Option<[AnyElement; 2]> {
        let chat = self.chat.upgrade()?;
        let chat = chat.read(cx);
        let store = chat.store();
        let limits = chat.image_limits();
        let media = store.media();
        let name = store
            .server(&self.server)
            .map(|server| server.name.as_str())
            .unwrap_or_default();
        let icon = avatar_preview(
            "server-icon-preview",
            Preview::of(
                &self.icon.image,
                ServerImage::Icon.tag(),
                media,
                Preview::Empty,
            ),
            name,
            cx,
        );
        let banner = banner_preview(
            "server-banner-preview",
            Preview::of(
                &self.banner.image,
                ServerImage::Banner.tag(),
                media,
                Preview::Empty,
            ),
            cx,
        );
        Some([
            self.image_row(ServerImage::Icon, icon, limits, cx),
            self.image_row(ServerImage::Banner, banner, limits, cx),
        ])
    }

    fn image_row(
        &self,
        kind: ServerImage,
        preview: AnyElement,
        limits: ImageLimits,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (id, label, slot) = match kind {
            ServerImage::Icon => ("server-icon", "Server icon", &self.icon),
            ServerImage::Banner => ("server-banner", "Server banner", &self.banner),
        };
        render_image_row(
            id,
            label,
            preview,
            slot,
            kind.limit(limits),
            self.saving.is_some(),
            cx.listener(move |this, _, window, cx| this.pick_image(kind, window, cx)),
            cx.listener(move |this, _, _, cx| {
                this.slot(kind).remove();
                cx.notify();
            }),
            cx,
        )
    }
}

/// An image slot showing `image`, with nothing wrong.
fn slot(image: ProfileImage) -> ImageSlot {
    ImageSlot {
        image,
        problem: None,
    }
}

impl Render for OverviewPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let edited = self.edited(cx);
        let saving = self.saving.is_some();
        let changed =
            edited != self.filled || self.icon.problem.is_some() || self.banner.problem.is_some();
        let can_save = self.can_save(&edited, cx);
        let images = self.render_images(cx);
        let name_count = edited.name.trim().chars().count();
        let name = field(
            "Server name",
            Input::new(&self.name).disabled(saving).into_any_element(),
            name_count,
            NAME_MAX,
            (name_count == 0).then(|| "A server needs a name.".to_string()),
            cx,
        );
        let description = field(
            "Server description",
            Textarea::new(&self.description)
                .disabled(saving)
                .into_any_element(),
            edited.description.trim().chars().count(),
            DESCRIPTION_MAX,
            None,
            cx,
        );
        v_flex()
            .w_full()
            .gap_4()
            .children(images.into_iter().flatten())
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
                            .disabled(!changed || saving)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    )
                    .child(
                        Button::new("overview-save")
                            .label(match self.saving {
                                Some(Saving::Uploading) => "Uploading…",
                                _ => "Save",
                            })
                            .primary()
                            .small()
                            .loading(saving)
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}
