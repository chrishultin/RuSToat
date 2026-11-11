//! Channel Settings' Webhooks page: a channel's (or group's) webhooks,
//! "Create webhook…", and each webhook's page on the same page: its URL to
//! copy, and Delete. The list lives in the store, fetched when the page
//! first shows and kept current by Stoat's webhook events. What it lists,
//! the URL and the texts are [`crate::webhooks`].

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::ApiError;
use crate::api::models::{Channel, Webhook};
use crate::profile_edit::ProfileImage;
use crate::runtime::spawn_io;
use crate::ui::chat::{ChatView, avatar};
use crate::ui::profile_editor::{
    ImageSlot, Preview, avatar_preview, field, prompt_for_image, render_image_row, upload_image,
};
use crate::ui::server_dialogs::name_field;
use crate::ui::server_settings::{open_focused_dialog, section_heading};
use crate::webhooks::{
    WEBHOOK_AVATAR_TAG, WEBHOOK_NAME_MAX, WebhookForm, WebhookRow, check_webhook_name,
    delete_prompt, list_error_text, shown_url, webhook_error_text, webhook_rows, webhook_url,
    webhooks_unavailable,
};

/// How big an avatar shows in the list.
const LIST_AVATAR: f32 = 24.;

/// Fetching the list.
enum Fetch {
    Idle,
    Loading {
        _task: Task<()>,
    },
    /// Why it couldn't be fetched.
    Failed(String),
    /// The instance has no webhooks at all.
    Unavailable,
}

/// What the page shows.
enum PageMode {
    List,
    View(Entity<WebhookView>),
}

/// The Webhooks page of one channel's settings.
pub struct WebhooksPage {
    chat: WeakEntity<ChatView>,
    channel: String,
    fetch: Fetch,
    mode: PageMode,
    _subscriptions: Vec<Subscription>,
}

impl WebhooksPage {
    pub fn new(chat: &Entity<ChatView>, channel: String, cx: &mut Context<Self>) -> Self {
        Self {
            chat: chat.downgrade(),
            channel,
            fetch: Fetch::Idle,
            mode: PageMode::List,
            _subscriptions: vec![cx.observe(chat, |this, _, cx| this.follow(cx))],
        }
    }

    /// The store changed: back to the list if the open webhook has gone.
    fn follow(&mut self, cx: &mut Context<Self>) {
        if let PageMode::View(view) = &self.mode {
            let id = view.read(cx).id.clone();
            let gone = self.chat.upgrade().is_none_or(|chat| {
                chat.read(cx)
                    .store()
                    .webhooks(&self.channel)
                    .is_some_and(|list| list.iter().all(|webhook| webhook.id != id))
            });
            if gone {
                self.mode = PageMode::List;
            }
        }
        cx.notify();
    }

    /// Whether the store has the list.
    fn fetched(&self, cx: &App) -> bool {
        self.chat
            .upgrade()
            .is_some_and(|chat| chat.read(cx).store().webhooks(&self.channel).is_some())
    }

