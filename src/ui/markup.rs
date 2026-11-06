//! Stoat markup that plain Markdown can't express, drawn inside `TextView`.
//!
//! For now, the copy button on fenced code blocks ([`code_block_actions`]).

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::text::CodeBlock;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// How long the copy button shows a check mark after a click.
const COPIED_FOR: Duration = Duration::from_secs(2);

/// The corner of a fenced code block: its language (when the fence names
/// one) and a button that copies the block's code. Hand it to
/// `TextView::code_block_actions`, which then leaves the language label to us.
pub fn code_block_actions(block: &CodeBlock, window: &mut Window, cx: &mut App) -> AnyElement {
    // Keyed by where the block starts, inside the `TextView`'s own id, so
    // each block of each message has its own "copied" state.
    let key = block.span.map_or(0, |span| span.start);
    let copied = window.use_keyed_state(("code-copied", key), cx, |_, _| false);
    let code = block.code();
    let done = *copied.read(cx);
    h_flex()
        .id("code-actions")
        .gap_1()
        .items_center()
        // A press here mustn't start a text selection in the block.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .when_some(block.lang().filter(|lang| !lang.is_empty()), |el, lang| {
            el.child(
                div()
                    .pl_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(lang),
            )
        })
        .child(
            Button::new("copy-code")
                .ghost()
                .xsmall()
                .icon(if done {
                    IconName::Check
                } else {
                    IconName::Copy
                })
                .tooltip(if done { "Copied" } else { "Copy code" })
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                    copied.update(cx, |copied, cx| {
                        *copied = true;
                        cx.notify();
                    });
                    let copied = copied.downgrade();
                    cx.spawn(async move |cx| {
                        cx.background_executor().timer(COPIED_FOR).await;
                        copied
                            .update(cx, |copied, cx| {
                                *copied = false;
                                cx.notify();
                            })
                            .ok();
                    })
                    .detach();
                }),
        )
        .into_any_element()
}
