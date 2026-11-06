//! Finding emoji by name, with no GPUI: Unicode emoji by their GitHub
//! (gemoji) shortcodes, from the `emojis` crate, and a server's custom
//! emoji by their names. What's sent for a Unicode emoji is the character
//! itself, so the shortcode set only matters for finding it.
//!
//! Choices always hold the base (untoned) emoji, and searching ignores skin
//! tones; the user's default tone is applied when an emoji is drawn or
//! inserted ([`toned`]). So changing the tone recolours everything,
//! recently used emoji included.
//!
//! Emoji newer than the system's emoji font are left out everywhere here
//! (see [`hide_undrawable`]), so they're never offered as a box with a
//! question mark.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::complete::{Insertion, MAX_SUGGESTIONS, Suggestion, TokenRef};
use crate::prefs::Tone;

/// Characters the system's emoji font has no glyph for. Unset (nothing
/// hidden) until [`hide_undrawable`].
static MISSING: OnceLock<HashSet<char>> = OnceLock::new();

/// Leave out every emoji with a character `has_glyph` says the emoji font
/// can't draw (Unicode's newest, before the OS catches up). Called once at
/// launch; later calls do nothing. Returns how many emoji are hidden.
pub fn hide_undrawable(has_glyph: impl Fn(char) -> bool) -> usize {
    let mut chars: HashSet<char> = HashSet::new();
    for emoji in emojis::iter() {
        chars.extend(emoji.as_str().chars());
        for toned in emoji.skin_tones().into_iter().flatten() {
            chars.extend(toned.as_str().chars());
        }
    }
    let missing = chars.into_iter().filter(|&c| !has_glyph(c)).collect();
    let _ = MISSING.set(missing);
    emojis::iter().filter(|emoji| !drawable(emoji)).count()
}

/// Leave out emoji Apple Color Emoji can't draw, at launch. Returns how
/// many are hidden, or `None` if the font isn't there (nothing's hidden).
#[cfg(target_os = "macos")]
pub fn hide_undrawable_by_system_font() -> Option<usize> {
    use std::ptr::NonNull;

    use objc2_core_foundation::CFString;
    use objc2_core_text::CTFont;

    const FAMILY: &str = "Apple Color Emoji";
    // SAFETY: a null matrix means the identity transform.
    let font = unsafe { CTFont::with_name(&CFString::from_str(FAMILY), 16.0, std::ptr::null()) };
    // CoreText substitutes another font for a missing one, which has no emoji.
    // SAFETY: `font` is a valid CTFont.
    if unsafe { font.family_name() }.to_string() != FAMILY {
        return None;
    }
    Some(hide_undrawable(|c| {
        let mut chars = [0u16; 2];
        let chars = c.encode_utf16(&mut chars);
        let mut glyphs = [0u16; 2];
        // SAFETY: both buffers hold `chars.len()` (1 or 2) elements.
        unsafe {
            font.glyphs_for_characters(
                NonNull::new_unchecked(chars.as_mut_ptr()),
                NonNull::new_unchecked(glyphs.as_mut_ptr()),
                chars.len() as isize,
            )
        }
    }))
}

#[cfg(not(target_os = "macos"))]
pub fn hide_undrawable_by_system_font() -> Option<usize> {
    None
}

