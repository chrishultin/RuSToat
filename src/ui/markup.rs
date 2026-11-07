//! Stoat markup that plain Markdown can't express, drawn inside `TextView`.
//!
//! [`crate::markdown::prepare`] rewrites it into links with internal
//! `stoat:` URLs, and [`MarkupPlugin`] claims those links and draws each as
//! a native element:
//!
//! - `<t:…>` timestamps, in local time (or relative, for `R`), with the
//!   full date and time on hover.
//! - Hidden spoilers: each word a bar, revealed (for the session) by a click.
//! - Role mentions in the role's colour (with a dot of it), and `@everyone`
//!   and `@online` in the theme's primary colour.
//! - Channel mentions, and links to a channel or message on this instance's
//!   web client, as a solid pill like the web client's: the channel's icon
//!   and name, then `› 💬` for a message ("Private Channel" for one this
//!   account can't see). Only with [`MarkupPlugin::with_channels`].
//!
//! Also the copy button on fenced code blocks ([`code_block_actions`]).

use std::sync::Arc;
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
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::invites::InviteHosts;
use crate::links::{ChannelLink, channel_from_link};
use crate::markdown::{InternalLink, parse_internal_link};
use crate::time::{format_timestamp, local_from_seconds, timestamp_tooltip};
use crate::ui::chat::hex_colour;

const NAME: &str = "stoat-markup";

/// How strongly a timestamp's background shows the text colour behind it.
const PILL_TINT: f32 = 0.12;

/// A hidden spoiler's line height, in ems: a little under the text's, so
/// the bars on two lines don't touch.
const SPOILER_LINE_HEIGHT: f32 = 1.3;

/// The colour dot in a role mention, as in the member list.
const ROLE_DOT: Pixels = px(10.);

/// Who a mention drawn here is of.
#[derive(Clone, Copy)]
enum Mention {
    /// A role, with its colour if it has a solid one.
    Role(Option<Hsla>),
    /// `@everyone` or `@online`.
    Everyone,
}

/// What a claimed link stands for.
enum Markup {
    Mention {
        label: String,
        kind: Mention,
    },
    Time {
        seconds: i64,
        style: Option<char>,
    },
    /// One word (with the space after it) of a hidden spoiler.
    Spoiler {
        index: usize,
        first: bool,
        last: bool,
        text: String,
    },
    /// A channel mention, or a link to a channel or message.
    Channel(ChannelLink),
}

/// What a channel pill shows: the channel's name and icon.
pub struct ChannelPill {
    pub name: String,
    pub icon: IconName,
}

/// Channel `id`'s pill, looked up as it's drawn (so a rename shows), or
/// `None` if this account can't see it.
pub type LookupChannel = Arc<dyn Fn(&str, &App) -> Option<ChannelPill> + Send + Sync>;
/// Opens a channel (at its message) when its pill is clicked.
pub type OpenChannel = Arc<dyn Fn(&ChannelLink, &mut Window, &mut App) + Send + Sync>;

/// How [`MarkupPlugin`] draws channel mentions and links as pills.
#[derive(Clone)]
pub struct ChannelPills {
    /// Which links lead into this instance's web client.
    pub hosts: InviteHosts,
    pub lookup: LookupChannel,
    /// `None` where a click does something else (search results jump).
    pub open: Option<OpenChannel>,
}

/// Reveals spoiler `n` of the message being drawn.
pub type RevealSpoiler = Arc<dyn Fn(usize, &mut Window, &mut App) + Send + Sync>;

/// Draws the `stoat:` links [`crate::markdown::prepare`] makes for markup
/// (see the module docs). Mentions are left to `TextView` as links.
#[derive(Default)]
pub struct MarkupPlugin {
    reveal: Option<RevealSpoiler>,
    channels: Option<ChannelPills>,
}

impl MarkupPlugin {
    /// Hidden spoilers are revealed through `reveal` when clicked. Without
    /// it (bios, embeds) they stay hidden.
    pub fn revealing(reveal: RevealSpoiler) -> Self {
        Self {
            reveal: Some(reveal),
            channels: None,
        }
    }

