//! Stoat message text → standard Markdown for gpui-component's `TextView`.
//!
//! Stoat's dialect is CommonMark/GFM plus a few extras. `TextView` can't
//! express the extras itself (its plugins replace whole Markdown nodes, so
//! they can't pick out a mention in the middle of a sentence), so they're
//! rewritten into standard Markdown first:
//!
//! - `<@user>` → `[@Name](stoat:user/ID)` and `<#channel>` → `[#name](stoat:channel/ID)`.
//!   Names are escaped so a name like `*]evil[*` can't break the formatting.
//! - `<%role>` → `[@Name](stoat:role/ID)`, or `stoat:role/ID/rrggbb` for a
//!   solid colour, which `ui::markup` draws in that colour. `@everyone` and
//!   `@online` (not part of a longer word) → `[@everyone](stoat:everyone)`
//!   and `[@online](stoat:online)`. Unknown roles stay as typed.
//! - `:ULID:` (custom emoji) → `<img src=… width=… height=…>`, generated here, so
//!   it's the only HTML that reaches `TextView` (every typed `<` is escaped).
//!   Markdown's own `![](…)` can't be sized and draws images at full size.
//!   Messages that are only emoji ("jumbo") get bigger ones.
//! - `<t:SECONDS>` / `<t:SECONDS:STYLE>` (a timestamp) →
//!   `[\<t\:SECONDS\>](stoat:time/SECONDS/STYLE)`, which `ui::markup` draws
//!   in local time. One chrono can't represent stays as typed.
//! - `||spoiler||`, numbered from 0 in the message: until revealed, each
//!   word (with the space after it) → `[word ](stoat:spoiler/N/W/C)` (word `W`
//!   of `C`), which `ui::markup` draws as a bar; once revealed, an inline code
//!   span (a muted background). Mentions and emoji inside are flattened to
//!   text. A spoiler that contains inline code is left as typed (code is
//!   split out first).
//! - Any other `<` is escaped. `TextView` renders raw HTML, which Stoat doesn't,
//!   so `<b>` or `<not a mention>` must show as typed. Autolinks
//!   (`<https://…>`, `<mailto:…>`) are kept.
//! - Every line break is kept (Stoat uses `remark-breaks`; CommonMark, and so
//!   `TextView`, joins lines into one paragraph): two trailing spaces make it
//!   a hard break.
//! - A link whose text isn't its address (`[docs](https://…)`) →
//!   `[docs](stoat:link?url=…&text=…)`, so the warning shown before it opens
//!   knows what the link said (see [`guard_links`]).
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
    /// A role's name, and its colour as `#rrggbb` if it's a solid one.
    /// Role IDs are unique across servers, so no server is needed.
    fn role(&self, _id: &str) -> Option<(String, Option<String>)> {
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

/// Turn Stoat message content into Markdown for `TextView`, with every
/// spoiler hidden.
pub fn prepare(content: &str, names: &impl Names) -> String {
    prepare_with(content, names, |_| false)
}

/// [`prepare`], with spoiler `n` (counting from 0 in the message) shown
/// when `revealed(n)`.
pub fn prepare_with(content: &str, names: &impl Names, revealed: impl Fn(usize) -> bool) -> String {
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
    let mut spoilers = Spoilers {
        next: 0,
        revealed: &revealed,
    };
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
                rewrite_text(text, names, emoji_px, &mut spoilers, &mut out);
            }
        }
        offset += segment.as_str().len();
    }
    // Only Markdown's own link syntax can give a link other text.
    if content.contains("](") || content.contains("]:") {
        guard_links(&out)
    } else {
        out
    }
}

