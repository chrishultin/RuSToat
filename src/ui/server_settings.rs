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
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::setting::{SelectIndex, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::ApiError;
use crate::api::models::{Role, Server};
use crate::links::invite_link;
use crate::profile_edit::{ImageLimits, ProfileImage};
use crate::rail::{PALETTE, parse_colour};
use crate::roles::{
    ColourChoice, ROLE_NAME_MAX, RoleForm, RoleRow, check_role_name, create_error_text,
    delete_error_text as delete_role_error_text, edit_error_text, hex_of, may_delete, may_edit,
    role_rows,
};
use crate::runtime::spawn_io;
use crate::server_settings::{
    Access, BanRow, Candidate, DESCRIPTION_MAX, InviteRow, NAME_MAX, OverviewForm, ServerImage,
    SystemChannels, SystemMessageKind, access, ban_rows, bans_error_text, delete_confirmed,
    delete_error_text, filter_bans, filter_candidates, invite_rows, invites_error_text,
    revoke_error_text, save_error_text, system_channel_choices, transfer_candidates,
    transfer_refusal, unban_error_text,
};
use crate::store::solid_colour;
use crate::themes;
use crate::ui::chat::{ChatView, avatar};
use crate::ui::confirm_dialog::{ConfirmRequest, confirm_then};
use crate::ui::invites::{CREATE_DIALOG_WIDTH, CreateInviteDialog, InviteCreated};
use crate::ui::moderation_dialogs::{MemberActionRequest, confirm_member_action_then};
use crate::ui::profile_editor::{
    ImageSlot, Preview, avatar_preview, banner_preview, field, prompt_for_image, render_image_row,
    upload_image,
};
use crate::ui::server_dialogs::name_field;

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
    roles: Entity<RolesPage>,
    invites: Entity<InvitesPage>,
    bans: Entity<BansPage>,
    ownership: Entity<OwnershipPage>,
    _subscriptions: Vec<Subscription>,
}

impl ServerSettingsView {
    /// The pages, in sidebar order.
    pub const PAGES: [ServerPageFn; 5] = [
        overview_page,
        roles_page,
        invites_page,
        bans_page,
        ownership_page,
    ];

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
        let roles = cx.new(|cx| RolesPage::new(&chat, server.clone(), window, cx));
        let invites = cx.new(|_| InvitesPage::new(&chat, server.clone()));
        let bans = cx.new(|cx| BansPage::new(&chat, server.clone(), window, cx));
        let ownership = cx.new(|_| OwnershipPage::new(&chat, server.clone()));
        Self {
            chat: chat.downgrade(),
            server,
            pages: Self::PAGES.to_vec(),
            shown: Vec::new(),
            page_key: 0,
            title,
            overview,
            roles,
            invites,
            bans,
            ownership,
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

/// Roles: the server's roles, highest first, and a role's editor.
fn roles_page(view: &ServerSettingsView, cx: &mut App) -> Option<SettingPage> {
    if !view.access(cx).roles {
        return None;
    }
    let roles = view.roles.clone();
    Some(
        SettingPage::new("Roles")
            .icon(IconName::Shield)
            .resettable(false)
            .group(
                SettingGroup::new().item(
                    SettingItem::render(move |_, _, _| roles.clone())
                        .keywords(["roles", "role", "rank", "colour", "color", "hoist"]),
                ),
            ),
    )
}

/// The Roles page: the server's roles, highest first, each with its colour,
/// icon, whether it's hoisted, and a lock if the user can't change it; and
/// "Create role…". Clicking a role opens its editor in the same page, with
/// a way back. It follows the store, which live role events keep.
pub struct RolesPage {
    chat: WeakEntity<ChatView>,
    server: String,
    mode: RolesMode,
    _chat: Subscription,
    _editor: Option<Subscription>,
    _create: Option<Subscription>,
}

/// What the Roles page shows.
enum RolesMode {
    List,
    Editing(Entity<RoleEditor>),
}

impl RolesPage {
    fn new(
        chat: &Entity<ChatView>,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            chat: chat.downgrade(),
            server,
            mode: RolesMode::List,
            _chat: cx.observe_in(chat, window, |this, _, window, cx| this.follow(window, cx)),
            _editor: None,
            _create: None,
        }
    }

    /// The store changed: if the role being edited was deleted (elsewhere:
    /// the user's own delete goes back first), or the user may no longer
    /// manage roles, go back to the list.
    fn follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let RolesMode::Editing(editor) = &self.mode
            && let Some(chat) = self.chat.upgrade()
        {
            let store = chat.read(cx).store();
            let role = editor.read(cx).role.clone();
            let exists = store
                .server(&self.server)
                .is_some_and(|server| server.roles.contains_key(&role));
            if !exists {
                window.push_notification(Notification::info("That role was deleted."), cx);
                self.back(cx);
            } else if !access(store, &self.server).roles {
                self.back(cx);
            }
        }
        cx.notify();
    }

