//! Prepared message Markdown → blocks and spans (`rich_text`).

use stoat_ffi::rich_text::{RichBlock, RichCache, RichSpan, SpanKind, blocks};

fn text(spans: &[RichSpan]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

fn paragraph(block: &RichBlock) -> &[RichSpan] {
    match block {
        RichBlock::Paragraph { spans } => spans,
        other => panic!("expected a paragraph, got {other:?}"),
    }
}

#[test]
fn plain_text_is_one_paragraph_span() {
    let blocks = blocks("hello there");
    assert_eq!(blocks.len(), 1);
    let spans = paragraph(&blocks[0]);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "hello there");
    assert_eq!(spans[0].kind, SpanKind::Text);
    assert!(!spans[0].bold && !spans[0].italic && !spans[0].code && spans[0].link.is_none());
}

#[test]
fn inline_styles_combine() {
    let blocks = blocks("a ***both*** ~~**gone**~~ `code` [docs](https://stoat.chat)");
    let spans = paragraph(&blocks[0]);
    let find = |t: &str| spans.iter().find(|s| s.text == t).unwrap_or_else(|| panic!("{t} in {spans:#?}"));
    let both = find("both");
    assert!(both.bold && both.italic);
    let gone = find("gone");
    assert!(gone.bold && gone.strike);
    assert!(find("code").code);
    assert_eq!(find("docs").link.as_deref(), Some("https://stoat.chat"));
    assert_eq!(text(spans), "a both gone code docs");
}

#[test]
fn neighbouring_spans_with_one_style_merge() {
    // The parser splits text at escapes; drawn, it's one run.
    let blocks = blocks(r"a \<b\> c");
    let spans = paragraph(&blocks[0]);
    assert_eq!(spans.len(), 1, "{spans:#?}");
    assert_eq!(spans[0].text, "a <b> c");
}

#[test]
fn hard_breaks_are_break_spans() {
    // `prepare` keeps every line break as two trailing spaces.
    let blocks = blocks("one  \ntwo");
    let spans = paragraph(&blocks[0]);
    assert_eq!(
        spans.iter().map(|s| (s.text.as_str(), s.kind.clone())).collect::<Vec<_>>(),
        [("one", SpanKind::Text), ("\n", SpanKind::Break), ("two", SpanKind::Text)]
    );
}

#[test]
fn headings_keep_their_level() {
    let blocks = blocks("# Big\n\n### Small");
    assert!(matches!(&blocks[0], RichBlock::Heading { level: 1, spans } if text(spans) == "Big"));
    assert!(matches!(&blocks[1], RichBlock::Heading { level: 3, spans } if text(spans) == "Small"));
}

#[test]
fn quotes_hold_blocks() {
    let blocks = blocks("> quoted\n> - with a list\n\nafter");
    let RichBlock::Quote { blocks: inner } = &blocks[0] else { panic!("{blocks:#?}") };
    assert_eq!(text(paragraph(&inner[0])), "quoted");
    assert!(matches!(&inner[1], RichBlock::List { .. }));
    assert_eq!(text(paragraph(&blocks[1])), "after");
}

#[test]
fn code_blocks_keep_their_text_and_language() {
    let blocks = blocks("```rust\nfn main() {\n    <b>\n}\n```");
    assert_eq!(
        blocks[0],
        RichBlock::Code { language: Some("rust".into()), code: "fn main() {\n    <b>\n}".into() }
    );
    let other = stoat_ffi::rich_text::blocks("```\nno language\n```");
    assert!(matches!(&other[0], RichBlock::Code { language: None, .. }));
}

#[test]
fn an_unterminated_fence_runs_to_the_end() {
    let blocks = blocks("```\nstill code");
    assert!(matches!(&blocks[0], RichBlock::Code { code, .. } if code == "still code"));
}

#[test]
fn lists_number_and_nest() {
    let blocks = blocks("3. three\n4. four\n   - nested\n\n- [x] done\n- [ ] todo");
    let RichBlock::List { ordered: true, start: 3, items } = &blocks[0] else { panic!("{blocks:#?}") };
    assert_eq!(items.len(), 2);
    assert!(matches!(&items[1].blocks[1], RichBlock::List { ordered: false, .. }));
    let RichBlock::List { ordered: false, items, .. } = &blocks[1] else { panic!("{blocks:#?}") };
    assert_eq!(items[0].checked, Some(true));
    assert_eq!(items[1].checked, Some(false));
}

#[test]
fn rules_maths_and_tables() {
    let blocks = blocks("above\n\n---\n\n$$\nx^2\n$$\n\n| a | b |\n|---|---|\n| 1 | **2** |");
    assert_eq!(blocks[1], RichBlock::Rule);
    assert_eq!(blocks[2], RichBlock::Math { source: "x^2".into() });
    let RichBlock::Table { header, rows } = &blocks[3] else { panic!("{blocks:#?}") };
    assert_eq!(header.iter().map(|c| text(&c.spans)).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].cells[1].spans[0].bold);
}

#[test]
fn inline_maths_is_code() {
    let blocks = blocks("area $\\pi r^2$ here");
    let spans = paragraph(&blocks[0]);
    assert!(spans.iter().any(|s| s.code && s.text == "\\pi r^2"), "{spans:#?}");
}

#[test]
fn reference_links_resolve() {
    let blocks = blocks("see [the docs][d]\n\n[d]: https://stoat.chat/docs");
    assert_eq!(blocks.len(), 1, "definitions draw nothing");
    let spans = paragraph(&blocks[0]);
    let docs = spans.iter().find(|s| s.text == "the docs").unwrap();
    assert_eq!(docs.link.as_deref(), Some("https://stoat.chat/docs"));
}

#[test]
fn custom_emoji_html_shows_its_name_for_now() {
    let blocks = blocks(r#"hi <img src="https://cdn/emojis/01E" alt=":party:" title=":party:" width="20" height="20">"#);
    assert_eq!(text(paragraph(&blocks[0])), "hi :party:");
}

#[test]
fn the_cache_parses_each_render_key_once() {
    let cache = RichCache::default();
    let a = cache.get("m1:1", "**one**");
    let again = cache.get("m1:1", "ignored: same key");
    assert!(std::sync::Arc::ptr_eq(&a, &again));
    let b = cache.get("m1:2", "two");
    assert_ne!(*a, *b);
    assert_eq!(cache.len(), 2);
}

#[test]
fn prepared_messages_round_trip() {
    // What `prepare` makes of a real message: escaped `<`, a mention link.
    let names = Names;
    let prepared = stoat_gpui::markdown::prepare("hey <@u-ada>, <not a tag> **now**", &names);
    let blocks = blocks(&prepared);
    let spans = paragraph(&blocks[0]);
    assert_eq!(text(spans), "hey @Ada, <not a tag> now");
    assert!(spans.iter().any(|s| s.link.as_deref() == Some("stoat:user/u-ada")));
}

struct Names;

impl stoat_gpui::markdown::Names for Names {
    fn user_name(&self, id: &str) -> Option<String> {
        (id == "u-ada").then(|| "Ada".into())
    }
    fn channel_name(&self, _: &str) -> Option<String> {
        None
    }
}