/// `prepared` with each external link whose text isn't its address (see
/// [`crate::safety::shows_address`]) pointing at
/// `stoat:link?url=…&text=…` instead, with the same text. Inline and
/// reference links alike; autolinks and bare URLs show their address, so
/// they're left alone. A typed `stoat:link` is checked against its real
/// text in the same way, so its `text` can't be forged.
pub fn guard_links(prepared: &str) -> String {
    use markdown::mdast::Node;
    use std::collections::HashMap;
    use std::ops::Range;

    /// Each link to rewrite: its source span and what replaces it.
    fn visit(
        node: &Node,
        source: &str,
        definitions: &HashMap<String, &str>,
        edits: &mut Vec<(Range<usize>, String)>,
    ) {
        let url = match node {
            Node::Link(link) => Some(link.url.as_str()),
            Node::LinkReference(link) => definitions.get(&label(&link.identifier)).copied(),
            _ => None,
        };
        if let Some(url) = url {
            if let Some(edit) = guard(node, url, source) {
                edits.push(edit);
            }
            // Links can't hold links.
            return;
        }
        for child in node.children().into_iter().flatten() {
            visit(child, source, definitions, edits);
        }
    }

    /// The rewrite of link `node` to `url`, if it needs one.
    fn guard(node: &Node, url: &str, source: &str) -> Option<(Range<usize>, String)> {
        let (url, typed) = match parse_internal_link(url) {
            Some(InternalLink::External { url, .. }) => (url, true),
            _ if is_external_link(url) => (url.to_string(), false),
            _ => return None,
        };
        let text = node.to_string();
        if !typed && crate::safety::shows_address(&url, &text) {
            return None;
        }
        let span = node.position()?.start.offset..node.position()?.end.offset;
        let children = node.children()?;
        let first = children.first()?.position()?.start.offset;
        let last = children.last()?.position()?.end.offset;
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", &url)
            .append_pair("text", &text)
            .finish();
        let replacement = format!("[{}]({INTERNAL_SCHEME}link?{query})", &source[first..last]);
        Some((span, replacement))
    }

    /// Reference definitions, by [`label`].
    fn collect<'a>(node: &'a Node, definitions: &mut HashMap<String, &'a str>) {
        if let Node::Definition(definition) = node {
            // The first of several with one label wins.
            definitions
                .entry(label(&definition.identifier))
                .or_insert(definition.url.as_str());
        }
        for child in node.children().into_iter().flatten() {
            collect(child, definitions);
        }
    }

    /// A reference label as references match it: case and runs of
    /// whitespace don't matter.
    fn label(identifier: &str) -> String {
        identifier
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }

    let mut options = markdown::ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    let Ok(tree) = markdown::to_mdast(prepared, &options) else {
        return prepared.to_string();
    };
    let mut definitions = HashMap::new();
    collect(&tree, &mut definitions);
    let mut edits = Vec::new();
    visit(&tree, prepared, &definitions, &mut edits);
    // From the last to the first, so earlier spans stay where they were.
    let mut out = prepared.to_string();
    for (span, replacement) in edits.into_iter().rev() {
        out.replace_range(span, &replacement);
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
    /// `stoat:time/{seconds}` or `stoat:time/{seconds}/{style}`: a
    /// `<t:…>` timestamp (see [`crate::time::format_timestamp`]).
    Time { seconds: i64, style: Option<char> },
    /// `stoat:role/{id}` or `stoat:role/{id}/{rrggbb}`: a role mention, in
    /// the role's colour when it has a solid one.
    Role { id: String, colour: Option<String> },
    /// `stoat:everyone`: an `@everyone` mention.
    Everyone,
    /// `stoat:online`: an `@online` mention.
    Online,
    /// `stoat:spoiler/{index}/{word}/{words}`: word `word` of the `words`
    /// in hidden spoiler `index` (see [`prepare_with`]).
    Spoiler {
        index: usize,
        word: usize,
        words: usize,
    },
    /// `stoat:link?url=…&text=…` (form-urlencoded): an external link whose
    /// text wasn't its address (see [`guard_links`]).
    External { url: String, text: String },
}

