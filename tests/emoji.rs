//! Finding emoji: Unicode by shortcode, custom emoji by name, and exact
//! shortcodes for `:smile:` as you type.

use stoat_gpui::complete::{Insertion, MAX_SUGGESTIONS, TokenRef};
use stoat_gpui::emoji::{
    CustomEmoji, EmojiChoice, by_shortcode, custom_by_name, search, toned,
};
use stoat_gpui::prefs::Tone;

fn glyphs(choices: &[EmojiChoice]) -> Vec<String> {
    choices
        .iter()
        .map(|c| match c {
            EmojiChoice::Unicode { emoji, .. } => emoji.to_string(),
            EmojiChoice::Custom(custom) => format!(":{}:", custom.name),
        })
        .collect()
}

fn parrot() -> CustomEmoji {
    CustomEmoji {
        id: "01M6GEW5A0CCCCCCCCCCCCCCCC".into(),
        name: "smile_parrot".into(),
        url: Some("https://cdn/emojis/01M6GEW5A0CCCCCCCCCCCCCCCC".into()),
    }
}

#[test]
fn shortcode_prefixes_find_unicode_emoji() {
    let found = glyphs(&search("smi", &[]));
    assert!(found.iter().take(3).any(|e| e == "😄"), "{found:?}");
    assert!(found.len() <= MAX_SUGGESTIONS);
    assert_eq!(glyphs(&search("thu", &[]))[0], "👍");
    assert_eq!(glyphs(&search("+1", &[]))[0], "👍");
    assert_eq!(glyphs(&search("TADA", &[]))[0], "🎉");
}

#[test]
fn an_exact_shortcode_comes_first() {
    // `smile` is exact; `smiley` comes earlier in CLDR order but only starts with it.
    assert_eq!(glyphs(&search("smile", &[]))[0], "😄");
    assert_eq!(glyphs(&search("cat", &[]))[0], "🐱");
}

#[test]
fn later_words_match_after_prefixes() {
    // `fire_engine` has a word starting with `engine`.
    assert!(glyphs(&search("engine", &[])).contains(&"🚒".to_string()));
    assert!(search("zzzz", &[]).is_empty());
    assert!(search("", &[]).is_empty());
}

#[test]
fn custom_emoji_are_found_by_name_before_unicode_ones() {
    let found = search("smile", &[parrot()]);
    // The Unicode exact match beats a custom prefix match…
    assert_eq!(glyphs(&found)[0], "😄");
    assert_eq!(glyphs(&found)[1], ":smile_parrot:");
    // …but a custom one comes first within a tier, and later words count.
    assert_eq!(glyphs(&search("smil", &[parrot()]))[0], ":smile_parrot:");
    assert_eq!(
        glyphs(&search("parrot", &[parrot()]))[..2],
        ["🦜".to_string(), ":smile_parrot:".to_string()]
    );
}

#[test]
fn suggestions_insert_text_or_a_custom_emoji_chip() {
    let smile = search("smile", &[])[0].suggestion(Tone::Default);
    assert_eq!(smile.title, ":smile:");
    assert_eq!(smile.glyph.as_deref(), Some("😄"));
    assert_eq!(smile.insert, Insertion::Text("😄".into()));

    let custom = EmojiChoice::Custom(parrot()).suggestion(Tone::Default);
    assert_eq!(custom.title, ":smile_parrot:");
    assert_eq!(custom.image, parrot().url);
    assert_eq!(
        custom.insert,
        Insertion::Token {
            token: TokenRef::Emoji(parrot().id),
            label: ":smile_parrot:".into()
        }
    );
    assert_eq!(TokenRef::Emoji(parrot().id).wire_text(), format!(":{}:", parrot().id));
}

#[test]
fn exact_shortcodes_are_looked_up() {
    assert_eq!(by_shortcode("smile", Tone::Default), Some("😄"));
    assert_eq!(by_shortcode("tada", Tone::Default), Some("🎉"));
    assert_eq!(by_shortcode("nope", Tone::Default), None);
    assert_eq!(by_shortcode("01M6GEW5A0CCCCCCCCCCCCCCCC", Tone::Default), None);
}

