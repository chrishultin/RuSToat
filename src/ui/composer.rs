//! The message box under a conversation. Enter sends, Shift+Enter adds a
//! new line, and each channel keeps its own draft.

use gpui_kit::component::input::{InputEvent, Position, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::draft::{Drafts, MAX_MESSAGE_CHARS, SendCheck, char_count, check_send};

pub enum ComposerEvent {
    /// Send this text (already checked and trimmed) in the composer's channel.
    Send(String),
}

pub struct Composer {
    input: Entity<TextareaState>,
    /// The channel being typed in.
    channel: Option<String>,
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
            drafts: Drafts::default(),
            _subscriptions: subscriptions,
        }
    }

    pub fn channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    /// Type in `channel` from now on: the current text is kept as the old
    /// channel's draft, and `channel`'s own draft comes back.
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
        let text = self.input.read(cx).value();
        if let Some(old) = &self.channel {
            self.drafts.save(old, &text);
        }
        let draft = channel.map(|c| self.drafts.take(c)).unwrap_or_default();
        self.channel = channel.map(str::to_string);
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
            input.set_value(draft.clone(), window, cx);
            // Carry on typing where the draft ends.
            input.set_cursor_position(end_of(&draft), window, cx);
        });
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.channel.is_none() {
            return;
        }
        let text = self.input.read(cx).value();
        // Blank and too-long messages stay in the box (the count says why).
        if let SendCheck::Ready(text) = check_send(&text) {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
            cx.emit(ComposerEvent::Send(text));
            cx.notify();
        }
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
        let theme = cx.theme();
        let count = char_count(&self.input.read(cx).value());
        v_flex()
            .flex_none()
            .px_4()
            .pt_1()
            .pb_4()
            .gap_1()
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
