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

## Chunk 3: Warn harder when a link's text is something else

### Changes Made
- `Cargo.toml`: `markdown = "1"` moved from `[dev-dependencies]` to `[dependencies]` (same version, already in the tree through gpui-base). `Cargo.lock` is unchanged.
- `src/markdown.rs`:
  - `prepare_with` ends with `guard_links(&out)` when the original content has `](` or `]:`.
  - `guard_links(prepared)` parses with GFM plus math, as `TextView` does. It collects reference definitions recursively, keyed by a case- and whitespace-folded label, and walks `Link` and `LinkReference` nodes.
  - Each external link whose text (`node.to_string()`) doesn't `safety::shows_address` its URL is rewritten to `[children source](stoat:link?url=…&text=…)` (form-urlencoded). The rewrites run from last to first.
  - A typed `stoat:link?…` is always rewritten with its real text, so its `text` can't be forged.
  - `InternalLink::External { url, text }` in `parse_internal_link`. It needs `url` (which must pass `is_external_link`) and `text`.
  - The module docs gain a line for the new rewrite.
- `src/safety.rs`:
  - `LinkCheck::Mismatch`.
  - `classify` returns `Plain` when the text shows the address, `Mismatch` when `text_host` gives a host that differs from the destination's, and `Text` otherwise.
  - New `pub fn shows_address(url, text)`: percent-decoded, no trailing `/`, case-insensitive. Text without a scheme is compared with the URL after its scheme.
  - `text_host`: a URL, or a dotted host with no whitespace, and not an email address.
  - The tests cover all three levels and the edge cases.
- `src/ui/markup.rs`: `InternalLink::External` is drawn as an ordinary link (`None`).
- `src/ui/chat.rs`:
  - `People::link_handler` sends `InternalLink::External` to `open_link(url, Some(text))`.
  - `LinkPrompt` regains `check` and gains `understood` and `ready()`.
  - The dialog adds `You clicked on "{text}"` for `Text`. For `Mismatch` it adds a "Be careful! This is not the same as the link that was displayed:" block in the danger colour, the text in a box, and an "I understand the risk" checkbox.
  - Continue is `.disabled(!ready)`, and `on_ok`/`follow_link_prompt` check `ready()` too. The trust checkbox appears only for `Plain` (unchanged).
- `tests/markdown_prepare.rs`:
  - New tests: inline guarding (including formatting and a title), links that show their address left alone (autolink, bare URL, `www.`, email, text equal to the URL), the three reference-link forms, code, images and non-web schemes untouched, a forged typed `stoat:link`, a URL round trip (`?`, `&`, `+`, `#`, `é`, parens, existing `%` escapes, mailto, with a non-ASCII character before the link), and malformed `stoat:link` URLs.
  - Two existing expectations updated: `ordinary_markdown_passes_through` now uses `[example.com](https://example.com)`, and the review-bot test expects its "view the full review" link guarded.

### Divergences
- Minor: the plan has `guard_links` compare the text with the URL "normalised as in assumption 2" (missing scheme, trailing `/`). That alone would guard GFM's `www.` autolinks (`http://`) and non-ASCII bare URLs (percent-encoded). So `shows_address` also decodes percent-escapes, ignores case, and compares scheme-less text with the URL after its scheme, whichever scheme it is.
- Minor: reference links are `LinkReference` nodes in mdast, not `Link`, so their URLs come from the `Definition`s.
- Minor: added handling for typed `stoat:link?…` URLs. Without it, someone could forge `text` to skip the warning.
- Minor: an email address as link text counts as `Text`, not `Mismatch`.

### Verification Performed
- `cargo test --test markdown_prepare`: 37 passed. `cargo test --lib safety`: all passed.
- `cargo test`: 676 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/ui/chat.rs` 17 and `src/markdown.rs` 1 (both the same diff as HEAD). `src/safety.rs`, `src/ui/markup.rs` and `tests/markdown_prepare.rs` had none at HEAD, so they were formatted fully and have 0.

### Manual Validation
Pending (batched to the end).

## Chunk 4: Age gate on mature channels

### Changes Made
- `src/store.rs`: `is_mature(channel)` is true for a `TextChannel` or `Group` with `nsfw: true`. A server's own `nsfw` is ignored. DMs and Saved Notes are never mature. The existing `apply_channel_update` already merges `nsfw`.
- `src/safety.rs`: `SafetySettings::allows(channel)` (`adult_confirmed && mature_channels.contains`) and `enter(channel)` (sets both). Tests for each, and for `enter` round-tripping through `SafetyFile`.
- `src/assets.rs`: a `safety_icons_load` test that `UserX` (chunk 1's row) and `TriangleAlert` (a default component icon) load.
- `src/ui/chat.rs`:
  - `age_gate_tick: Option<(channel, bool)>`. The box's state applies only to the channel it was set in; anywhere else it starts as `safety.adult_confirmed`, so it resets on channel change without a hook.
  - `gated(channel)` = `store.is_mature && !safety.allows`.
  - `ensure_current_loaded`, `maybe_ack_current`, `search_for`, `toggle_pins` and `jump_to_message` do nothing for a gated channel.
  - `enter_mature_channel` calls `safety.enter`, saves, resets the box state, then `ensure_current_loaded`.
  - `render_main` computes `gated`/`usable`. While gated it draws `render_age_gate` in place of the body (built before `theme` is borrowed), and it hides the typing line, the composer, the search box, the pins button, and the file drop target and overlay. The member-list toggle stays.
  - `render_age_gate`: a centred card, at most `AGE_GATE_WIDTH` (420px) wide, with a 40px `TriangleAlert` in the warning colour, `#name` (or the group's name), "This channel is marked as mature.", the "I confirm that I am at least 18 years old." `Checkbox`, and a primary "Enter channel" button, disabled until ticked.
  - The `ChannelUpdate` handler now also calls `ensure_current_loaded` when the update is for the open channel, so a channel that stops being mature loads at once. Turning it mature needs nothing: `render_main` checks `gated` on every render, and the handler already notifies.