    /// Open `role`'s editor.
    fn open(&mut self, role: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let server = self.server.clone();
        let editor = cx.new(|cx| RoleEditor::new(&chat, server, role, window, cx));
        self._editor = Some(cx.subscribe(&editor, |this, _, _: &BackToRoles, cx| this.back(cx)));
        self.mode = RolesMode::Editing(editor);
        cx.notify();
    }

    /// Back to the list, dropping the editor and anything unsaved in it.
    fn back(&mut self, cx: &mut Context<Self>) {
        self.mode = RolesMode::List;
        self._editor = None;
        cx.notify();
    }

    /// "Create role": a name, then Create, which opens the new role.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let server = self.server.clone();
        let dialog = cx.new(|cx| CreateRoleDialog::new(chat, server, window, cx));
        self._create = Some(cx.subscribe_in(
            &dialog,
            window,
            |this, _, created: &RoleCreated, window, cx| this.open(created.0.clone(), window, cx),
        ));
        let focus = dialog.read(cx).input.read(cx).focus_handle(cx);
        open_focused_dialog("Create role".into(), dialog, focus, window, cx);
    }

    fn render_row(&self, row: &RoleRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let icon = row.icon.as_ref().and_then(|id| {
            let chat = self.chat.upgrade()?;
            chat.read(cx).store().media().file_by_id("icons", id)
        });
        let open = {
            let id = row.id.clone();
            cx.listener(move |this, _, window, cx| this.open(id.clone(), window, cx))
        };
        h_flex()
            .id(SharedString::from(format!("role-{}", row.id)))
            .w_full()
            .gap_3()
            .px_2()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|el| el.bg(theme.secondary))
            .on_click(open)
            .child(colour_dot(row.colour.as_deref(), cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(row.name.clone()),
            )
            .children(icon.map(|url| {
                img(url)
                    .size(px(ROLE_ICON))
                    .flex_none()
                    .rounded_sm()
                    .object_fit(ObjectFit::Cover)
            }))
            .when(row.hoist, |this| {
                this.child(div().text_xs().text_color(muted).child("Hoisted"))
            })
            .children(row.lock_text().map(|text| {
                div()
                    .id(SharedString::from(format!("role-lock-{}", row.id)))
                    .flex_none()
                    .text_color(muted)
                    .child(Icon::new(IconName::Lock).small())
                    .tooltip(move |window, cx| Tooltip::new(text).build(window, cx))
            }))
            .into_any_element()
    }

    fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self
            .chat
            .upgrade()
            .map(|chat| role_rows(chat.read(cx).store(), &self.server))
            .unwrap_or_default();
        let muted = cx.theme().muted_foreground;
        let header = h_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(muted)
                    .child("Roles higher in the list outrank those below them."),
            )
            .child(
                Button::new("roles-create")
                    .label("Create role…")
                    .icon(IconName::Plus)
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
            );
        let body = if rows.is_empty() {
            div()
                .text_sm()
                .text_color(muted)
                .child("This server has no roles.")
                .into_any_element()
        } else {
            v_flex()
                .w_full()
                .children(rows.iter().map(|row| self.render_row(row, cx)))
                .into_any_element()
        };
        v_flex()
            .w_full()
            .gap_3()
            .child(header)
            .child(body)
            .into_any_element()
    }
}

/// A role's icon in the list.
const ROLE_ICON: f32 = 18.;
/// A role's colour dot.
const COLOUR_DOT: f32 = 12.;

/// A role's colour as a dot, or a dashed circle with no colour (or one
/// that isn't a solid colour).
fn colour_dot(colour: Option<&str>, cx: &App) -> Div {
    let dot = div().size(px(COLOUR_DOT)).flex_none().rounded_full();
    match colour.and_then(parse_colour) {
        Some(colour) => dot.bg(colour),
        None => dot
            .border_1()
            .border_dashed()
            .border_color(cx.theme().muted_foreground),
    }
}

