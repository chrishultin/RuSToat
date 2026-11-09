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

use revolt_result::ErrorType;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::setting::{SelectIndex, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::ApiError;
use crate::api::models::Server;
use crate::links::invite_link;
use crate::profile_edit::{ImageLimits, ProfileImage};
use crate::runtime::spawn_io;
use crate::server_settings::{
    Access, BanRow, DESCRIPTION_MAX, InviteRow, NAME_MAX, OverviewForm, ServerImage,
    SystemChannels, SystemMessageKind, access, ban_rows, bans_error_text, filter_bans, invite_rows,
    invites_error_text, revoke_error_text, save_error_text, system_channel_choices,
    unban_error_text,
};
use crate::themes;
use crate::ui::chat::{ChatView, avatar};
use crate::ui::invites::{CREATE_DIALOG_WIDTH, CreateInviteDialog, InviteCreated};
use crate::ui::moderation_dialogs::{MemberActionRequest, confirm_member_action_then};
use crate::ui::profile_editor::{
    ImageSlot, Preview, avatar_preview, banner_preview, field, prompt_for_image, render_image_row,
    upload_image,
};

/// Rows the description field grows between before it scrolls.
const DESCRIPTION_ROWS: (usize, usize) = (3, 10);
/// The tallest a system-message channel menu grows before it scrolls.
const SYSTEM_MENU_HEIGHT: f32 = 320.;

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
    invites: Entity<InvitesPage>,
    bans: Entity<BansPage>,
    _subscriptions: Vec<Subscription>,
}

