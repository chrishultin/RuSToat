//! Autocomplete in the message box, with no GPUI: when to suggest (`@`,
//! `#`, `:`), how suggestions rank, and the mentions in a message's text
//! that the box shows as chips.

use std::ops::Range;

/// Most suggestions shown at once.
pub const MAX_SUGGESTIONS: usize = 8;

/// Fewest characters after `:` before emoji are suggested (so `:)` and
/// `a:b` don't open anything).
pub const MIN_EMOJI_QUERY: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryKind {
    /// `@`: people, and in a server roles, `@everyone` and `@online`.
    Mention,
    /// `#`: channels.
    Channel,
    /// `:`: emoji.
    Emoji,
}

/// What's being completed: the trigger and what follows it, up to the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub kind: QueryKind,
    /// The trigger and the query, as a byte range of the text (replaced
    /// when a suggestion is picked).
    pub range: Range<usize>,
    /// What's typed after the trigger.
    pub query: String,
}

/// What's being completed at byte offset `cursor` of `text`, if anything.
///
/// The trigger must start the text or follow whitespace or opening
/// punctuation (so `email@x` and `12:30` don't count), the cursor must be
/// at the end of the word, and it mustn't be in code. Emoji need
/// [`MIN_EMOJI_QUERY`] characters of shortcode after the `:`.
pub fn completion_query(text: &str, cursor: usize) -> Option<Query> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    // In the middle of a word: nothing to complete.
    if text[cursor..].chars().next().is_some_and(is_query_char) {
        return None;
    }
    let before = &text[..cursor];
    let (start, kind) = before.char_indices().rev().find_map(|(ix, c)| match c {
        '@' => Some(Some((ix, QueryKind::Mention))),
        '#' => Some(Some((ix, QueryKind::Channel))),
        ':' => Some(Some((ix, QueryKind::Emoji))),
        c if is_query_char(c) => None,
        _ => Some(None),
    })??;
    let opens = before[..start]
        .chars()
        .next_back()
        .is_none_or(|c| c.is_whitespace() || OPENING.contains(&c));
    if !opens || in_code(text, start) {
        return None;
    }
    let query = &before[start + 1..];
    if kind == QueryKind::Emoji
        && (query.chars().count() < MIN_EMOJI_QUERY || !query.chars().all(is_shortcode_char))
    {
        return None;
    }
    Some(Query {
        kind,
        range: start..cursor,
        query: query.to_string(),
    })
}

/// A `:shortcode:` just closed by the `:` before byte offset `cursor`:
/// its byte range (both colons) and the shortcode. Same rules as
/// [`completion_query`] for where it may start, and not in code.
pub fn closed_shortcode(text: &str, cursor: usize) -> Option<(Range<usize>, String)> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    let before = text[..cursor].strip_suffix(':')?;
    let start = before.rfind(':')?;
    let code = &before[start + 1..];
    if code.is_empty() || !code.chars().all(is_shortcode_char) {
        return None;
    }
    let opens = before[..start]
        .chars()
        .next_back()
        .is_none_or(|c| c.is_whitespace() || OPENING.contains(&c));
    (opens && !in_code(text, start)).then(|| (start..cursor, code.to_string()))
}

/// Punctuation a trigger may follow.
const OPENING: [char; 7] = ['(', '[', '{', '"', '\'', '“', '‘'];

/// Part of what's typed after a trigger: anything but whitespace, another
/// trigger, or the brackets of a mention's wire text.
fn is_query_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '@' | '#' | ':' | '<' | '>' | '`')
}

/// Part of an emoji shortcode (`+1`, `thumbs_up`, `t-rex`).
fn is_shortcode_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '+' | '-')
}

/// Whether byte offset `pos` of `text` is inside a fenced code block or
/// inline code.
fn in_code(text: &str, pos: usize) -> bool {
    let mut fenced = false;
    let mut line_start = 0;
    for line in text[..pos].split_inclusive('\n') {
        if line.ends_with('\n') {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fenced = !fenced;
            }
            line_start += line.len();
        }
    }
    if fenced {
        return true;
    }
    // An odd number of backticks before it on its line: inside `code`.
    text[line_start..pos].matches('`').count() % 2 == 1
}

/// Someone who can be mentioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    /// What they're called here: their nickname, else their display name.
    pub name: String,
    /// Their display name, when a nickname is shown instead.
    pub display_name: Option<String>,
    pub username: String,
    pub avatar: Option<String>,
    /// Wrote in this conversation recently: ranked first.
    pub recent: bool,
}