impl Render for RolesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.mode {
            RolesMode::List => self.render_list(cx),
            RolesMode::Editing(editor) => editor.clone().into_any_element(),
        }
    }
}

/// "Create role": its name, then Create. The new role shows at once, at
/// the bottom of the list, and its editor opens ([`RoleCreated`]).
pub struct CreateRoleDialog {
    chat: WeakEntity<ChatView>,
    server: String,
    input: Entity<InputState>,
    creating: bool,
    error: Option<String>,
    _input: Subscription,
}

impl CreateRoleDialog {
    fn new(
        chat: Entity<ChatView>,
        server: String,
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
            server,
            input,
            creating: false,
            error: None,
            _input: subscription,
        }
    }

    /// Create the role with the typed name. Success closes the dialog, adds
    /// the role to the store and opens it; a refusal says why and keeps the
    /// dialog open.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating {
            return;
        }
        let Some(name) = check_role_name(&self.input.read(cx).value()) else {
            return;
        };
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.creating = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let (client, server) = (chat.read(cx).client().clone(), self.server.clone());
            async move { client.create_role(&server, &name).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match result {
                    Ok(Ok(created)) => {
                        log::info!("created role {} in {}", created.id, this.server);
                        let server = this.server.clone();
                        this.chat
                            .update(cx, |chat, cx| chat.role_saved(&server, &created.role, cx))
                            .ok();
                        window.close_dialog(cx);
                        cx.emit(RoleCreated(created.id));
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't create a role in {}: {err:?}", this.server);
                        this.error = Some(create_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("role create task failed: {err:?}");
                        this.error = Some("Couldn't create the role.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// A role was created, by its ID.
pub struct RoleCreated(pub String);

impl EventEmitter<RoleCreated> for CreateRoleDialog {}

impl Render for CreateRoleDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let valid = check_role_name(&self.input.read(cx).value()).is_some();
        v_flex()
            .gap_3()
            .child(name_field(
                "Role name",
                &self.input,
                self.creating,
                self.error.clone(),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("create-role-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.creating)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("create-role")
                            .primary()
                            .label("Create")
                            .loading(self.creating)
                            .disabled(self.creating || !valid)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
    }
}

/// The role editor's "‹ Roles", or its role was deleted from it: the
/// Roles page goes back to its list.
pub struct BackToRoles;

/// One role's editor, in the Roles page: its name, colour and hoisting,
/// saved together with one `PATCH` (see [`RoleForm::changes`]); "Copy role ID";
/// and "Delete role…". Untouched fields follow changes made elsewhere. A
/// role the user can't change (rank) opens read-only, and a bot's can't be
/// deleted.
pub struct RoleEditor {
    chat: WeakEntity<ChatView>,
    server: String,
    role: String,
    name: Entity<InputState>,
    hoist: bool,
    colour: ColourChoice,
    /// The colour picker. Only its changes set [`Self::colour`], so a hex
    /// value shown in it never reads as an edit.
    picker: Entity<ColorPickerState>,
    /// The role's values the fields were last filled with. A field still
    /// showing its value is untouched, and follows changes made elsewhere;
    /// once it's edited, it keeps the edit.
    filled: RoleForm,
    saving: Option<Saving>,
    /// Focus the name once the editor is first drawn.
    focus_name: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BackToRoles> for RoleEditor {}

impl RoleEditor {
    fn new(
        chat: &Entity<ChatView>,
        server: String,
        role: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let picker = cx.new(|cx| ColorPickerState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.save(window, cx),
                _ => cx.notify(),
            }),
            cx.subscribe(&picker, |this, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(value) = event;
                this.colour = value.map_or(ColourChoice::None, |colour| {
                    ColourChoice::Hex(hex_of(colour))
                });
                cx.notify();
            }),
            cx.observe_in(chat, window, |this, _, window, cx| this.follow(window, cx)),
        ];
        let focus_name = may_edit(chat.read(cx).store(), &server, &role);
        let mut this = Self {
            chat: chat.downgrade(),
            server,
            role,
            name,
            hoist: false,
            colour: ColourChoice::None,
            picker,
            filled: RoleForm::default(),
            saving: None,
            focus_name,
            _subscriptions: subscriptions,
        };
        this.reset(window, cx);
        this
    }

    /// The role as the store has it now.
    fn current(&self, cx: &App) -> Option<Role> {
        let chat = self.chat.upgrade()?;
        let store = chat.read(cx).store();
        store.server(&self.server)?.roles.get(&self.role).cloned()
    }

    /// Whether the user may change the role, and delete it, now.
    fn rights(&self, cx: &App) -> (bool, bool) {
        let Some(chat) = self.chat.upgrade() else {
            return (false, false);
        };
        let store = chat.read(cx).store();
        (
            may_edit(store, &self.server, &self.role),
            may_delete(store, &self.server, &self.role),
        )
    }

    /// The form as edited.
    fn edited(&self, cx: &App) -> RoleForm {
        RoleForm {
            name: self.name.read(cx).value().to_string(),
            hoist: self.hoist,
            colour: self.colour.clone(),
        }
    }

    /// Whether Save can send the form: the user may change the role,
    /// something changed, and not already saving.
    fn can_save(&self, edited: &RoleForm, cx: &App) -> bool {
        self.saving.is_none()
            && self.rights(cx).0
            && self
                .current(cx)
                .is_some_and(|role| edited.changes(&role).is_some())
    }

    /// Fill every field with the role's values.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(role) = self.current(cx) else {
            return;
        };
        let form = RoleForm::of(&role);
        self.name.update(cx, |input, cx| {
            input.set_value(form.name.clone(), window, cx)
        });
        self.hoist = form.hoist;
        self.set_colour(form.colour.clone(), window, cx);
        self.filled = form;
        cx.notify();
    }

    /// Set the colour, and show it in the picker (a custom one as none).
    fn set_colour(&mut self, colour: ColourChoice, window: &mut Window, cx: &mut Context<Self>) {
        let shown = match &colour {
            ColourChoice::Hex(hex) => parse_colour(hex).map(Hsla::from),
            _ => None,
        };
        self.picker.update(cx, |picker, cx| match shown {
            Some(shown) => picker.set_value(shown, window, cx),
            None => picker.clear_value(window, cx),
        });
        self.colour = colour;
        cx.notify();
    }

    /// The role changed (here or elsewhere): untouched fields take its new
    /// values, edited ones keep theirs. (A deleted role is the page's to
    /// notice.)
    fn follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(role) = self.current(cx) else {
            return;
        };
        let now = RoleForm::of(&role);
        if now == self.filled {
            cx.notify();
            return;
        }
        let edited = self.edited(cx);
        if edited.name == self.filled.name && now.name != self.filled.name {
            let name = now.name.clone();
            self.name
                .update(cx, |input, cx| input.set_value(name, window, cx));
        }
        if edited.hoist == self.filled.hoist {
            self.hoist = now.hoist;
        }
        if edited.colour == self.filled.colour && now.colour != self.filled.colour {
            self.set_colour(now.colour.clone(), window, cx);
        }
        self.filled = now;
        cx.notify();
    }

    /// Send what changed in one `PATCH`. Stoat's answer shows at once, in
    /// the main window too; a refusal says why in a toast, and the form
    /// keeps the edit.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edited = self.edited(cx);
        if !self.can_save(&edited, cx) {
            return;
        }
        let (Some(chat), Some(edit)) = (
            self.chat.upgrade(),
            self.current(cx).and_then(|role| edited.changes(&role)),
        ) else {
            return;
        };
        self.saving = Some(Saving::Sending);
        cx.notify();
        let request = spawn_io({
            let client = chat.read(cx).client().clone();
            let (server, role) = (self.server.clone(), self.role.clone());
            async move { client.edit_role(&server, &role, &edit).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.saving = None;
                match result {
                    Ok(Ok(saved)) => {
                        let server = this.server.clone();
                        this.chat
                            .update(cx, |chat, cx| chat.role_saved(&server, &saved, cx))
                            .ok();
                        this.reset(window, cx);
                        window.push_notification(Notification::success("Saved."), cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't save role {}: {err:?}", this.role);
                        window.push_notification(Notification::error(edit_error_text(&err)), cx);
                    }
                    Err(err) => {
                        log::warn!("role save task failed: {err:?}");
                        window
                            .push_notification(Notification::error("Couldn't save the role."), cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn copy_id(&self, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.role.clone()));
        window.push_notification(Notification::success("Copied the role ID."), cx);
    }

    /// Delete the role, once confirmed. When Stoat agrees (or says it's
    /// already gone), the page goes back to the list and the role leaves
    /// the store at once.
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(chat), Some(role)) = (self.chat.upgrade(), self.current(cx)) else {
            return;
        };
        let client = chat.read(cx).client().clone();
        let request = MemberActionRequest {
            title: format!("Delete {}?", role.name).into(),
            detail: "Members with it lose it, and so does every channel's override for it.".into(),
            ok_label: "Delete",
            danger: true,
            failed_text: "Couldn't delete the role.".into(),
        };
        let (server, id) = (self.server.clone(), self.role.clone());
        let action = {
            let (server, id) = (server.clone(), id.clone());
            Box::pin(async move { client.delete_role(&server, &id).await })
        };
        let editor = cx.entity().downgrade();
        let chat = chat.downgrade();
        let name = role.name;
        let after = Box::new(
            move |result: Result<(), ApiError>, window: &mut Window, cx: &mut App| match result {
                Err(err) if !matches!(err.error_type(), Some(ErrorType::NotFound)) => {
                    log::warn!("couldn't delete role {id}: {err:?}");
                    window.push_notification(Notification::error(delete_role_error_text(&err)), cx);
                }
                _ => {
                    log::info!("deleted role {id} in {server}");
                    // Back first, so the page doesn't take this for a
                    // delete made elsewhere.
                    editor.update(cx, |_, cx| cx.emit(BackToRoles)).ok();
                    chat.update(cx, |chat, cx| chat.role_deleted(&server, &id, cx))
                        .ok();
                    let text = format!("Deleted {name}.");
                    window.push_notification(Notification::success(text), cx);
                }
            },
        );
        confirm_member_action_then(request, action, after, window, cx);
    }

    /// "Role colour": the picker (the rail's palette, or a hex value) and
    /// "No colour", or just the colour when the role can't be changed; a
    /// note for a colour set elsewhere that can't be picked; and the name
    /// in the colour, on the theme's background.
    fn render_colour(&self, name: &str, editable: bool, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let solid = self.colour.solid();
        let control = if editable {
            let featured = PALETTE
                .iter()
                .filter_map(|(_, hex)| parse_colour(hex).map(Hsla::from))
                .collect();
            h_flex()
                .gap_2()
                .child(
                    ColorPicker::new(&self.picker)
                        .featured_colors(featured)
                        .small(),
                )
                .child(
                    Button::new("role-no-colour")
                        .label("No colour")
                        .outline()
                        .small()
                        .disabled(self.saving.is_some() || self.colour == ColourChoice::None)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.set_colour(ColourChoice::None, window, cx)
                        })),
                )
                .into_any_element()
        } else {
            colour_dot(solid.as_deref(), cx).into_any_element()
        };
        let custom = match &self.colour {
            ColourChoice::Custom(css) => Some(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(colour_dot(solid.as_deref(), cx))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(format!("Custom colour (set elsewhere): {css}")),
                    ),
            ),
            _ => None,
        };
        let preview = div()
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(
                solid
                    .as_deref()
                    .and_then(parse_colour)
                    .map_or(theme.foreground, Hsla::from),
            )
            .child(name.to_string());
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Role colour"),
            )
            .child(h_flex().gap_3().child(control).child(preview))
            .children(custom)
            .into_any_element()
    }

    /// Why the role can't be changed, or deleted, if it can't.
    fn render_lock(&self, editable: bool, deletable: bool, cx: &App) -> Option<AnyElement> {
        let text = if !editable {
            "This role ranks at or above your highest role, so you can't change it."
        } else if !deletable {
            "A bot manages this role. You can edit it, but not delete it."
        } else {
            return None;
        };
        Some(
            h_flex()
                .gap_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::Lock).small())
                .child(text)
                .into_any_element(),
        )
    }
}

