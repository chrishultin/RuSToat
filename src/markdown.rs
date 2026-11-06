//! Stoat message text → standard Markdown for gpui-component's `TextView`.
//!
//! Stoat's dialect is CommonMark/GFM plus a few extras. `TextView` can't
//! express the extras itself (its plugins replace whole Markdown nodes, so
//! they can't pick out a mention in the middle of a sentence), so they're
//! rewritten into standard Markdown first:
//!
//! - `<@user>` → `[@Name](stoat:user/ID)` and `<#channel>` → `[#name](stoat:channel/ID)`.
//!   Names are escaped so a name like `*]evil[*` can't break the formatting.
//! - `:ULID:` (custom emoji) → `<img src=… width=… height=…>`, generated here, so
//!   it's the only HTML that reaches `TextView` (every typed `<` is escaped).
//!   Markdown's own `![](…)` can't be sized and draws images at full size.
//!   Messages that are only emoji ("jumbo") get bigger ones.
//! - `||spoiler||` → an inline code span (a muted background). Hiding is deferred.
//!   A spoiler that contains inline code is left as typed (code is split out first).
//! - Any other `<` is escaped. `TextView` renders raw HTML, which Stoat doesn't,
//!   so `<b>` or `<not a mention>` must show as typed. Autolinks
//!   (`<https://…>`, `<mailto:…>`) are kept.
//! - Every line break is kept (Stoat uses `remark-breaks`; CommonMark, and so
//!   `TextView`, joins lines into one paragraph): two trailing spaces make it
//!   a hard break.
//! - A block quote ends at its last `>` line. Stoat inserts a blank line there,
//!   so the next line isn't pulled into the quote (CommonMark's "lazy
//!   continuation").
//!
//! Nothing is rewritten inside code (fenced blocks or inline spans). Inside
//! `$$…$$` math only `<` is escaped (`\<`, which [`crate::math`] reads back
//! as `<`); `ui::math` draws the formula.

/// Name lookups for mentions.
pub trait Names {
    fn user_name(&self, id: &str) -> Option<String>;
    fn channel_name(&self, id: &str) -> Option<String>;
    /// A custom emoji's name, if it's from one of the user's servers.
    fn emoji_name(&self, _id: &str) -> Option<String> {
        None
    }
    /// Where a custom emoji's image is. `None` leaves `:ID:` as typed.
    fn emoji_url(&self, _id: &str) -> Option<String> {
        None
    }
}

/// Custom emoji size inline with `text_sm` text (about 1.4× the font size).
pub const EMOJI_PX: u32 = 20;
/// Custom emoji size in an emoji-only ("jumbo") message.
pub const JUMBO_EMOJI_PX: u32 = 96;
/// Font size for Unicode emoji in a jumbo message, to roughly match
/// [`JUMBO_EMOJI_PX`] custom emoji.
pub const JUMBO_TEXT_PX: f32 = 60.;
/// Messages with more emoji than this aren't drawn jumbo.
pub const JUMBO_MAX: usize = 10;