/// Whether the emoji font can draw all of `emoji`.
fn drawable(emoji: &emojis::Emoji) -> bool {
    MISSING
        .get()
        .is_none_or(|missing| !emoji.as_str().chars().any(|c| missing.contains(&c)))
}

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

    /// The suggestion row: the emoji (in `tone`) and `:shortcode:`. Picking
    /// a Unicode emoji inserts it as text; a custom one, as a chip sent as
    /// `:id:`.
    pub fn suggestion(&self, tone: Tone) -> Suggestion {
        match self {
            EmojiChoice::Unicode { emoji, shortcode } => Suggestion {
                key: format!("emoji:{emoji}"),
                title: format!(":{shortcode}:"),
                detail: None,
                image: None,
                glyph: Some(toned(emoji, tone).to_string()),
                insert: Insertion::Text(toned(emoji, tone).to_string()),
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
    search_all(query, custom, MAX_SUGGESTIONS)
}

/// Like [`search`], but up to `limit` results (the emoji picker's search).
pub fn search_all(query: &str, custom: &[CustomEmoji], limit: usize) -> Vec<EmojiChoice> {
    let query = query.to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let custom = custom
        .iter()
        .filter_map(|emoji| Some((tier(&query, [emoji.name.as_str()])?, EmojiChoice::Custom(emoji.clone()))));
    let unicode = emojis::iter().filter(|emoji| drawable(emoji)).filter_map(|emoji| {
        let name = emoji.name().to_lowercase();
        let tier = tier(&query, emoji.shortcodes().chain([name.as_str()]))?;
        Some((tier, EmojiChoice::unicode(emoji)))
    });
    let mut found: Vec<(u8, EmojiChoice)> = custom.chain(unicode).collect();
    // Stable: custom first, then CLDR order, within each tier.
    found.sort_by_key(|(tier, _)| *tier);
    found
        .into_iter()
        .take(limit)
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

/// The Unicode emoji whose shortcode is exactly `code` (`smile` → 😄), in
/// `tone` if it has skin tones (`wave` → 👋🏻 in [`Tone::Light`]). `None`
/// for one the emoji font can't draw, so `:code:` stays as typed.
pub fn by_shortcode(code: &str, tone: Tone) -> Option<&'static str> {
    emojis::get_by_shortcode(code)
        .filter(|emoji| drawable(emoji))
        .map(|emoji| toned(emoji.as_str(), tone))
}

/// `emoji` in `tone`, if it has skin tones; otherwise (😄, flags, custom
/// text) unchanged. Two-person emoji (🤝, 💏…) get the same tone for both
/// people, or stay as they are if there's no such variant.
pub fn toned(emoji: &'static str, tone: Tone) -> &'static str {
    if tone == Tone::Default {
        return emoji;
    }
    emojis::get(emoji)
        .and_then(|e| e.with_skin_tone(tone.skin_tone()))
        .filter(|e| drawable(e))
        .map_or(emoji, |e| e.as_str())
}

/// The custom emoji named exactly `name` (Stoat's names are
/// case-sensitive), for converting a typed `:name:`.
pub fn custom_by_name<'a>(custom: &'a [CustomEmoji], name: &str) -> Option<&'a CustomEmoji> {
    custom.iter().find(|emoji| emoji.name == name)
}

/// Every Unicode emoji, by category in Unicode (CLDR) order, with each
/// category's name. Default skin tones only, and only ones the emoji font
/// can draw.
pub fn groups() -> Vec<(&'static str, Vec<EmojiChoice>)> {
    emojis::Group::iter()
        .map(|group| {
            let emoji = emojis::iter()
                .filter(|emoji| emoji.group() == group && drawable(emoji))
                .map(EmojiChoice::unicode)
                .collect();
            (group_name(group), emoji)
        })
        .collect()
}

fn group_name(group: emojis::Group) -> &'static str {
    use emojis::Group;
    match group {
        Group::SmileysAndEmotion => "Smileys & Emotion",
        Group::PeopleAndBody => "People & Body",
        Group::AnimalsAndNature => "Animals & Nature",
        Group::FoodAndDrink => "Food & Drink",
        Group::TravelAndPlaces => "Travel & Places",
        Group::Activities => "Activities",
        Group::Objects => "Objects",
        Group::Symbols => "Symbols",
        Group::Flags => "Flags",
    }
}

/// Most emoji kept as recently used.
pub const MAX_RECENT: usize = 16;

/// Emoji the user picked recently, most recent first, while the app runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentEmoji(Vec<EmojiChoice>);

impl RecentEmoji {
    /// `choice` was just used: it moves to the front.
    pub fn used(&mut self, choice: EmojiChoice) {
        self.0.retain(|c| *c != choice);
        self.0.insert(0, choice);
        self.0.truncate(MAX_RECENT);
    }

    pub fn list(&self) -> &[EmojiChoice] {
        &self.0
    }
}

/// Most results the picker's search shows.
pub const MAX_SEARCH_RESULTS: usize = 64;

/// What the emoji picker shows, as titled sections: for a search, its
/// results; otherwise recently used, every category, then this server's
/// custom emoji. Empty sections are left out.
pub fn picker_sections(
    query: &str,
    recent: &RecentEmoji,
    custom: &[CustomEmoji],
) -> Vec<(String, Vec<EmojiChoice>)> {
    let query = query.trim();
    let sections = if query.is_empty() {
        let mut sections = vec![("Recently used".to_string(), recent.list().to_vec())];
        sections.extend(groups().into_iter().map(|(name, emoji)| (name.to_string(), emoji)));
        sections.push((
            "Custom".to_string(),
            custom.iter().cloned().map(EmojiChoice::Custom).collect(),
        ));
        sections
    } else {
        let results = search_all(query.trim_matches(':'), custom, MAX_SEARCH_RESULTS);
        vec![("Results".to_string(), results)]
    };
    sections
        .into_iter()
        .filter(|(_, emoji)| !emoji.is_empty())
        .collect()
}

/// One row of the picker's list: all rows are the same height, so the
/// list can be virtualized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerRow {
    Header(String),
    /// Up to a row's worth of emoji.
    Emoji(Vec<EmojiChoice>),
}

/// `sections` laid out as rows of at most `columns` emoji, each section
/// under its header.
pub fn picker_rows(sections: Vec<(String, Vec<EmojiChoice>)>, columns: usize) -> Vec<PickerRow> {
    let mut rows = Vec::new();
    for (title, emoji) in sections {
        rows.push(PickerRow::Header(title));
        rows.extend(emoji.chunks(columns.max(1)).map(|row| PickerRow::Emoji(row.to_vec())));
    }
    rows
}