    /// Fetch the list if the store doesn't have it (first shown, or since a
    /// reconnect), unless that's under way or has failed.
    pub fn load_once(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.fetch, Fetch::Idle) && !self.fetched(cx) {
            self.refresh(window, cx);
        }
    }

    /// Fetch the list again.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let (client, channel) = (chat.read(cx).client().clone(), self.channel.clone());
            async move { client.fetch_webhooks(&channel).await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.fetch = match result {
                    Ok(Ok(webhooks)) => {
                        log::debug!("{} webhooks in {}", webhooks.len(), this.channel);
                        let channel = this.channel.clone();
                        this.chat
                            .update(cx, |chat, cx| chat.webhooks_fetched(&channel, webhooks, cx))
                            .ok();
                        Fetch::Idle
                    }
                    Ok(Err(err)) if webhooks_unavailable(&err) => Fetch::Unavailable,
                    Ok(Err(err)) => {
                        log::warn!("couldn't list the webhooks of {}: {err:?}", this.channel);
                        Fetch::Failed(list_error_text(&err))
                    }
                    Err(err) => {
                        log::warn!("webhooks task failed: {err:?}");
                        Fetch::Failed("Couldn't load the webhooks.".into())
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.fetch = Fetch::Loading { _task: task };
        cx.notify();
    }

    fn open(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let channel = self.channel.clone();
        let view = cx.new(|cx| WebhookView::new(&chat, channel, id, window, cx));
        self._subscriptions
            .push(cx.subscribe(&view, |this, _, _: &BackToWebhooks, cx| {
                this.mode = PageMode::List;
                cx.notify();
            }));
        self.mode = PageMode::View(view);
        cx.notify();
    }

    /// "Create webhook": a name, then Create, which opens the new webhook.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let channel = self.channel.clone();
        let dialog = cx.new(|cx| CreateWebhookDialog::new(chat, channel, window, cx));
        self._subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |this, _, created: &WebhookCreated, window, cx| {
                this.open(created.0.clone(), window, cx)
            },
        ));
        let focus = dialog.read(cx).input.read(cx).focus_handle(cx);
        open_focused_dialog("Create webhook".into(), dialog, focus, window, cx);
    }

    fn render_row(&self, row: WebhookRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let id = row.id.clone();
        h_flex()
            .id(SharedString::from(format!("webhook-{}", row.id)))
            .w_full()
            .gap_3()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|el| el.bg(theme.secondary))
            .on_click(cx.listener(move |this, _, window, cx| this.open(id.clone(), window, cx)))
            .child(avatar(
                format!("webhook-avatar-{}", row.id),
                row.avatar_url,
                &row.name,
                LIST_AVATAR,
                theme.secondary,
                theme.secondary_foreground,
            ))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(row.name),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(row.id),
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
        let rows = self
            .chat
            .upgrade()
            .and_then(|chat| webhook_rows(chat.read(cx).store(), &self.channel));
        let body = match (&self.fetch, rows) {
            (Fetch::Unavailable, _) => {
                note("This Stoat instance doesn't have webhooks.".into()).into_any_element()
            }
            (Fetch::Failed(text), None) => v_flex()
                .gap_2()
                .child(note(text.clone()))
                .child(
                    div().px_2().child(
                        Button::new("webhooks-retry")
                            .label("Try again")
                            .outline()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                    ),
                )
                .into_any_element(),
            (_, None) => note("Loading…".into()).into_any_element(),
            (_, Some(rows)) => {
                let empty = rows.is_empty();
                let rows: Vec<AnyElement> = rows
                    .into_iter()
                    .map(|row| self.render_row(row, cx))
                    .collect();
                v_flex()
                    .gap_1()
                    .when(empty, |el| {
                        el.child(note("This channel has no webhooks yet.".into()))
                    })
                    .children(rows)
                    .into_any_element()
            }
        };
        let can_create = self.fetched(cx) && !matches!(self.fetch, Fetch::Unavailable);
        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(muted)
                    .child(Icon::new(IconName::Webhook).small())
                    .child(
                        div().flex_1().child(
                            "Webhooks post messages here from other apps, through their URL.",
                        ),
                    )
                    .when(can_create, |el| {
                        el.child(
                            Button::new("create-webhook")
                                .label("Create webhook…")
                                .icon(IconName::Plus)
                                .outline()
                                .small()
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.create(window, cx)),
                                ),
                        )
                    }),
            )
            .child(body)
            .into_any_element()
    }
}

impl Render for WebhooksPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.mode {
            PageMode::List => self.render_list(window, cx),
            PageMode::View(view) => view.clone().into_any_element(),
        }
    }
}

/// "Create webhook": its name, then Create.
pub struct CreateWebhookDialog {
    chat: WeakEntity<ChatView>,
    channel: String,
    input: Entity<InputState>,
    creating: bool,
    error: Option<String>,
    _input: Subscription,
}

/// A webhook was created, by its ID.
pub struct WebhookCreated(pub String);

impl EventEmitter<WebhookCreated> for CreateWebhookDialog {}

