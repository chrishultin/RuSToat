//! The message box under a conversation. Enter sends, Shift+Enter adds a
//! new line, and each channel keeps its own draft, replies included.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, InputEvent, Position, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::models::ReplyIntent;
use crate::draft::{
    Draft, Drafts, MAX_MESSAGE_CHARS, ReplyTarget, SendCheck, TooManyReplies, char_count,
    check_send,
};

pub enum ComposerEvent {
    /// Send this text (already checked and trimmed) in the composer's
    /// channel, replying to `replies`.
    Send {
        text: String,
        replies: Vec<ReplyIntent>,
    },
}

pub struct Composer {
    input: Entity<TextareaState>,
    /// The channel being typed in.
    channel: Option<String>,
    /// What the message replies to. (Its text lives in `input`.)
    draft: Draft,
    drafts: Drafts,
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
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        Self {
            input,
            channel: None,
            draft: Draft::default(),
            drafts: Drafts::default(),
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
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
            input.set_value(text.clone(), window, cx);
            // Carry on typing where the draft ends.
            input.set_cursor_position(end_of(&text), window, cx);
        });
        cx.notify();
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

    /// Esc in the message box: stop replying.
    fn on_escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if self.draft.replies.is_empty() {
            cx.propagate();
            return;
        }
        self.draft.replies.clear();
        cx.notify();
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
        let reply_bar = (!self.draft.replies.is_empty()).then(|| self.render_reply_bar(cx));
        let theme = cx.theme();
        v_flex()
            .flex_none()
            .px_4()
            .pt_1()
            .pb_4()
            .gap_1()
            .on_action(cx.listener(Self::on_escape))
            .children(reply_bar)
            .child(Textarea::new(&self.input))
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