/// Turn Stoat message content into Markdown for `TextView`.
pub fn prepare(content: &str, names: &impl Names) -> String {
    let emoji_px = if is_jumbo(content) {
        JUMBO_EMOJI_PX
    } else {
        EMOJI_PX
    };
    // One kind of line ending, so line breaks only need to look for `\n`.
    let content = if content.contains('\r') {
        std::borrow::Cow::Owned(content.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(content)
    };
    let mut out = String::with_capacity(content.len() + 16);
    let mut offset = 0;
    for segment in segments(&content) {
        match segment {
            Segment::Code(code) => out.push_str(code),
            Segment::Math(math) => out.push_str(&math.replace('<', "\\<")),
            Segment::Text(text) => {
                let text = Text {
                    content: &content,
                    offset,
                    text,
                };
                rewrite_text(text, names, emoji_px, &mut out);
            }
        }
        offset += segment.as_str().len();
    }
    out
}

/// Whether a message is nothing but 1 to [`JUMBO_MAX`] emoji (custom
/// `:ULID:` or Unicode) and whitespace.
pub fn is_jumbo(content: &str) -> bool {
    use unicode_properties::UnicodeEmoji;
    let mut count = 0;
    let mut rest = content;
    while let Some(c) = rest.chars().next() {
        if custom_emoji_id(rest).is_some() {
            count += 1;
            rest = &rest[CUSTOM_EMOJI_LEN..];
            continue;
        }
        if c.is_whitespace() {
            // fine
        } else if !c.is_ascii() && c.is_emoji_char_or_emoji_component() {
            // Joiners and variation selectors are parts of one emoji.
            if !c.is_emoji_component() {
                count += 1;
            }
        } else {
            return false;
        }
        rest = &rest[c.len_utf8()..];
    }
    (1..=JUMBO_MAX).contains(&count)
}

/// `:` + 26-character ID + `:`.
const CUSTOM_EMOJI_LEN: usize = 28;

/// The emoji ID if `text` starts with a custom emoji (`:ULID:`).
fn custom_emoji_id(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.len() < CUSTOM_EMOJI_LEN || bytes[0] != b':' || bytes[CUSTOM_EMOJI_LEN - 1] != b':' {
        return None;
    }
    // Checked as bytes first, so the slice below is on ASCII boundaries.
    let id_bytes = &bytes[1..CUSTOM_EMOJI_LEN - 1];
    if !id_bytes.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    let id = &text[1..CUSTOM_EMOJI_LEN - 1];
    crate::time::is_ulid(id).then_some(id)
}

/// The IDs of the custom emoji (`:ULID:`) in `content`, in order.
pub fn custom_emoji_ids(content: &str) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut rest = content;
    while let Some(at) = rest.find(':') {
        rest = &rest[at..];
        if let Some(id) = custom_emoji_id(rest) {
            ids.push(id);
            rest = &rest[CUSTOM_EMOJI_LEN..];
        } else {
            rest = &rest[1..];
        }
    }
    ids
}

/// A sized `<img>` for a custom emoji, or `None` to leave `:ID:` as typed.
fn emoji_img(id: &str, names: &impl Names, px: u32) -> Option<String> {
    let url = names.emoji_url(id)?;
    let name = names.emoji_name(id).unwrap_or_else(|| "emoji".into());
    Some(format!(
        r#"<img src="{}" alt=":{}:" title=":{}:" width="{px}" height="{px}">"#,
        html_attr(&url),
        html_attr(&name),
        html_attr(&name),
    ))
}

/// Escape a value for a double-quoted HTML attribute.
fn html_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The scheme of the links this module makes for mentions (and the store
/// makes for people in system messages). See [`parse_internal_link`].
pub const INTERNAL_SCHEME: &str = "stoat:";

/// What an internal `stoat:` link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalLink {
    /// `stoat:user/{id}`: a person (a mention).
    User(String),
    /// `stoat:channel/{id}`: a channel.
    Channel(String),
}

/// The person or channel a clicked `stoat:` link points at. `None` for
/// external links and anything malformed.
pub fn parse_internal_link(url: &str) -> Option<InternalLink> {
    let rest = url.strip_prefix(INTERNAL_SCHEME)?;
    let (kind, id) = rest.split_once('/')?;
    if id.is_empty() || id.contains(['/', '?', '#']) {
        return None;
    }
    match kind {
        "user" => Some(InternalLink::User(id.to_string())),
        "channel" => Some(InternalLink::Channel(id.to_string())),
        _ => None,
    }
}

/// Whether a clicked link should open in the browser.
pub fn is_external_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