impl Render for RoleEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_name) {
            let focus = self.name.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
        let Some(role) = self.current(cx) else {
            return div().into_any_element();
        };
        let (editable, deletable) = self.rights(cx);
        let edited = self.edited(cx);
        let saving = self.saving.is_some();
        let changed = edited != self.filled;
        let can_save = self.can_save(&edited, cx);
        let colour = role.colour.as_deref().and_then(solid_colour);
        let muted = cx.theme().muted_foreground;
        let name_count = edited.name.trim().chars().count();
        let name = field(
            "Role name",
            Input::new(&self.name)
                .disabled(saving || !editable)
                .into_any_element(),
            name_count,
            ROLE_NAME_MAX,
            (name_count == 0).then(|| "A role needs a name.".to_string()),
            cx,
        );
        let hoist = v_flex()
            .gap_1()
            .child(
                Checkbox::new("role-hoist")
                    .label("Hoist role")
                    .checked(self.hoist)
                    .disabled(saving || !editable)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.hoist = *checked;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .pl_6()
                    .text_xs()
                    .text_color(muted)
                    .child("Display members with this role separately in the member list."),
            );
        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex().child(
                    Button::new("roles-back")
                        .label("Roles")
                        .icon(IconName::ChevronLeft)
                        .ghost()
                        .small()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(BackToRoles))),
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(colour_dot(colour.as_deref(), cx))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(role.name.clone()),
                    ),
            )
            .children(self.render_lock(editable, deletable, cx))
            .child(name)
            .child(self.render_colour(
                match edited.name.trim() {
                    "" => &role.name,
                    typed => typed,
                },
                editable,
                cx,
            ))
            .child(hoist)
            .when(editable, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("role-reset")
                                .label("Reset")
                                .outline()
                                .small()
                                .disabled(!changed || saving)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.reset(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("role-save")
                                .label("Save")
                                .primary()
                                .small()
                                .loading(saving)
                                .disabled(!can_save)
                                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                        ),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("role-copy-id")
                            .label("Copy role ID")
                            .icon(IconName::Copy)
                            .outline()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| this.copy_id(window, cx))),
                    )
                    .when(deletable, |this| {
                        this.child(
                            Button::new("role-delete")
                                .label("Delete role…")
                                .icon(IconName::Trash)
                                .danger()
                                .small()
                                .disabled(saving)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.delete(window, cx)),
                                ),
                        )
                    }),
            )
            .into_any_element()
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

