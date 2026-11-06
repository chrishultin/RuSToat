# Implementation Record: Safety (milestone 11)

_Plan: docs/plans/safety.md_
_Started: 2026-11-06_

Run in continuous mode. The user chose to run every chunk's manual check together at the end, so each chunk's "Manual Validation" says pending until then.

Already dirty before this work, and kept out of every commit: `.gitignore` (a duplicated Riptide block) and `.shadow/` (untracked).

## Chunk 1: Collapse blocked users' messages

### Changes Made
- `src/store.rs`:
  - `TimelineRow::Blocked { ids }`.
  - `revealed_blocked: HashSet<String>`, plus `reveal_blocked(ids)`, `reveal_blocked_run_of(channel, id)` (shows the run that hides `id`, for jumps) and `is_blocked(user)` (`Blocked` only, not `BlockedOther`).
  - `timeline()` folds hidden messages into the run as it builds rows. A message is hidden if its author is blocked, it isn't a system or pending message, and it hasn't been revealed. The `previous` author still advances, so the next visible message from another author starts its own group.
  - `row_key` of a blocked row is its first ID, so `diff_rows` turns a reveal into a `Remeasure` of that slot plus `Insert`s, not a `Reset`.
  - `with_unread_divider` treats a blocked row as its first message.
  - `reply_preview` of a blocked author's message gives `BLOCKED_AUTHOR` ("Blocked user"), an empty snippet, and no avatar or author ID.
- `src/ui/chat.rs`:
  - `render_blocked_row`: a centred, muted "N blocked messages" with `IconName::UserX`, a pointer cursor and an underline on hover.
  - `RowAction::RevealBlocked(ids)`, which reveals the run and calls `sync_pane`.
  - `jump_to_message` calls `reveal_blocked_run_of` on both paths (already loaded, and after `set_window`).
  - `relationship_changed` now takes `cx` and resyncs every loaded pane when someone is blocked or unblocked.
- `tests/store_blocked.rs` (new): 9 tests (folding, system message splits a run, `BlockedOther` not folded, reveal plus its `diff_rows` ops, revealing for a jump, unread divider before a run and partway through one, reply preview, unblock and re-block).
- `tests/store_unreads.rs`: its `shape` helper gains the new arm.

### Divergences
- Minor: the plan says the divider goes before a run "if its first ID is after `after`". When a run is read partway, the divider lands on the next unread row after the run. This is tested.
- Minor: added `reveal_blocked_run_of` to the store so jumps can reveal the run of a hidden message. The plan put this logic in the view.
- Minor: the blocked-author reply preview also clears `author_id`, so it doesn't open the person's card.

### Verification Performed
- `cargo test --test store_blocked`: 9 passed.
- `cargo test`: 659 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts are unchanged: `src/store.rs` 15 (as at HEAD), `src/ui/chat.rs` 17 (identical diff to HEAD). `tests/store_blocked.rs` is formatted and has 0.

### Manual Validation
Pending (batched to the end).