    /// Channel mentions and links drawn as pills. Without this they're
    /// ordinary links.
    pub fn with_channels(mut self, channels: ChannelPills) -> Self {
        self.channels = Some(channels);
        self
    }

    /// Where `url` leads, if it's to be drawn as a channel pill.
    fn channel_link(&self, url: &str) -> Option<ChannelLink> {
        channel_from_link(url, &self.channels.as_ref()?.hosts)
    }
}

/// A channel pill's node, copied as `copied` (what was sent, so a copy can
/// be sent again).
fn channel_node(link: ChannelLink, copied: String) -> MarkdownNode {
    MarkdownNode::new(NAME, Markup::Channel(link))
        .text(copied.clone())
        .markdown(copied)
}

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
        let Some(internal) = parse_internal_link(&link.url) else {
            // A plain web link.
            let channel = self.channel_link(&link.url)?;
            return Some(channel_node(channel, link.url.clone()));
        };
        match internal {
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
            InternalLink::Spoiler { index, word, words } => {
                let text = node.to_string();
                // Copying a hidden spoiler doesn't give it away either.
                let copied = if word == 0 { "[spoiler]" } else { " " };
                Some(
                    MarkdownNode::new(
                        NAME,
                        Markup::Spoiler {
                            index,
                            first: word == 0,
                            last: word + 1 == words,
                            text,
                        },
                    )
                    .text(copied)
                    .markdown(copied),
                )
            }
            InternalLink::Role { colour, .. } => Some(mention(
                node.to_string(),
                Mention::Role(colour.as_deref().and_then(hex_colour)),
            )),
            InternalLink::Everyone | InternalLink::Online => {
                Some(mention(node.to_string(), Mention::Everyone))
            }
            InternalLink::Channel(channel) if self.channels.is_some() => {
                let copied = format!("<#{channel}>");
                let link = ChannelLink {
                    channel,
                    message: None,
                };
                Some(channel_node(link, copied))
            }
            // Its text isn't its address, but the pill says where it leads.
            InternalLink::External { url, .. } => {
                let link = self.channel_link(&url)?;
                Some(channel_node(link, url))
            }
            // Drawn as ordinary links.
            InternalLink::User(_) | InternalLink::Channel(_) => None,
        }
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        // Several of the same timestamp in one message each need their own id.
        let at = node.source_range().map_or(0, |range| range.start);
        match node.data::<Markup>()? {
            Markup::Time { seconds, style } => render_time(*seconds, *style, at, context, window),
            Markup::Mention { label, kind } => {
                Some(render_mention(label, *kind, context, window, cx))
            }
            Markup::Spoiler {
                index,
                first,
                last,
                text,
            } => Some(render_spoiler_word(
                text,
                (*first, *last),
                at,
                self.reveal.clone().map(|reveal| (*index, reveal)),
                context,
                window,
                cx,
            )),
            Markup::Channel(link) => {
                let channels = self.channels.as_ref()?;
                Some(render_channel(link, channels, at, context, window, cx))
            }
        }
    }
}

/// A mention node, copied as its label (`@Name`), as user mentions are.
fn mention(label: String, kind: Mention) -> MarkdownNode {
    MarkdownNode::new(
        NAME,
        Markup::Mention {
            label: label.clone(),
            kind,
        },
    )
    .text(label.clone())
    .markdown(label)
}

/// A role, `@everyone` or `@online` mention as a pill in the surrounding
/// text's font. A role has a dot of its colour; one without a colour is
/// muted.
fn render_mention(
    label: &str,
    kind: Mention,
    context: &InlineRenderContext,
    window: &mut Window,
    cx: &App,
) -> InlineElement {
    let colour = match kind {
        Mention::Role(colour) => colour.unwrap_or(cx.theme().muted_foreground),
        Mention::Everyone => cx.theme().primary,
    };
    let font = context.text_style().font();
    let (size, line_height) = (context.font_size(), context.line_height());
    let text_system = window.text_system();
    let baseline = text_system.baseline_offset(text_system.resolve_font(&font), size, line_height);
    let pill = h_flex()
        .flex_none()
        .whitespace_nowrap()
        .gap(size * 0.25)
        .px(size * 0.3)
        .rounded(size * 0.25)
        .bg(colour.opacity(PILL_TINT))
        .font(font)
        .text_size(size)
        .line_height(line_height)
        .font_weight(FontWeight::MEDIUM)
        .text_color(colour)
        .when(matches!(kind, Mention::Role(_)), |el| {
            el.child(div().flex_none().size(ROLE_DOT).rounded_full().bg(colour))
        })
        .child(label.to_string());
    InlineElement::new(pill).with_baseline(baseline)
}

