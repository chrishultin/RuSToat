use stoat_gpui::nav::neighbour;

const CHANNELS: [&str; 4] = ["a", "b", "c", "d"];

fn next(current: Option<&'static str>, step: isize) -> Option<&'static str> {
    neighbour(&CHANNELS, current.as_ref(), step, |_| true).copied()
}

#[test]
fn next_and_previous_channel_wrap_around() {
    assert_eq!(next(Some("b"), 1), Some("c"));
    assert_eq!(next(Some("b"), -1), Some("a"));
    assert_eq!(next(Some("d"), 1), Some("a"));
    assert_eq!(next(Some("a"), -1), Some("d"));
}

#[test]
fn without_a_current_item_it_starts_at_an_end() {
    assert_eq!(next(None, 1), Some("a"));
    assert_eq!(next(None, -1), Some("d"));
    // A current item that isn't listed (e.g. folded away) counts as none.
    assert_eq!(next(Some("zz"), 1), Some("a"));
}

#[test]
fn only_eligible_items_count() {
    let unread = |c: &&str| matches!(*c, "a" | "d");
    let walk =
        |current: &'static str, step| neighbour(&CHANNELS, Some(&current), step, unread).copied();
    assert_eq!(walk("b", 1), Some("d"));
    assert_eq!(walk("b", -1), Some("a"));
    assert_eq!(walk("d", 1), Some("a"));
    // The current item itself doesn't count, even when eligible.
    let only_b = |c: &&str| *c == "b";
    assert_eq!(neighbour(&CHANNELS, Some(&"b"), 1, only_b), None);
    assert_eq!(neighbour(&CHANNELS, Some(&"a"), 1, |_: &&str| false), None);
}

#[test]
fn servers_with_home_first() {
    let places = ["home", "srv-1", "srv-2"];
    let walk =
        |current: &'static str, step| neighbour(&places, Some(&current), step, |_| true).copied();
    assert_eq!(walk("home", 1), Some("srv-1"));
    assert_eq!(walk("srv-2", 1), Some("home"));
    assert_eq!(walk("home", -1), Some("srv-2"));
}

#[test]
fn a_single_item_or_an_empty_list_goes_nowhere() {
    assert_eq!(neighbour(&["only"], Some(&"only"), 1, |_| true), None);
    assert_eq!(neighbour::<&str>(&[], None, 1, |_| true), None);
}
