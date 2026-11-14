# Implementation Record: RuSToat for iPhone, milestone 2 (rich messages)

_Plan: docs/plans/ios-rich-messages.md_
_Started: 2026-11-13_

Mode: continuous, with manual checks batched to the end, as for milestone 1. Branch `i-d-like-to-port-this-app-to-ios-so-that-i-can-u-24de`, level with `main` (`b439c82`) at the start.

## Chunk 1: Rich text blocks

### Changes Made
- `ios/stoat-ffi/src/rich_text.rs` (new):
  - `RichBlock` (Paragraph, Heading, Quote, Code, List, Rule, Math, Table), which nests through Quote and List; `RichListItem`, `RichCell`, `RichRow`.
  - `RichSpan` (text, bold/italic/strike/code flags, optional link, `SpanKind`), with `SpanKind` holding Text and Break.
  - `blocks(prepared)` parses with `markdown::to_mdast` and desktop's options (GFM plus `math_text`/`math_flow`).
  - Neighbouring text spans with the same style are merged. Link-reference definitions are resolved (normalised labels, as in `src/markdown.rs`), and images become links with their alt text.
  - Inline HTML: a custom emoji's `<img>` shows its `:name:` alt text until chunk 3; any other HTML shows as typed.
  - Anything else falls back to its text.
  - `RichCache` keeps parsed blocks by `render_key` (cleared beyond 4,000 entries). `#[uniffi::export] rich_text(markdown)` is for Swift tests.
