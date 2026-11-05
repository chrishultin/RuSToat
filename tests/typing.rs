//! Typing indicators: others' typing with expiry, our own Begin/End
//! signals, and the label.

use std::time::{Duration, Instant};

use stoat_gpui::typing::{
    EXPIRE_AFTER, IDLE_AFTER, OutgoingTyping, RESEND_EVERY, TypingSignal, TypingState,
    typing_label,
};

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn start_and_stop() {
    let t0 = Instant::now();
    let mut typing = TypingState::new("u-me");
    typing.start("ch", "u-ada", t0);
    typing.start("ch", "u-bob", t0 + secs(1));
    typing.start("other", "u-cleo", t0);
    assert_eq!(typing.typists("ch", t0 + secs(1)), vec!["u-ada", "u-bob"]);

    assert!(typing.stop("ch", "u-ada"));
    assert!(!typing.stop("ch", "u-ada"), "already stopped");
    assert_eq!(typing.typists("ch", t0 + secs(1)), vec!["u-bob"]);
    assert_eq!(typing.typists("other", t0), vec!["u-cleo"]);
}

#[test]
fn typists_lapse_without_a_refresh() {
    let t0 = Instant::now();
    let mut typing = TypingState::new("u-me");
    typing.start("ch", "u-ada", t0);
    assert_eq!(typing.next_expiry(t0), Some(t0 + EXPIRE_AFTER));
    assert_eq!(typing.typists("ch", t0 + secs(4)), vec!["u-ada"]);
    assert!(typing.typists("ch", t0 + EXPIRE_AFTER).is_empty());

    // A resend keeps them going, and keeps their place.
    typing.start("ch", "u-ada", t0);
    typing.start("ch", "u-bob", t0 + secs(1));
    typing.start("ch", "u-ada", t0 + secs(3));
    assert_eq!(typing.typists("ch", t0 + secs(7)), vec!["u-ada"]);
    assert_eq!(typing.next_expiry(t0 + secs(1)), Some(t0 + secs(6)));

    typing.prune(t0 + secs(20));
    assert_eq!(typing.next_expiry(t0), None);
}

#[test]
fn our_own_typing_is_ignored() {
    let t0 = Instant::now();
    let mut typing = TypingState::new("u-me");
    typing.start("ch", "u-me", t0);
    assert!(typing.typists("ch", t0).is_empty());
}

#[test]
fn clearing_forgets_everyone() {
    let t0 = Instant::now();
    let mut typing = TypingState::new("u-me");
    typing.start("ch", "u-ada", t0);
    typing.clear();
    assert!(typing.typists("ch", t0).is_empty());
}

#[test]
fn labels_for_one_two_and_several() {
    let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(typing_label(&[]), None);
    assert_eq!(typing_label(&names(&["Ada"])).unwrap(), "Ada is typing…");
    assert_eq!(
        typing_label(&names(&["Ada", "Bob"])).unwrap(),
        "Ada and Bob are typing…"
    );
    assert_eq!(
        typing_label(&names(&["Ada", "Bob", "Cleo"])).unwrap(),
        "Several people are typing…"
    );
}

fn begin(ch: &str) -> TypingSignal {
    TypingSignal::Begin(ch.into())
}

fn end(ch: &str) -> TypingSignal {
    TypingSignal::End(ch.into())
}

#[test]
fn begin_goes_out_at_most_every_three_seconds() {
    let t0 = Instant::now();
    let mut out = OutgoingTyping::default();
    assert_eq!(out.on_input("ch", false, t0), vec![begin("ch")]);
    assert!(out.on_input("ch", false, t0 + secs(1)).is_empty());
    assert!(out.on_input("ch", false, t0 + secs(2)).is_empty());
    assert_eq!(out.on_input("ch", false, t0 + RESEND_EVERY), vec![begin("ch")]);
    assert!(out.on_input("ch", false, t0 + secs(4)).is_empty());
}

#[test]
fn end_on_send_clear_switch_and_idle() {
    let t0 = Instant::now();
    let mut out = OutgoingTyping::default();
    // Sent.
    out.on_input("ch", false, t0);
    assert_eq!(out.stop(), Some(end("ch")));
    assert_eq!(out.stop(), None, "only once");

    // Cleared.
    out.on_input("ch", false, t0);
    assert_eq!(out.on_input("ch", true, t0 + secs(1)), vec![end("ch")]);
    assert!(out.on_input("ch", true, t0 + secs(2)).is_empty());

    // Typing in another channel: End there, Begin here.
    out.on_input("ch", false, t0);
    assert_eq!(
        out.on_input("other", false, t0 + secs(1)),
        vec![end("ch"), begin("other")]
    );

    // Idle: nothing typed for 5 s.
    let t1 = t0 + secs(10);
    out.stop();
    out.on_input("ch", false, t1);
    out.on_input("ch", false, t1 + secs(2));
    assert_eq!(out.deadline(), Some(t1 + secs(2) + IDLE_AFTER));
    assert_eq!(out.on_tick(t1 + secs(6)), None);
    assert_eq!(out.on_tick(t1 + secs(2) + IDLE_AFTER), Some(end("ch")));
    assert_eq!(out.deadline(), None);
}