/// What a `stoat:` link (a mention, a timestamp) points at. `None` for
/// external links and anything malformed.
pub fn parse_internal_link(url: &str) -> Option<InternalLink> {
    let rest = url.strip_prefix(INTERNAL_SCHEME)?;
    if let Some(query) = rest.strip_prefix("link?") {
        let (mut url, mut text) = (None, None);
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            match &*key {
                "url" => url = Some(value.into_owned()),
                "text" => text = Some(value.into_owned()),
                _ => {}
            }
        }
        let url = url.filter(|url| is_external_link(url))?;
        return Some(InternalLink::External { url, text: text? });
    }
    match rest {
        "everyone" => return Some(InternalLink::Everyone),
        "online" => return Some(InternalLink::Online),
        _ => {}
    }
    let (kind, id) = rest.split_once('/')?;
    if kind == "role" {
        let (id, colour) = match id.split_once('/') {
            Some((id, colour)) => {
                let hex = colour.len() == 6 && colour.bytes().all(|b| b.is_ascii_hexdigit());
                (id, Some(hex.then(|| format!("#{colour}"))?))
            }
            None => (id, None),
        };
        return (!id.is_empty() && !id.contains(['?', '#'])).then(|| InternalLink::Role {
            id: id.to_string(),
            colour,
        });
    }
    if kind == "time" {
        let (seconds, style) = match id.split_once('/') {
            Some((seconds, style)) => (seconds, Some(timestamp_style(style)?)),
            None => (id, None),
        };
        return Some(InternalLink::Time {
            seconds: timestamp_seconds(seconds)?,
            style,
        });
    }
    if kind == "spoiler" {
        let mut numbers = id.split('/').map(|n| {
            (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .then(|| n.parse::<usize>().ok())
                .flatten()
        });
        let (index, word, words) = (numbers.next()??, numbers.next()??, numbers.next()??);
        return (numbers.next().is_none() && word < words).then_some(InternalLink::Spoiler {
            index,
            word,
            words,
        });
    }
    if id.is_empty() || id.contains(['/', '?', '#']) {
        return None;
    }
    match kind {
        "user" => Some(InternalLink::User(id.to_string())),
        "channel" => Some(InternalLink::Channel(id.to_string())),
        _ => None,
    }
}

/// The seconds and style of a timestamp's inside (`t:SECONDS` or
/// `t:SECONDS:STYLE`), as Stoat's web client reads them
/// (`<t:([0-9]+)(?::(\w))?>`). `None` if it isn't one, or if it's a time
/// chrono can't represent.
fn timestamp(inner: &str) -> Option<(i64, Option<char>)> {
    let rest = inner.strip_prefix("t:")?;
    let (seconds, style) = match rest.split_once(':') {
        Some((seconds, style)) => (seconds, Some(timestamp_style(style)?)),
        None => (rest, None),
    };
    Some((timestamp_seconds(seconds)?, style))
}

/// Digits that make a time chrono can represent.
fn timestamp_seconds(digits: &str) -> Option<i64> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds = digits.parse().ok()?;
    chrono::DateTime::from_timestamp(seconds, 0)?;
    Some(seconds)
}

/// A single word character (`\w`): a timestamp's style.
fn timestamp_style(style: &str) -> Option<char> {
    let mut chars = style.chars();
    let c = chars.next()?;
    (chars.next().is_none() && (c.is_ascii_alphanumeric() || c == '_')).then_some(c)
}

/// A timestamp as plain text, where it can't update (notifications, reply
/// previews): relative ones show the date and time instead.
fn plain_timestamp(seconds: i64, style: Option<char>) -> Option<String> {
    let when = crate::time::local_from_seconds(seconds)?;
    let style = style.filter(|&style| style != 'R');
    Some(crate::time::format_timestamp(
        &when,
        style,
        &chrono::Local::now(),
    ))
}

