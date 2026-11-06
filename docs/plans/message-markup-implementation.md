# Implementation Record: Message markup (milestone 10)

_Plan: docs/plans/message-markup.md_
_Started: 2026-11-06_

Run in continuous mode. The user asked for every chunk's manual check to be batched at the end rather than done per chunk, so each chunk's "Manual Validation" says "Deferred" until then.

Before chunk 1 the branch was fast-forwarded to local `main` (02460a0, one commit ahead of `origin/main`).

Pre-existing dirty files kept out of every commit: `.gitignore` (a duplicated Riptide entry) and the untracked `.shadow/`. `docs/plans/roadmap.md` (the link to this plan) and `docs/plans/message-markup.md` belong to this milestone and went in with chunk 1.

Baseline before any change: 611 tests passed, clippy clean, `src/ui/chat.rs` rustfmt drift 17.

## Chunk 1: Copy button on code blocks

### Changes Made
- `src/ui/markup.rs` (new): `code_block_actions(block, window, cx)`. It returns an `h_flex` with the block's language (muted, `text_xs`) when the fence names one, and a ghost `xsmall` `Button` with `IconName::Copy`. A click writes `block.code()` to the clipboard and sets a keyed `Entity<bool>` (`window.use_keyed_state(("code-copied", span.start))`, scoped by the `TextView`'s own element id). The icon becomes `IconName::Check` with a "Copied" tooltip, and a 2-second timer sets it back. The wrapper stops left mouse-down propagation, so a press doesn't start a text selection.
- `src/ui/mod.rs`: `pub mod markup;`
- `src/ui/chat.rs`: `.code_block_actions(code_block_actions)` on the message body, system message and embed description `TextView`s, and the import.
- `src/ui/profile_card.rs`: the same on the bio `TextView`.

### Divergences
- `CodeBlock` is imported from `gpui_kit::base::text`. gpui-component's `text` facade doesn't re-export it.
- **The button overlays the first code line rather than getting extra top padding.** With actions set, gpui-base gives the block only 14px of top padding, and its actions container has the code background, so a long first line runs under the badge. Extra padding would need an explicit `TextViewStyle` (`with_code_block`) at each call site. That takes `cx` in the row renderers, which don't have it, and it pins the style to the current theme. The plan's risk allows either. Left as an overlay for the manual check to judge.

