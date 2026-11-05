//! Finding emoji by name, with no GPUI: Unicode emoji by their GitHub
//! (gemoji) shortcodes, from the `emojis` crate, and a server's custom
//! emoji by their names. What's sent for a Unicode emoji is the character
//! itself, so the shortcode set only matters for finding it.

use crate::complete::{Insertion, MAX_SUGGESTIONS, Suggestion, TokenRef};

/// A server's custom emoji.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomEmoji {
    pub id: String,
    pub name: String,
    /// Its image. `None` until the file server is known.
    pub url: Option<String>,
}

/// An emoji the message box can insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmojiChoice {
    Unicode {
        emoji: &'static str,
        /// Its main shortcode (or its name in snake case if it has none).
        shortcode: String,
    },
    Custom(CustomEmoji),
}

impl EmojiChoice {
    fn unicode(emoji: &'static emojis::Emoji) -> Self {
        EmojiChoice::Unicode {
            emoji: emoji.as_str(),
            shortcode: shortcode_of(emoji),
        }
    }

    /// The suggestion row: the emoji and `:shortcode:`. Picking a Unicode
    /// emoji inserts it as text; a custom one, as a chip sent as `:id:`.
    pub fn suggestion(&self) -> Suggestion {
        match self {
            EmojiChoice::Unicode { emoji, shortcode } => Suggestion {
                key: format!("emoji:{emoji}"),
                title: format!(":{shortcode}:"),
                detail: None,
                image: None,
                glyph: Some(emoji.to_string()),
                insert: Insertion::Text(emoji.to_string()),
            },
            EmojiChoice::Custom(custom) => Suggestion {
                key: format!("emoji:{}", custom.id),
                title: format!(":{}:", custom.name),
                detail: None,
                image: custom.url.clone(),
                glyph: None,
                insert: Insertion::Token {
                    token: TokenRef::Emoji(custom.id.clone()),
                    label: format!(":{}:", custom.name),
                },
            },
        }
    }
}

fn shortcode_of(emoji: &emojis::Emoji) -> String {
    emoji
        .shortcode()
        .map(str::to_string)
        .unwrap_or_else(|| emoji.name().to_lowercase().replace(' ', "_"))
}

/// Emoji for `query` (what's typed after `:`), best first, at most
/// [`MAX_SUGGESTIONS`]: an exact shortcode or name, then ones starting with
/// it, then ones with a later word starting with it (`:engine` finds
/// `fire_engine`). Custom emoji come before Unicode ones that match as well.
pub fn search(query: &str, custom: &[CustomEmoji]) -> Vec<EmojiChoice> {
    let query = query.to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let custom = custom
        .iter()
        .filter_map(|emoji| Some((tier(&query, [emoji.name.as_str()])?, EmojiChoice::Custom(emoji.clone()))));
    let unicode = emojis::iter().filter_map(|emoji| {
        let name = emoji.name().to_lowercase();
        let tier = tier(&query, emoji.shortcodes().chain([name.as_str()]))?;
        Some((tier, EmojiChoice::unicode(emoji)))
    });
    let mut found: Vec<(u8, EmojiChoice)> = custom.chain(unicode).collect();
    // Stable: custom first, then CLDR order, within each tier.
    found.sort_by_key(|(tier, _)| *tier);
    found
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, choice)| choice)
        .collect()
}

/// 0: one of `names` is `query`; 1: starts with it; 2: has a word (split on
/// `_`, `-` or spaces) starting with it. `query` is lowercase.
fn tier<'a>(query: &str, names: impl IntoIterator<Item = &'a str>) -> Option<u8> {
    names
        .into_iter()
        .filter_map(|name| {
            let name = name.to_lowercase();
            if name == query {
                Some(0)
            } else if name.starts_with(query) {
                Some(1)
            } else if name
                .split(['_', '-', ' '])
                .skip(1)
                .any(|word| word.starts_with(query))
            {
                Some(2)
            } else {
                None
            }
        })
        .min()
}

/// The Unicode emoji whose shortcode is exactly `code` (`smile` → 😄).
pub fn by_shortcode(code: &str) -> Option<&'static str> {
    emojis::get_by_shortcode(code).map(|emoji| emoji.as_str())
}
