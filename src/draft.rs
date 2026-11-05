//! The message box's rules, with no GPUI: what can be sent, the length
//! limit, replies, and drafts kept per channel.

use std::collections::HashMap;

use crate::api::models::ReplyIntent;
use crate::store::MAX_REPLIES;

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

/// A message the user is replying to, from the "Replying to" bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    pub id: String,
    /// Who wrote it, for the bar.
    pub author: String,
    /// Ping them (the bar's "@" toggle).
    pub mention: bool,
}

/// Replying to more messages than Stoat allows (`MAX_REPLIES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyReplies;

/// What's in the message box: text, and the messages it replies to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub text: String,
    pub replies: Vec<ReplyTarget>,
}

impl Draft {
    /// Nothing typed and nothing being replied to.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.replies.is_empty()
    }

    /// Reply to `target` as well. Replying to the same message twice
    /// changes nothing; past [`MAX_REPLIES`] it's refused.
    pub fn add_reply(&mut self, target: ReplyTarget) -> Result<(), TooManyReplies> {
        if self.replies.iter().any(|r| r.id == target.id) {
            return Ok(());
        }
        if self.replies.len() >= MAX_REPLIES {
            return Err(TooManyReplies);
        }
        self.replies.push(target);
        Ok(())
    }

    pub fn toggle_mention(&mut self, id: &str) {
        if let Some(reply) = self.replies.iter_mut().find(|r| r.id == id) {
            reply.mention = !reply.mention;
        }
    }

    pub fn remove_reply(&mut self, id: &str) {
        self.replies.retain(|r| r.id != id);
    }

    /// The replies as Stoat's send body takes them.
    pub fn reply_intents(&self) -> Vec<ReplyIntent> {
        self.replies
            .iter()
            .map(|r| ReplyIntent {
                id: r.id.clone(),
                mention: r.mention,
                fail_if_not_exists: None,
            })
            .collect()
    }
}

/// Unsent drafts, kept per channel while the app runs.
#[derive(Debug, Default)]
pub struct Drafts {
    by_channel: HashMap<String, Draft>,
}

impl Drafts {
    /// Keep `draft` as `channel`'s (forgetting it if it's empty).
    pub fn save(&mut self, channel: &str, draft: Draft) {
        if draft.is_empty() {
            self.by_channel.remove(channel);
        } else {
            self.by_channel.insert(channel.to_string(), draft);
        }
    }

    /// Take `channel`'s draft out, to put back in the message box.
    pub fn take(&mut self, channel: &str) -> Draft {
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