/// The best `items` for `query`, at most [`MAX_SUGGESTIONS`]: those with a
/// name starting with it, then a word of a name starting with it, then a
/// name containing it (ignoring case). Within each, `recent` ones first,
/// then in the given order. An empty query matches everything.
pub fn rank<T: Clone>(
    query: &str,
    items: &[T],
    names: impl Fn(&T) -> Vec<&str>,
    recent: impl Fn(&T) -> bool,
) -> Vec<T> {
    let query = query.to_lowercase();
    let mut matches: Vec<(u8, bool, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(ix, item)| {
            let tier = names(item)
                .into_iter()
                .filter_map(|name| match_tier(&query, name))
                .min()?;
            Some((tier, !recent(item), ix))
        })
        .collect();
    matches.sort();
    matches
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, _, ix)| items[ix].clone())
        .collect()
}

/// People for `query`, ranked on their nickname, display name and username.
pub fn rank_people(query: &str, people: &[Candidate]) -> Vec<Candidate> {
    rank(
        query,
        people,
        |p| {
            let mut names = vec![p.name.as_str(), p.username.as_str()];
            names.extend(p.display_name.as_deref());
            names
        },
        |p| p.recent,
    )
}

/// A server role that can be mentioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleCandidate {
    pub id: String,
    pub name: String,
    /// Its colour as `#rrggbb`, if it has a solid one.
    pub colour: Option<String>,
}

/// Who `@` can suggest. `people`, `roles` and (with `everyone`) `@everyone`
/// and `@online`, ranked together on their names (see [`rank`]): on equal
/// matches people come first, then roles, then the two specials.
pub fn rank_mentions(
    query: &str,
    people: &[Candidate],
    roles: &[RoleCandidate],
    everyone: bool,
) -> Vec<Suggestion> {
    #[derive(Clone)]
    enum Mentionable<'a> {
        Person(&'a Candidate),
        Role(&'a RoleCandidate),
        Special(Suggestion, &'static str),
    }
    let specials = [
        (Suggestion::everyone(), "everyone"),
        (Suggestion::online(), "online"),
    ];
    let items: Vec<Mentionable> = people
        .iter()
        .map(Mentionable::Person)
        .chain(roles.iter().map(Mentionable::Role))
        .chain(
            specials
                .into_iter()
                .filter(|_| everyone)
                .map(|(suggestion, name)| Mentionable::Special(suggestion, name)),
        )
        .collect();
    rank(
        query,
        &items,
        |item| match item {
            Mentionable::Person(p) => {
                let mut names = vec![p.name.as_str(), p.username.as_str()];
                names.extend(p.display_name.as_deref());
                names
            }
            Mentionable::Role(role) => vec![role.name.as_str()],
            Mentionable::Special(_, name) => vec![*name],
        },
        |item| matches!(item, Mentionable::Person(p) if p.recent),
    )
    .into_iter()
    .map(|item| match item {
        Mentionable::Person(person) => Suggestion::person(person),
        Mentionable::Role(role) => Suggestion::role(role),
        Mentionable::Special(suggestion, _) => suggestion,
    })
    .collect()
}

/// 0: `name` starts with `query`; 1: one of its words does; 2: it contains
/// it. `query` is lowercase.
fn match_tier(query: &str, name: &str) -> Option<u8> {
    let name = name.to_lowercase();
    if name.starts_with(query) {
        Some(0)
    } else if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| !word.is_empty() && word.starts_with(query))
    {
        Some(1)
    } else if name.contains(query) {
        Some(2)
    } else {
        None
    }
}

/// A user, role, channel or custom emoji referred to in a message's text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TokenRef {
    /// `<@ID>`
    User(String),
    /// `<%ID>`
    Role(String),
    /// `<#ID>`
    Channel(String),
    /// `:ID:`
    Emoji(String),
}

impl TokenRef {
    /// How it's written in a message.
    pub fn wire_text(&self) -> String {
        match self {
            TokenRef::User(id) => format!("<@{id}>"),
            TokenRef::Role(id) => format!("<%{id}>"),
            TokenRef::Channel(id) => format!("<#{id}>"),
            TokenRef::Emoji(id) => format!(":{id}:"),
        }
    }

    /// The message box's ID for its chip.
    pub fn token_id(&self) -> String {
        match self {
            TokenRef::User(id) => format!("user:{id}"),
            TokenRef::Role(id) => format!("role:{id}"),
            TokenRef::Channel(id) => format!("channel:{id}"),
            TokenRef::Emoji(id) => format!("emoji:{id}"),
        }
    }

