//! Stoat content → Markdown: mentions, spoilers, HTML escaping, code safety,
//! line breaks, quotes and `$$…$$` math.

use stoat_gpui::markdown::{Names, is_external_link, is_jumbo, prepare};

struct Fixture;

impl Names for Fixture {
    fn user_name(&self, id: &str) -> Option<String> {
        match id {
            "u-ada" => Some("Ada L.".into()),
            "u-evil" => Some("*](javascript:x)[*".into()),
            _ => None,
        }
    }
    fn channel_name(&self, id: &str) -> Option<String> {
        (id == "ch-general").then(|| "general".into())
    }
    fn emoji_name(&self, id: &str) -> Option<String> {
        (id == STOAT).then(|| "stoat".into())
    }
    fn emoji_url(&self, id: &str) -> Option<String> {
        Some(format!("https://cdn.example/emojis/{id}"))
    }
}

/// A known server emoji, and one from a server the user isn't in.
const STOAT: &str = "01ARYZ6S41TSV4RRFFQ69G5FAV";
const OTHER: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

fn img(id: &str, name: &str, px: u32) -> String {
    format!(
        r#"<img src="https://cdn.example/emojis/{id}" alt=":{name}:" title=":{name}:" width="{px}" height="{px}">"#
    )
}

fn md(content: &str) -> String {
    prepare(content, &Fixture)
}

#[test]
fn mentions_become_internal_links_with_escaped_names() {
    assert_eq!(
        md("hi <@u-ada>, see <#ch-general>"),
        r"hi [@Ada L\.](stoat:user/u-ada), see [#general](stoat:channel/ch-general)"
    );
}

#[test]
fn hostile_names_cannot_break_out_of_the_link() {
    assert_eq!(
        md("<@u-evil>"),
        r"[@\*\]\(javascript\:x\)\[\*](stoat:user/u-evil)"
    );
}

#[test]
fn unknown_mentions_and_other_angle_brackets_stay_literal() {
    // Unknown IDs keep their raw form, but `<` is escaped so it isn't HTML.
    assert_eq!(md("<@u-nobody>"), r"\<@u-nobody>");
    assert_eq!(md("<b>bold?</b>"), r"\<b>bold?\</b>");
    assert_eq!(md("a <not a mention>"), r"a \<not a mention>");
    assert_eq!(md("1 < 2"), r"1 \< 2");
}

#[test]
fn autolinks_are_kept() {
    assert_eq!(
        md("go to <https://stoat.chat>"),
        "go to <https://stoat.chat>"
    );
    assert_eq!(md("<mailto:me@example.com>"), "<mailto:me@example.com>");
}

#[test]
fn ordinary_markdown_passes_through() {
    let text = "**bold** _it_ [link](https://example.com)\n\n> quote\n\n- item";
    assert_eq!(md(text), text);
}

#[test]
fn nothing_is_rewritten_inside_code() {
    assert_eq!(md("`<@u-ada>`"), "`<@u-ada>`");
    assert_eq!(md("```\n<@u-ada> <b>\n```"), "```\n<@u-ada> <b>\n```");
    assert_eq!(md("``a ` <b>``"), "``a ` <b>``");
}

#[test]
fn spoilers_become_code_spans_with_plain_mentions() {
    assert_eq!(md("the end: ||he wins||"), "the end: `he wins`");
    assert_eq!(md("||<@u-ada> did it||"), "`@Ada L. did it`");
    // A spoiler wrapped around inline code is left as typed; the code still renders.
    assert_eq!(md("||has `ticks`||"), "||has `ticks`||");
    // Unclosed or empty markers are left alone.
    assert_eq!(md("a || b"), "a || b");
    assert_eq!(md("||||"), "||||");
}

#[test]
fn only_web_and_mail_links_open_externally() {
    assert!(is_external_link("https://stoat.chat"));
    assert!(is_external_link("HTTP://example.com"));
    assert!(is_external_link("mailto:a@b.c"));
    assert!(!is_external_link("stoat:user/u-ada"));
    assert!(!is_external_link("javascript:alert(1)"));
    assert!(!is_external_link("file:///etc/passwd"));
}

