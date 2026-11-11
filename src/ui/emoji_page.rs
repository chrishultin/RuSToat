//! Server Settings' Emojis page: a server's custom emoji, each with who
//! uploaded it, and Delete. What it lists, who may manage them and the
//! texts are [`crate::custom_emoji`].

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::custom_emoji::{EmojiRow, count_line, delete_prompt, emoji_error_text, emoji_rows};
use crate::runtime::spawn_io;
use crate::ui::chat::ChatView;

/// How big each emoji shows in the list.
const EMOJI_SIZE: f32 = 32.;

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
                    .child(count_line(count, max)),
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