// The emoji picker.

use stoat_gpui::emoji::{
    MAX_RECENT, PickerRow, RecentEmoji, groups, picker_rows, picker_sections, search_all,
};

#[test]
fn groups_come_in_unicode_order_and_none_is_empty() {
    let groups = groups();
    let names: Vec<&str> = groups.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        [
            "Smileys & Emotion",
            "People & Body",
            "Animals & Nature",
            "Food & Drink",
            "Travel & Places",
            "Activities",
            "Objects",
            "Symbols",
            "Flags"
        ]
    );
    assert!(groups.iter().all(|(_, emoji)| !emoji.is_empty()));
    let total: usize = groups.iter().map(|(_, emoji)| emoji.len()).sum();
    assert!(total > 1800, "{total}");
    // The first emoji of all, and no skin-tone variants.
    assert_eq!(glyphs(&groups[0].1[..1]), ["😀"]);
    assert!(!glyphs(&groups[1].1).iter().any(|e| e.contains('\u{1F3FB}')));
}

fn unicode(emoji: &'static str) -> EmojiChoice {
    match search_all(emoji, &[], 1).first() {
        Some(choice) => choice.clone(),
        None => EmojiChoice::Unicode {
            emoji,
            shortcode: emoji.into(),
        },
    }
}

#[test]
fn recently_used_is_most_recent_first_without_duplicates_and_capped() {
    let mut recent = RecentEmoji::default();
    recent.used(unicode("smile"));
    recent.used(unicode("tada"));
    recent.used(unicode("smile"));
    assert_eq!(glyphs(recent.list()), ["😄", "🎉"]);
    recent.used(EmojiChoice::Custom(parrot()));
    assert_eq!(glyphs(recent.list()), [":smile_parrot:", "😄", "🎉"]);

    for choice in groups()[3].1.iter().take(20) {
        recent.used(choice.clone());
    }
    assert_eq!(recent.list().len(), MAX_RECENT);
    assert_eq!(recent.list()[0], groups()[3].1[19]);
}

#[test]
fn the_pickers_search_gives_what_completion_gives_then_more() {
    for query in ["smi", "cat", "heart", "parrot"] {
        let all = search_all(query, &[parrot()], 64);
        let completion = search(query, &[parrot()]);
        assert_eq!(all[..completion.len()], completion[..], "{query}");
    }
    assert!(search_all("heart", &[], 64).len() > MAX_SUGGESTIONS);
}

#[test]
fn the_picker_shows_recent_then_categories_then_custom_or_search_results() {
    let mut recent = RecentEmoji::default();
    let titles = |sections: &[(String, Vec<EmojiChoice>)]| -> Vec<String> {
        sections.iter().map(|(title, _)| title.clone()).collect()
    };
    // Nothing recent or custom: just the categories.
    let sections = picker_sections("", &recent, &[]);
    assert_eq!(titles(&sections).len(), 9);
    assert_eq!(titles(&sections)[0], "Smileys & Emotion");

    recent.used(unicode("tada"));
    let sections = picker_sections("", &recent, &[parrot()]);
    let names = titles(&sections);
    assert_eq!(names.first().map(String::as_str), Some("Recently used"));
    assert_eq!(names.last().map(String::as_str), Some("Custom"));
    assert_eq!(glyphs(&sections.last().unwrap().1), [":smile_parrot:"]);

    // A search (with or without colons) shows only results.
    let sections = picker_sections(" :cat ", &recent, &[]);
    assert_eq!(titles(&sections), ["Results"]);
    assert_eq!(glyphs(&sections[0].1)[0], "🐱");
    assert!(picker_sections("zzzz", &recent, &[]).is_empty());
}