#[test]
fn multibyte_text_never_panics_and_survives_intact() {
    // The crash: a text run starting with a multi-byte character.
    assert_eq!(md("⚠ heads up"), "⚠ heads up");
    assert_eq!(md("⚠<@u-ada>⚠"), r"⚠[@Ada L\.](stoat:user/u-ada)⚠");
    assert_eq!(md("日本語 || 🦦 ||"), "日本語 ` 🦦 `");

    // Every substring (on char boundaries) of a string mixing multi-byte
    // characters with every piece of syntax `prepare` looks at.
    let tricky = "⚠<@u-ada>é||🦦 spoil||`co😀de`<#ch-general>:01ARYZ6S41TSV4RRFFQ69G5FAV:\n```\n日本\n```\n<b>ü</b> <https://x.y/ö> |\r\n> 引用 $$\\color{red}ü<é$$ ö\\\n🦦 $ü$$";
    let boundaries: Vec<usize> = tricky
        .char_indices()
        .map(|(i, _)| i)
        .chain([tricky.len()])
        .collect();
    for &start in &boundaries {
        for &end in boundaries.iter().filter(|&&e| e >= start) {
            let _ = md(&tricky[start..end]);
        }
    }
}

#[test]
fn custom_emoji_become_sized_images() {
    assert_eq!(
        md(&format!("nice :{STOAT}: work")),
        format!("nice {} work", img(STOAT, "stoat", 20))
    );
    // Not in the user's servers: still drawn from the CDN, with a generic name.
    assert_eq!(
        md(&format!(":{OTHER}:!")),
        format!("{}!", img(OTHER, "emoji", 20))
    );
}

#[test]
fn colons_that_are_not_custom_emoji_are_left_alone() {
    assert_eq!(md(":smile: at 12:30:45"), ":smile: at 12:30:45");
    assert_eq!(md("note: see below"), "note: see below");
    // Almost an ID (25 chars, or a non-Crockford letter).
    assert_eq!(
        md(":01ARYZ6S41TSV4RRFFQ69G5FA:"),
        ":01ARYZ6S41TSV4RRFFQ69G5FA:"
    );
    assert_eq!(
        md(":01ARYZ6S41TSV4RRFFQ69G5FAI:"),
        ":01ARYZ6S41TSV4RRFFQ69G5FAI:"
    );
}

#[test]
fn custom_emoji_in_code_and_spoilers() {
    let code = format!("`:{STOAT}:`");
    assert_eq!(md(&code), code);
    assert_eq!(md(&format!("||:{STOAT}: wins||")), "`:stoat: wins`");
}

#[test]
fn emoji_only_messages_are_jumbo() {
    assert!(is_jumbo(&format!(":{STOAT}:")));
    assert!(is_jumbo(&format!(" :{STOAT}: :{OTHER}: ")));
    assert!(is_jumbo("🦦🦦"));
    assert!(is_jumbo("👍🏽")); // skin-tone modifier is part of the emoji
    assert!(is_jumbo("❤️")); // with variation selector
    assert!(is_jumbo(&format!("🦦 :{STOAT}:")));
    assert!(is_jumbo(&"🦦".repeat(10)));

    assert!(!is_jumbo(&"🦦".repeat(11)));
    assert!(!is_jumbo(&format!("hi :{STOAT}:")));
    assert!(!is_jumbo("123")); // ASCII digits count as emoji characters in Unicode
    assert!(!is_jumbo("#"));
    assert!(!is_jumbo(""));
    assert!(!is_jumbo("   "));
}

#[test]
fn jumbo_messages_get_big_custom_emoji() {
    assert_eq!(
        md(&format!(":{STOAT}: :{OTHER}:")),
        format!("{} {}", img(STOAT, "stoat", 96), img(OTHER, "emoji", 96))
    );
}

/// Parse prepared Markdown the way `TextView` does (GFM plus `$…$` math).
fn parse(markdown: &str) -> markdown::mdast::Node {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    markdown::to_mdast(markdown, &options).unwrap()
}

/// The node types under `node`, depth first, as `type(child, …)`.
fn shape(node: &markdown::mdast::Node) -> String {
    use markdown::mdast::Node;
    let name = match node {
        Node::Root(_) => "root",
        Node::Paragraph(_) => "p",
        Node::Blockquote(_) => "quote",
        Node::Heading(_) => "h",
        Node::Break(_) => "br",
        Node::Text(_) => "text",
        Node::InlineMath(_) => "math",
        Node::InlineCode(_) => "code",
        Node::Code(_) => "codeblock",
        Node::Strong(_) => "strong",
        Node::Emphasis(_) => "em",
        Node::Link(_) => "link",
        Node::Table(_) => "table",
        Node::TableRow(_) => "row",
        Node::TableCell(_) => "cell",
        Node::List(_) => "list",
        Node::ListItem(_) => "item",
        _ => "other",
    };
    match node.children() {
        Some(children) if !children.is_empty() => {
            let inner: Vec<String> = children.iter().map(shape).collect();
            format!("{name}({})", inner.join(" "))
        }
        _ => name.to_string(),
    }
}

