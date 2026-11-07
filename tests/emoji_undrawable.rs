//! Emoji the system's emoji font can't draw are never offered. A file of
//! its own: what's hidden is set once per process.

use stoat_gpui::emoji::{
    EmojiChoice, RecentEmoji, by_shortcode, drawable_text, groups, hide_undrawable,
    picker_sections, search, toned,
};
use stoat_gpui::markdown::{Names, plain_text, prepare, snippet};
use stoat_gpui::prefs::Tone;

/// 🫝 pickle (Unicode 18.0).
const PICKLE: char = '\u{1FADD}';
/// 🫹 leftwards thumb sign (Unicode 18.0), which takes skin tones.
const THUMB: char = '\u{1FAF9}';
/// 🏽, the medium skin tone modifier.
const MEDIUM: char = '\u{1F3FD}';

/// Hide the pickle, the thumb and the medium tone. Every test asks for the
/// same, since only the first call counts. Returns how many are hidden.
fn hide() -> usize {
    hide_undrawable(|c| c != PICKLE && c != THUMB && c != MEDIUM)
}

struct NoNames;

impl Names for NoNames {
    fn user_name(&self, _: &str) -> Option<String> {
        None
    }
    fn channel_name(&self, _: &str) -> Option<String> {
        None
    }
}

fn has(choices: &[EmojiChoice], emoji: &str) -> bool {
    choices
        .iter()
        .any(|c| matches!(c, EmojiChoice::Unicode { emoji: e, .. } if *e == emoji))
}

#[test]
fn emoji_the_font_lacks_are_left_out_everywhere() {
    let pickle = PICKLE.to_string();
    let thumb = THUMB.to_string();
    // It's there to hide (tests run together, so another may have hidden
    // it already).
    assert!(emojis::get(&pickle).is_some());

    let hidden = hide();
    // Pickle, the thumb, and everything in a medium skin tone.
    assert!(hidden >= 2, "{hidden}");

    assert!(!has(&search("pickle", &[]), &pickle));
    assert!(by_shortcode("pickle", Tone::Default).is_none());
    assert!(
        groups()
            .iter()
            .all(|(_, emoji)| !has(emoji, &pickle) && !has(emoji, &thumb))
    );
    let sections = picker_sections("pickle", &RecentEmoji::default(), &[]);
    assert!(sections.iter().all(|(_, emoji)| !has(emoji, &pickle)));

    // Ones it can draw are still there, and a missing tone falls back to
    // the base emoji rather than a box.
    assert_eq!(by_shortcode("smile", Tone::Default), Some("😄"));
    assert_eq!(toned("👋", Tone::Medium), "👋");
    assert_eq!(toned("👋", Tone::Light), "👋🏻");
}

#[test]
fn text_shows_no_boxes() {
    hide();
    // As Amelia sent it: a private-use character before the emoji.
    assert_eq!(
        drawable_text("no introspection \u{E0E4}😔"),
        "no introspection 😔"
    );
    for c in ['\u{E000}', '\u{F8FE}', '\u{F0000}', '\u{10FFFD}'] {
        assert_eq!(drawable_text(&format!("a{c}b")), "ab", "{:X}", c as u32);
    }
    // Apple's logo is drawn.
    assert_eq!(drawable_text("\u{F8FF}"), "\u{F8FF}");

    // Emoji the font lacks go by name, without their tone.
    assert_eq!(drawable_text(&format!("a {PICKLE} b")), "a :pickle: b");
    assert_eq!(
        drawable_text(&format!("{THUMB}{MEDIUM}")),
        ":leftwards_thumb_sign:"
    );
    // A drawable emoji in a missing tone is named, not drawn half.
    assert_eq!(drawable_text(&format!("👋{MEDIUM}!")), ":wave:!");
    // The rest is left alone, sequences and all.
    let plain = "héllo 👋🏻 👨‍👩‍👧 ❤️ 中文";
    assert!(matches!(
        drawable_text(plain),
        std::borrow::Cow::Borrowed(_)
    ));
    assert_eq!(
        drawable_text(&format!("{plain}{PICKLE}")),
        format!("{plain}:pickle:")
    );
}

#[test]
fn messages_previews_and_notices_show_no_boxes() {
    hide();
    let content = format!("no introspection \u{E0E4}😔 {PICKLE}");
    assert_eq!(prepare(&content, &NoNames), "no introspection 😔 :pickle:");
    assert_eq!(
        plain_text(&content, &NoNames),
        "no introspection 😔 :pickle:"
    );
    assert_eq!(
        snippet(&content, &NoNames, 100),
        "no introspection 😔 :pickle:"
    );
    // In code too.
    assert_eq!(prepare("`\u{E0E4}x`", &NoNames), "`x`");
    // An emoji-only message whose emoji can't be drawn is a name, not jumbo.
    assert_eq!(prepare(&PICKLE.to_string(), &NoNames), ":pickle:");
}