- `ios/stoat-ffi/src/timeline.rs`: `MessageItem.rich` and `EmbedItem.rich_description` (a bot embed's Markdown description), both through the cache. `message`, `item` and `embed` take the cache.
- `ios/stoat-ffi/src/controller.rs`: `Shared.rich: RichCache`, used by `timeline()`.
- `ios/stoat-ffi/src/lib.rs`: `pub mod rich_text`. `ios/stoat-ffi/Cargo.toml`: `markdown = "1"`, the version already in the tree.
- `ios/stoat-ffi/tests/rich_text.rs` (new, 15 tests): plain text; combined inline styles; span merging across escapes; hard breaks; heading levels; a quote holding a list; code with and without a language, with `<b>` kept; an unterminated fence; ordered lists with a start number, nesting and task items; rules; maths blocks; tables with a bold cell; inline maths; reference links; emoji HTML; the cache; and a real `prepare` output (escaped `<`, a mention link).
- `tests/fixtures/messages_rich.json` (new): a message with every block kind. `ios/stoat-ffi/examples/fake_stoat.rs` serves it as #random's history.
- Swift:
  - `ios/RuSToat/Chat/RichTextView.swift` (new): `RichTextView`, and `RichBlockView` (nested blocks through `AnyView`; the quote bar; list markers 1./•/☑/☐; tables in a `Grid` that scrolls sideways).
  - `RichSpansText`: one selectable `Text` per span run, with `nonisolated attributed(_:)` mapping the flags to `InlinePresentationIntent` and links to `.link`.
  - `CodeBlockView`: monospaced, scrolling sideways, with a copy button. Maths blocks use it too, labelled "math".
- `ios/RuSToat/Chat/MessageRowView.swift`: rows draw `message.rich`; embed cards draw `richDescription` when there is one.
- `ios/RuSToatTests/RichTextTests.swift` (new) replaces `MessageTextTests.swift`. Its 7 tests cover styles, combined styles, code, links (http and `stoat:`), line breaks, blocks through the bridge, and list markers. `MessageText.attributed` stays for system rows and copying.

### Divergences
- **UniFFI handles the nested `RichBlock` type as is.** No flattening was needed.
- **Spans are a flat list per block, not a nested inline tree,** which maps straight onto `AttributedString`. The plan's `RichInline` became `RichSpan` plus `SpanKind`.

### Verification Performed
- `cargo test -p stoat-ffi --test rich_text`: 15 passed. The full `stoat-ffi` suite: 51 passed.
- `cargo clippy -p stoat-ffi --all-targets -- -D warnings`: clean (via `scripts/ios/test.sh`).
- Swift: 12 tests in 4 suites passed, with no warnings.
- Desktop: `cargo test`: 1499 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Simulator against `fake_stoat`, opening #random: heading, inline styles and a link, a quote with a nested list, a Rust code block with its copy button and `<there>` kept, a numbered list with a nested bullet, task items, a rule, maths as source, and the table, all as expected.

### Manual Validation
Deferred to the end: compare a real channel with code blocks and quotes against desktop.

## Chunk 2: Mentions, timestamps, spoilers and guarded links

### Changes Made
- `ios/stoat-ffi/src/rich_text.rs`:
  - `SpanKind` gains `Mention { kind: MentionKind, colour }`, `Time { full }` and `Spoiler { index }`, with `MentionKind` covering User, Role, Channel, Everyone and Online.
  - `stoat:` link targets are decoded with `markdown::parse_internal_link`; a role's colour comes through as `#rrggbb`.
  - Timestamps are formatted in local time with `time::format_timestamp`, plus `timestamp_tooltip` as `full`.
  - Spoiler words keep their `stoat:spoiler/…` link for taps. `stoat:link?…` (guarded) links keep their internal URL.
  - Spans merge only with the same kind.
  - `RichCache::get` keys text with a relative timestamp by the current minute too, so "in 3 hours" is reparsed each minute.
  - Added a `chrono` dependency (the version already in the tree).
- `ios/stoat-ffi/src/timeline.rs`: `MessageItem.relative_time` (from `markdown::has_relative_time`).
- `ios/stoat-ffi/src/controller.rs`: `reveal_spoiler(channel, message, index)`, which calls `ChatStore::reveal_spoiler` and reports a timeline change.
- `ios/stoat-ffi/tests/rich_text.rs`: 5 more tests, 20 in all:
  - mention kinds and colours;
  - a timestamp's text and its full form;
  - hidden spoiler words through the real `prepare`, and revealed ones as code;
  - a guarded link through `prepare`;
  - the relative-time cache key.
- `ios/stoat-ffi/tests/reading.rs`: `revealing_a_spoiler_redraws_its_message` (the `render_key` changes, no spoiler words remain, and a timeline change is reported).
- `tests/fixtures/messages_rich.json`: a message with a user mention, a channel mention, `@everyone`, absolute and relative timestamps, a spoiler, and a disguised link.
- Swift:
  - `ios/RuSToat/Chat/RichTextView.swift`:
    - `Pill` and `SpoilerBar` (`TextAttribute`s), and `RichTextRenderer` (`TextRenderer`), which draws a tinted rounded pill behind mentions and timestamps and a grey bar in place of hidden spoiler words, without moving the layout.
    - `RichSpansText.text(_:)` builds one `Text` per paragraph from span `Text`s through interpolation, marking each.
    - `mentionTint` gives a role its own colour, otherwise the accent colour.
    - Mention spans are bold, without an underline.
  - `ios/RuSToat/Chat/MessageRowView.swift`:
    - `InternalLinkTap` parses `stoat:spoiler/N/…` and `stoat:link?url=…&text=…`, with form decoding: `+` becomes a space.
    - A per-row `OpenURLAction` reveals spoilers and asks before opening guarded links; other `stoat:` links do nothing.
    - `MessageActions` gains `revealSpoiler` and `openGuarded`.
  - `ios/RuSToat/Chat/ChatView.swift`: an "Open this link?" alert showing the text and the real address; open-channel rows with relative times are redrawn each minute.
- `ios/RuSToatTests/RichTextTests.swift`: `InternalLinkTapTests` (spoiler, guarded link, mention ignored, role colour).

### Divergences
- **`.textSelection(.enabled)` turns off custom `TextRenderer`s.** The first screenshot showed no pills or bars, and the spoiler as a plain link. Removing selection made them draw. So paragraphs containing a mention, timestamp or spoiler use the renderer and aren't selectable; all other text stays selectable. Copy Text (chunk 4) covers whole messages. This is the risk the plan named; the planned fallback (no pills) wasn't needed.
- **No separate `Link { url, external_text }` span kind.** A guarded link keeps its `stoat:link?…` URL, and Swift decodes the real address from it when tapped.
- **Desktop's `guard_links` is already applied inside `prepare`.** Guarding again in `blocks` was tried, broke reference links, and was reverted.

### Verification Performed
- `cargo test -p stoat-ffi`: 57 passed (rich_text 20, reading 9, plus the rest).
- Swift: 16 tests in 5 suites passed, no warnings.
- Desktop: `cargo test`: 1499 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Simulator against `fake_stoat`, opening #random:
  - `@me`, `#general` and `@everyone` drawn as tinted pills;
  - "Friday, 15 January 2027 01:00" and "in 2 months" as pills;
  - "the cake is a lie" as grey bars;
  - "click me" as a link (the tap warning is covered by unit tests).

### Manual Validation
Deferred to the end: tap a spoiler on a real message; tap a disguised link and see the warning.