impl CreateWebhookDialog {
    fn new(
        chat: Entity<ChatView>,
        channel: String,
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
            channel,
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
        let Some(name) = check_webhook_name(&self.input.read(cx).value()) else {
            return;
        };
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.creating = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let (client, channel) = (chat.read(cx).client().clone(), self.channel.clone());
            async move { client.create_webhook(&channel, &name).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match result {
                    Ok(Ok(webhook)) => {
                        log::info!("created webhook {} in {}", webhook.id, this.channel);
                        let id = webhook.id.clone();
                        this.chat
                            .update(cx, |chat, cx| chat.webhook_saved(webhook, cx))
                            .ok();
                        window.close_dialog(cx);
                        cx.emit(WebhookCreated(id));
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't create a webhook in {}: {err:?}", this.channel);
                        this.error = Some(webhook_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("webhook create task failed: {err:?}");
                        this.error = Some("Couldn't create the webhook.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for CreateWebhookDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let valid = check_webhook_name(&self.input.read(cx).value()).is_some();
        v_flex()
            .gap_3()
            .child(name_field(
                "Webhook name",
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
                        Button::new("create-webhook-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.creating)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("create-webhook-ok")
                            .primary()
                            .label("Create")
                            .loading(self.creating)
                            .disabled(self.creating || !valid)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
    }
}

/// "‹ Webhooks" was clicked, or the webhook was deleted.
pub struct BackToWebhooks;

/// One webhook's page: its avatar and name (saved together, after
/// uploading a new avatar), its URL to copy, and Delete. Untouched fields
/// follow changes made elsewhere.
pub struct WebhookView {
    chat: WeakEntity<ChatView>,
    channel: String,
    id: String,
    name: Entity<InputState>,
    avatar: ImageSlot,
    /// The webhook's values the fields were last filled with.
    filled: WebhookForm,
    /// Waiting for the upload, then the `PATCH`.
    saving: bool,
    /// Copying is waiting for the list (and so the token).
    copying: bool,
    deleting: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BackToWebhooks> for WebhookView {}

impl WebhookView {
    fn new(
        chat: &Entity<ChatView>,
        channel: String,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.save(window, cx),
                _ => cx.notify(),
            }),
            cx.observe_in(chat, window, |this, _, window, cx| this.follow(window, cx)),
        ];
        let mut this = Self {
            chat: chat.downgrade(),
            channel,
            id,
            name,
            avatar: ImageSlot::default(),
            filled: WebhookForm::default(),
            saving: false,
            copying: false,
            deleting: false,
            _subscriptions: subscriptions,
        };
        this.reset(window, cx);
        this
    }

    /// The form as edited.
    fn edited(&self, cx: &App) -> WebhookForm {
        WebhookForm {
            name: self.name.read(cx).value().to_string(),
            avatar: self.avatar.image.clone(),
        }
    }

    fn can_save(&self, edited: &WebhookForm, cx: &App) -> bool {
        !self.saving
            && self.avatar.problem.is_none()
            && edited.is_valid()
            && self
                .current(cx)
                .is_some_and(|webhook| edited.has_changes(&webhook))
    }

    /// Fill the fields with the webhook's values.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.current(cx).as_ref().map(WebhookForm::of) else {
            return;
        };
        let name = form.name.clone();
        self.name
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.avatar = ImageSlot {
            image: form.avatar.clone(),
            problem: None,
        };
        self.filled = form;
        cx.notify();
    }

    /// The webhook changed (here or elsewhere): untouched fields take its
    /// new values, edited ones keep theirs.
    fn follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(now) = self.current(cx).as_ref().map(WebhookForm::of) else {
            cx.notify();
            return;
        };
        if now != self.filled {
            let edited = self.edited(cx);
            if edited.name == self.filled.name && now.name != self.filled.name {
                let name = now.name.clone();
                self.name
                    .update(cx, |input, cx| input.set_value(name, window, cx));
            }
            if edited.avatar == self.filled.avatar && self.avatar.problem.is_none() {
                self.avatar = ImageSlot {
                    image: now.avatar.clone(),
                    problem: None,
                };
            }
            self.filled = now;
        }
        cx.notify();
    }

    /// Change… on the avatar's row.
    fn pick_avatar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let limit = self
            .chat
            .upgrade()
            .map(|chat| chat.read(cx).image_limits())
            .unwrap_or_default()
            .avatar;
        prompt_for_image(window, cx, move |this: &mut Self, path, _| {
            this.avatar.pick(path, limit);
        });
    }

    /// Send what changed: upload a new avatar first, then one `PATCH` built
    /// against the webhook as stored then. Stoat's answer shows at once; a
    /// failure says why, and the form keeps the edit.
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
        let upload = edited.new_file().map(|path| path.to_path_buf());
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
        self.saving = true;
        cx.notify();
        let id = self.id.clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut edited = edited;
            if let Some(path) = upload {
                let upload = spawn_io(upload_image(
                    client.clone(),
                    file_server,
                    WEBHOOK_AVATAR_TAG,
                    path,
                ));
                match upload.await {
                    Ok(Ok(uploaded)) => edited.avatar = ProfileImage::Current(uploaded),
                    failed => {
                        let failed = failed.map(|result| result.map(|_| ()));
                        this.update_in(cx, |this, window, cx| this.save_failed(failed, window, cx))
                            .ok();
                        return;
                    }
                }
            }
            let Ok(edit) = this.update(cx, |this, cx| {
                this.current(cx)
                    .and_then(|webhook| edited.changes(&webhook))
            }) else {
                return;
            };
            let Some(edit) = edit else {
                this.update(cx, |this, cx| {
                    this.saving = false;
                    cx.notify();
                })
                .ok();
                return;
            };
            let request = spawn_io(async move { client.edit_webhook(&id, &edit).await });
            let result = request.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(saved)) => {
                    this.saving = false;
                    log::info!("saved webhook {}", this.id);
                    this.chat
                        .update(cx, |chat, cx| chat.webhook_saved(saved, cx))
                        .ok();
                    this.reset(window, cx);
                    window.push_notification(Notification::success("Saved."), cx);
                }
                failed => this.save_failed(failed.map(|result| result.map(|_| ())), window, cx),
            })
            .ok();
        })
        .detach();
    }

    /// The upload or the `PATCH` failed: say why, and keep the edit.
    fn save_failed(
        &mut self,
        result: Result<Result<(), ApiError>, impl std::fmt::Debug>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving = false;
        let text = match result {
            Ok(Err(err)) => {
                log::warn!("couldn't save webhook {}: {err:?}", self.id);
                webhook_error_text(&err)
            }
            Ok(Ok(())) => return,
            Err(err) => {
                log::warn!("webhook save task failed: {err:?}");
                "Couldn't save the webhook.".into()
            }
        };
        window.push_notification(Notification::error(text), cx);
        cx.notify();
    }

    /// The webhook as the store has it now.
    fn current(&self, cx: &App) -> Option<Webhook> {
        let chat = self.chat.upgrade()?;
        chat.read(cx)
            .store()
            .webhooks(&self.channel)?
            .iter()
            .find(|webhook| webhook.id == self.id)
            .cloned()
    }

    /// The API's address, which the URL starts with.
    fn api(&self, cx: &App) -> String {
        self.chat
            .upgrade()
            .map(|chat| chat.read(cx).client().base_url().to_string())
            .unwrap_or_default()
    }

    /// Copy the URL. Its token is known unless the webhook arrived by
    /// event, and then the list is fetched again first.
    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.copying {
            return;
        }
        let api = self.api(cx);
        if let Some(token) = self.current(cx).and_then(|webhook| webhook.token) {
            cx.write_to_clipboard(ClipboardItem::new_string(webhook_url(
                &api, &self.id, &token,
            )));
            window.push_notification(Notification::success("Copied."), cx);
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.copying = true;
        cx.notify();
        let request = spawn_io({
            let (client, channel) = (chat.read(cx).client().clone(), self.channel.clone());
            async move { client.fetch_webhooks(&channel).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.copying = false;
                let token = match result {
                    Ok(Ok(webhooks)) => {
                        let token = webhooks
                            .iter()
                            .find(|webhook| webhook.id == this.id)
                            .and_then(|webhook| webhook.token.clone());
                        let channel = this.channel.clone();
                        this.chat
                            .update(cx, |chat, cx| chat.webhooks_fetched(&channel, webhooks, cx))
                            .ok();
                        token
                    }
                    failed => {
                        log::warn!("couldn't fetch webhook {}'s token: {failed:?}", this.id);
                        None
                    }
                };
                match token {
                    Some(token) => {
                        let url = webhook_url(&api, &this.id, &token);
                        cx.write_to_clipboard(ClipboardItem::new_string(url));
                        window.push_notification(Notification::success("Copied."), cx);
                    }
                    None => window.push_notification(
                        Notification::error("Couldn't get the webhook's URL."),
                        cx,
                    ),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Ask before deleting the webhook.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(webhook) = self.current(cx) else {
            return;
        };
        let (title, detail) = delete_prompt(&webhook.name);
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
                            Button::new("delete-webhook-cancel")
                                .label("Cancel")
                                .disabled(busy)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("delete-webhook-ok")
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

    /// Delete the webhook (the confirmation's Delete). In a group, Stoat
    /// deletes it and then fails, so a failure there fetches the list
    /// again, and the webhook being gone counts as deleted.
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let group = matches!(
            chat.read(cx).store().channel(&self.channel),
            Some(Channel::Group { .. })
        );
        self.deleting = true;
        window.refresh();
        let client = chat.read(cx).client().clone();
        let (id, channel) = (self.id.clone(), self.channel.clone());
        let request = spawn_io(async move {
            match client.delete_webhook(&id).await {
                Ok(()) => Ok(None),
                Err(err) if group => match client.fetch_webhooks(&channel).await {
                    Ok(list) if list.iter().all(|webhook| webhook.id != id) => Ok(Some(list)),
                    _ => Err(err),
                },
                Err(err) => Err(err),
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result: Result<Result<Option<Vec<Webhook>>, ApiError>, _> = request.await;
            this.update_in(cx, |this, window, cx| {
                this.deleting = false;
                match result {
                    Ok(Ok(list)) => {
                        log::info!("deleted webhook {}", this.id);
                        let (channel, id) = (this.channel.clone(), this.id.clone());
                        this.chat
                            .update(cx, |chat, cx| match list {
                                Some(list) => chat.webhooks_fetched(&channel, list, cx),
                                None => chat.webhook_deleted(&channel, &id, cx),
                            })
                            .ok();
                        window.close_dialog(cx);
                        cx.emit(BackToWebhooks);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't delete webhook {}: {err:?}", this.id);
                        window.push_notification(Notification::error(webhook_error_text(&err)), cx);
                    }
                    Err(err) => {
                        log::warn!("webhook delete task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't delete the webhook."),
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
}

impl Render for WebhookView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(webhook) = self.current(cx) else {
            return div().into_any_element();
        };
        let edited = self.edited(cx);
        let changed = edited != self.filled || self.avatar.problem.is_some();
        let can_save = self.can_save(&edited, cx);
        let (limit, preview) = match self.chat.upgrade() {
            Some(chat) => {
                let chat = chat.read(cx);
                let preview = Preview::of(
                    &self.avatar.image,
                    WEBHOOK_AVATAR_TAG,
                    chat.store().media(),
                    Preview::Empty,
                );
                (chat.image_limits().avatar, preview)
            }
            None => (0, Preview::Empty),
        };
        let avatar_row = render_image_row(
            "webhook-avatar",
            "Avatar",
            avatar_preview("webhook-avatar-preview", preview, &edited.name, cx),
            &self.avatar,
            limit,
            self.saving,
            cx.listener(|this, _, window, cx| this.pick_avatar(window, cx)),
            cx.listener(|this, _, _, cx| {
                this.avatar.remove();
                cx.notify();
            }),
            cx,
        );
        let name_count = edited.name.trim().chars().count();
        let name = field(
            "Name",
            Input::new(&self.name)
                .disabled(self.saving)
                .into_any_element(),
            name_count,
            WEBHOOK_NAME_MAX,
            (name_count == 0).then(|| "A webhook needs a name.".to_string()),
            cx,
        );
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let api = self.api(cx);
        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("webhooks-back")
                            .label("Webhooks")
                            .icon(IconName::ChevronLeft)
                            .ghost()
                            .small()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(BackToWebhooks))),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("webhook-reset")
                            .label("Reset")
                            .outline()
                            .small()
                            .disabled(!changed || self.saving)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    )
                    .child(
                        Button::new("webhook-save")
                            .label("Save")
                            .primary()
                            .small()
                            .loading(self.saving)
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
            .child(div().text_xs().text_color(muted).child(webhook.id.clone()))
            .child(avatar_row)
            .child(name)
            .child(
                v_flex()
                    .gap_2()
                    .child(section_heading("Webhook URL"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child("Anything with this URL can post here as the webhook."),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(theme.secondary)
                                    .text_sm()
                                    .font_family("Menlo")
                                    .child(shown_url(&api, &webhook.id)),
                            )
                            .child(
                                Button::new("webhook-copy-url")
                                    .label("Copy URL")
                                    .icon(IconName::Copy)
                                    .outline()
                                    .small()
                                    .loading(self.copying)
                                    .disabled(self.copying)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.copy(window, cx)),
                                    ),
                            ),
                    ),
            )
            .child(
                div().pt_4().border_t_1().border_color(theme.border).child(
                    Button::new("webhook-delete")
                        .label("Delete webhook…")
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
            .into_any_element()
    }
}
