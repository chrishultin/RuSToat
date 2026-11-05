//! Finding emoji: Unicode by shortcode, custom emoji by name, and exact
//! shortcodes for `:smile:` as you type.

use stoat_gpui::complete::{Insertion, MAX_SUGGESTIONS, TokenRef};
use stoat_gpui::emoji::{CustomEmoji, EmojiChoice, by_shortcode, search};

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
    let smile = search("smile", &[])[0].suggestion();
    assert_eq!(smile.title, ":smile:");
    assert_eq!(smile.glyph.as_deref(), Some("😄"));
    assert_eq!(smile.insert, Insertion::Text("😄".into()));

    let custom = EmojiChoice::Custom(parrot()).suggestion();
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
    assert_eq!(by_shortcode("smile"), Some("😄"));
    assert_eq!(by_shortcode("tada"), Some("🎉"));
    assert_eq!(by_shortcode("nope"), None);
    assert_eq!(by_shortcode("01M6GEW5A0CCCCCCCCCCCCCCCC"), None);
}