/// Ownership: hand the server to someone else, or delete it. The owner's
/// only.
fn ownership_page(view: &ServerSettingsView, cx: &mut App) -> Option<SettingPage> {
    if !view.access(cx).ownership {
        return None;
    }
    let ownership = view.ownership.clone();
    Some(
        SettingPage::new("Ownership")
            .icon(IconName::Crown)
            .resettable(false)
            .group(SettingGroup::new().item(
                SettingItem::render(move |_, _, _| ownership.clone()).keywords([
                    "ownership",
                    "owner",
                    "transfer",
                    "delete server",
                ]),
            )),
    )
}

/// The Ownership page: Transfer ownership… and Delete server….
pub struct OwnershipPage {
    chat: WeakEntity<ChatView>,
    server: String,
}

impl OwnershipPage {
    fn new(chat: &Entity<ChatView>, server: String) -> Self {
        Self {
            chat: chat.downgrade(),
            server,
        }
    }

    fn open_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let server = self.server.clone();
        let dialog = cx.new(|cx| TransferDialog::new(chat, server, window, cx));
        let focus = dialog.read(cx).filter.read(cx).focus_handle(cx);
        open_focused_dialog("Transfer ownership".into(), dialog, focus, window, cx);
    }

    fn open_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let Some(name) = chat
            .read(cx)
            .store()
            .server(&self.server)
            .map(|s| s.name.clone())
        else {
            return;
        };
        let server = self.server.clone();
        let title = SharedString::from(format!("Delete {name}?"));
        let dialog = cx.new(|cx| DeleteServerDialog::new(chat, server, name, window, cx));
        let focus = dialog.read(cx).typed.read(cx).focus_handle(cx);
        open_focused_dialog(title, dialog, focus, window, cx);
    }
}