/// A channel or message link as a solid pill in the theme's primary colour
/// (which reads on both bubbles: the user's own are only tinted): the
/// channel's icon and name, then `›` and a speech bubble for a message.
/// Clicking it opens the channel at the message. One this account can't see
/// says "Private Channel", and does nothing.
fn render_channel(
    link: &ChannelLink,
    channels: &ChannelPills,
    at: usize,
    context: &InlineRenderContext,
    window: &mut Window,
    cx: &App,
) -> InlineElement {
    let theme = cx.theme();
    let (bg, fg) = (theme.primary, theme.primary_foreground);
    let pill = (channels.lookup)(&link.channel, cx);
    let open = channels.open.clone().filter(|_| pill.is_some());
    let (channel_icon, name) = match pill {
        Some(pill) => (pill.icon, pill.name),
        None => (IconName::Hash, "Private Channel".to_string()),
    };
    let font = context.text_style().font();
    let (size, line_height) = (context.font_size(), context.line_height());
    let text_system = window.text_system();
    let baseline = text_system.baseline_offset(text_system.resolve_font(&font), size, line_height);
    let icon = |name: IconName| Icon::new(name).size(size).text_color(fg);
    let pill = h_flex()
        .id(("stoat-channel", at))
        .flex_none()
        .whitespace_nowrap()
        .items_center()
        .gap(size * 0.25)
        .pl(size * 0.15)
        .pr(size * 0.4)
        .rounded(size * 0.5)
        .bg(bg)
        .font(font)
        .text_size(size)
        .line_height(line_height)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(icon(channel_icon))
        .child(name)
        .when(link.message.is_some(), |el| {
            el.child(icon(IconName::ChevronRight))
                .child(icon(IconName::MessageCircle))
        })
        .when_some(open, |el, open| {
            let link = link.clone();
            el.cursor_pointer()
                // A press here mustn't start a text selection.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| open(&link, window, cx))
        });
    InlineElement::new(pill).with_baseline(baseline)
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

/// One word of a hidden spoiler: a bar the size of the word, rounded at
/// the spoiler's two ends. Clicking it reveals the whole spoiler, when
/// `reveal` (the spoiler's index and how) is given.
fn render_spoiler_word(
    text: &str,
    (first, last): (bool, bool),
    at: usize,
    reveal: Option<(usize, RevealSpoiler)>,
    context: &InlineRenderContext,
    window: &mut Window,
    cx: &App,
) -> InlineElement {
    let font = context.text_style().font();
    let size = context.font_size();
    let line_height = size * SPOILER_LINE_HEIGHT;
    let text_system = window.text_system();
    let baseline = text_system.baseline_offset(text_system.resolve_font(&font), size, line_height);
    let radius = size * 0.25;
    let bar = div()
        .id(("stoat-spoiler", at))
        .flex_none()
        .whitespace_nowrap()
        .font(font)
        .text_size(size)
        .line_height(line_height)
        // The word is there to size the bar, not to be read.
        .text_color(transparent_black())
        .bg(cx.theme().muted_foreground)
        .when(first, |el| el.rounded_l(radius))
        .when(last, |el| el.rounded_r(radius))
        .child(text.to_string())
        .when_some(reveal, |el, (index, reveal)| {
            el.cursor_pointer()
                // A press here mustn't start a text selection.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| reveal(index, window, cx))
        });
    InlineElement::new(bar).with_baseline(baseline)
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
