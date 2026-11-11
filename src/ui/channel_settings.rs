//! The Channel Settings window: one server channel's or group's pages, for
//! those with Manage Channels on it, in a window of its own beside the main
//! one. There's only ever one, for one channel of one account
//! ([`ChannelSettingsWindow`]), apart from Server Settings' own.
//!
//! A page is a function returning it, or `None` when the user may not see
//! it ([`ChannelPageFn`]), as in Server Settings, so later pages
//! (permissions, webhooks) are one more function in
//! [`ChannelSettingsView::PAGES`]. Who sees what is
//! [`crate::channel_settings::access`].
//!
//! The window follows its account's [`ChatView`], and closes itself when
//! the channel leaves the store (deleted, or hidden from the user), when
//! the user may see no page of it any more, or when the account signs out.

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::setting::{SelectIndex, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::ApiError;
use crate::api::models::Channel;
use crate::channel_settings::{
    ChannelAccess, ChannelForm, ChannelKind, DESCRIPTION_MAX, ICON_TAG, NAME_MAX, access,
    mature_edit, mature_error_text, mature_prompt, mature_state, save_error_text, slowmode_choices,
    slowmode_label, window_title,
};
use crate::profile_edit::ProfileImage;
use crate::runtime::spawn_io;
use crate::themes;
use crate::ui::channel_permissions::ChannelPermissionsPage;
use crate::ui::chat::ChatView;
use crate::ui::profile_editor::{
    ImageSlot, Preview, avatar_preview, field, prompt_for_image, render_image_row, upload_image,
};

/// Rows the description field grows between before it scrolls.
const DESCRIPTION_ROWS: (usize, usize) = (3, 10);
/// The tallest the slowmode menu grows before it scrolls.
const SLOWMODE_MENU_HEIGHT: f32 = 320.;

/// Builds one page of the window, each time it renders: `None` hides it.
pub type ChannelPageFn = fn(&ChannelSettingsView, &mut App) -> Option<SettingPage>;

/// The Channel Settings window and its content, while it's open.
#[derive(Default)]
pub struct ChannelSettingsWindow(Option<(WindowHandle<Root>, WeakEntity<ChannelSettingsView>)>);

/// gpui-component's window root, which hosts dialogs and toasts.
type Root = gpui_kit::component::Root;

impl Global for ChannelSettingsWindow {}

impl ChannelSettingsWindow {
    /// Open `channel`'s settings for `chat`'s account, or bring them
    /// forward if they're already open. Another channel's (or account's)
    /// settings close first, unsaved edits and all. Deferred: the click
    /// that asks arrives while GPUI is updating the main window.
    pub fn open(chat: Entity<ChatView>, channel: String, cx: &mut App) {
        cx.defer(move |cx| {
            if let Some((window, view)) = cx.default_global::<Self>().0.clone() {
                let same = view
                    .upgrade()
                    .is_some_and(|view| view.read(cx).is_for(&chat, &channel));
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
            match Self::open_new(chat, channel, cx) {
                Ok(opened) => cx.global_mut::<Self>().0 = Some(opened),
                Err(err) => log::error!("couldn't open the channel settings window: {err:#}"),
            }
        });
    }

    fn open_new(
        chat: Entity<ChatView>,
        channel: String,
        cx: &mut App,
    ) -> anyhow::Result<(WindowHandle<Root>, WeakEntity<ChannelSettingsView>)> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(680.), px(520.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(window_title(chat.read(cx).store().channel(&channel)).into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (window, view) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| ChannelSettingsView::new(chat, channel, window, cx))
        })?;
        cx.activate(true);
        let window = window
            .downcast()
            .ok_or_else(|| anyhow::anyhow!("the channel settings window isn't a Root"))?;
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

/// One channel's settings.
pub struct ChannelSettingsView {
    /// The account's chat: its client and store.
    chat: WeakEntity<ChatView>,
    channel: String,
    pages: Vec<ChannelPageFn>,
    /// Which pages showed at the last render. When that changes, the
    /// sidebar goes back to its first page: gpui-component picks pages by
    /// position, so it would otherwise show the wrong one.
    shown: Vec<bool>,
    /// Bumped to go back to the first page: a new ID gives the sidebar new
    /// state, which starts there.
    page_key: usize,
    /// The window's title, as last set.
    title: String,
    overview: Entity<ChannelOverviewPage>,
    permissions: Entity<ChannelPermissionsPage>,
    _subscriptions: Vec<Subscription>,
}

impl ChannelSettingsView {
    /// The pages, in sidebar order.
    pub const PAGES: [ChannelPageFn; 2] = [overview_page, permissions_page];

    pub fn new(
        chat: Entity<ChatView>,
        channel: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            // Follow the channel, and close once it's gone or there's
            // nothing left to show of it.
            cx.observe_in(&chat, window, |this, chat, window, cx| {
                let store = chat.read(cx).store();
                if store.channel(&this.channel).is_none() || !access(store, &this.channel).any() {
                    log::debug!("closing channel settings for {}", this.channel);
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
        let title = window_title(chat.read(cx).store().channel(&channel));
        let overview = cx.new(|cx| ChannelOverviewPage::new(&chat, channel.clone(), window, cx));
        let permissions = cx.new(|cx| ChannelPermissionsPage::new(&chat, channel.clone(), cx));
        Self {
            chat: chat.downgrade(),
            channel,
            pages: Self::PAGES.to_vec(),
            shown: Vec::new(),
            page_key: 0,
            title,
            overview,
            permissions,
            _subscriptions: subscriptions,
        }
    }

    /// Whether these are `channel`'s settings for `chat`'s account.
    pub fn is_for(&self, chat: &Entity<ChatView>, channel: &str) -> bool {
        self.chat.entity_id() == chat.entity_id() && self.channel == channel
    }

    /// The account's chat, while it's signed in.
    pub fn chat(&self) -> Option<Entity<ChatView>> {
        self.chat.upgrade()
    }

    /// The channel these settings are for.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Which pages the user may see now.
    pub fn access(&self, cx: &App) -> ChannelAccess {
        self.chat()
            .map(|chat| access(chat.read(cx).store(), &self.channel))
            .unwrap_or_default()
    }
}

impl Render for ChannelSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = window_title(
            self.chat()
                .as_ref()
                .and_then(|chat| chat.read(cx).store().channel(&self.channel)),
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
            Settings::new(("channel-settings", self.page_key))
                .default_selected_index(SelectIndex::default())
                .pages(pages.into_iter().flatten()),
        )
    }
}

/// Overview: the channel's icon, name and description.
fn overview_page(view: &ChannelSettingsView, cx: &mut App) -> Option<SettingPage> {
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
                    "slowmode",
                    "mature",
                    "nsfw",
                ]),
            )),
    )
}

