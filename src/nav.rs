//! Moving through lists from the keyboard: the next or previous channel,
//! unread channel, or server. Plain data with no GPUI.

/// The item `step` places from `current` in `items` (`-1` is the previous
/// one), counting only items that are `eligible`. Wraps around at either
/// end. `None` if nothing else is eligible.
///
/// Without a `current` (or one that isn't in `items`), the first eligible
/// item going forwards is the first in the list, and going backwards the
/// last.
pub fn neighbour<'a, T: PartialEq>(
    items: &'a [T],
    current: Option<&T>,
    step: isize,
    eligible: impl Fn(&T) -> bool,
) -> Option<&'a T> {
    if items.is_empty() || step == 0 {
        return None;
    }
    let len = items.len() as isize;
    let start = match current.and_then(|c| items.iter().position(|item| item == c)) {
        Some(ix) => ix as isize,
        // One before the first, or one after the last.
        None if step > 0 => -1,
        None => len,
    };
    let direction = step.signum();
    let mut remaining = step.abs();
    let mut ix = start;
    // Each lap visits every other item once; stop before coming back round
    // to where we started.
    for _ in 0..len {
        ix = (ix + direction).rem_euclid(len);
        let item = &items[ix as usize];
        if Some(item) == current {
            break;
        }
        if eligible(item) {
            remaining -= 1;
            if remaining == 0 {
                return Some(item);
            }
        }
    }
    None
}
