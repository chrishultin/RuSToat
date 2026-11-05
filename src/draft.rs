//! The message box's rules, with no GPUI: what can be sent, the length
//! limit, and drafts kept per channel.

use std::collections::HashMap;

/// Longest message Stoat accepts, in characters (`limits.global.message_length`).
pub const MAX_MESSAGE_CHARS: usize = 2000;
/// From this many characters, the message box shows a count.
pub const SHOW_COUNT_FROM: usize = 1800;

/// Whether the message box's text can be sent as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendCheck {
    /// Send this: the text without leading blank lines or trailing whitespace.
    Ready(String),
    /// Nothing but whitespace: nothing to send.
    Empty,
    /// More than [`MAX_MESSAGE_CHARS`] characters (this many).
    TooLong(usize),
}

pub fn check_send(text: &str) -> SendCheck {
    let text = trim_message(text);
    if text.is_empty() {
        return SendCheck::Empty;
    }
    let chars = text.chars().count();
    if chars > MAX_MESSAGE_CHARS {
        return SendCheck::TooLong(chars);
    }
    SendCheck::Ready(text.to_string())
}

/// Trailing whitespace and leading blank lines go; leading spaces on the
/// first line stay (they may be indentation).
fn trim_message(text: &str) -> &str {
    let text = text.trim_end();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        if !line.trim().is_empty() {
            break;
        }
        start += line.len();
    }
    &text[start..]
}

/// The character count to show under the message box, if any: from
/// [`SHOW_COUNT_FROM`] characters. `true` when over the limit.
pub fn char_count(text: &str) -> Option<(usize, bool)> {
    let chars = trim_message(text).chars().count();
    (chars >= SHOW_COUNT_FROM).then_some((chars, chars > MAX_MESSAGE_CHARS))
}

/// Unsent text, kept per channel while the app runs.
#[derive(Debug, Default)]
pub struct Drafts {
    by_channel: HashMap<String, String>,
}

impl Drafts {
    /// Keep `text` as `channel`'s draft (forgetting it if it's blank).
    pub fn save(&mut self, channel: &str, text: &str) {
        if text.trim().is_empty() {
            self.by_channel.remove(channel);
        } else {
            self.by_channel.insert(channel.to_string(), text.to_string());
        }
    }

    /// Take `channel`'s draft out, to put back in the message box.
    pub fn take(&mut self, channel: &str) -> String {
        self.by_channel.remove(channel).unwrap_or_default()
    }
}

/// A new `Idempotency-Key` for a send: the time, a counter, and a random
/// part (Stoat's duplicate check is shared by every client, so time alone
/// isn't enough). Well under Stoat's 64-byte limit.
pub fn new_nonce(now_ms: u64) -> String {
    use std::hash::{BuildHasher, RandomState};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    // A fresh `RandomState` is randomly keyed: hashing anything gives noise.
    let random = RandomState::new().hash_one(count);
    format!("rustoat-{now_ms}-{count}-{random:016x}")
}