    /// Back from [`Self::token_id`].
    pub fn from_token_id(token_id: &str) -> Option<Self> {
        let (kind, id) = token_id.split_once(':')?;
        let id = id.to_string();
        match kind {
            "user" => Some(TokenRef::User(id)),
            "role" => Some(TokenRef::Role(id)),
            "channel" => Some(TokenRef::Channel(id)),
            "emoji" => Some(TokenRef::Emoji(id)),
            _ => None,
        }
    }
}

/// The mentions (of people and roles), channel links and custom emoji in
/// `text`, in order, as
/// byte ranges of their wire text. Only real IDs (ULIDs) count.
pub fn mention_spans(text: &str) -> Vec<(Range<usize>, TokenRef)> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut ix = 0;
    while ix < bytes.len() {
        let found = match bytes[ix] {
            b'<' if matches!(bytes.get(ix + 1), Some(b'@' | b'%' | b'#')) => {
                ulid_at(text, ix + 2)
                    .filter(|_| bytes.get(ix + 2 + ULID_LEN) == Some(&b'>'))
                    .map(|id| {
                        let token = match bytes[ix + 1] {
                            b'@' => TokenRef::User(id),
                            b'%' => TokenRef::Role(id),
                            _ => TokenRef::Channel(id),
                        };
                        (ix..ix + 3 + ULID_LEN, token)
                    })
            }
            b':' => ulid_at(text, ix + 1)
                .filter(|_| bytes.get(ix + 1 + ULID_LEN) == Some(&b':'))
                .map(|id| (ix..ix + 2 + ULID_LEN, TokenRef::Emoji(id))),
            _ => None,
        };
        match found {
            Some((range, token)) => {
                ix = range.end;
                spans.push((range, token));
            }
            None => ix += 1,
        }
    }
    spans
}

const ULID_LEN: usize = 26;

/// The ULID starting at byte `start` of `text`, if there is one.
fn ulid_at(text: &str, start: usize) -> Option<String> {
    let candidate = text.as_bytes().get(start..start + ULID_LEN)?;
    candidate
        .iter()
        .all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'H' | b'J' | b'K' | b'M' | b'N' | b'P'..=b'T' | b'V'..=b'Z'))
        .then(|| String::from_utf8_lossy(candidate).into_owned())
}

/// What picking a suggestion puts in the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Insertion {
    /// An atomic chip: `text` is sent, `label` is shown.
    Token { token: TokenRef, label: String },
    /// Plain text (a Unicode emoji, `@everyone`).
    Text(String),
}

/// One row of the suggestion list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// Tells rows apart.
    pub key: String,
    pub title: String,
    /// Shown muted after the title, e.g. `@username`.
    pub detail: Option<String>,
    /// A picture at the start of the row (an avatar, a custom emoji).
    pub image: Option<String>,
    /// Or a glyph there instead (a Unicode emoji).
    pub glyph: Option<String>,
    pub insert: Insertion,
}

impl Suggestion {
    /// `@name`, sent as `<@id>`.
    pub fn person(person: &Candidate) -> Self {
        Suggestion {
            key: format!("user:{}", person.id),
            title: person.name.clone(),
            detail: Some(format!("@{}", person.username)),
            image: person.avatar.clone(),
            glyph: None,
            insert: Insertion::Token {
                token: TokenRef::User(person.id.clone()),
                label: format!("@{}", person.name),
            },
        }
    }

    /// A role's `@name`, sent as `<%id>`.
    pub fn role(role: &RoleCandidate) -> Self {
        Suggestion {
            key: format!("role:{}", role.id),
            title: role.name.clone(),
            detail: Some("Role".into()),
            image: None,
            glyph: Some("@".into()),
            insert: Insertion::Token {
                token: TokenRef::Role(role.id.clone()),
                label: format!("@{}", role.name),
            },
        }
    }

    /// `@everyone`, sent as typed.
    pub fn everyone() -> Self {
        Self::special("everyone", "Everyone in the channel")
    }

    /// `@online`, sent as typed.
    pub fn online() -> Self {
        Self::special("online", "Everyone online in the channel")
    }

    fn special(name: &str, detail: &str) -> Self {
        Suggestion {
            key: name.to_string(),
            title: format!("@{name}"),
            detail: Some(detail.to_string()),
            image: None,
            glyph: Some("@".into()),
            insert: Insertion::Text(format!("@{name}")),
        }
    }

    /// `#name`, sent as `<#id>`.
    pub fn channel(id: &str, name: &str) -> Self {
        Suggestion {
            key: format!("channel:{id}"),
            title: name.to_string(),
            detail: None,
            image: None,
            glyph: Some("#".into()),
            insert: Insertion::Token {
                token: TokenRef::Channel(id.to_string()),
                label: format!("#{name}"),
            },
        }
    }
}