/// Whether Markdown made by [`prepare`] has a relative (`R`) timestamp,
/// which needs drawing again as time passes.
pub fn has_relative_time(prepared: &str) -> bool {
    let link = format!("]({INTERNAL_SCHEME}time/");
    prepared.match_indices(&link).any(|(at, _)| {
        let url = &prepared[at + 2..];
        url[..url.find(')').unwrap_or(url.len())].ends_with("/R")
    })
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

/// Numbers a message's spoilers and says which are revealed.
struct Spoilers<'a> {
    next: usize,
    revealed: &'a dyn Fn(usize) -> bool,
}

/// The spoiler at the start of `text` (`||inside||`, on one line, with
/// something inside): what's inside, and the rest after it.
fn spoiler(text: &str) -> Option<(&str, &str)> {
    let after = text.strip_prefix("||")?;
    let end = after.find("||")?;
    (end > 0 && !after[..end].contains('\n')).then(|| (&after[..end], &after[end + 2..]))
}

/// Spoiler `index` (`text`, already plain), hidden: each word with the
/// whitespace after it is its own link, so the bar wraps between words.
fn hidden_spoiler(text: &str, index: usize) -> String {
    // Each piece runs from the start of one word to the start of the next;
    // leading whitespace goes with the first.
    let mut starts = vec![0];
    let (mut seen_word, mut after_space) = (false, false);
    for (at, c) in text.char_indices() {
        if !c.is_whitespace() {
            if seen_word && after_space {
                starts.push(at);
            }
            seen_word = true;
        }
        after_space = c.is_whitespace();
    }
    let words = starts.len();
    let mut out = String::new();
    for (word, &start) in starts.iter().enumerate() {
        let end = starts.get(word + 1).copied().unwrap_or(text.len());
        out.push_str(&format!(
            "[{}]({INTERNAL_SCHEME}spoiler/{index}/{word}/{words})",
            escape(&text[start..end])
        ));
    }
    out
}

fn rewrite_text(
    segment: Text<'_>,
    names: &impl Names,
    emoji_px: u32,
    spoilers: &mut Spoilers<'_>,
    out: &mut String,
) {
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
        if let Some((inside, after)) = spoiler(rest) {
            let index = spoilers.next;
            spoilers.next += 1;
            let text = plain_mentions(inside, names);
            if (spoilers.revealed)(index) {
                out.push_str(&code_span(&text));
            } else {
                out.push_str(&hidden_spoiler(&text, index));
            }
            rest = after;
            continue;
        }
        if rest.starts_with('<') {
            let (replacement, consumed) = angle_bracket(rest, names);
            out.push_str(&replacement);
            rest = &rest[consumed..];
            continue;
        }
        let at = segment.offset + (text.len() - rest.len());
        if let Some((mention, len)) = everyone_or_online(segment.content, at) {
            out.push_str(mention);
            rest = &rest[len..];
            continue;
        }
        // Skip at least one whole character: `rest` may start with a
        // multi-byte one (emoji, CJK), and slicing mid-character panics.
        let first = rest.chars().next().map_or(1, char::len_utf8);
        let next = rest[first..]
            .find(['<', '|', ':', '@', '\n'])
            .map_or(rest.len(), |i| i + first);
        out.push_str(&rest[..next]);
        rest = &rest[next..];
    }
}

