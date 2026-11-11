//! Server Settings' Emojis page: a server's custom emoji, each with who
//! uploaded it, Rename and Delete, and "Upload emoji…". What it lists, who
//! may manage them, the name rule and the texts are
//! [`crate::custom_emoji`].

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::custom_emoji::{
    EMOJI_NAME_MAX, EMOJI_TAG, EmojiRow, at_limit, check_emoji_name, count_line, delete_prompt,
    emoji_error_text, emoji_rows, name_from_file,
};
use crate::profile_edit::check_image;
use crate::runtime::spawn_io;
use crate::ui::chat::ChatView;
use crate::ui::profile_editor::{field, upload_image};
use crate::ui::server_settings::open_focused_dialog;

/// How big each emoji shows in the list.
const EMOJI_SIZE: f32 = 32.;
/// How big the emoji to upload shows in its dialog.
const PREVIEW_SIZE: f32 = 64.;

/// The Emojis page of one server's settings.
pub struct EmojiPage {
    chat: WeakEntity<ChatView>,
    server: String,
    /// The emoji whose "Delete :name:?" is waiting for Stoat to answer.
    deleting: Option<String>,
    _chat: Subscription,
}

impl EmojiPage {
    pub fn new(chat: &Entity<ChatView>, server: String, cx: &mut Context<Self>) -> Self {
        Self {
            chat: chat.downgrade(),
            server,
            deleting: None,
            _chat: cx.observe(chat, |_, _, cx| cx.notify()),
        }
    }

    /// "Upload emoji…": pick an image, then name it in a dialog.
    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            this.update_in(cx, |this, window, cx| this.open_upload(path, window, cx))
                .ok();
        })
        .detach();
    }

    fn open_upload(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let server = self.server.clone();
        let dialog = cx.new(|cx| UploadEmojiDialog::new(chat, server, path, window, cx));
        let focus = dialog.read(cx).input.read(cx).focus_handle(cx);
        open_focused_dialog("Upload emoji".into(), dialog, focus, window, cx);
    }

    fn open_rename(&mut self, row: &EmojiRow, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let dialog =
            cx.new(|cx| RenameEmojiDialog::new(chat, row.id.clone(), &row.name, window, cx));
        let focus = dialog.read(cx).input.read(cx).focus_handle(cx);
        open_focused_dialog(format!(":{}:", row.name).into(), dialog, focus, window, cx);
    }

    /// Ask before deleting `row`. The buttons wait, disabled, for Stoat's
    /// answer; on success the emoji goes at once and the dialog closes.
    fn confirm_delete(&mut self, row: &EmojiRow, window: &mut Window, cx: &mut Context<Self>) {
        let (title, detail) = delete_prompt(&row.name);
        let title = SharedString::from(title);
        let id = row.id.clone();
        let page = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let busy = page
                .upgrade()
                .is_some_and(|page| page.read(cx).deleting.as_ref() == Some(&id));
            let delete = {
                let (page, id) = (page.clone(), id.clone());
                move |window: &mut Window, cx: &mut App| {
                    page.update(cx, |this, cx| this.delete(id.clone(), window, cx))
                        .ok();
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
                            Button::new("delete-emoji-cancel")
                                .label("Cancel")
                                .disabled(busy)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("delete-emoji-ok")
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

    /// Delete emoji `id` (the confirmation's Delete).
    fn delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.deleting.is_some() {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.deleting = Some(id.clone());
        window.refresh();
        let request = spawn_io({
            let (client, id) = (chat.read(cx).client().clone(), id.clone());
            async move { client.delete_emoji(&id).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.deleting = None;
                match result {
                    Ok(Ok(())) => {
                        log::info!("deleted emoji {id} of {}", this.server);
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.emoji_deleted(&id, cx));
                        }
                        window.close_dialog(cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't delete emoji {id}: {err:?}");
                        window.push_notification(Notification::error(emoji_error_text(&err)), cx);
                    }
                    Err(err) => {
                        log::warn!("emoji delete task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't delete the emoji."),
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

    fn render_row(&self, row: EmojiRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let image = match row.url.clone() {
            Some(url) => img(SharedUri::from(url))
                .id(SharedString::from(format!("emoji-image-{}", row.id)))
                .size(px(EMOJI_SIZE))
                .flex_none()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div().size(px(EMOJI_SIZE)).flex_none().into_any_element(),
        };
        let delete = {
            let row = row.clone();
            cx.listener(move |this, _, window, cx| this.confirm_delete(&row, window, cx))
        };
        let rename = {
            let row = row.clone();
            cx.listener(move |this, _, window, cx| this.open_rename(&row, window, cx))
        };
        h_flex()
            .id(SharedString::from(format!("emoji-{}", row.id)))
            .w_full()
            .gap_3()
            .px_2()
            .py_1()
            .rounded_md()
            .child(image)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!(":{}:", row.name)),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("Uploaded by {}", row.creator_name)),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("emoji-rename-{}", row.id)))
                    .icon(IconName::Pencil)
                    .ghost()
                    .small()
                    .tooltip("Rename emoji")
                    .on_click(rename),
            )
            .child(
                Button::new(SharedString::from(format!("emoji-delete-{}", row.id)))
                    .icon(IconName::Trash)
                    .ghost()
                    .small()
                    .tooltip("Delete emoji")
                    .on_click(delete),
            )
            .into_any_element()
    }
}

impl Render for EmojiPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (rows, max) = self
            .chat
            .upgrade()
            .map(|chat| {
                let chat = chat.read(cx);
                (emoji_rows(chat.store(), &self.server), chat.emoji_limit())
            })
            .unwrap_or_default();
        let count = rows.len();
        let full = at_limit(count, max);
        let muted = cx.theme().muted_foreground;
        let list: Vec<AnyElement> = rows
            .into_iter()
            .map(|row| self.render_row(row, cx))
            .collect();
        let empty = list.is_empty();
        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(muted)
                    .child(Icon::new(IconName::FaceSlightlySmiling).small())
                    .child(div().flex_1().child(count_line(count, max)))
                    .child(
                        Button::new("upload-emoji")
                            .label("Upload emoji…")
                            .icon(IconName::Plus)
                            .outline()
                            .small()
                            .disabled(full.is_some())
                            .when_some(full, |button, text| button.tooltip(text))
                            .on_click(cx.listener(|this, _, window, cx| this.pick(window, cx))),
                    ),
            )
            .when(empty, |el| {
                el.child(
                    div()
                        .px_2()
                        .text_sm()
                        .text_color(muted)
                        .child("This server has no custom emoji yet."),
                )
            })
            .child(v_flex().gap_1().children(list))
    }
}