### Verification Performed
- `cargo build`: clean.
- `cargo test`: 611 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --edition 2024` on `markup.rs` and `profile_card.rs` (clean at HEAD). `chat.rs` drift is still 17.

### Manual Validation
Deferred to the end (see the plan's chunk 1 manual check).

## Chunk 2: Quote a message into the message box

### Changes Made
- `src/draft.rs`: `pub fn quote(content)` prefixes each line with `> ` (a blank line becomes `>`, using `str::lines`, so a trailing newline or CRLF adds no extra line) and adds a blank line after. `pub fn append_quote(existing, quote)` puts the quote on a line of its own. There are two unit tests.
- `src/ui/composer.rs`: `Composer::append_quote(content, labels, window, cx)`. It quotes `content`, appends it to the box's current value, learns labels for the mentions in the quote, then calls `set_text` (which rebuilds chips for the whole value and puts the cursor at the end) and focuses the box.
- `src/ui/chat.rs`: `RowAction::Quote(id)`, handled by reading `store.raw_content` and labelling with `store.token_label`, as `start_edit` does. The "Quote" menu item sits right after "Reply" and only when `message.body.is_some()`, so attachment-only messages don't get it. System messages use `render_system_row`, which has no menu.
- `src/assets.rs`: `TextQuote` added to `ExtraIcons` and to the icon-loads test.

### Divergences
- The composer method is called `append_quote(content, labels, …)` rather than `append_text(text, …)`. It takes the raw content and quotes it itself, so the caller can't forget to.

### Verification Performed
- `cargo test`: 613 passed (2 new), 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- Formatted `draft.rs` and `assets.rs` (clean at HEAD). `composer.rs` drift is still 14 (14 at HEAD), and `chat.rs` is still 17.

### Manual Validation
Deferred to the end (see the plan's chunk 2 manual check).

## Chunk 3: The markup plugin, and `<t:…>` timestamps

### Changes Made
- `src/time.rs`:
  - `format_timestamp(when, style, now)`: assumption 1's table. Generic over `TimeZone`, so tests use a `FixedOffset`.
  - `relative_time(when, now)`: dayjs `fromNow` thresholds (44 s, 89 s, 44 min, 89 min, 21 h, 35 h, 25 d, 45 d, 10 months, 17 months), with its "a count that rounds to 1 reads as a/an" rule.
  - `timestamp_tooltip(when)`: e.g. "Friday, 6 November 2026 at 14:05:09 GMT+1", or "GMT+5:30", or "GMT".
  - `local_from_seconds`.
  - Four unit tests.
- `src/markdown.rs`:
  - `angle_bracket` turns `<t:SECONDS>` / `<t:SECONDS:X>` (`X` one `\w` character) into `[escaped source](stoat:time/SECONDS[/X])`. Seconds that chrono can't represent (`DateTime::from_timestamp` is `None`), extra characters, spaces and a missing `>` all stay as typed.
  - `InternalLink::Time { seconds, style }` in `parse_internal_link`.
  - `plain_mentions` (so `plain_text`, `snippet` and spoilers) shows timestamps in local time, with `R` shown as the date and time.
  - New `pub fn has_relative_time(prepared)`.
  - Module doc updated.
  - Unit tests for the link parsing (including malformed URLs) and for `has_relative_time`.
- `src/ui/markup.rs`: `MarkupPlugin` claims `Link` nodes whose URL parses as `InternalLink::Time`. It stores `Markup::Time`, and `.text()`/`.markdown()` are the `<t:…>` source, so copying gives back the tag. `render_inline` draws a pill: the text style's font, size and line height, a background of the text colour at 12% (readable on accent bubbles too), `with_baseline` from the font's baseline, an id keyed by the node's source offset, and a `Tooltip` with the full date. The module doc now covers the plugin.
- `src/ui/chat.rs`:
  - `.plugin(MarkupPlugin)` on the message body, embed description and system message `TextView`s.
  - A `_relative_tick` task in `ChatView::new` that loops every `RELATIVE_TIME_TICK` (60 s) and calls `cx.notify()` when `open_channel_has_relative_times()`.
  - The link handler ignores `InternalLink::Time`.
- `src/ui/profile_card.rs`: `.plugin(MarkupPlugin)` on the bio.
- `tests/markdown_prepare.rs`: five tests:
  - the rewrite, plus a URL round trip through the `markdown` crate for `""`, `R`, `t`, `T`, `D`, `F`, `f`, `_` and `9`
  - malformed tags
  - tags in code
  - a tag next to a mention
  - `plain_text`/`snippet`

### Divergences
- **The relative-time check lives in `ChatView`, not `ChatStore`.** It's `open_channel_has_relative_times` over the open pane's rows, using `markdown::has_relative_time`, instead of `ChatStore::has_relative_times(channel)`. The pane's rows are exactly what's on screen, and the pure helper is unit-tested.
- **The timer is one loop for the life of the view,** not a re-armed `schedule_typing_tick`-style task. Each minute it checks whether the open channel has an `R` timestamp and only then notifies. That's simpler, with the same effect.
- **Times under 45 seconds read "just now"** in both directions, as the plan's examples say. dayjs's wording would be "a few seconds ago" / "in a few seconds".
- **dayjs counts calendar months; this uses an average month** (365.2425 / 12 days). The two only differ at month boundaries.
- **The system-message `TextView` also gets `MarkupPlugin`** (it had no `MathPlugin`), per "every `TextView`".
- **Checked in gpui-base:** paragraphs that hold plugin objects use the inline flow, which is rebuilt every frame (only text-only paragraphs hit `ParagraphRenderCache`). So `cx.notify()` should redraw relative times without the plan's fallback. The manual check confirms it.

### Verification Performed
- `cargo test`: 624 passed (13 new), 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- Formatted `time.rs`, `markup.rs`, `profile_card.rs` and `tests/markdown_prepare.rs`. `markdown.rs` keeps its 1 drift diff from before this work (in `strip_line_prefix`), and `chat.rs` is still 17.

### Manual Validation
Deferred to the end (see the plan's chunk 3 manual check).