/// `@everyone` or `@online` at `at` in `content`, as a whole word (as the
/// start of `foo@everyone.com` it isn't): its link, and its length.
fn everyone_or_online(content: &str, at: usize) -> Option<(&'static str, usize)> {
    let rest = &content[at..];
    let (word, link) = [
        ("@everyone", "[@everyone](stoat:everyone)"),
        ("@online", "[@online](stoat:online)"),
    ]
    .into_iter()
    .find(|(word, _)| rest.starts_with(word))?;
    let before = content[..at].chars().next_back();
    let after = rest[word.len()..].chars().next();
    let joined = |c: Option<char>, underscore: bool| {
        c.is_some_and(|c| c.is_alphanumeric() || (underscore && c == '_'))
    };
    (!joined(before, false) && !joined(after, true)).then_some((link, word.len()))
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
        if let Some(id) = inner.strip_prefix('%')
            && let Some((name, colour)) = names.role(id)
        {
            let colour = colour
                .as_deref()
                .and_then(|c| c.strip_prefix('#'))
                .map_or(String::new(), |hex| {
                    format!("/{}", hex.to_ascii_lowercase())
                });
            return (
                format!("[@{}]({INTERNAL_SCHEME}role/{id}{colour})", escape(&name)),
                close + 1,
            );
        }
        if let Some((seconds, style)) = timestamp(inner) {
            let url = match style {
                Some(style) => format!("{INTERNAL_SCHEME}time/{seconds}/{style}"),
                None => format!("{INTERNAL_SCHEME}time/{seconds}"),
            };
            return (format!("[{}]({url})", escape(&text[..=close])), close + 1);
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
/// custom emoji as `:name:`, timestamps in local time, and spoilers as
/// `[spoiler]`, so they don't give anything away. Markdown punctuation is
/// left as typed.
pub fn plain_text(content: &str, names: &impl Names) -> String {
    plain_mentions(&hide_spoilers(content), names)
}

/// Every spoiler outside code as `[spoiler]`.
fn hide_spoilers(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for segment in segments(content) {
        let Segment::Text(text) = segment else {
            out.push_str(segment.as_str());
            continue;
        };
        let mut rest = text;
        while let Some(at) = rest.find("||") {
            out.push_str(&rest[..at]);
            match spoiler(&rest[at..]) {
                Some((_, after)) => {
                    out.push_str("[spoiler]");
                    rest = after;
                }
                None => {
                    out.push_str("||");
                    rest = &rest[at + 2..];
                }
            }
        }
        out.push_str(rest);
    }
    out
}

/// The pieces of `content` that show as text: outside code, math and
/// spoilers (where invite links make cards, see [`crate::invites`]).
pub fn visible_text_spans(content: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for segment in segments(content) {
        let Segment::Text(text) = segment else {
            continue;
        };
        let mut rest = text;
        let mut start = 0;
        while let Some(at) = rest[start..].find("||").map(|at| start + at) {
            match spoiler(&rest[at..]) {
                Some((_, after)) => {
                    out.push(&rest[..at]);
                    rest = after;
                    start = 0;
                }
                None => start = at + 2,
            }
        }
        out.push(rest);
    }
    out.retain(|span| !span.is_empty());
    out
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
    // Spoilers go before the markers, which would drop their `||`.
    let line = hide_spoilers(strip_line_prefix(line));
    let text = plain_text(&strip_markers(&line), names);
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

/// Mentions as plain `@Name` text and timestamps in local time, for places
/// links can't go (spoilers).
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
                _ if inner.starts_with('%') => format!("@{}", names.role(&inner[1..])?.0),
                _ => {
                    let (seconds, style) = timestamp(inner)?;
                    plain_timestamp(seconds, style)?
                }
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
            "stoat:time/",
            "stoat:time/abc",
            "stoat:time/12/",
            "stoat:time/12/RR",
            "stoat:time/12/R/x",
            "stoat:time/-12",
            "stoat:time/99999999999999999999",
        ] {
            assert_eq!(parse_internal_link(url), None, "{url}");
        }
    }

    #[test]
    fn internal_links_parse_to_timestamps() {
        assert_eq!(
            parse_internal_link("stoat:time/1762430000"),
            Some(InternalLink::Time {
                seconds: 1_762_430_000,
                style: None
            })
        );
        assert_eq!(
            parse_internal_link("stoat:time/1762430000/R"),
            Some(InternalLink::Time {
                seconds: 1_762_430_000,
                style: Some('R')
            })
        );
    }

    #[test]
    fn internal_links_parse_to_roles_and_everyone() {
        assert_eq!(
            parse_internal_link("stoat:role/01ROLE"),
            Some(InternalLink::Role {
                id: "01ROLE".into(),
                colour: None
            })
        );
        assert_eq!(
            parse_internal_link("stoat:role/01ROLE/ff8800"),
            Some(InternalLink::Role {
                id: "01ROLE".into(),
                colour: Some("#ff8800".into())
            })
        );
        assert_eq!(
            parse_internal_link("stoat:everyone"),
            Some(InternalLink::Everyone)
        );
        assert_eq!(
            parse_internal_link("stoat:online"),
            Some(InternalLink::Online)
        );
        for url in [
            "stoat:role/",
            "stoat:role/01ROLE/ff88",
            "stoat:role/01ROLE/gg8800",
            "stoat:role/01ROLE/ff8800/x",
            "stoat:everyone/x",
            "stoat:Everyone",
        ] {
            assert_eq!(parse_internal_link(url), None, "{url}");
        }
    }

    #[test]
    fn everyone_and_online_only_as_whole_words() {
        let at = |content: &str, at: usize| everyone_or_online(content, at).map(|(link, _)| link);
        assert_eq!(at("@everyone", 0), Some("[@everyone](stoat:everyone)"));
        assert_eq!(at("hi @online!", 3), Some("[@online](stoat:online)"));
        assert_eq!(at("(@everyone)", 1), Some("[@everyone](stoat:everyone)"));
        assert_eq!(at("foo@everyone.com", 3), None);
        assert_eq!(at("@everyones", 0), None);
        assert_eq!(at("@online_now", 0), None);
        assert_eq!(at("é@online", 2), None);
        assert_eq!(at("@every", 0), None);
    }

    #[test]
    fn internal_links_parse_to_spoiler_words() {
        assert_eq!(
            parse_internal_link("stoat:spoiler/2/0/3"),
            Some(InternalLink::Spoiler {
                index: 2,
                word: 0,
                words: 3
            })
        );
        for url in [
            "stoat:spoiler/",
            "stoat:spoiler/1",
            "stoat:spoiler/1/0",
            "stoat:spoiler/1/3/3",
            "stoat:spoiler/1/0/2/9",
            "stoat:spoiler/a/0/1",
            "stoat:spoiler/1//1",
        ] {
            assert_eq!(parse_internal_link(url), None, "{url}");
        }
    }

    #[test]
    fn hidden_spoilers_are_one_link_per_word() {
        assert_eq!(hidden_spoiler("he", 0), "[he](stoat:spoiler/0/0/1)");
        assert_eq!(
            hidden_spoiler("he wins", 1),
            "[he ](stoat:spoiler/1/0/2)[wins](stoat:spoiler/1/1/2)"
        );
        // Leading whitespace joins the first word; runs of it stay together.
        assert_eq!(
            hidden_spoiler(" a  b ", 0),
            "[ a  ](stoat:spoiler/0/0/2)[b ](stoat:spoiler/0/1/2)"
        );
        assert_eq!(hidden_spoiler("   ", 0), "[   ](stoat:spoiler/0/0/1)");
        assert_eq!(hidden_spoiler("a.b", 0), r"[a\.b](stoat:spoiler/0/0/1)");
    }

    #[test]
    fn spoilers_are_hidden_outside_code() {
        assert_eq!(hide_spoilers("a ||b|| c ||d||"), "a [spoiler] c [spoiler]");
        assert_eq!(hide_spoilers("`||b||` ||"), "`||b||` ||");
        assert_eq!(hide_spoilers("||||"), "||||");
    }

    #[test]
    fn relative_timestamps_are_found_in_prepared_markdown() {
        assert!(has_relative_time(r"in [\<t\:1\:R\>](stoat:time/1/R)!"));
        assert!(!has_relative_time(r"[\<t\:1\:F\>](stoat:time/1/F)"));
        assert!(!has_relative_time(r"[\<t\:1\>](stoat:time/1)"));
        assert!(!has_relative_time("plain /R) text"));
    }

    #[test]
    fn escape_covers_markdown_punctuation() {
        assert_eq!(escape("*]x[*"), "\\*\\]x\\[\\*");
        assert_eq!(escape("Ada L."), "Ada L\\.");
        assert_eq!(escape("日本"), "日本");
    }
}