/// Open `content` as a dialog titled `title`, with `focus` focused once
/// it's drawn (a dialog takes focus as it opens).
fn open_focused_dialog<V: Render>(
    title: SharedString,
    content: Entity<V>,
    focus: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let first_build = std::cell::Cell::new(true);
    window.open_dialog(cx, move |modal, window, cx| {
        if first_build.replace(false) {
            window.focus(&focus, cx);
        }
        modal
            .title(title.clone())
            .w(px(OWNERSHIP_DIALOG_WIDTH))
            .child(content.clone())
    });
}

/// The transfer and delete dialogs' width.
const OWNERSHIP_DIALOG_WIDTH: f32 = 420.;

impl Render for OwnershipPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let section = |title: &'static str, detail: &'static str, button: Button| {
            h_flex()
                .w_full()
                .gap_4()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(detail),
                        ),
                )
                .child(button)
        };
        v_flex()
            .w_full()
            .gap_4()
            .child(section(
                "Transfer ownership",
                "Make another member the owner. You'll keep your roles, but only they can \
                 give it back.",
                Button::new("ownership-transfer")
                    .label("Transfer ownership…")
                    .icon(IconName::Crown)
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| this.open_transfer(window, cx))),
            ))
            .child(section(
                "Delete server",
                "Delete the server, its channels and their messages, for everyone. There's \
                 no going back.",
                Button::new("ownership-delete")
                    .label("Delete server…")
                    .icon(IconName::Trash)
                    .danger()
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| this.open_delete(window, cx))),
            ))
    }
}