#[test]
fn every_line_break_is_kept() {
    // Stoat shows each newline; CommonMark would join the lines.
    assert_eq!(md("one\ntwo\nthree"), "one  \ntwo  \nthree");
    assert_eq!(shape(&parse(&md("one\ntwo"))), "root(p(text br text))");
    // Blank lines and existing hard breaks are left alone.
    assert_eq!(md("one\n\ntwo"), "one\n\ntwo");
    assert_eq!(md("one  \ntwo"), "one  \ntwo");
    assert_eq!(md("one\\\ntwo"), "one\\\ntwo");
    // An escaped backslash isn't a break, so one is added.
    assert_eq!(md("one\\\\\ntwo"), "one\\\\  \ntwo");
    assert_eq!(md("one\n"), "one\n");
    // Windows line endings too.
    assert_eq!(md("one\r\ntwo"), "one  \ntwo");
}

#[test]
fn line_breaks_leave_other_blocks_intact() {
    let list = md("- a\n- b\nmore");
    assert_eq!(list, "- a  \n- b  \nmore");
    assert_eq!(
        shape(&parse(&list)),
        "root(list(item(p(text)) item(p(text br text))))"
    );
    let heading = md("# Title\nbody");
    assert_eq!(shape(&parse(&heading)), "root(h(text) p(text))");
    let table = md("| a | b |\n| - | - |\n| 1 | 2 |");
    assert_eq!(
        shape(&parse(&table)),
        "root(table(row(cell(text) cell(text)) row(cell(text) cell(text))))"
    );
    // Lines inside code blocks stay as they are.
    assert_eq!(md("x\n```\na\nb\n```\ny"), "x  \n```\na\nb\n```\ny");
    assert_eq!(
        shape(&parse(&md("x\n```\na\n```\ny"))),
        "root(p(text) codeblock p(text))"
    );
}

#[test]
fn quotes_end_at_their_last_quoted_line() {
    // CommonMark would pull "after" into the quote ("lazy continuation");
    // Stoat ends the quote first.
    let quoted = md("> one\n> two\nafter");
    assert_eq!(quoted, "> one  \n> two\n\nafter");
    assert_eq!(
        shape(&parse(&quoted)),
        "root(quote(p(text br text)) p(text))"
    );
    assert_eq!(md("> one\n\nafter"), "> one\n\nafter");
    assert_eq!(md("  > indented\nafter"), "  > indented\n\nafter");
}

#[test]
fn double_dollar_math_is_left_for_the_math_renderer() {
    // Only `<` is escaped inside (`ui::math` reads `\<` back); mentions,
    // emoji and spoilers aren't rewritten.
    assert_eq!(
        md(r"x $$\color{red}<@u-ada> a<b ||c||$$ y"),
        r"x $$\color{red}\<@u-ada> a\<b ||c||$$ y"
    );
    assert_eq!(
        shape(&parse(&md(r"x $$\color{red}a<b$$ y"))),
        "root(p(text math text))"
    );
    // One `$` isn't math in Stoat: everything is rewritten as usual.
    assert_eq!(
        md("$5 for <@u-ada>, $10"),
        r"$5 for [@Ada L\.](stoat:user/u-ada), $10"
    );
    // Escaped, unclosed, empty or spread over lines: not math either.
    assert_eq!(md(r"\$$<b>$$"), r"\$$\<b>$$");
    assert_eq!(md("$$ <b>"), r"$$ \<b>");
    assert_eq!(md("$$$$"), "$$$$");
    assert_eq!(md("$$a\nb<$$"), "$$a  \nb\\<$$");
    // Code still wins when it opens first.
    assert_eq!(md("`$$<b>$$`"), "`$$<b>$$`");
}