impl ServerSettingsView {
    /// The pages, in sidebar order.
    pub const PAGES: [ServerPageFn; 3] = [overview_page, invites_page, bans_page];

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
        let invites = cx.new(|_| InvitesPage::new(&chat, server.clone()));
        let bans = cx.new(|cx| BansPage::new(&chat, server.clone(), window, cx));
        Self {
            chat: chat.downgrade(),
            server,
            pages: Self::PAGES.to_vec(),
            shown: Vec::new(),
            page_key: 0,
            title,
            overview,
            invites,
            bans,
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
        // Lists are fetched the first time they may be shown.
        let access = self.access(cx);
        if access.invites {
            self.invites
                .update(cx, |page, cx| page.load_once(window, cx));
        }
        if access.bans {
            self.bans.update(cx, |page, cx| page.load_once(window, cx));
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

/// Overview: the server's icon, banner, name, description and
/// system-message channels.
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
                    "system messages",
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
    system: SystemChannels,
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
            system: SystemChannels::default(),
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
            system: self.system.clone(),
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
        self.system = form.system.clone();
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
        for kind in SystemMessageKind::ALL {
            if edited.system.get(kind) == self.filled.system.get(kind) {
                let channel = now.system.get(kind).map(str::to_string);
                self.system.set(kind, channel);
            }
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

impl OverviewPage {
    /// "System messages": where each kind goes, picked from the server's
    /// channels or Off.
    fn render_system_messages(&self, cx: &Context<Self>) -> AnyElement {
        let choices = self
            .chat
            .upgrade()
            .map(|chat| system_channel_choices(chat.read(cx).store(), &self.server))
            .unwrap_or_default();
        let theme = cx.theme();
        v_flex()
            .gap_2()
            .child(
                v_flex()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("System messages"),
                    )
                    .child(
                        div().text_xs().text_color(theme.muted_foreground).child(
                            "Where Stoat says someone joined, left, or was kicked or banned.",
                        ),
                    ),
            )
            .children(SystemMessageKind::ALL.map(|kind| {
                let chosen = self.system.get(kind).map(str::to_string);
                let value = match &chosen {
                    Some(id) => choices
                        .iter()
                        .find(|(listed, _)| listed == id)
                        .map_or_else(|| "Unknown channel".into(), |(_, name)| format!("#{name}")),
                    None => "Off".into(),
                };
                h_flex()
                    .justify_between()
                    .gap_3()
                    .child(div().text_sm().child(kind.label()))
                    .child(
                        Button::new(SharedString::from(format!("system-{kind:?}")))
                            .outline()
                            .small()
                            .label(value)
                            .icon(IconName::ChevronDown)
                            .disabled(self.saving.is_some())
                            .dropdown_menu(self.system_menu(kind, chosen, choices.clone(), cx)),
                    )
            }))
            .into_any_element()
    }

    /// The menu for one kind: Off, then each channel, the chosen one ticked.
    fn system_menu(
        &self,
        kind: SystemMessageKind,
        chosen: Option<String>,
        choices: Vec<(String, String)>,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let entity = cx.entity().downgrade();
        move |menu, _, _| {
            let pick = |channel: Option<String>| {
                let entity = entity.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    entity
                        .update(cx, |this, cx| {
                            this.system.set(kind, channel.clone());
                            cx.notify();
                        })
                        .ok();
                }
            };
            let menu = menu.scrollable(true).max_h(px(SYSTEM_MENU_HEIGHT)).item(
                PopupMenuItem::new("Off")
                    .checked(chosen.is_none())
                    .on_click(pick(None)),
            );
            choices.iter().fold(menu, |menu, (id, name)| {
                menu.item(
                    PopupMenuItem::new(format!("#{name}"))
                        .checked(chosen.as_ref() == Some(id))
                        .on_click(pick(Some(id.clone()))),
                )
            })
        }
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
            .child(self.render_system_messages(cx))
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

/// Where loading one of the window's lists is at.
enum Load<T> {
    /// Not asked for yet.
    Idle,
    Loading {
        _task: Task<()>,
    },
    Loaded(T),
    /// Why it couldn't be loaded.
    Failed(String),
}

/// Invites: every invite to the server, with who made it and to where.
fn invites_page(view: &ServerSettingsView, cx: &mut App) -> Option<SettingPage> {
    if !view.access(cx).invites {
        return None;
    }
    let invites = view.invites.clone();
    Some(
        SettingPage::new("Invites")
            .icon(IconName::Link)
            .resettable(false)
            .group(
                SettingGroup::new().item(
                    SettingItem::render(move |_, _, _| invites.clone())
                        .keywords(["invites", "invite", "links", "revoke"]),
                ),
            ),
    )
}

/// The Invites page: the server's invites, each with Copy link and Revoke,
/// and "Create invite…". Stoat sends no event for invites, so the list is
/// changed here after a revoke, and fetched again after a create.
pub struct InvitesPage {
    chat: WeakEntity<ChatView>,
    server: String,
    invites: Load<Vec<InviteRow>>,
    _create: Option<Subscription>,
}

impl InvitesPage {
    fn new(chat: &Entity<ChatView>, server: String) -> Self {
        Self {
            chat: chat.downgrade(),
            server,
            invites: Load::Idle,
            _create: None,
        }
    }

    /// Fetch the list, unless that's already been done.
    fn load_once(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.invites, Load::Idle) {
            self.refresh(window, cx);
        }
    }

    /// Fetch the list again.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let (client, server) = (chat.read(cx).client().clone(), self.server.clone());
            async move { client.fetch_server_invites(&server).await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.invites = match result {
                    Ok(Ok(invites)) => {
                        log::debug!("{} invites to {}", invites.len(), this.server);
                        match this.chat.upgrade() {
                            Some(chat) => Load::Loaded(invite_rows(
                                chat.read(cx).store(),
                                &invites,
                                chrono::Utc::now(),
                            )),
                            None => Load::Idle,
                        }
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't load invites to {}: {err:?}", this.server);
                        Load::Failed(invites_error_text(&err))
                    }
                    Err(err) => {
                        log::warn!("invites task failed: {err:?}");
                        Load::Failed("Couldn't load the invites.".into())
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.invites = Load::Loading { _task: task };
        cx.notify();
    }

    /// The channel "Create invite…" makes an invite to: the first one the
    /// user may invite to.
    fn invite_channel(&self, cx: &App) -> Option<String> {
        let chat = self.chat.upgrade()?;
        chat.read(cx).first_invitable_channel(&self.server)
    }

    /// "Invite people to {server}", in this window; the list is fetched
    /// again once it's made one.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(chat), Some(channel)) = (self.chat.upgrade(), self.invite_channel(cx)) else {
            return;
        };
        let (built, title) = {
            let chat = chat.read(cx);
            let Some(name) = chat.store().server(&self.server).map(|s| s.name.clone()) else {
                return;
            };
            let built = CreateInviteDialog::new(chat, channel, chat.invite_max_days());
            (
                built,
                SharedString::from(format!("Invite people to {name}")),
            )
        };
        let dialog = cx.new(|_| built);
        self._create = Some(cx.subscribe_in(
            &dialog,
            window,
            |this, _, _: &InviteCreated, window, cx| this.refresh(window, cx),
        ));
        window.open_dialog(cx, move |modal, _, _| {
            modal
                .title(title.clone())
                .w(px(CREATE_DIALOG_WIDTH))
                .child(dialog.clone())
        });
    }

    /// Copy an invite's link (or its code, if the web client's address
    /// isn't known).
    fn copy(&self, row: &InviteRow, window: &mut Window, cx: &mut Context<Self>) {
        let app = self
            .chat
            .upgrade()
            .and_then(|chat| chat.read(cx).web_app().map(str::to_string));
        let (text, toast) = match app {
            Some(app) => (invite_link(&app, &row.code), "Copied the invite link."),
            None => (row.code.clone(), "Copied the invite code."),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        window.push_notification(Notification::success(toast), cx);
    }

    /// Revoke an invite, once confirmed. Its row goes when Stoat agrees, or
    /// says it's already gone.
    fn revoke(&mut self, row: &InviteRow, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let client = chat.read(cx).client().clone();
        let code = row.code.clone();
        let request = MemberActionRequest {
            title: format!("Revoke invite {code}?").into(),
            detail: "Its link will stop working. Anyone who joined with it stays.".into(),
            ok_label: "Revoke",
            danger: true,
            failed_text: "Couldn't revoke the invite.".into(),
        };
        let page = cx.entity().downgrade();
        let action = {
            let code = code.clone();
            Box::pin(async move { client.delete_invite(&code).await })
        };
        let after = Box::new(
            move |result: Result<(), ApiError>, window: &mut Window, cx: &mut App| match result {
                Err(err) if !matches!(err.error_type(), Some(ErrorType::NotFound)) => {
                    log::warn!("couldn't revoke invite {code}: {err:?}");
                    window.push_notification(Notification::error(revoke_error_text(&err)), cx);
                }
                _ => {
                    page.update(cx, |page, cx| {
                        if let Load::Loaded(rows) = &mut page.invites {
                            rows.retain(|row| row.code != code);
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        );
        confirm_member_action_then(request, action, after, window, cx);
    }

    fn render_row(&self, row: &InviteRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let face = avatar(
            SharedString::from(format!("invite-creator-{}", row.code)),
            row.creator_avatar.clone(),
            &row.creator_name,
            INVITE_AVATAR,
            theme.secondary,
            theme.secondary_foreground,
        );
        let copy = {
            let row = row.clone();
            cx.listener(move |this, _, window, cx| this.copy(&row, window, cx))
        };
        let revoke = {
            let row = row.clone();
            cx.listener(move |this, _, window, cx| this.revoke(&row, window, cx))
        };
        h_flex()
            .w_full()
            .gap_3()
            .py_1()
            .child(face)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(row.creator_name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} · {}", row.channel, row.code)),
                    ),
            )
            .child(
                v_flex()
                    .items_end()
                    .text_xs()
                    .text_color(muted)
                    .child(row.uses.clone())
                    .child(row.expiry.clone()),
            )
            .child(
                Button::new(SharedString::from(format!("invite-copy-{}", row.code)))
                    .icon(IconName::Copy)
                    .ghost()
                    .small()
                    .tooltip("Copy link")
                    .on_click(copy),
            )
            .child(
                Button::new(SharedString::from(format!("invite-revoke-{}", row.code)))
                    .icon(IconName::Trash)
                    .ghost()
                    .small()
                    .text_color(theme.danger)
                    .tooltip("Revoke")
                    .on_click(revoke),
            )
            .into_any_element()
    }
}

/// The invite creator's avatar.
const INVITE_AVATAR: f32 = 28.;

impl Render for InvitesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let can_create = self.invite_channel(cx).is_some();
        let header = h_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(muted)
                    .child("Anyone with one of these links can join."),
            )
            .child(
                Button::new("invites-create")
                    .label("Create invite…")
                    .icon(IconName::UserPlus)
                    .outline()
                    .small()
                    .disabled(!can_create)
                    .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
            )
            .child(
                Button::new("invites-refresh")
                    .icon(IconName::RefreshCw)
                    .ghost()
                    .small()
                    .tooltip("Refresh")
                    .disabled(matches!(self.invites, Load::Loading { .. }))
                    .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
            );
        let body = match &self.invites {
            Load::Idle | Load::Loading { .. } => div()
                .text_sm()
                .text_color(muted)
                .child("Loading invites…")
                .into_any_element(),
            Load::Failed(text) => h_flex()
                .gap_2()
                .text_sm()
                .text_color(danger)
                .child(text.clone())
                .child(
                    Button::new("invites-retry")
                        .label("Retry")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                )
                .into_any_element(),
            Load::Loaded(rows) if rows.is_empty() => div()
                .text_sm()
                .text_color(muted)
                .child("This server has no invites.")
                .into_any_element(),
            Load::Loaded(rows) => {
                let rows = rows.clone();
                v_flex()
                    .w_full()
                    .gap_1()
                    .children(rows.iter().map(|row| self.render_row(row, cx)))
                    .into_any_element()
            }
        };
        v_flex().w_full().gap_3().child(header).child(body)
    }
}

/// Bans: everyone banned from the server, and why.
fn bans_page(view: &ServerSettingsView, cx: &mut App) -> Option<SettingPage> {
    if !view.access(cx).bans {
        return None;
    }
    let bans = view.bans.clone();
    Some(
        SettingPage::new("Bans")
            .icon(IconName::Ban)
            .resettable(false)
            .group(
                SettingGroup::new().item(
                    SettingItem::render(move |_, _, _| bans.clone())
                        .keywords(["bans", "banned", "unban", "pardon"]),
                ),
            ),
    )
}

/// The Bans page: who's banned and why, filtered by name or reason, each
/// with Unban. Stoat sends no event for bans, so a lifted one is removed
/// here, and Refresh fetches the list again (after banning someone from
/// the main window, say).
pub struct BansPage {
    chat: WeakEntity<ChatView>,
    server: String,
    filter: Entity<InputState>,
    bans: Load<Vec<BanRow>>,
    _filter: Subscription,
}

impl BansPage {
    fn new(
        chat: &Entity<ChatView>,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter by name or reason"));
        let subscription = cx.subscribe(&filter, |_, _, _: &InputEvent, cx| cx.notify());
        Self {
            chat: chat.downgrade(),
            server,
            filter,
            bans: Load::Idle,
            _filter: subscription,
        }
    }

    /// Fetch the list, unless that's already been done.
    fn load_once(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.bans, Load::Idle) {
            self.refresh(window, cx);
        }
    }

    /// Fetch the list again.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let (client, server) = (chat.read(cx).client().clone(), self.server.clone());
            async move { client.fetch_bans(&server).await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.bans = match result {
                    Ok(Ok(list)) => {
                        log::debug!("{} bans in {}", list.bans.len(), this.server);
                        match this.chat.upgrade() {
                            Some(chat) => Load::Loaded(ban_rows(chat.read(cx).store(), &list)),
                            None => Load::Idle,
                        }
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't load bans in {}: {err:?}", this.server);
                        Load::Failed(bans_error_text(&err))
                    }
                    Err(err) => {
                        log::warn!("bans task failed: {err:?}");
                        Load::Failed("Couldn't load the bans.".into())
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.bans = Load::Loading { _task: task };
        cx.notify();
    }

    /// Lift a ban, once confirmed. Its row goes when Stoat agrees, or says
    /// there's no such ban any more.
    fn unban(&mut self, row: &BanRow, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let client = chat.read(cx).client().clone();
        let (server, user) = (self.server.clone(), row.user.clone());
        let request = MemberActionRequest {
            title: format!("Unban {}?", row.name).into(),
            detail: "They can rejoin with an invite.".into(),
            ok_label: "Unban",
            danger: false,
            failed_text: "Couldn't lift the ban.".into(),
        };
        let page = cx.entity().downgrade();
        let action = {
            let user = user.clone();
            Box::pin(async move { client.unban(&server, &user).await })
        };
        let after = Box::new(
            move |result: Result<(), ApiError>, window: &mut Window, cx: &mut App| {
                let gone = match &result {
                    Ok(()) => true,
                    Err(err) => {
                        log::warn!("couldn't unban {user}: {err:?}");
                        window.push_notification(Notification::error(unban_error_text(err)), cx);
                        matches!(err.error_type(), Some(ErrorType::NotFound))
                    }
                };
                if gone {
                    page.update(cx, |page, cx| {
                        if let Load::Loaded(rows) = &mut page.bans {
                            rows.retain(|row| row.user != user);
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        );
        confirm_member_action_then(request, action, after, window, cx);
    }

    fn render_row(&self, row: &BanRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let face = avatar(
            SharedString::from(format!("ban-{}", row.user)),
            row.avatar.clone(),
            &row.name,
            BAN_AVATAR,
            theme.secondary,
            theme.secondary_foreground,
        );
        let unban = {
            let row = row.clone();
            cx.listener(move |this, _, window, cx| this.unban(&row, window, cx))
        };
        h_flex()
            .w_full()
            .gap_3()
            .py_1()
            .child(face)
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
                    .child(
                        div().text_xs().text_color(muted).child(
                            row.reason
                                .clone()
                                .unwrap_or_else(|| "No reason given".into()),
                        ),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("unban-{}", row.user)))
                    .label("Unban")
                    .outline()
                    .small()
                    .on_click(unban),
            )
            .into_any_element()
    }
}

/// A banned user's avatar.
const BAN_AVATAR: f32 = 28.;

impl Render for BansPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let header = h_flex()
            .w_full()
            .gap_2()
            .child(div().flex_1().child(Input::new(&self.filter).small()))
            .child(
                Button::new("bans-refresh")
                    .icon(IconName::RefreshCw)
                    .ghost()
                    .small()
                    .tooltip("Refresh")
                    .disabled(matches!(self.bans, Load::Loading { .. }))
                    .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
            );
        let body = match &self.bans {
            Load::Idle | Load::Loading { .. } => div()
                .text_sm()
                .text_color(muted)
                .child("Loading bans…")
                .into_any_element(),
            Load::Failed(text) => h_flex()
                .gap_2()
                .text_sm()
                .text_color(danger)
                .child(text.clone())
                .child(
                    Button::new("bans-retry")
                        .label("Retry")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                )
                .into_any_element(),
            Load::Loaded(rows) if rows.is_empty() => div()
                .text_sm()
                .text_color(muted)
                .child("Nobody is banned from this server.")
                .into_any_element(),
            Load::Loaded(rows) => {
                let query = self.filter.read(cx).value().to_string();
                let shown: Vec<BanRow> = filter_bans(rows, &query).into_iter().cloned().collect();
                if shown.is_empty() {
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("No bans match.")
                        .into_any_element()
                } else {
                    v_flex()
                        .w_full()
                        .gap_1()
                        .children(shown.iter().map(|row| self.render_row(row, cx)))
                        .into_any_element()
                }
            }
        };
        v_flex().w_full().gap_3().child(header).child(body)
    }
}
