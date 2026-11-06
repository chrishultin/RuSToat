//! Stoat markup that plain Markdown can't express, drawn inside `TextView`.
//!
//! [`crate::markdown::prepare`] rewrites it into links with internal
//! `stoat:` URLs, and [`MarkupPlugin`] claims those links and draws each as
//! a native element:
//!
//! - `<t:…>` timestamps, in local time (or relative, for `R`), with the
//!   full date and time on hover.
//!
//! Also the copy button on fenced code blocks ([`code_block_actions`]).

use std::time::Duration;

use chrono::Local;
use gpui_kit::assets::IconName;
use gpui_kit::base::text::CodeBlock;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::text::{
    InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
    markdown_ast,
};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::markdown::{InternalLink, parse_internal_link};
use crate::time::{format_timestamp, local_from_seconds, timestamp_tooltip};

const NAME: &str = "stoat-markup";

/// How strongly a timestamp's background shows the text colour behind it.
const PILL_TINT: f32 = 0.12;

/// What a claimed link stands for.
enum Markup {
    Time { seconds: i64, style: Option<char> },
}

/// Draws the `stoat:` links [`crate::markdown::prepare`] makes for markup
/// (see the module docs). Mentions are left to `TextView` as links.
pub struct MarkupPlugin;

impl MarkdownPlugin for MarkupPlugin {
    fn name(&self) -> &str {
        NAME
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else {
            return None;
        };
        match parse_internal_link(&link.url)? {
            InternalLink::Time { seconds, style } => {
                // Copied as typed, so a copy can be sent again.
                let source = match style {
                    Some(style) => format!("<t:{seconds}:{style}>"),
                    None => format!("<t:{seconds}>"),
                };
                Some(
                    MarkdownNode::new(NAME, Markup::Time { seconds, style })
                        .text(source.clone())
                        .markdown(source),
                )
            }
            InternalLink::User(_) | InternalLink::Channel(_) => None,
        }
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        _cx: &mut App,
    ) -> Option<InlineElement> {
        // Several of the same timestamp in one message each need their own id.
        let at = node.source_range().map_or(0, |range| range.start);
        match node.data::<Markup>()? {
            Markup::Time { seconds, style } => render_time(*seconds, *style, at, context, window),
        }
    }
}

/// A timestamp as a small pill in the surrounding text's font. Drawn
/// against the time now, so a relative one moves on when it's drawn again.
fn render_time(
    seconds: i64,
    style: Option<char>,
    at: usize,
    context: &InlineRenderContext,
    window: &mut Window,
) -> Option<InlineElement> {
    let when = local_from_seconds(seconds)?;
    let label = format_timestamp(&when, style, &Local::now());
    let tooltip: SharedString = timestamp_tooltip(&when).into();
    let base = context.text_style();
    let font = base.font();
    let (size, line_height) = (context.font_size(), context.line_height());
    let text_system = window.text_system();
    let baseline = text_system.baseline_offset(text_system.resolve_font(&font), size, line_height);
    let pill = div()
        .id(("stoat-time", at))
        .flex_none()
        .whitespace_nowrap()
        .px(size * 0.25)
        .rounded(size * 0.25)
        // A tint of the text colour reads on every surface, our own
        // accent-coloured bubbles included.
        .bg(base.color.opacity(PILL_TINT))
        .font(font)
        .text_size(size)
        .line_height(line_height)
        .text_color(base.color)
        .child(label)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
    Some(InlineElement::new(pill).with_baseline(baseline))
}

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