#[test]
fn review_bot_messages_keep_their_lines_quotes_and_math() {
    // The shape of the messages that prompted this: a status line, a quote of
    // steps with grey `$$…$$` captions, then lines after the quote.
    let content = "📝 **reviewed for-web#1774** $$\\color{gray}\\scriptsize \\textsf{17.7s}$$\n\
        > ✅ 🔎 grep `mediaDevices` $$\\color{gray}\\scriptsize \\textsf{5 matches}\\ \\cdot\\ \\textsf{0.1s}$$\n\
        > ✅ ✍\u{fe0f} writing $$\\color{gray}\\scriptsize \\textsf{812 tokens}$$\n\
        🟡 `index.ts:10` a finding\n\
        _full review · $0.05 · 17s_\n\
        # 📖 [view the full review](https://example.com/review)";
    let prepared = md(content);
    assert_eq!(
        prepared,
        "📝 **reviewed for-web#1774** $$\\color{gray}\\scriptsize \\textsf{17.7s}$$  \n\
        > ✅ 🔎 grep `mediaDevices` $$\\color{gray}\\scriptsize \\textsf{5 matches}\\ \\cdot\\ \\textsf{0.1s}$$  \n\
        > ✅ ✍\u{fe0f} writing $$\\color{gray}\\scriptsize \\textsf{812 tokens}$$\n\
        \n\
        🟡 `index.ts:10` a finding  \n\
        _full review · $0.05 · 17s_  \n\
        # 📖 [view the full review](https://example.com/review)"
    );
    assert_eq!(
        shape(&parse(&prepared)),
        // A break at the end of a paragraph is dropped, so none before the
        // quote or the heading.
        "root(\
            p(text strong(text) text math) \
            quote(p(text code text math br text math)) \
            p(text code text br em(text)) \
            h(text link(text)))"
    );
}

/// A Friday afternoon in November 2026.
const WHEN: i64 = 1_762_430_000;

/// Every link in `node`, as (link text, URL).
fn links(node: &markdown::mdast::Node) -> Vec<(String, String)> {
    use markdown::mdast::Node;
    let mut found = Vec::new();
    if let Node::Link(link) = node {
        found.push((node.to_string(), link.url.clone()));
    }
    for child in node.children().into_iter().flatten() {
        found.extend(links(child));
    }
    found
}

#[test]
fn timestamps_become_internal_links_that_keep_their_source() {
    assert_eq!(
        md("<t:1762430000>"),
        r"[\<t\:1762430000\>](stoat:time/1762430000)"
    );
    assert_eq!(
        md("at <t:1762430000:R>!"),
        r"at [\<t\:1762430000\:R\>](stoat:time/1762430000/R)!"
    );
    // The link text is the tag as typed, and the URL survives parsing
    // whatever the style's case.
    for style in ["", ":R", ":t", ":T", ":D", ":F", ":f", ":_", ":9"] {
        let source = format!("<t:{WHEN}{style}>");
        let url = format!("stoat:time/{WHEN}{}", style.replace(':', "/"));
        assert_eq!(links(&parse(&md(&source))), vec![(source.clone(), url)]);
    }
}

#[test]
fn malformed_timestamps_stay_as_typed() {
    assert_eq!(md("<t:abc>"), r"\<t:abc>");
    assert_eq!(md("<t:>"), r"\<t:>");
    assert_eq!(md("<t:1762430000:R"), r"\<t:1762430000:R");
    assert_eq!(md("<t:1762430000:RR>"), r"\<t:1762430000:RR>");
    assert_eq!(md("<t:-5>"), r"\<t:-5>");
    assert_eq!(md("<t:1762430000 >"), r"\<t:1762430000 >");
    // Out of chrono's range.
    assert_eq!(md("<t:99999999999999999>"), r"\<t:99999999999999999>");
}

#[test]
fn timestamps_in_code_are_untouched() {
    assert_eq!(md("`<t:1762430000>`"), "`<t:1762430000>`");
    assert_eq!(
        md("```\n<t:1762430000:R>\n```"),
        "```\n<t:1762430000:R>\n```"
    );
}

#[test]
fn timestamps_next_to_mentions() {
    assert_eq!(
        md("<@u-ada><t:1762430000:t>"),
        r"[@Ada L\.](stoat:user/u-ada)[\<t\:1762430000\:t\>](stoat:time/1762430000/t)"
    );
    let parsed = links(&parse(&md("<@u-ada><t:1762430000:t>")));
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[1].1, "stoat:time/1762430000/t");
}

#[test]
fn plain_text_shows_timestamps_in_local_time() {
    use chrono::TimeZone;
    use stoat_gpui::markdown::{plain_text, snippet};
    use stoat_gpui::time::format_timestamp;
    let when = chrono::Local.timestamp_opt(WHEN, 0).unwrap();
    let now = chrono::Local::now();
    let time = format_timestamp(&when, Some('t'), &now);
    assert_eq!(
        plain_text("at <t:1762430000:t>", &Fixture),
        format!("at {time}")
    );
    // A relative one can't update here, so it's the date and time.
    let date_time = format_timestamp(&when, None, &now);
    assert_eq!(plain_text("<t:1762430000:R>", &Fixture), date_time);
    assert_eq!(
        snippet("**due** <t:1762430000:R>", &Fixture, 80),
        format!("due {date_time}")
    );
    // Malformed ones stay as typed.
    assert_eq!(plain_text("<t:abc>", &Fixture), "<t:abc>");
}
