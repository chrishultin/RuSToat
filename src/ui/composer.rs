//! The message box under a conversation. Enter sends, Shift+Enter adds a
//! new line, and each channel keeps its own draft, replies included.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{
    Escape, InputEvent, MoveUp, Position, Textarea, TextareaState,
};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::models::ReplyIntent;
use crate::draft::{
    Draft, Drafts, EditStash, MAX_MESSAGE_CHARS, ReplyTarget, SendCheck, TooManyReplies, char_count,
    check_send,
};

pub enum ComposerEvent {
    /// Send this text (already checked and trimmed) in the composer's
    /// channel, replying to `replies`.
    Send {
        text: String,
        replies: Vec<ReplyIntent>,
    },
    /// Save new text for the user's message `id` (already checked and trimmed).
    Edit { id: String, content: String },
    /// Up in an empty box: edit the user's last message.
    EditLast,
    /// The user typed (or cleared the box): `empty` if nothing's left. Not
    /// sent while editing, which isn't typing a message.
    Input { empty: bool },
}

/// Shown when an edit would leave the message empty (Stoat needs at least
/// one character).
const EMPTY_EDIT_TEXT: &str = "To delete the message, use Delete.";

pub struct Composer {
    input: Entity<TextareaState>,
    /// The channel being typed in.
    channel: Option<String>,
    /// What the message replies to. (Its text lives in `input`.)
    draft: Draft,
    drafts: Drafts,
    /// Editing one of the user's messages, and the draft set aside meanwhile.
    editing: EditStash,
    /// Why Enter didn't do anything (cleared when the text changes).
    hint: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ComposerEvent> for Composer {}

/// Rows the message box grows to before it scrolls.
const MAX_ROWS: usize = 8;

impl Composer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, MAX_ROWS)
                // Enter emits `PressEnter` without inserting a newline;
                // Shift+Enter still inserts one.
                .submit_on_enter(true)
        });
        let subscriptions = vec![cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                // The character count follows the text.
                InputEvent::Change => {
                    this.hint = None;
                    if this.editing.editing().is_none() {
                        let empty = this.input.read(cx).value().trim().is_empty();
                        cx.emit(ComposerEvent::Input { empty });
                    }
                    cx.notify()
                }
                _ => {}
            },
        )];
        Self {
            input,
            channel: None,
            draft: Draft::default(),
            drafts: Drafts::default(),
            editing: EditStash::default(),
            hint: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    /// Type in `channel` from now on: the current text and replies are kept
    /// as the old channel's draft, and `channel`'s own draft comes back.
    pub fn set_channel(
        &mut self,
        channel: Option<&str>,
        placeholder: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.channel.as_deref() == channel {
            return;
        }
        // Leaving mid-edit cancels it; the draft underneath is kept.
        self.cancel_edit(window, cx);
        self.hint = None;
        if let Some(old) = &self.channel {
            let draft = Draft {
                text: self.input.read(cx).value().to_string(),
                replies: std::mem::take(&mut self.draft.replies),
            };
            self.drafts.save(old, draft);
        }
        let Draft { text, replies } = channel.map(|c| self.drafts.take(c)).unwrap_or_default();
        self.draft.replies = replies;
        self.channel = channel.map(str::to_string);
        self.input
            .update(cx, |input, cx| input.set_placeholder(placeholder, window, cx));
        self.set_text(&text, window, cx);
        cx.notify();
    }

    /// Replace the box's text, with the cursor at its end.
    fn set_text(&self, text: &str, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| {
            input.set_value(text.to_string(), window, cx);
            input.set_cursor_position(end_of(text), window, cx);
        });
    }

    /// Edit the user's message `id`, whose text is `content`, in the box.
    /// Whatever was being typed (and replied to) comes back afterwards.
    pub fn start_edit(
        &mut self,
        id: String,
        content: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = Draft {
            text: self.input.read(cx).value().to_string(),
            replies: std::mem::take(&mut self.draft.replies),
        };
        self.editing.begin(id, current);
        self.hint = None;
        self.set_text(content, window, cx);
        self.focus(window, cx);
        cx.notify();
    }

    /// Stop editing `id` (it's being deleted), putting the draft back.
    pub fn stop_editing(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.editing() == Some(id) {
            self.cancel_edit(window, cx);
        }
    }

    /// Leave edit mode, putting back what was being typed. Returns false if
    /// there was no edit.
    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(stash) = self.editing.end() else {
            return false;
        };
        self.draft.replies = stash.replies;
        self.hint = None;
        self.set_text(&stash.text, window, cx);
        cx.notify();
        true
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Reply to `target` too, and focus the box to type the reply.
    pub fn add_reply(
        &mut self,
        target: ReplyTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), TooManyReplies> {
        self.draft.add_reply(target)?;
        self.focus(window, cx);
        cx.notify();
        Ok(())
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.channel.is_none() {
            return;
        }
        let text = self.input.read(cx).value();
        if let Some(id) = self.editing.editing().map(str::to_string) {
            match check_send(&text) {
                SendCheck::Ready(content) => {
                    self.cancel_edit(window, cx);
                    cx.emit(ComposerEvent::Edit { id, content });
                }
                SendCheck::Empty => {
                    self.hint = Some(EMPTY_EDIT_TEXT.into());
                    cx.notify();
                }
                SendCheck::TooLong(_) => {}
            }
            return;
        }
        // Blank and too-long messages stay in the box (the count says why).
        if let SendCheck::Ready(text) = check_send(&text) {
            let replies = self.draft.reply_intents();
            self.draft.replies.clear();
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
            cx.emit(ComposerEvent::Send { text, replies });
            cx.notify();
        }
    }

    /// Esc in the message box: stop editing, or else stop replying.
    fn on_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.cancel_edit(window, cx) {
            return;
        }
        if self.draft.replies.is_empty() {
            cx.propagate();
            return;
        }
        self.draft.replies.clear();
        cx.notify();
    }

    /// Up in an empty box (not already editing): edit the last message.
    /// Otherwise Up moves the cursor as usual.
    fn on_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing.editing().is_none() && self.input.read(cx).value().is_empty() {
            cx.stop_propagation();
            cx.emit(ComposerEvent::EditLast);
        }
    }

    /// "Editing message", with how to save or cancel.
    fn render_edit_bar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .px_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(Icon::new(IconName::Pencil).xsmall())
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child("Editing message"),
            )
            .child("Esc to cancel · Enter to save")
            .child(
                Button::new("edit-cancel")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Close)
                    .tooltip("Cancel (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_edit(window, cx);
                    })),
            )
    }

    /// "Replying to Name", one line per reply, with the mention toggle and ×.
    fn render_reply_bar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        v_flex()
            .gap_1()
            .children(self.draft.replies.iter().map(|reply| {
                let (toggle, remove) = (reply.id.clone(), reply.id.clone());
                h_flex()
                    .gap_2()
                    .px_2()
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::new(IconName::Reply).xsmall())
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child("Replying to")
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child(reply.author.clone()),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("reply-mention-{}", reply.id)))
                            .xsmall()
                            .map(|button| {
                                if reply.mention {
                                    button.primary()
                                } else {
                                    button.ghost()
                                }
                            })
                            .label(if reply.mention { "@ On" } else { "@ Off" })
                            .tooltip(if reply.mention {
                                "They'll be notified. Click to turn off."
                            } else {
                                "They won't be notified. Click to turn on."
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft.toggle_mention(&toggle);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("reply-remove-{}", reply.id)))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip("Don't reply to this (Esc for all)")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft.remove_reply(&remove);
                                cx.notify();
                            })),
                    )
            }))
    }
}