/// The most members the transfer picker draws at once; typing narrows it.
const CANDIDATES_SHOWN: usize = 200;

/// "Transfer ownership": pick a member from the whole list (Stoat 0.16.2
/// has no member search), filtered as you type, then confirm it's you.
pub struct TransferDialog {
    chat: WeakEntity<ChatView>,
    server: String,
    filter: Entity<InputState>,
    candidates: Load<Vec<Candidate>>,
    _filter: Subscription,
}

impl TransferDialog {
    fn new(
        chat: Entity<ChatView>,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search members"));
        let subscription = cx.subscribe(&filter, |_, _, _: &InputEvent, cx| cx.notify());
        let mut this = Self {
            chat: chat.downgrade(),
            server,
            filter,
            candidates: Load::Idle,
            _filter: subscription,
        };
        this.load(window, cx);
        this
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let (client, server) = (chat.read(cx).client().clone(), self.server.clone());
            async move { client.fetch_members(&server, false).await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.candidates = match (result, this.chat.upgrade()) {
                    (Ok(Ok(members)), Some(chat)) => {
                        let store = chat.read(cx).store();
                        Load::Loaded(transfer_candidates(&members, store.me(), store.media()))
                    }
                    (Ok(Err(err)), _) => {
                        log::warn!("couldn't load members of {}: {err:?}", this.server);
                        Load::Failed(crate::profile_edit::error_text(
                            &err,
                            "Couldn't load the members.",
                        ))
                    }
                    _ => Load::Failed("Couldn't load the members.".into()),
                };
                cx.notify();
            })
            .ok();
        });
        self.candidates = Load::Loading { _task: task };
        cx.notify();
    }

    /// `candidate` was picked: confirm it's the user, then send.
    fn pick(&mut self, candidate: &Candidate, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let Some(server_name) = chat
            .read(cx)
            .store()
            .server(&self.server)
            .map(|s| s.name.clone())
        else {
            return;
        };
        window.close_dialog(cx);
        let client = chat.read(cx).client().clone();
        let (server, owner, name) = (
            self.server.clone(),
            candidate.id.clone(),
            candidate.name.clone(),
        );
        let request = ConfirmRequest {
            title: format!("Transfer {server_name} to {name}?").into(),
            detail: "They'll own the server. You'll keep your roles, but lose the owner's \
                     powers. Only they can give it back."
                .into(),
            confirm_label: "Transfer".into(),
            failed_text: "Couldn't transfer the server.",
        };
        let weak = chat.downgrade();
        confirm_then(
            client,
            request,
            move |client, ticket| {
                Box::pin(async move {
                    // Stoat's own refusals are explained in the toast after;
                    // anything else (an expired ticket) by the dialog.
                    match client.transfer_server(&ticket, &server, &owner).await {
                        Ok(saved) => Ok(Ok(saved)),
                        Err(err) => match transfer_refusal(&err) {
                            Some(text) => Ok(Err(text)),
                            None => Err(err),
                        },
                    }
                })
            },
            move |result, window, cx| match result {
                Ok(saved) => {
                    weak.update(cx, |chat, cx| chat.server_edited(&saved, cx))
                        .ok();
                    let text = format!("{name} owns {server_name} now.");
                    window.push_notification(Notification::success(text), cx);
                }
                Err(text) => window.push_notification(Notification::error(text), cx),
            },
            window,
            cx,
        );
    }

    fn render_candidate(&self, candidate: &Candidate, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let face = avatar(
            SharedString::from(format!("candidate-{}", candidate.id)),
            Some(candidate.avatar.clone()),
            &candidate.name,
            CANDIDATE_AVATAR,
            theme.secondary,
            theme.secondary_foreground,
        );
        let pick = {
            let candidate = candidate.clone();
            cx.listener(move |this, _, window, cx| this.pick(&candidate, window, cx))
        };
        h_flex()
            .id(SharedString::from(format!("transfer-to-{}", candidate.id)))
            .w_full()
            .gap_3()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|el| el.bg(theme.secondary))
            .child(face)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().text_sm().child(candidate.name.clone()))
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(candidate.handle.clone()),
                    ),
            )
            .on_click(pick)
            .into_any_element()
    }
}

