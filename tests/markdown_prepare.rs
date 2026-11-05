//! Stoat content → Markdown: mentions, spoilers, HTML escaping, code safety.

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
    let text = "**bold** _it_ [link](https://example.com)\n> quote\n- item";
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
    let tricky = "⚠<@u-ada>é||🦦 spoil||`co😀de`<#ch-general>:01ARYZ6S41TSV4RRFFQ69G5FAV:\n```\n日本\n```\n<b>ü</b> <https://x.y/ö> |";
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
