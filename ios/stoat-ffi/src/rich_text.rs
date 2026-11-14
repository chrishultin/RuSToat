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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

/// `prepared` (Markdown from `markdown::prepare`) as blocks.
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
            self.inline(node, Style::default(), None, &mut out);
        }
        merge(out)
    }

    fn inline(&self, node: &Node, style: Style, link: Option<&str>, out: &mut Vec<RichSpan>) {
        let children = |style: Style, link: Option<&str>, out: &mut Vec<RichSpan>| {
            for child in node.children().into_iter().flatten() {
                self.inline(child, style, link, out);
            }
        };
        match node {
            Node::Text(text) => out.push(span(&text.value, style, link, SpanKind::Text)),
            Node::Break(_) => out.push(span("\n", style, link, SpanKind::Break)),
            Node::Strong(_) => children(Style { bold: true, ..style }, link, out),
            Node::Emphasis(_) => children(Style { italic: true, ..style }, link, out),
            Node::Delete(_) => children(Style { strike: true, ..style }, link, out),
            Node::InlineCode(code) => {
                out.push(span(&code.value, Style { code: true, ..style }, link, SpanKind::Text))
            }
            Node::InlineMath(math) => {
                out.push(span(&math.value, Style { code: true, ..style }, link, SpanKind::Text))
            }
            Node::Link(l) => children(style, Some(&l.url), out),
            Node::LinkReference(reference) => {
                match self.definitions.get(&label(&reference.identifier)) {
                    Some(url) => children(style, Some(url), out),
                    // Not a reference after all: as typed.
                    None => out.push(span(&node_source(node), style, link, SpanKind::Text)),
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
            Node::Html(html) => out.push(span(&html_text(&html.value), style, link, SpanKind::Text)),
            other => {
                if other.children().is_some() {
                    children(style, link, out);
                } else {
                    out.push(span(&other.to_string(), style, link, SpanKind::Text));
                }
            }
        }
    }
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

/// Join neighbouring text spans with the same style and link.
fn merge(spans: Vec<RichSpan>) -> Vec<RichSpan> {
    let mut out: Vec<RichSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        if let Some(last) = out.last_mut()
            && last.kind == SpanKind::Text
            && span.kind == SpanKind::Text
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
    pub fn get(&self, render_key: &str, prepared: &str) -> Arc<Vec<RichBlock>> {
        let mut cache = self.blocks.lock().unwrap();
        if let Some(blocks) = cache.get(render_key) {
            return blocks.clone();
        }
        if cache.len() >= CACHE_MAX {
            cache.clear();
        }
        let parsed = Arc::new(blocks(prepared));
        cache.insert(render_key.to_string(), parsed.clone());
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
