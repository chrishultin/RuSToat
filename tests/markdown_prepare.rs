//! Stoat content → Markdown: mentions, spoilers, HTML escaping, code safety.

use stoat_gpui::markdown::{Names, is_external_link, prepare};

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
    let tricky =
        "⚠<@u-ada>é||🦦 spoil||`co😀de`<#ch-general>\n```\n日本\n```\n<b>ü</b> <https://x.y/ö> |";
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
