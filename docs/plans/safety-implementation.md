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

## Chunk 2: Ask before opening external links

### Changes Made
- `src/safety.rs` (new):
  - `SafetySettings { trusted_origins, adult_confirmed, mature_channels }`, every field `#[serde(default)]`, with `trusts(url)` and `trust(url)`.
  - `SafetyFile::for_account`/`at`/`load`/`save`, modelled on `PlacesFile` (`safety.json` in the app folder, keyed `"{api} {user}"`).
  - `origin(url)`: `url` crate, http/https only, default ports dropped.
  - `is_own_instance(url, bases)`.
  - `LinkCheck` (`Plain`, `Text`) and `classify(url, text)`, which compares a normalised text and URL (missing scheme added, trailing `/` dropped). `Mismatch` comes in chunk 3.
  - 8 unit tests.
- `src/lib.rs`: `pub mod safety;`.
- `src/media.rs`: `MediaUrls::bases()` (API, file server and proxy bases), for own-instance detection.
- `src/ui/chat.rs`:
  - `ChatView` gets `safety_file`, `safety` (loaded with the account, as `places` is) and `link_prompt: Option<LinkPrompt>`.
  - `open_external(url, text)` opens own-instance (`MediaUrls::bases()` plus `web_app`) and trusted URLs at once. Anything else opens an alert dialog titled "External links can be dangerous!" with the description "Are you sure you want to go to {url}?". When the link has an origin and its check is `Plain`, the dialog also has a "Don't ask me again for {origin}" `Checkbox`. The footer is custom (Cancel, then a primary Continue) so chunk 3 can disable Continue; `on_ok` handles Enter. The dialog reads `link_prompt` from the view on every build, and ticking the box updates it and calls `cx.notify()`.
  - `follow_link_prompt` opens the URL, and trusts and saves the origin if the box was ticked. `save_safety()` writes the file.
  - `People::open_link(url, text)` reaches the view. `People::link_handler` uses it for external links, which covers message bodies, system messages and profile bios.
  - `RowAction::OpenLink(url)` handles embed titles, playable thumbnails and video cards.
  - `render_embed`/`render_embed_card` now take `&People`, and an embed's Markdown description uses `people.link_handler()`.
  - Attachments (file cards) and the image viewer's "Open in browser" still call `cx.open_url` directly.

### Divergences
- Minor: the embed description's link handler must be `Send + Sync`, so it can't capture the `Rc` row-action callback. It uses `People::link_handler()` instead, so a user mention in an embed description now opens that person's card, where before it did nothing.
- Minor: `LinkPrompt` doesn't hold the `LinkCheck` yet. It isn't read until chunk 3, and clippy rejects an unused field.
- Minor: the dialog uses a custom footer (Cancel and Continue buttons) instead of the alert's default footer, so chunk 3 can disable Continue.
- Embed titles, thumbnails and video cards pass no link text (`None`). Their titles come from the linked page or the bot, so they're treated like bare links. The plan only gives text to Markdown links.

### Verification Performed
- `cargo test --lib safety`: 8 passed.
- `cargo test`: 667 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/ui/chat.rs` 17 (same diff as HEAD), `src/media.rs` 0, `src/safety.rs` 0. `src/lib.rs`'s one existing diff (`nav`/`media` order) is unchanged.

### Manual Validation
Pending (batched to the end).