- `tests/store_mature.rs` (new): `is_mature` for a mature text channel, a group, a plain channel, a DM, Saved Notes and an unknown channel, and a `ChannelUpdate` turning `nsfw` on and off.

### Divergences
- Minor: `jump_to_message` is gated too, so a jump can't load a gated channel's history behind the gate. The plan named only `ensure_current_loaded` and `maybe_ack_current`.
- Minor: the box state is `Option<(channel, bool)>` rather than a `bool` reset on channel change, so no channel-change hook is needed.
- Minor: when the open channel stops being mature, the `ChannelUpdate` handler loads it straight away.
- The store tests are in a new `tests/store_mature.rs`. The plan didn't name a file.

### Verification Performed
- `cargo test --test store_mature`: 2 passed. `cargo test --lib safety assets`: all passed.
- `cargo test`: 681 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/store.rs` (15) and `src/ui/chat.rs` (17) have the same diff as HEAD. `src/safety.rs`, `src/assets.rs` and `tests/store_mature.rs` are formatted and have 0.

### Manual Validation
Pending (batched to the end).

## Summary

_Completed: 2026-11-06_

### Chunks Completed
1. Blocked users' messages fold into "N blocked messages", revealed for the session by clicking (`2b9d141`).
2. External links ask before opening, with a per-account trust list in `safety.json` (`deaf25f`).
3. Links whose text isn't their address carry it to the dialog (`stoat:link?…`), with the "You clicked on" and "Be careful!" levels (`00e72ea`).
4. Mature channels are behind an 18+ gate. Nothing loads or is acknowledged until the user enters (`27916fc`).

### Divergences
All minor, and listed under each chunk:
- A blocked run read partway puts the divider after it.
- `reveal_blocked_run_of` is in the store.
- Blocked-author reply previews also drop `author_id`.
- Embed descriptions use `People::link_handler` (so mentions there open cards).
- The link dialog uses a custom footer.
- Embed links pass no text.
- `shows_address` also decodes `%`, ignores case, and matches scheme-less text against any scheme.
- Reference links resolve through `Definition`s.
- Typed `stoat:link` URLs get their real text.
- An email address as link text is `Text`.
- `jump_to_message` is gated.
- The age gate's box state is per channel.
- A `ChannelUpdate` for the open channel reloads it.

### Definition of Done
- [x] Runs of `Blocked` authors show as "1 blocked message" / "N blocked messages", clicking shows them for the session, and replies preview "Blocked user". Covered by `tests/store_blocked.rs`; not yet tried by hand.
- [x] Every external link (messages, embeds, bios) asks, except own-instance URLs, attachments and trusted origins. Covered by code review and the `safety` unit tests; not yet tried by hand.
- [x] "Don't ask me again" appears only for `Plain` links and trusts the origin per account, across relaunches (`SafetyFile` round-trip tests).
- [x] A different-address text gives "Be careful!", and Continue waits for "I understand the risk" (`classify` tests, and `ready()` gates the button, Enter and `follow_link_prompt`).
- [x] Mature text channels and groups are gated, with no load or acknowledgement until entered, and the confirmation and channels are saved per account (`store_mature` and `safety` tests).
- [x] `cargo test` (681 passed) and `cargo clippy --all-targets -- -D warnings` pass, and no file's rustfmt drift grew.
- [ ] Every chunk's manual check recorded here: **not done.** The user chose to merge to main and move on to the next milestone without running them.

### Remaining Manual Steps
The plan's manual checks haven't been run. They need a second account and a server where a channel can be marked mature:
1. Chunk 1: block an account, post 3 + 1 (another author) + 2 messages, and expect "3 blocked messages", the other message, then "2 blocked messages". Click to reveal, and check it survives a channel switch. A reply to a hidden message previews "Blocked user". Unblock and re-block update at once.
2. Chunk 2: a bare URL asks. "Don't ask me again" persists across relaunches. `mailto:` always asks with no checkbox. Attachments don't ask.
3. Chunk 3: `[google.com](https://example.com)` gives "Be careful!" with Continue disabled until ticked. `[the docs](https://example.com)` gives "You clicked on" with no trust box. Also try embed titles, a YouTube card and `<https://…>`.
4. Chunk 4: the gate shows with no messages and the channel stays unread. "Enter channel" is disabled until ticked. Entering loads the channel and marks it read. After a relaunch it opens straight in. A second mature channel has the box pre-ticked. Marking an open, never-entered channel mature puts the gate up live.
5. Two accounts don't share trust or entered channels.
6. Light and dark themes are readable for the blocked row, both dialog variants and the gate.