/// "Upload emoji": the picked image, a name for it (from its file name, to
/// start with), and Create, which uploads it, then makes it the server's.
pub struct UploadEmojiDialog {
    chat: WeakEntity<ChatView>,
    server: String,
    path: PathBuf,
    /// Why the image can't be used, if it can't.
    problem: Option<String>,
    input: Entity<InputState>,
    creating: bool,
    error: Option<String>,
    _input: Subscription,
}

impl UploadEmojiDialog {
    fn new(
        chat: Entity<ChatView>,
        server: String,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let limit = chat.read(cx).emoji_size_limit();
        let problem = std::fs::metadata(&path)
            .map_err(|err| format!("Couldn't read the file ({err})."))
            .and_then(|meta| {
                check_image(&path, meta.len(), limit).map_err(|problem| problem.to_string())
            })
            .err();
        let name = name_from_file(&path);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
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
            path,
            problem,
            input,
            creating: false,
            error: None,
            _input: subscription,
        }
    }

    /// Upload the image, then make it an emoji with the typed name. Success
    /// shows it at once and closes the dialog; a refusal says why and keeps
    /// the dialog open.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating || self.problem.is_some() {
            return;
        }
        let Ok(name) = check_emoji_name(&self.input.read(cx).value()) else {
            return;
        };
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let (client, file_server) = {
            let chat = chat.read(cx);
            let file_server = chat.store().media().file_server().map(str::to_string);
            (chat.client().clone(), file_server)
        };
        let Some(file_server) = file_server else {
            self.error = Some("Couldn't upload: Stoat's file server isn't known yet.".into());
            cx.notify();
            return;
        };
        self.creating = true;
        self.error = None;
        cx.notify();
        let (server, path) = (self.server.clone(), self.path.clone());
        let request = spawn_io({
            let name = name.clone();
            async move {
                let id = upload_image(client.clone(), file_server, EMOJI_TAG, path).await?;
                client.create_emoji(&id, &server, &name).await
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match result {
                    Ok(Ok(emoji)) => {
                        log::info!("added emoji {} to {}", emoji.id, this.server);
                        this.chat
                            .update(cx, |chat, cx| chat.emoji_saved(emoji, cx))
                            .ok();
                        window.close_dialog(cx);
                        window.push_notification(
                            Notification::success(format!("Added :{name}:.")),
                            cx,
                        );
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't add an emoji to {}: {err:?}", this.server);
                        this.error = Some(emoji_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("emoji upload task failed: {err:?}");
                        this.error = Some("Couldn't add the emoji.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for UploadEmojiDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let value = self.input.read(cx).value().to_string();
        let checked = check_emoji_name(&value);
        let problem = match (&checked, value.trim().is_empty()) {
            // Blank isn't shouted at: Create is just disabled.
            (Err(_), true) => None,
            (Err(problem), false) => Some(problem.to_string()),
            (Ok(_), _) => None,
        };
        let name = field(
            "Emoji name",
            Input::new(&self.input)
                .disabled(self.creating)
                .into_any_element(),
            value.trim().chars().count(),
            EMOJI_NAME_MAX,
            problem.or_else(|| self.error.clone()),
            cx,
        );
        let can_create = !self.creating && self.problem.is_none() && checked.is_ok();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_4()
                    .items_center()
                    .child(
                        img(self.path.clone())
                            .size(px(PREVIEW_SIZE))
                            .flex_none()
                            .object_fit(ObjectFit::Contain),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(if self.problem.is_some() {
                                danger
                            } else {
                                muted
                            })
                            .child(self.problem.clone().unwrap_or_else(|| {
                                "Lowercase letters, numbers and underscores only.".into()
                            })),
                    ),
            )
            .child(name)
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("upload-emoji-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.creating)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("upload-emoji-create")
                            .primary()
                            .label(if self.creating {
                                "Uploading…"
                            } else {
                                "Create"
                            })
                            .loading(self.creating)
                            .disabled(!can_create)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
    }
}

/// "Rename :name:": a new name, then Rename.
pub struct RenameEmojiDialog {
    chat: WeakEntity<ChatView>,
    id: String,
    /// The name it had when the dialog opened.
    old: String,
    input: Entity<InputState>,
    renaming: bool,
    error: Option<String>,
    _input: Subscription,
}

impl RenameEmojiDialog {
    fn new(
        chat: Entity<ChatView>,
        id: String,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name.to_string()));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.rename(window, cx),
                _ => {}
            });
        Self {
            chat: chat.downgrade(),
            id,
            old: name.to_string(),
            input,
            renaming: false,
            error: None,
            _input: subscription,
        }
    }

    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming {
            return;
        }
        let Ok(name) = check_emoji_name(&self.input.read(cx).value()) else {
            return;
        };
        if name == self.old {
            window.close_dialog(cx);
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        self.renaming = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let (client, id) = (chat.read(cx).client().clone(), self.id.clone());
            async move { client.rename_emoji(&id, &name).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.renaming = false;
                match result {
                    Ok(Ok(emoji)) => {
                        log::info!("renamed emoji {}", this.id);
                        this.chat
                            .update(cx, |chat, cx| chat.emoji_saved(emoji, cx))
                            .ok();
                        window.close_dialog(cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't rename emoji {}: {err:?}", this.id);
                        this.error = Some(emoji_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("emoji rename task failed: {err:?}");
                        this.error = Some("Couldn't rename the emoji.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for RenameEmojiDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.input.read(cx).value().to_string();
        let checked = check_emoji_name(&value);
        let problem = match &checked {
            Err(problem) if !value.trim().is_empty() => Some(problem.to_string()),
            _ => None,
        };
        let name = field(
            "Emoji name",
            Input::new(&self.input)
                .disabled(self.renaming)
                .into_any_element(),
            value.trim().chars().count(),
            EMOJI_NAME_MAX,
            problem.or_else(|| self.error.clone()),
            cx,
        );
        v_flex().gap_3().child(name).child(
            h_flex()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("rename-emoji-cancel")
                        .outline()
                        .label("Cancel")
                        .disabled(self.renaming)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("rename-emoji-ok")
                        .primary()
                        .label("Rename")
                        .loading(self.renaming)
                        .disabled(self.renaming || checked.is_err())
                        .on_click(cx.listener(|this, _, window, cx| this.rename(window, cx))),
                ),
        )
    }
}