#[derive(Debug, PartialEq, Eq)]
enum Segment<'a> {
    Text(&'a str),
    Code(&'a str),
    /// `$$…$$` on one line, delimiters included.
    Math(&'a str),
}

impl<'a> Segment<'a> {
    fn as_str(&self) -> &'a str {
        match self {
            Segment::Text(s) | Segment::Code(s) | Segment::Math(s) => s,
        }
    }
}

/// Split into code (fenced blocks, inline spans), math (`$$…$$` within a
/// line) and everything else.
fn segments(content: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut text_start = 0;
    let mut pos = 0;
    let bytes = content.as_bytes();

    while pos < content.len() {
        let at_line_start = pos == 0 || bytes[pos - 1] == b'\n';
        if at_line_start && let Some(end) = fenced_block_end(content, pos) {
            if text_start < pos {
                out.push(Segment::Text(&content[text_start..pos]));
            }
            out.push(Segment::Code(&content[pos..end]));
            pos = end;
            text_start = end;
            continue;
        }
        if bytes[pos] == b'`' {
            let run = count_run(bytes, pos, b'`');
            if let Some(close) = find_closing_backticks(content, pos + run, run) {
                if text_start < pos {
                    out.push(Segment::Text(&content[text_start..pos]));
                }
                let end = close + run;
                out.push(Segment::Code(&content[pos..end]));
                pos = end;
                text_start = end;
            } else {
                // No matching run: these backticks are just text.
                pos += run;
            }
            continue;
        }
        if bytes[pos] == b'$' {
            let run = count_run(bytes, pos, b'$');
            if let Some(end) = math_span_end(content, pos, run) {
                if text_start < pos {
                    out.push(Segment::Text(&content[text_start..pos]));
                }
                out.push(Segment::Math(&content[pos..end]));
                pos = end;
                text_start = end;
            } else {
                pos += run;
            }
            continue;
        }
        pos += content[pos..].chars().next().map_or(1, char::len_utf8);
    }
    if text_start < content.len() {
        out.push(Segment::Text(&content[text_start..]));
    }
    out
}

fn count_run(bytes: &[u8], from: usize, byte: u8) -> usize {
    bytes[from..].iter().take_while(|&&b| b == byte).count()
}

/// The end of a `$$…$$` span opened by the `run` dollars at `start`: the
/// next run of exactly as many on the same line, with something between.
/// One `$` isn't math in Stoat, and an escaped `\$` doesn't open anything.
fn math_span_end(content: &str, start: usize, run: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let backslashes = bytes[..start]
        .iter()
        .rev()
        .take_while(|&&b| b == b'\\')
        .count();
    if run < 2 || backslashes % 2 == 1 {
        return None;
    }
    let from = start + run;
    let line_end = content[from..]
        .find('\n')
        .map_or(content.len(), |i| from + i);
    let mut pos = from;
    while pos < line_end {
        if bytes[pos] == b'$' {
            let len = count_run(bytes, pos, b'$');
            if len == run {
                return (pos > from).then_some(pos + run);
            }
            pos += len;
        } else {
            pos += 1;
        }
    }
    None
}

/// A fenced code block starting at `start` (a line start): its end, including
/// the closing fence line, or the end of the content if it's never closed.
fn fenced_block_end(content: &str, start: usize) -> Option<usize> {
    let line_end = |from: usize| {
        content[from..]
            .find('\n')
            .map_or(content.len(), |i| from + i + 1)
    };
    let line = &content[start..line_end(start)];
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let fence_char = line[indent..].bytes().next()?;
    if fence_char != b'`' && fence_char != b'~' {
        return None;
    }
    let fence_len = count_run(line.as_bytes(), indent, fence_char);
    if fence_len < 3 {
        return None;
    }

    let mut pos = line_end(start);
    while pos < content.len() {
        let next = line_end(pos);
        let candidate = content[pos..next].trim_start_matches(' ');
        if count_run(candidate.as_bytes(), 0, fence_char) >= fence_len
            && candidate
                .trim_start_matches(fence_char as char)
                .trim()
                .is_empty()
        {
            return Some(next);
        }
        pos = next;
    }
    Some(content.len())
}

/// The start of the next run of exactly `run` backticks at or after `from`.
fn find_closing_backticks(content: &str, from: usize, run: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut pos = from;
    while pos < bytes.len() {
        if bytes[pos] == b'`' {
            let len = count_run(bytes, pos, b'`');
            if len == run {
                return Some(pos);
            }
            pos += len;
        } else {
            pos += 1;
        }
    }
    None
}

/// A text segment, and where it is in the whole message (line breaks look
/// at the lines either side, which can be in other segments).
struct Text<'a> {
    content: &'a str,
    offset: usize,
    text: &'a str,
}

