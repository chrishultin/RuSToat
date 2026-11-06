//! Emoji the system's emoji font can't draw are never offered. A file of
//! its own: what's hidden is set once per process.

use stoat_gpui::emoji::{
    EmojiChoice, RecentEmoji, by_shortcode, groups, hide_undrawable, picker_sections, search,
    toned,
};
use stoat_gpui::prefs::Tone;

/// 🫝 pickle (Unicode 18.0).
const PICKLE: char = '\u{1FADD}';
/// 🫹 leftwards thumb sign (Unicode 18.0), which takes skin tones.
const THUMB: char = '\u{1FAF9}';
/// 🏽, the medium skin tone modifier.
const MEDIUM: char = '\u{1F3FD}';

fn has(choices: &[EmojiChoice], emoji: &str) -> bool {
    choices
        .iter()
        .any(|c| matches!(c, EmojiChoice::Unicode { emoji: e, .. } if *e == emoji))
}

#[test]
fn emoji_the_font_lacks_are_left_out_everywhere() {
    let pickle = PICKLE.to_string();
    let thumb = THUMB.to_string();
    assert!(has(&search("pickle", &[]), &pickle), "found before hiding");

    let hidden = hide_undrawable(|c| c != PICKLE && c != THUMB && c != MEDIUM);
    // Pickle, the thumb, and everything in a medium skin tone.
    assert!(hidden >= 2, "{hidden}");

    assert!(!has(&search("pickle", &[]), &pickle));
    assert!(by_shortcode("pickle", Tone::Default).is_none());
    assert!(groups().iter().all(|(_, emoji)| !has(emoji, &pickle) && !has(emoji, &thumb)));
    let sections = picker_sections("pickle", &RecentEmoji::default(), &[]);
    assert!(sections.iter().all(|(_, emoji)| !has(emoji, &pickle)));

    // Ones it can draw are still there, and a missing tone falls back to
    // the base emoji rather than a box.
    assert_eq!(by_shortcode("smile", Tone::Default), Some("😄"));
    assert_eq!(toned("👋", Tone::Medium), "👋");
    assert_eq!(toned("👋", Tone::Light), "👋🏻");
}