/// Permissions: a server channel's overrides, or a group's member
/// permissions.
fn permissions_page(view: &ChannelSettingsView, cx: &mut App) -> Option<SettingPage> {
    if !view.access(cx).permissions {
        return None;
    }
    let permissions = view.permissions.clone();
    Some(
        SettingPage::new("Permissions")
            .icon(IconName::ShieldCheck)
            .resettable(false)
            .group(SettingGroup::new().item(
                SettingItem::render(move |_, _, _| permissions.clone()).keywords([
                    "permissions",
                    "overrides",
                    "roles",
                    "everyone",
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

/// The Overview form: the channel's icon, name, description and slowmode,
/// saved together with one `PATCH /channels/{id}` (see
/// [`ChannelForm::changes`]). A new icon is uploaded first. Marking it
/// mature is a button of its own, which asks first and saves at once.
pub struct ChannelOverviewPage {
    chat: WeakEntity<ChatView>,
    channel: String,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    icon: ImageSlot,
    /// Seconds, as chosen (0 is off).
    slowmode: u64,
    /// The channel's values the fields were last filled with. A field still
    /// showing its value is untouched, and follows changes made elsewhere;
    /// once it's edited, it keeps the edit.
    filled: ChannelForm,
    saving: Option<Saving>,
    /// Waiting for Stoat to (un)mark it mature.
    marking: bool,
    _subscriptions: Vec<Subscription>,
}

impl ChannelOverviewPage {
    fn new(
        chat: &Entity<ChatView>,
        channel: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kind = chat
            .read(cx)
            .store()
            .channel(&channel)
            .and_then(ChannelKind::of)
            .unwrap_or(ChannelKind::Channel);
        let name = cx.new(|cx| InputState::new(window, cx));
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(DESCRIPTION_ROWS.0, DESCRIPTION_ROWS.1)
                .placeholder(format!("This {} is about…", kind.noun()))
        });
        let subscriptions = vec![
            // Counters and Save follow what's typed.
            cx.subscribe(&name, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&description, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.observe_in(chat, window, |this, _, window, cx| this.follow(window, cx)),
        ];
        let mut this = Self {
            chat: chat.downgrade(),
            channel,
            name,
            description,
            icon: ImageSlot::default(),
            slowmode: 0,
            filled: ChannelForm::default(),
            saving: None,
            marking: false,
            _subscriptions: subscriptions,
        };
        this.reset(window, cx);
        this
    }

    /// The channel as the store has it now.
    fn current(&self, cx: &App) -> Option<Channel> {
        let chat = self.chat.upgrade()?;
        chat.read(cx).store().channel(&self.channel).cloned()
    }

    /// What the settings are for, as the store has it.
    fn kind(&self, cx: &App) -> ChannelKind {
        self.current(cx)
            .as_ref()
            .and_then(ChannelKind::of)
            .unwrap_or(ChannelKind::Channel)
    }

    /// The form as edited.
    fn edited(&self, cx: &App) -> ChannelForm {
        ChannelForm {
            name: self.name.read(cx).value().to_string(),
            description: self.description.read(cx).value().to_string(),
            icon: self.icon.image.clone(),
            slowmode: self.slowmode,
        }
    }

    /// Whether Save can send the form: every field and the icon fine,
    /// something changed, and not already saving.
    fn can_save(&self, edited: &ChannelForm, cx: &App) -> bool {
        self.saving.is_none()
            && edited.is_valid()
            && self.icon.problem.is_none()
            && self
                .current(cx)
                .is_some_and(|channel| edited.has_changes(&channel))
    }

    /// Fill every field with the channel's values.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.current(cx).as_ref().and_then(ChannelForm::of) else {
            return;
        };
        self.name.update(cx, |input, cx| {
            input.set_value(form.name.clone(), window, cx)
        });
        self.description.update(cx, |input, cx| {
            input.set_value(form.description.clone(), window, cx)
        });
        self.icon = slot(form.icon.clone());
        self.slowmode = form.slowmode;
        self.filled = form;
        cx.notify();
    }

    /// The channel changed (here or elsewhere): untouched fields take its
    /// new values, edited ones keep theirs.
    fn follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(now) = self.current(cx).as_ref().and_then(ChannelForm::of) else {
            return;
        };
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
        if edited.slowmode == self.filled.slowmode {
            self.slowmode = now.slowmode;
        }
        self.filled = now;
        cx.notify();
    }

    /// Change… on the icon's row.
    fn pick_icon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let limit = self
            .chat
            .upgrade()
            .map(|chat| chat.read(cx).image_limits())
            .unwrap_or_default()
            .server_icon;
        prompt_for_image(window, cx, move |this: &mut Self, path, _| {
            this.icon.pick(path, limit);
        });
    }

    /// Send what changed: upload a new icon first, then one `PATCH` with
    /// its ID, only if the upload worked. Stoat's answer shows at once, in
    /// the main window too; a failure says why in a toast, and the form
    /// keeps the edit.
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
        let upload: Option<PathBuf> = edited.new_file().map(|path| path.to_path_buf());
        let file_server = match (file_server, upload.is_none()) {
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
        self.saving = Some(if upload.is_none() {
            Saving::Sending
        } else {
            Saving::Uploading
        });
        cx.notify();
        let id = self.channel.clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut edited = edited;
            if let Some(path) = upload {
                let upload = spawn_io(upload_image(
                    client.clone(),
                    file_server.clone(),
                    ICON_TAG,
                    path,
                ));
                match upload.await {
                    Ok(Ok(uploaded)) => edited.icon = ProfileImage::Current(uploaded),
                    failed => {
                        let failed = failed.map(|result| result.map(|_| ()));
                        this.update_in(cx, |this, window, cx| this.save_failed(failed, window, cx))
                            .ok();
                        return;
                    }
                }
            }
            // Against the channel as it is now, which may have changed
            // while the icon uploaded.
            let Ok(edit) = this.update(cx, |this, cx| {
                this.saving = Some(Saving::Sending);
                cx.notify();
                this.current(cx)
                    .and_then(|channel| edited.changes(&channel))
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
            let request = spawn_io(async move { client.edit_channel(&id, &edit).await });
            let result = request.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(saved)) => {
                    this.saving = None;
                    if let Some(chat) = this.chat.upgrade() {
                        chat.update(cx, |chat, cx| chat.channel_edited(&saved, cx));
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

    /// The upload or the `PATCH` failed: say why, and keep what was edited.
    fn save_failed(
        &mut self,
        result: Result<Result<(), ApiError>, impl std::fmt::Debug>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving = None;
        let text = match result {
            Ok(Err(err)) => {
                log::warn!("couldn't save channel {}: {err:?}", self.channel);
                save_error_text(&err)
            }
            Ok(Ok(())) => return,
            Err(err) => {
                log::warn!("channel save task failed: {err:?}");
                "Couldn't save the channel's settings.".into()
            }
        };
        window.push_notification(Notification::error(text), cx);
        cx.notify();
    }

    /// Whether the channel is marked mature, as the store has it.
    fn mature(&self, cx: &App) -> bool {
        self.chat
            .upgrade()
            .is_some_and(|chat| chat.read(cx).store().is_mature(&self.channel))
    }

    /// Ask before marking the channel mature (or not), in the web client's
    /// words. The button waits, disabled, for Stoat's answer, which shows
    /// at once; a refusal leaves the dialog open with a toast.
    fn confirm_mature(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mature = self.mature(cx);
        let prompt = mature_prompt(self.kind(cx), mature);
        let page = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let busy = page.upgrade().is_some_and(|page| page.read(cx).marking);
            let mark = {
                let page = page.clone();
                move |window: &mut Window, cx: &mut App| {
                    page.update(cx, |this, cx| this.set_mature(!mature, window, cx))
                        .ok();
                }
            };
            alert
                .title(prompt.title.clone())
                .description(prompt.text.clone())
                .keyboard(!busy)
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("mature-cancel")
                                .label("Keep as is")
                                .disabled(busy)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("mature-ok")
                                .with_variant(ButtonVariant::Primary)
                                .label(prompt.confirm)
                                .loading(busy)
                                .disabled(busy)
                                .on_click({
                                    let mark = mark.clone();
                                    move |_, window, cx| mark(window, cx)
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    if !busy {
                        mark(window, cx);
                    }
                    false
                })
        });
    }

    /// Send `{ nsfw: mature }` alone. On Stoat's answer it shows at once
    /// (the main window gates or ungates the channel) and the confirmation
    /// closes; otherwise a toast says why.
    fn set_mature(&mut self, mature: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.marking {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let client = chat.read(cx).client().clone();
        self.marking = true;
        window.refresh();
        let id = self.channel.clone();
        let request = spawn_io(async move { client.edit_channel(&id, &mature_edit(mature)).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.marking = false;
                match result {
                    Ok(Ok(saved)) => {
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.channel_edited(&saved, cx));
                        }
                        window.close_dialog(cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't mark channel {} mature: {err:?}", this.channel);
                        window.push_notification(Notification::error(mature_error_text(&err)), cx);
                    }
                    Err(err) => {
                        log::warn!("mature task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't change whether the channel is mature."),
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

    /// "Slowmode": how long members wait between messages, picked from the
    /// web client's choices (and the stored value, if it's another).
    fn render_slowmode(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let chosen = self.slowmode;
        let entity = cx.entity().downgrade();
        // The stored value, and the one chosen (which may be an earlier
        // stored value, since followed), are always offered.
        let mut choices = slowmode_choices(self.filled.slowmode);
        if !choices.iter().any(|(secs, _)| *secs == chosen) {
            let at = choices.partition_point(|(secs, _)| *secs < chosen);
            choices.insert(at, (chosen, slowmode_label(chosen)));
        }
        h_flex()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Slowmode"),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        "How long members wait between messages. Members with Bypass Slowmode \
                         don't wait.",
                    )),
            )
            .child(
                Button::new("channel-slowmode")
                    .outline()
                    .small()
                    .label(slowmode_label(chosen))
                    .icon(IconName::ChevronDown)
                    .disabled(self.saving.is_some())
                    .dropdown_menu(move |menu: PopupMenu, _, _| {
                        let menu = menu.scrollable(true).max_h(px(SLOWMODE_MENU_HEIGHT));
                        choices.iter().fold(menu, |menu, (secs, label)| {
                            let (entity, secs) = (entity.clone(), *secs);
                            menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(secs == chosen)
                                    .on_click(move |_, _, cx| {
                                        entity
                                            .update(cx, |this, cx| {
                                                this.slowmode = secs;
                                                cx.notify();
                                            })
                                            .ok();
                                    }),
                            )
                        })
                    }),
            )
            .into_any_element()
    }

    /// "Mature": whether members confirm their age first, and the button
    /// that changes it.
    fn render_mature(&self, kind: ChannelKind, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let mature = self.mature(cx);
        h_flex()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Mature"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(mature_state(kind, mature)),
                    ),
            )
            .child(
                Button::new("channel-mature")
                    .outline()
                    .small()
                    .label(if mature {
                        "Unmark as mature…"
                    } else {
                        "Mark as mature…"
                    })
                    .disabled(self.marking)
                    .on_click(cx.listener(|this, _, window, cx| this.confirm_mature(window, cx))),
            )
            .into_any_element()
    }

    /// The icon's row.
    fn render_icon(&self, kind: ChannelKind, cx: &mut Context<Self>) -> Option<AnyElement> {
        let chat = self.chat.upgrade()?;
        let chat = chat.read(cx);
        let store = chat.store();
        let limit = chat.image_limits().server_icon;
        let name = self
            .current(cx)
            .and_then(|channel| ChannelForm::of(&channel))
            .map(|form| form.name)
            .unwrap_or_default();
        let preview = avatar_preview(
            "channel-icon-preview",
            Preview::of(&self.icon.image, ICON_TAG, store.media(), Preview::Empty),
            &name,
            cx,
        );
        let label = match kind {
            ChannelKind::Channel => "Channel icon",
            ChannelKind::Group => "Group icon",
        };
        Some(render_image_row(
            "channel-icon",
            label,
            preview,
            &self.icon,
            limit,
            self.saving.is_some(),
            cx.listener(|this, _, window, cx| this.pick_icon(window, cx)),
            cx.listener(|this, _, _, cx| {
                this.icon.remove();
                cx.notify();
            }),
            cx,
        ))
    }
}

/// An image slot showing `image`, with nothing wrong.
fn slot(image: ProfileImage) -> ImageSlot {
    ImageSlot {
        image,
        problem: None,
    }
}

impl Render for ChannelOverviewPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind(cx);
        let edited = self.edited(cx);
        let saving = self.saving.is_some();
        let changed = edited != self.filled || self.icon.problem.is_some();
        let can_save = self.can_save(&edited, cx);
        let icon = self.render_icon(kind, cx);
        let (name_label, description_label, no_name) = match kind {
            ChannelKind::Channel => (
                "Channel name",
                "Channel description",
                "A channel needs a name.",
            ),
            ChannelKind::Group => ("Group name", "Group description", "A group needs a name."),
        };
        let name_count = edited.name.trim().chars().count();
        let name = field(
            name_label,
            Input::new(&self.name).disabled(saving).into_any_element(),
            name_count,
            NAME_MAX,
            (name_count == 0).then(|| no_name.to_string()),
            cx,
        );
        let description = field(
            description_label,
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
            .children(icon)
            .child(name)
            .child(description)
            .when(kind == ChannelKind::Channel, |el| {
                el.child(self.render_slowmode(cx))
            })
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("channel-overview-reset")
                            .label("Reset")
                            .outline()
                            .small()
                            .disabled(!changed || saving)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    )
                    .child(
                        Button::new("channel-overview-save")
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
            // Saved on its own, after asking, as in the web client.
            .child(
                div()
                    .pt_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(self.render_mature(kind, cx)),
            )
    }
}