fn rewrite_text(segment: Text<'_>, names: &impl Names, emoji_px: u32, out: &mut String) {
    let text = segment.text;
    let mut rest = text;
    while !rest.is_empty() {
        if rest.starts_with('\n') {
            let at = segment.offset + (text.len() - rest.len());
            out.push_str(line_break(segment.content, at));
            rest = &rest[1..];
            continue;
        }
        if let Some(id) = custom_emoji_id(rest)
            && let Some(img) = emoji_img(id, names, emoji_px)
        {
            out.push_str(&img);
            rest = &rest[CUSTOM_EMOJI_LEN..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("||")
            && let Some(end) = after.find("||")
            && end > 0
            && !after[..end].contains('\n')
        {
            out.push_str(&code_span(&plain_mentions(&after[..end], names)));
            rest = &after[end + 2..];
            continue;
        }
        if rest.starts_with('<') {
            let (replacement, consumed) = angle_bracket(rest, names);
            out.push_str(&replacement);
            rest = &rest[consumed..];
            continue;
        }
        // Skip at least one whole character: `rest` may start with a
        // multi-byte one (emoji, CJK), and slicing mid-character panics.
        let first = rest.chars().next().map_or(1, char::len_utf8);
        let next = rest[first..]
            .find(['<', '|', ':', '\n'])
            .map_or(rest.len(), |i| i + first);
        out.push_str(&rest[..next]);
        rest = &rest[next..];
    }
}

/// What the `\n` at `at` in `content` becomes. Stoat keeps every line break,
/// so a line followed by another becomes a hard break (two trailing spaces),
/// and a quote's last `>` line gets a blank line after it.
fn line_break(content: &str, at: usize) -> &'static str {
    let line = &content[content[..at].rfind('\n').map_or(0, |i| i + 1)..at];
    let rest = &content[at + 1..];
    let next = &rest[..rest.find('\n').unwrap_or(rest.len())];
    let is_quote = |line: &str| line.trim_start().starts_with('>');
    if line.trim().is_empty() || next.trim().is_empty() {
        // Already a paragraph break.
        "\n"
    } else if is_quote(line) && !is_quote(next) {
        "\n\n"
    } else if line.ends_with("  ") || (line.len() - line.trim_end_matches('\\').len()) % 2 == 1 {
        // Already a hard break (two spaces, or an unescaped `\`).
        "\n"
    } else {
        "  \n"
    }
}

/// `<…>` at the start of `text`: a mention, an autolink, or a literal `<`.
fn angle_bracket(text: &str, names: &impl Names) -> (String, usize) {
    // A mention or autolink has no spaces, newlines or nested `<` inside.
    let close = text
        .find('>')
        .filter(|&i| !text[1..i].contains(['\n', ' ', '<']));
    if let Some(close) = close {
        let inner = &text[1..close];
        if let Some(id) = inner.strip_prefix('@')
            && let Some(name) = names.user_name(id)
        {
            return (
                format!("[@{}]({INTERNAL_SCHEME}user/{id})", escape(&name)),
                close + 1,
            );
        }
        if let Some(id) = inner.strip_prefix('#')
            && let Some(name) = names.channel_name(id)
        {
            return (
                format!("[#{}]({INTERNAL_SCHEME}channel/{id})", escape(&name)),
                close + 1,
            );
        }
        if is_autolink(inner) {
            return (text[..=close].to_string(), close + 1);
        }
    }
    ("\\<".to_string(), 1)
}

fn is_autolink(inner: &str) -> bool {
    !inner.contains(char::is_whitespace) && is_external_link(inner)
}

/// Message text as plain text (notification bodies): mentions as `@Name`,
/// custom emoji as `:name:`. Markdown punctuation is left as typed.
pub fn plain_text(content: &str, names: &impl Names) -> String {
    plain_mentions(content, names)
}

/// A one-line plain-text preview of message content (reply previews): the
/// first line with text in it, Markdown markers removed, mentions as
/// `@Name`, and at most `max_chars` characters (cut with "…").
pub fn snippet(content: &str, names: &impl Names, max_chars: usize) -> String {
    let line = content
        .lines()
        .map(str::trim)
        // Code fences alone say nothing; the code under them does.
        .find(|line| !line.is_empty() && !line.starts_with("```"))
        .unwrap_or_default();
    let text = plain_text(&strip_markers(strip_line_prefix(line)), names);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.char_indices().nth(max_chars) {
        Some((cut, _)) => format!("{}…", text[..cut].trim_end()),
        None => text,
    }
}

/// Block markers at the start of a line: headings, quotes, list bullets.
fn strip_line_prefix(mut line: &str) -> &str {
    loop {
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix('>') {
            rest
        } else if trimmed.starts_with('#') {
            let rest = trimmed.trim_start_matches('#');
            if !rest.starts_with(' ') {
                return trimmed;
            }
            rest
        } else if let Some(rest) = ["- ", "* ", "+ "]
            .iter()
            .find_map(|bullet| trimmed.strip_prefix(bullet))
        {
            rest
        } else {
            let digits = trimmed.len() - trimmed.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            match trimmed[digits..].strip_prefix(". ") {
                Some(rest) if digits > 0 => rest,
                _ => return trimmed,
            }
        };
        line = rest;
    }
}

/// Inline Markdown markers removed: emphasis, strikethrough, spoilers, code
/// ticks, `$$` math, and links (`[text](url)` → `text`). A lone `_` stays,
/// so `snake_case` survives.
fn strip_markers(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if c == '['
            && let Some(close) = rest.find("](")
            && let Some(end) = rest[close..].find(')')
        {
            out.push_str(&rest[1..close]);
            rest = &rest[close + end + 1..];
            continue;
        }
        if let Some(marker) = ["**", "__", "~~", "||", "$$"]
            .iter()
            .find(|marker| rest.starts_with(**marker))
        {
            rest = &rest[marker.len()..];
            continue;
        }
        if !matches!(c, '*' | '`') {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Mentions as plain `@Name` text, for places links can't go (spoilers).
fn plain_mentions(text: &str, names: &impl Names) -> String {
    let text = plain_emoji(text, names);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let replaced = tail.find('>').and_then(|end| {
            let inner = &tail[1..end];
            let name = match (inner.strip_prefix('@'), inner.strip_prefix('#')) {
                (Some(user), _) => format!("@{}", names.user_name(user)?),
                (_, Some(channel)) => format!("#{}", names.channel_name(channel)?),
                _ => return None,
            };
            Some((name, end + 1))
        });
        match replaced {
            Some((name, consumed)) => {
                out.push_str(&name);
                rest = &tail[consumed..];
            }
            None => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Custom emoji as `:name:` text, for places images can't go (spoilers).
fn plain_emoji(text: &str, names: &impl Names) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(id) = custom_emoji_id(rest)
            && let Some(name) = names.emoji_name(id)
        {
            out.push(':');
            out.push_str(&name);
            out.push(':');
            rest = &rest[CUSTOM_EMOJI_LEN..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// A CommonMark code span around `text`, with a fence longer than any
/// backtick run inside it.
fn code_span(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/// Backslash-escape every ASCII punctuation character (always valid in CommonMark).
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_split_code_from_text() {
        assert_eq!(
            segments("a `b` c"),
            vec![
                Segment::Text("a "),
                Segment::Code("`b`"),
                Segment::Text(" c")
            ]
        );
        assert_eq!(
            segments("x ``a`b`` y"),
            vec![
                Segment::Text("x "),
                Segment::Code("``a`b``"),
                Segment::Text(" y")
            ]
        );
        // An unmatched backtick is text.
        assert_eq!(segments("it`s"), vec![Segment::Text("it`s")]);
        assert_eq!(
            segments("hi\n```rust\nlet x = <@u>;\n```\nbye"),
            vec![
                Segment::Text("hi\n"),
                Segment::Code("```rust\nlet x = <@u>;\n```\n"),
                Segment::Text("bye")
            ]
        );
        // An unclosed fence runs to the end.
        assert_eq!(segments("```\nopen"), vec![Segment::Code("```\nopen")]);
    }

    #[test]
    fn segments_split_double_dollar_math_within_a_line() {
        assert_eq!(
            segments("a $$x<y$$ b"),
            vec![
                Segment::Text("a "),
                Segment::Math("$$x<y$$"),
                Segment::Text(" b")
            ]
        );
        assert_eq!(segments("$1 and $2"), vec![Segment::Text("$1 and $2")]);
        assert_eq!(segments("$$a\nb$$"), vec![Segment::Text("$$a\nb$$")]);
        assert_eq!(segments("\\$$a$$"), vec![Segment::Text("\\$$a$$")]);
        // An escaped backslash doesn't escape the dollars.
        assert_eq!(
            segments("\\\\$$a$$"),
            vec![Segment::Text("\\\\"), Segment::Math("$$a$$")]
        );
    }

    #[test]
    fn code_spans_fit_their_contents() {
        assert_eq!(code_span("plain"), "`plain`");
        assert_eq!(code_span("a`b"), "``a`b``");
        assert_eq!(code_span("`tick"), "`` `tick ``");
    }

    #[test]
    fn internal_links_parse_to_people_and_channels() {
        assert_eq!(
            parse_internal_link("stoat:user/01ABC"),
            Some(InternalLink::User("01ABC".into()))
        );
        assert_eq!(
            parse_internal_link("stoat:channel/ch-random"),
            Some(InternalLink::Channel("ch-random".into()))
        );
        for url in [
            "https://stoat.chat/user/01ABC",
            "stoat:user/",
            "stoat:user",
            "stoat:server/s",
            "stoat:user/a/b",
        ] {
            assert_eq!(parse_internal_link(url), None, "{url}");
        }
    }

    #[test]
    fn escape_covers_markdown_punctuation() {
        assert_eq!(escape("*]x[*"), "\\*\\]x\\[\\*");
        assert_eq!(escape("Ada L."), "Ada L\\.");
        assert_eq!(escape("日本"), "日本");
    }
}
