//! Slowmode cooldowns, with no GPUI: until when the user must wait before
//! sending again in each channel. Stoat says so with `UserSlowmodes` events
//! (after each send, and on every reconnect); a successful send or a
//! slowmode refusal starts one locally too, in case the event is late. Times
//! are passed in (milliseconds since the Unix epoch), so it's testable.

use std::collections::HashMap;

use crate::api::models::ChannelSlowmode;

/// The user's running cooldowns, per channel.
#[derive(Debug, Default)]
pub struct Cooldowns {
    /// Channel → when its cooldown ends.
    until: HashMap<String, u64>,
}

impl Cooldowns {
    /// What a `UserSlowmodes` event says, received at `now_ms`: each listed
    /// channel's cooldown ends `retry_after` seconds from now, replacing
    /// what was known for it. Channels not listed are left alone.
    pub fn apply(&mut self, slowmodes: &[ChannelSlowmode], now_ms: u64) {
        for slowmode in slowmodes {
            self.start(&slowmode.channel_id, slowmode.retry_after, now_ms);
        }
    }

    /// Wait `secs` seconds from `from_ms` before sending in `channel` again,
    /// replacing what was known for it. Zero seconds ends any cooldown.
    pub fn start(&mut self, channel: &str, secs: u64, from_ms: u64) {
        if secs == 0 {
            self.until.remove(channel);
        } else {
            self.until
                .insert(channel.to_string(), from_ms.saturating_add(secs * 1000));
        }
    }

    /// Whole seconds left (rounded up) before the user may send in `channel`
    /// at `now_ms`, or `None` if they may now.
    pub fn left(&self, channel: &str, now_ms: u64) -> Option<u64> {
        let until = *self.until.get(channel)?;
        (until > now_ms).then(|| (until - now_ms).div_ceil(1000))
    }

    /// Forget cooldowns that have ended by `now_ms`.
    pub fn prune(&mut self, now_ms: u64) {
        self.until.retain(|_, until| *until > now_ms);
    }

    /// When `channel`'s cooldown ends, if it's still running at `now_ms`:
    /// for a ticker to wake up as each second runs out.
    pub fn ends(&self, channel: &str, now_ms: u64) -> Option<u64> {
        self.until
            .get(channel)
            .copied()
            .filter(|until| *until > now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slowmode(channel: &str, duration: u64, retry_after: u64) -> ChannelSlowmode {
        ChannelSlowmode {
            channel_id: channel.into(),
            duration,
            retry_after,
        }
    }

    #[test]
    fn rounds_up_to_whole_seconds() {
        let mut cooldowns = Cooldowns::default();
        cooldowns.start("ch", 30, 1_000);
        assert_eq!(cooldowns.left("ch", 1_000), Some(30));
        assert_eq!(cooldowns.left("ch", 1_001), Some(30));
        assert_eq!(cooldowns.left("ch", 2_000), Some(29));
        assert_eq!(cooldowns.left("ch", 30_999), Some(1));
        assert_eq!(cooldowns.left("ch", 31_000), None);
    }

    #[test]
    fn zero_ends_a_cooldown() {
        let mut cooldowns = Cooldowns::default();
        cooldowns.start("ch", 30, 0);
        cooldowns.apply(&[slowmode("ch", 30, 0)], 5_000);
        assert_eq!(cooldowns.left("ch", 5_000), None);
    }

    #[test]
    fn prune_and_ends_ignore_ended_ones() {
        let mut cooldowns = Cooldowns::default();
        cooldowns.start("a", 10, 0);
        cooldowns.start("b", 20, 0);
        assert_eq!(cooldowns.ends("a", 0), Some(10_000));
        assert_eq!(cooldowns.ends("a", 10_000), None);
        assert_eq!(cooldowns.ends("b", 10_000), Some(20_000));
        assert_eq!(cooldowns.ends("c", 0), None);
        cooldowns.prune(10_000);
        assert_eq!(cooldowns.until.len(), 1);
    }
}