#[test]
fn picker_rows_put_a_header_over_rows_of_eight() {
    let ten: Vec<EmojiChoice> = groups()[0].1[..10].to_vec();
    let rows = picker_rows(
        vec![("A".into(), ten.clone()), ("B".into(), ten[..3].to_vec())],
        8,
    );
    assert_eq!(
        rows,
        vec![
            PickerRow::Header("A".into()),
            PickerRow::Emoji(ten[..8].to_vec()),
            PickerRow::Emoji(ten[8..].to_vec()),
            PickerRow::Header("B".into()),
            PickerRow::Emoji(ten[..3].to_vec()),
        ]
    );
    // Every category, virtualized: a few hundred rows, not 1,900 cells.
    let all = picker_rows(picker_sections("", &RecentEmoji::default(), &[]), 8);
    assert!(all.len() < 300, "{}", all.len());
}

#[test]
fn custom_emoji_are_found_by_exact_name() {
    let party = CustomEmoji {
        id: "01M6GEW5A0DDDDDDDDDDDDDDDD".into(),
        name: "party".into(),
        url: None,
    };
    let custom = [parrot(), party.clone()];
    assert_eq!(custom_by_name(&custom, "party"), Some(&party));
    assert_eq!(
        custom_by_name(&custom, "smile_parrot").map(|e| e.id.as_str()),
        Some(parrot().id.as_str())
    );
    // Exact: no case folding, no prefixes.
    assert_eq!(custom_by_name(&custom, "Party"), None);
    assert_eq!(custom_by_name(&custom, "part"), None);
    assert_eq!(custom_by_name(&custom, "missing"), None);
    assert_eq!(custom_by_name(&[], "party"), None);
}

// The default skin tone.

#[test]
fn toneable_emoji_take_the_tone_and_others_dont() {
    assert_eq!(toned("👍", Tone::Medium), "👍🏽");
    assert_eq!(toned("👋", Tone::Light), "👋🏻");
    assert_eq!(toned("👍", Tone::Default), "👍");
    for tone in Tone::ALL {
        assert_eq!(toned("😄", tone), "😄", "{tone:?}");
        assert_eq!(toned("🎉", tone), "🎉", "{tone:?}");
    }
    // Not an emoji the crate knows: left alone.
    assert_eq!(toned("x", Tone::Dark), "x");
}

#[test]
fn two_person_emoji_get_one_tone_for_both_or_stay_as_they_are() {
    for base in ["🤝", "👫", "💏", "🧑‍🤝‍🧑"] {
        for tone in Tone::ALL {
            let glyph = toned(base, tone);
            let found = emojis::get(glyph).expect("a real emoji");
            if tone == Tone::Default {
                assert_eq!(glyph, base);
            } else if glyph != base {
                // Toned: the same tone for everyone in it.
                assert_eq!(found.skin_tone(), Some(tone.skin_tone()), "{base} {tone:?}");
            }
        }
    }
    // The handshake has single-tone variants.
    assert_eq!(
        emojis::get(toned("🤝", Tone::Dark)).unwrap().skin_tone(),
        Some(emojis::SkinTone::Dark)
    );
}

#[test]
fn shortcodes_and_suggestions_use_the_tone() {
    assert_eq!(by_shortcode("wave", Tone::Light), Some("👋🏻"));
    assert_eq!(by_shortcode("+1", Tone::Medium), Some("👍🏽"));
    assert_eq!(by_shortcode("smile", Tone::Dark), Some("😄"));

    // Matching stays on shortcodes, so a toned suggestion is still found.
    let wave = search("wave", &[])[0].clone();
    let row = wave.suggestion(Tone::Medium);
    assert_eq!(row.glyph.as_deref(), Some("👋🏽"));
    assert_eq!(row.insert, Insertion::Text("👋🏽".into()));
    // The row's key doesn't depend on the tone, so the selection survives a change.
    assert_eq!(row.key, wave.suggestion(Tone::Default).key);
}

#[test]
fn recently_used_keeps_the_base_emoji() {
    let mut recent = RecentEmoji::default();
    let thumbs = search("+1", &[])[0].clone();
    recent.used(thumbs.clone());
    let EmojiChoice::Unicode { emoji, .. } = &recent.list()[0] else {
        panic!("unicode");
    };
    assert_eq!(*emoji, "👍");
    // So the tone applies when it's drawn or inserted, whatever it is then.
    assert_eq!(toned(emoji, Tone::Dark), "👍🏿");
}
