//! Typing indicators, with no GPUI: who's typing where (each lasting a few
//! seconds unless refreshed), when to tell Stoat the user is typing, and
//! the "Ada is typing…" line. Times are passed in, so it's testable.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Someone's "typing" lasts this long unless they send it again. Clients
/// resend while typing ([`RESEND_EVERY`]); this outlasts a resend.
pub const EXPIRE_AFTER: Duration = Duration::from_secs(5);
/// While the user types, `BeginTyping` goes out at most this often.
pub const RESEND_EVERY: Duration = Duration::from_secs(3);
/// The user counts as stopped after this long without typing.
pub const IDLE_AFTER: Duration = Duration::from_secs(5);

/// Others typing, per channel.
#[derive(Debug, Default)]
pub struct TypingState {
    me: String,
    /// Channel → (user, when it lapses), in the order they started.
    by_channel: HashMap<String, Vec<(String, Instant)>>,
}

impl TypingState {
    pub fn new(me: impl Into<String>) -> Self {
        Self {
            me: me.into(),
            by_channel: HashMap::new(),
        }
    }

    /// `user` is typing in `channel` (or still is). Ourselves are ignored:
    /// other sessions of ours aren't someone else typing.
    pub fn start(&mut self, channel: &str, user: &str, now: Instant) {
        if user == self.me {
            return;
        }
        let typists = self.by_channel.entry(channel.to_string()).or_default();
        let until = now + EXPIRE_AFTER;
        match typists.iter_mut().find(|(u, _)| u == user) {
            Some((_, expires)) => *expires = until,
            None => typists.push((user.to_string(), until)),
        }
    }

    /// `user` stopped typing (or sent their message). Returns true if they
    /// were shown.
    pub fn stop(&mut self, channel: &str, user: &str) -> bool {
        let Some(typists) = self.by_channel.get_mut(channel) else {
            return false;
        };
        let before = typists.len();
        typists.retain(|(u, _)| u != user);
        let stopped = typists.len() != before;
        if typists.is_empty() {
            self.by_channel.remove(channel);
        }
        stopped
    }

    /// Who's typing in `channel` now, in the order they started.
    pub fn typists(&self, channel: &str, now: Instant) -> Vec<&str> {
        self.by_channel
            .get(channel)
            .into_iter()
            .flatten()
            .filter(|(_, expires)| *expires > now)
            .map(|(user, _)| user.as_str())
            .collect()
    }

    /// When the next typist lapses, if anyone's typing: when to redraw.
    pub fn next_expiry(&self, now: Instant) -> Option<Instant> {
        self.by_channel
            .values()
            .flatten()
            .map(|(_, expires)| *expires)
            .filter(|expires| *expires > now)
            .min()
    }

    /// Forget lapsed typists.
    pub fn prune(&mut self, now: Instant) {
        self.by_channel.retain(|_, typists| {
            typists.retain(|(_, expires)| *expires > now);
            !typists.is_empty()
        });
    }

    /// Forget everyone (the connection dropped: their stops may be lost).
    pub fn clear(&mut self) {
        self.by_channel.clear();
    }
}

/// What to tell Stoat about the user's own typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypingSignal {
    Begin(String),
    End(String),
}

/// The user's own typing: when to send `BeginTyping` and `EndTyping`.
#[derive(Debug, Default)]
pub struct OutgoingTyping {
    active: Option<Active>,
}

#[derive(Debug)]
struct Active {
    channel: String,
    last_begin: Instant,
    last_input: Instant,
}

impl OutgoingTyping {
    /// The message box's text changed in `channel`: `empty` if there's
    /// nothing in it now.
    pub fn on_input(&mut self, channel: &str, empty: bool, now: Instant) -> Vec<TypingSignal> {
        let mut signals = Vec::new();
        if self.active.as_ref().is_some_and(|a| a.channel != channel) {
            signals.extend(self.stop());
        }
        if empty {
            signals.extend(self.stop());
            return signals;
        }
        match &mut self.active {
            Some(active) => {
                active.last_input = now;
                if now.duration_since(active.last_begin) >= RESEND_EVERY {
                    active.last_begin = now;
                    signals.push(TypingSignal::Begin(channel.to_string()));
                }
            }
            None => {
                self.active = Some(Active {
                    channel: channel.to_string(),
                    last_begin: now,
                    last_input: now,
                });
                signals.push(TypingSignal::Begin(channel.to_string()));
            }
        }
        signals
    }

    /// The user sent their message, switched channel, or started editing:
    /// they're not typing here any more.
    pub fn stop(&mut self) -> Option<TypingSignal> {
        self.active.take().map(|a| TypingSignal::End(a.channel))
    }

    /// Time passed: [`IDLE_AFTER`] without typing ends it.
    pub fn on_tick(&mut self, now: Instant) -> Option<TypingSignal> {
        let idle = self
            .active
            .as_ref()
            .is_some_and(|a| now.duration_since(a.last_input) >= IDLE_AFTER);
        if idle { self.stop() } else { None }
    }

    /// When [`Self::on_tick`] would next end it, if the user's typing.
    pub fn deadline(&self) -> Option<Instant> {
        self.active.as_ref().map(|a| a.last_input + IDLE_AFTER)
    }
}

/// The line above the message box: "Ada is typing…", "Ada and Bob are
/// typing…", "Several people are typing…". `None` when nobody is.
pub fn typing_label(names: &[String]) -> Option<String> {
    match names {
        [] => None,
        [one] => Some(format!("{one} is typing…")),
        [a, b] => Some(format!("{a} and {b} are typing…")),
        _ => Some("Several people are typing…".into()),
    }
}
