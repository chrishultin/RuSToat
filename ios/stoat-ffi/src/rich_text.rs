//! A message's text as blocks and styled spans, for SwiftUI to draw.
//!
//! The input is the Markdown [`stoat_gpui::markdown::prepare`] writes for
//! desktop's `TextView`. It's parsed here with the same parser and options
//! desktop uses (`markdown::to_mdast`, GFM plus math: see
//! `src/markdown.rs`), so both apps split a message into the same
//! paragraphs, quotes, lists and code blocks. Blocks nest; the text inside a
//! block is a flat run of spans, each with its own style and link, which
//! maps straight onto an `AttributedString`.
//!
//! Anything the tree has that isn't drawn specially falls back to its text,
//! never to nothing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use markdown::mdast::Node;
use stoat_gpui::markdown::{InternalLink, has_relative_time, parse_internal_link};

/// A block of a message.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RichBlock {
    Paragraph {
        spans: Vec<RichSpan>,
    },
    /// `level` 1 to 6.
    Heading {
        level: u8,
        spans: Vec<RichSpan>,
    },
    Quote {
        blocks: Vec<RichBlock>,
    },
    /// A fenced or indented code block.
    Code {
        language: Option<String>,
        code: String,
    },
    List {
        ordered: bool,
        /// The first item's number, for ordered lists.
        start: u32,
        items: Vec<RichListItem>,
    },
    /// A thematic break (`---`).
    Rule,
    /// A `$$…$$` block: shown as its TeX source for now.
    Math {
        source: String,
    },
    Table {
        header: Vec<RichCell>,
        rows: Vec<RichRow>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RichListItem {
    /// A task list item's box: `Some(true)` ticked.
    pub checked: Option<bool>,
    pub blocks: Vec<RichBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RichCell {
    pub spans: Vec<RichSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RichRow {
    pub cells: Vec<RichCell>,
}

/// A run of text with one style.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RichSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    /// Inline code (or inline maths): monospaced.
    pub code: bool,
    /// Where it links to, if anywhere.
    pub link: Option<String>,
    pub kind: SpanKind,
}

/// What a span is, beyond styled text.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SpanKind {
    Text,
    /// A line break inside the block (`text` is "\n").
    Break,
    /// A mention, drawn as a pill. `colour` (`#rrggbb`) for a role with a
    /// solid one.
    Mention {
        kind: MentionKind,
        colour: Option<String>,
    },
    /// A `<t:…>` timestamp, its `text` in local time (as desktop shows
    /// it). `full` is the whole date and time, for accessibility.
    Time { full: String },
    /// A word of a hidden spoiler, drawn as a bar. Tapping it (its link is
    /// `stoat:spoiler/…`) reveals spoiler `index` of the message.
    Spoiler { index: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MentionKind {
    User,
    Role,
    Channel,
    Everyone,
    Online,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

/// `prepared` (Markdown from `markdown::prepare`, whose links are already
/// guarded: see `markdown::guard_links`) as blocks.
pub fn blocks(prepared: &str) -> Vec<RichBlock> {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    let Ok(tree) = markdown::to_mdast(prepared, &options) else {
        // The parser only fails on MDX, which isn't enabled; just in case.
        return vec![RichBlock::Paragraph {
            spans: vec![span(prepared, Style::default(), None, SpanKind::Text)],
        }];
    };
    let mut definitions = HashMap::new();
    collect_definitions(&tree, &mut definitions);
    let parser = Parser { definitions };
    parser.blocks(tree.children().map_or(&[][..], Vec::as_slice))
}

/// Link reference definitions (`[label]: url`), by normalised label.
fn collect_definitions(node: &Node, out: &mut HashMap<String, String>) {
    if let Node::Definition(definition) = node {
        out.entry(label(&definition.identifier))
            .or_insert_with(|| definition.url.clone());
    }
    for child in node.children().into_iter().flatten() {
        collect_definitions(child, out);
    }
}

/// A reference label as references match it: case and runs of whitespace
/// don't matter (as `src/markdown.rs` matches them).
fn label(identifier: &str) -> String {
    identifier
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn span(text: &str, style: Style, link: Option<&str>, kind: SpanKind) -> RichSpan {
    RichSpan {
        text: text.to_string(),
        bold: style.bold,
        italic: style.italic,
        strike: style.strike,
        code: style.code,
        link: link.map(str::to_string),
        kind,
    }
}

struct Parser {
    definitions: HashMap<String, String>,
}

impl Parser {
    fn blocks(&self, nodes: &[Node]) -> Vec<RichBlock> {
        nodes.iter().filter_map(|node| self.block(node)).collect()
    }

    fn block(&self, node: &Node) -> Option<RichBlock> {
        Some(match node {
            Node::Paragraph(p) => RichBlock::Paragraph {
                spans: self.spans(&p.children),
            },
            Node::Heading(h) => RichBlock::Heading {
                level: h.depth,
                spans: self.spans(&h.children),
            },
            Node::Blockquote(q) => RichBlock::Quote {
                blocks: self.blocks(&q.children),
            },
            Node::Code(code) => RichBlock::Code {
                language: code.lang.clone().filter(|l| !l.is_empty()),
                code: code.value.clone(),
            },
            Node::Math(math) => RichBlock::Math {
                source: math.value.clone(),
            },
            Node::List(list) => RichBlock::List {
                ordered: list.ordered,
                start: list.start.unwrap_or(1),
                items: list
                    .children
                    .iter()
                    .map(|item| match item {
                        Node::ListItem(item) => RichListItem {
                            checked: item.checked,
                            blocks: self.blocks(&item.children),
                        },
                        other => RichListItem {
                            checked: None,
                            blocks: self.block(other).into_iter().collect(),
                        },
                    })
                    .collect(),
            },
            Node::ThematicBreak(_) => RichBlock::Rule,
            Node::Table(table) => {
                let mut rows = table.children.iter().map(|row| RichRow {
                    cells: row
                        .children()
                        .into_iter()
                        .flatten()
                        .map(|cell| RichCell {
                            spans: self.spans(cell.children().map_or(&[][..], Vec::as_slice)),
                        })
                        .collect(),
                });
                let header = rows.next().map(|row| row.cells).unwrap_or_default();
                RichBlock::Table {
                    header,
                    rows: rows.collect(),
                }
            }
            // Raw HTML is never rendered, as in Stoat: show it as typed.
            Node::Html(html) => RichBlock::Paragraph {
                spans: vec![span(&html.value, Style::default(), None, SpanKind::Text)],
            },
            Node::Definition(_) => return None,
            // Footnote definitions and anything else: their text.
            other => {
                let text = other.to_string();
                if text.trim().is_empty() {
                    return None;
                }
                RichBlock::Paragraph {
                    spans: vec![span(&text, Style::default(), None, SpanKind::Text)],
                }
            }
        })
    }

    fn spans(&self, nodes: &[Node]) -> Vec<RichSpan> {
        let mut out = Vec::new();
        for node in nodes {
            self.inline(node, Style::default(), None, &SpanKind::Text, &mut out);
        }
        merge(out)
    }

    fn inline(
        &self,
        node: &Node,
        style: Style,
        link: Option<&str>,
        kind: &SpanKind,
        out: &mut Vec<RichSpan>,
    ) {
        let children = |style: Style, link: Option<&str>, kind: &SpanKind, out: &mut Vec<RichSpan>| {
            for child in node.children().into_iter().flatten() {
                self.inline(child, style, link, kind, out);
            }
        };
        match node {
            Node::Text(text) => out.push(span(&text.value, style, link, kind.clone())),
            Node::Break(_) => out.push(span("\n", style, link, SpanKind::Break)),
            Node::Strong(_) => children(Style { bold: true, ..style }, link, kind, out),
            Node::Emphasis(_) => children(Style { italic: true, ..style }, link, kind, out),
            Node::Delete(_) => children(Style { strike: true, ..style }, link, kind, out),
            Node::InlineCode(code) => out.push(span(
                &code.value,
                Style { code: true, ..style },
                link,
                kind.clone(),
            )),
            Node::InlineMath(math) => out.push(span(
                &math.value,
                Style { code: true, ..style },
                link,
                kind.clone(),
            )),
            Node::Link(l) => {
                let url = l.url.as_str();
                let mention = |kind, colour| SpanKind::Mention { kind, colour };
                match parse_internal_link(url) {
                    Some(InternalLink::User(_)) => {
                        children(style, Some(url), &mention(MentionKind::User, None), out)
                    }
                    Some(InternalLink::Channel(_)) => {
                        children(style, Some(url), &mention(MentionKind::Channel, None), out)
                    }
                    Some(InternalLink::Role { colour, .. }) => children(
                        style,
                        Some(url),
                        &mention(MentionKind::Role, colour),
                        out,
                    ),
                    Some(InternalLink::Everyone) => {
                        children(style, Some(url), &mention(MentionKind::Everyone, None), out)
                    }
                    Some(InternalLink::Online) => {
                        children(style, Some(url), &mention(MentionKind::Online, None), out)
                    }
                    Some(InternalLink::Time {
                        seconds,
                        style: time_style,
                    }) => match timestamp(seconds, time_style) {
                        Some((text, full)) => {
                            out.push(span(&text, style, Some(url), SpanKind::Time { full }))
                        }
                        None => children(style, link, kind, out),
                    },
                    Some(InternalLink::Spoiler { index, .. }) => children(
                        style,
                        Some(url),
                        &SpanKind::Spoiler {
                            index: u32::try_from(index).unwrap_or(u32::MAX),
                        },
                        out,
                    ),
                    // `stoat:link?…` (a link whose text isn't its address)
                    // and ordinary links: Swift opens them, warning first
                    // for the former.
                    Some(InternalLink::External { .. }) | None => {
                        children(style, Some(url), kind, out)
                    }
                }
            }
            Node::LinkReference(reference) => {
                match self.definitions.get(&label(&reference.identifier)) {
                    Some(url) => children(style, Some(url), kind, out),
                    // Not a reference after all: as typed.
                    None => out.push(span(&node_source(node), style, link, kind.clone())),
                }
            }
            // Images aren't embedded in messages: a link with their alt text.
            Node::Image(image) => {
                let text = if image.alt.is_empty() { &image.url } else { &image.alt };
                out.push(span(text, style, Some(&image.url), SpanKind::Text));
            }
            Node::ImageReference(reference) => {
                let url = self.definitions.get(&label(&reference.identifier));
                out.push(span(&reference.alt, style, url.map(String::as_str), SpanKind::Text));
            }
            Node::Html(html) => {
                out.push(span(&html_text(&html.value), style, link, kind.clone()))
            }
            other => {
                if other.children().is_some() {
                    children(style, link, kind, out);
                } else {
                    out.push(span(&other.to_string(), style, link, kind.clone()));
                }
            }
        }
    }
}

/// A `<t:…>` timestamp's text in local time, as desktop shows it, and its
/// full date and time.
fn timestamp(seconds: i64, style: Option<char>) -> Option<(String, String)> {
    let when = stoat_gpui::time::local_from_seconds(seconds)?;
    let now = chrono::Local::now();
    Some((
        stoat_gpui::time::format_timestamp(&when, style, &now),
        stoat_gpui::time::timestamp_tooltip(&when),
    ))
}

/// A link reference that didn't resolve, as typed: `[text]`.
fn node_source(node: &Node) -> String {
    format!("[{}]", node.to_string())
}

/// Inline HTML as it's shown. A custom emoji's `<img>` (the only HTML
/// `prepare` writes) shows as its `:name:`; anything else, as typed.
fn html_text(html: &str) -> String {
    if html.starts_with("<img ")
        && let Some(alt) = attribute(html, "alt")
    {
        return alt;
    }
    html.to_string()
}

/// The value of `name="…"` in an HTML tag `prepare` wrote (its values are
/// escaped with `&quot;`, `&amp;`, `&lt;`).
pub(crate) fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = start + tag[start..].find('"')?;
    Some(
        tag[start..end]
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

/// Join neighbouring spans with the same kind, style and link (a mention's
/// words, say). Breaks stay on their own.
fn merge(spans: Vec<RichSpan>) -> Vec<RichSpan> {
    let mut out: Vec<RichSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(last) = out.last_mut()
            && last.kind != SpanKind::Break
            && last.kind == span.kind
            && (last.bold, last.italic, last.strike, last.code, &last.link)
                == (span.bold, span.italic, span.strike, span.code, &span.link)
        {
            last.text.push_str(&span.text);
            continue;
        }
        out.push(span);
    }
    out
}

/// Parsed blocks by the text's `render_key`, so a timeline that's read
/// again and again only parses each message once (until it changes).
#[derive(Default)]
pub struct RichCache {
    blocks: Mutex<HashMap<String, Arc<Vec<RichBlock>>>>,
}

/// Beyond this many entries the cache starts again (a long session reading
/// many channels; parsing is cheap enough to redo).
const CACHE_MAX: usize = 4000;

impl RichCache {
    /// `prepared`'s blocks, parsed once per `render_key`. Text with a
    /// relative timestamp ("in 3 hours") is parsed again each minute.
    pub fn get(&self, render_key: &str, prepared: &str) -> Arc<Vec<RichBlock>> {
        let key = if has_relative_time(prepared) {
            let minute = chrono::Utc::now().timestamp() / 60;
            format!("{render_key}@{minute}")
        } else {
            render_key.to_string()
        };
        let mut cache = self.blocks.lock().unwrap();
        if let Some(blocks) = cache.get(&key) {
            return blocks.clone();
        }
        if cache.len() >= CACHE_MAX {
            cache.clear();
        }
        let parsed = Arc::new(blocks(prepared));
        cache.insert(key, parsed.clone());
        parsed
    }

    pub fn len(&self) -> usize {
        self.blocks.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `markdown` (as `prepare` writes it) parsed into blocks, for tests and
/// previews on the Swift side.
#[uniffi::export]
pub fn rich_text(markdown: String) -> Vec<RichBlock> {
    blocks(&markdown)
}