/// A member's avatar in the transfer picker.
const CANDIDATE_AVATAR: f32 = 28.;

impl Render for TransferDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let body = match &self.candidates {
            Load::Idle | Load::Loading { .. } => div()
                .text_sm()
                .text_color(muted)
                .child("Loading members…")
                .into_any_element(),
            Load::Failed(text) => h_flex()
                .gap_2()
                .text_sm()
                .text_color(danger)
                .child(text.clone())
                .child(
                    Button::new("transfer-retry")
                        .label("Retry")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.load(window, cx))),
                )
                .into_any_element(),
            Load::Loaded(candidates) => {
                let query = self.filter.read(cx).value().to_string();
                let matches = filter_candidates(candidates, &query);
                let more = matches.len().saturating_sub(CANDIDATES_SHOWN);
                let shown: Vec<Candidate> = matches
                    .into_iter()
                    .take(CANDIDATES_SHOWN)
                    .cloned()
                    .collect();
                if shown.is_empty() {
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(if candidates.is_empty() {
                            "There's nobody else to hand it to."
                        } else {
                            "Nobody matches."
                        })
                        .into_any_element()
                } else {
                    v_flex()
                        .id("transfer-candidates")
                        .max_h(px(CANDIDATES_HEIGHT))
                        .overflow_y_scroll()
                        .children(shown.iter().map(|c| self.render_candidate(c, cx)))
                        .when(more > 0, |this| {
                            this.child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{more} more. Type to narrow the list.")),
                            )
                        })
                        .into_any_element()
                }
            }
        };
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Choose who'll own the server. Bots can't."),
            )
            .child(Input::new(&self.filter))
            .child(body)
            .child(
                h_flex().justify_end().child(
                    Button::new("transfer-cancel")
                        .outline()
                        .label("Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    }
}

/// The tallest the transfer picker's list grows before it scrolls.
const CANDIDATES_HEIGHT: f32 = 320.;

/// "Delete {name}?": type the server's name, then Delete.
pub struct DeleteServerDialog {
    chat: WeakEntity<ChatView>,
    server: String,
    name: String,
    typed: Entity<InputState>,
    deleting: bool,
    _typed: Subscription,
}

impl DeleteServerDialog {
    fn new(
        chat: Entity<ChatView>,
        server: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let typed = cx.new(|cx| InputState::new(window, cx).placeholder(name.clone()));
        let subscription = cx.subscribe(&typed, |_, _, _: &InputEvent, cx| cx.notify());
        Self {
            chat: chat.downgrade(),
            server,
            name,
            typed,
            deleting: false,
            _typed: subscription,
        }
    }

    fn confirmed(&self, cx: &App) -> bool {
        delete_confirmed(&self.typed.read(cx).value(), &self.name)
    }

    /// Delete the server. Once Stoat agrees, it leaves the main window at
    /// once, and with it this window closes.
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.deleting || !self.confirmed(cx) {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.deleting = true;
        cx.notify();
        let request = spawn_io({
            let (client, server) = (chat.read(cx).client().clone(), self.server.clone());
            async move { client.delete_server(&server).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.deleting = false;
                match result {
                    Ok(Ok(())) => {
                        log::info!("deleted server {}", this.server);
                        window.close_dialog(cx);
                        let server = this.server.clone();
                        this.chat
                            .update(cx, |chat, cx| chat.server_deleted(&server, cx))
                            .ok();
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't delete server {}: {err:?}", this.server);
                        window.push_notification(Notification::error(delete_error_text(&err)), cx);
                    }
                    Err(err) => {
                        log::warn!("delete task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't delete the server."),
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

impl Render for DeleteServerDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let confirmed = self.confirmed(cx);
        v_flex()
            .gap_3()
            .child(div().text_sm().text_color(muted).child(format!(
                "Once it's deleted, there's no going back. Type the server's name, {}, to \
                 confirm.",
                self.name
            )))
            .child(Input::new(&self.typed).disabled(self.deleting))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("delete-server-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.deleting)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("delete-server")
                            .danger()
                            .label("Delete")
                            .loading(self.deleting)
                            .disabled(!confirmed || self.deleting)
                            .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                    ),
            )
    }
}