/// Where the cursor goes at the end of `text`.
fn end_of(text: &str) -> Position {
    let line = text.matches('\n').count();
    let column = text.rsplit('\n').next().unwrap_or_default().chars().count();
    Position::new(line as u32, column as u32)
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = char_count(&self.input.read(cx).value());
        let bar = if self.editing.editing().is_some() {
            Some(self.render_edit_bar(cx))
        } else {
            (!self.draft.replies.is_empty()).then(|| self.render_reply_bar(cx))
        };
        let hint = self.hint.clone();
        let theme = cx.theme();
        v_flex()
            .flex_none()
            .px_4()
            .pt_1()
            .pb_4()
            .gap_1()
            .on_action(cx.listener(Self::on_escape))
            // Before the textarea moves the cursor.
            .capture_action(cx.listener(Self::on_move_up))
            .children(bar)
            .child(Textarea::new(&self.input))
            .when_some(hint, |el, hint| {
                el.child(div().px_2().text_xs().text_color(theme.danger).child(hint))
            })
            .when_some(count, |el, (chars, over)| {
                el.child(
                    h_flex()
                        .justify_end()
                        .text_xs()
                        .text_color(if over {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(if over {
                            format!(
                                "{chars} / {MAX_MESSAGE_CHARS} · {} too many to send",
                                chars - MAX_MESSAGE_CHARS
                            )
                        } else {
                            format!("{chars} / {MAX_MESSAGE_CHARS}")
                        }),
                )
            })
    }
}
