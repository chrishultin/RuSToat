//! Stoat message text → standard Markdown for gpui-component's `TextView`.
//!
//! Stoat's dialect is CommonMark/GFM plus a few extras. `TextView` can't
//! express the extras itself (its plugins replace whole Markdown nodes, so
//! they can't pick out a mention in the middle of a sentence), so they're
//! rewritten into standard Markdown first:
//!
//! - `<@user>` → `[@Name](stoat:user/ID)` and `<#channel>` → `[#name](stoat:channel/ID)`.
//!   Names are escaped so a name like `*]evil[*` can't break the formatting.
//! - `||spoiler||` → an inline code span (a muted background). Hiding is deferred.
//!   A spoiler that contains inline code is left as typed (code is split out first).
//! - Any other `<` is escaped. `TextView` renders raw HTML, which Stoat doesn't,
//!   so `<b>` or `<not a mention>` must show as typed. Autolinks
//!   (`<https://…>`, `<mailto:…>`) are kept.
//!
//! Nothing is rewritten inside code (fenced blocks or inline spans).

/// Name lookups for mentions.
pub trait Names {
    fn user_name(&self, id: &str) -> Option<String>;
    fn channel_name(&self, id: &str) -> Option<String>;
}

/// Turn Stoat message content into Markdown for `TextView`.
pub fn prepare(content: &str, names: &impl Names) -> String {
    let mut out = String::with_capacity(content.len() + 16);
    for segment in segments(content) {
        match segment {
            Segment::Code(code) => out.push_str(code),
            Segment::Text(text) => rewrite_text(text, names, &mut out),
        }
    }
    out
}

/// The scheme of the links this module makes for mentions. Clicking one does nothing (yet).
pub const INTERNAL_SCHEME: &str = "stoat:";

/// Whether a clicked link should open in the browser.
pub fn is_external_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

#[derive(Debug, PartialEq, Eq)]
enum Segment<'a> {
    Text(&'a str),
    Code(&'a str),
}

/// Split into code (fenced blocks, inline spans) and everything else.
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

fn rewrite_text(text: &str, names: &impl Names, out: &mut String) {
    let mut rest = text;
    while !rest.is_empty() {
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
            .find(['<', '|'])
            .map_or(rest.len(), |i| i + first);
        out.push_str(&rest[..next]);
        rest = &rest[next..];
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

/// Mentions as plain `@Name` text, for places links can't go (spoilers).
fn plain_mentions(text: &str, names: &impl Names) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
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
    fn code_spans_fit_their_contents() {
        assert_eq!(code_span("plain"), "`plain`");
        assert_eq!(code_span("a`b"), "``a`b``");
        assert_eq!(code_span("`tick"), "`` `tick ``");
    }

    #[test]
    fn escape_covers_markdown_punctuation() {
        assert_eq!(escape("*]x[*"), "\\*\\]x\\[\\*");
        assert_eq!(escape("Ada L."), "Ada L\\.");
        assert_eq!(escape("日本"), "日本");
    }
}
