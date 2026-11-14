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

## Chunk 3: Custom emoji

### Changes Made
- `ios/stoat-ffi/src/rich_text.rs`:
  - `SpanKind::Emoji { url, jumbo }`, from the `<img>` `prepare` writes (`src`, `alt`, and `width >= markdown::JUMBO_EMOJI_PX` for jumbo). The span's `text` is the `:name:` alt, shown until the image loads.
  - Other HTML stays as typed (the old `html_text` became `emoji`).
  - Emoji spans never merge.
- `ios/stoat-ffi/tests/rich_text.rs`: 22 tests. The emoji test now checks the `Emoji` span. Added: emoji-only messages through the real `prepare` give unmerged jumbo spans (and inline ones aren't jumbo); and other HTML stays as typed.
- Swift:
  - `ios/RuSToat/Shared/EmojiImages.swift` (new): an `@MainActor @Observable` cache of `UIImage`s keyed by size and URL.
    - `image(url, size:)` returns the image or starts one download per key (de-duplicated while loading).
    - Images are scaled to 20 pt inline or 48 pt jumbo, keeping their shape. The `URLSession` is injectable for tests.
  - `ios/RuSToat/Chat/RichTextView.swift`:
    - Emoji spans become `Text(Image(uiImage:))` with a small negative baseline offset once loaded, and a secondary-coloured `:name:` until then. Reading the cache in `body` makes Observation redraw the paragraph when the image arrives.
    - `RichSpan.needsRenderer`: paragraphs with only text, breaks and emoji stay selectable.
- `ios/RuSToatTests/EmojiImagesTests.swift` (new): a stub `URLProtocol`. Two requests while loading start one download, and the result is 20×20; scaling keeps the shape.
- `ios/stoat-ffi/examples/fake_stoat.rs`: a higher-priority `GET /` whose `autumn` file server is the fake itself, and `GET /emojis/{id}` serving the app icon PNG.
- `tests/fixtures/messages_rich.json`: an inline-emoji message and an emoji-only one.

### Divergences
- **No `id` or `name` field on the emoji span:** the `:name:` is the span's text, and the URL is enough to load it.
- **Jumbo emoji are 48 pt** (desktop's 96 px is for a larger window), and inline ones 20 pt, as planned.

### Verification Performed
- `cargo test -p stoat-ffi`: 59 passed (rich_text 22).
- Swift: 18 tests in 6 suites passed, no warnings.
- Desktop: `cargo test`: 1499 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Simulator against `fake_stoat`: "Ship it [emoji] today" with the emoji inline on the text line, and an emoji-only message drawing two large emoji.

### Manual Validation
Deferred to the end: custom emoji on a real server, inline and jumbo.

## Chunk 4: The message menu and pins

### Changes Made
- `src/api/error.rs`: `ApiError::pin_failure_text(pin)`, moved from the private `pin_error_text` in `src/ui/chat.rs` (desktop now calls the method; same text).
- `ios/stoat-ffi/src/actions.rs` (new, a second exported `impl AccountController`), each following desktop's handling:
  - `raw_text(channel, id)`: `ChatStore::raw_content`.
  - `message_link(channel, id)`: `links::message_link` with the web app from `GET /`, and the channel's server, or none for DMs and groups.
  - `can_pin(channel)`.
  - `async set_pinned(channel, id, pinned) -> Result`, as desktop's `toggle_pin`. `POST`/`DELETE …/pin`, and the row changes when Stoat's `MessageUpdate` arrives. `AlreadyPinned`/`NotPinned` call `set_pinned` locally and report a timeline change. Other errors return `pin_failure_text`.
- `ios/stoat-ffi/src/controller.rs`: `Shared.web_app` (filled from `root.app` in `fetch_media`); `timeline_changed` is now `pub(crate)`.
- `ios/stoat-ffi/src/timeline.rs`: `MessageItem.pinned`.
- `ios/stoat-ffi/tests/actions.rs` (new, 5 tests): Copy Text is the source Markdown; message links for a server channel and a DM; a pin shown once Stoat's update arrives (with `expect(1)`); already pinned catches up; a refused unpin says "You can't unpin messages here.".
- Swift:
  - `ios/RuSToat/Chat/MessageRowView.swift`:
    - `MessageActions` gains `rawText`, `link`, `canPin` and `setPinned`.
    - The menu offers Reply, Copy Text (as typed), Copy Link (when there's a web app), Pin/Unpin (when `canPin`), and Retry/Discard.
    - Pinned messages get an orange pin in their header.
  - `ios/RuSToat/Chat/ChatView.swift`: `run(_:)` runs an async action and shows its error for 4 s under the conversation (reusing the send-error line), via `show(_:)`.
- **Build fix,** found while verifying this chunk:
  - `ios/project.yml` no longer lists the XCFramework as a linked dependency, because Xcode copies linked XCFrameworks before any script phase runs. After a change to the core's API, the first build compiled the new bindings against the old headers ("cannot find 'uniffi_…' in scope"); `test.sh` had hidden this by building the core first.
  - Instead, the project-level `SWIFT_INCLUDE_PATHS` and the app's `LIBRARY_SEARCH_PATHS` point at the XCFramework's slices per SDK, with `-lstoat_ffi`. The "Build Rust core" phase declares `Generated/StoatCore.swift` as its output. Declaring the XCFramework itself made a dependency cycle.
  - `Generated/StoatCore.swift` is listed in `project.yml` as an optional source, so a project generated before the first build still compiles it.
  - `README.md`: building is now `xcodegen generate`, then Run; `build-core.sh` is optional, with the reason noted.

### Divergences
- **Pinning isn't optimistic.** The plan said to flip the row at once and roll back on failure, but desktop waits for Stoat's update and catches up on `AlreadyPinned`. iOS does the same, which avoids a flicker when Stoat refuses. The plan's chunk 4 text was amended.
- **The fixture's server gives no Manage Messages,** so `can_pin` is false there. The tests cover the request paths directly.
- **The build-system fix** above wasn't planned, but it's needed for you to build straight from Xcode after pulling core changes.

### Verification Performed
- `cargo test -p stoat-ffi`: 64 passed (actions 5).
- Build-system checks, all from `xcodebuild` alone:
  - adding an exported Rust function, building, removing it, building: both builds succeeded first time (before the fix, the first failed);
  - a simulated fresh clone (no `ios/build`, no `Generated`, project regenerated) built;
  - the signed device build (`generic/platform=iOS`, `-allowProvisioningUpdates`) succeeded.
- Swift: 18 tests passed.
- Desktop: `cargo test`: 1499 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Deferred to the end: pin on the phone and see it on desktop (needs a server where you have Manage Messages); Copy Link and open it.

## Chunk 5: Reactions

### Changes Made
- Core, so both apps share desktop's text:
  - `src/api/error.rs`: `ApiError::reaction_failure_text()`, moved from the private `reaction_error_text` in `src/ui/chat.rs`.
  - `src/store.rs`: `pub const FULL_REACTIONS_TEXT`, moved from `src/ui/chat.rs`.
  - Desktop now uses both; the text is unchanged.
- `ios/stoat-ffi/src/timeline.rs`: `MessageItem.reactions: Vec<ReactionItem { key, emoji, count, mine, names }>`, `EmojiItem` (Unicode, or Custom with id, name and URL) and `PickerEmojiItem`, mapped from the store's `ReactionChip`, `ReactionEmoji` and `PickerEmoji`.
- `ios/stoat-ffi/src/actions.rs`:
  - `ReactionPicker { choices, full, restricted, note }`.
  - `reaction_picker(channel, message)`: `MessageRow::picker_choices` over `ChatStore::reaction_choices`, the message's row found in `store.timeline`.
  - `search_emoji(channel, query)`: `emoji::search_all` over Unicode and `custom_emoji_for(channel)`, up to 200, ignoring surrounding `:`.
  - `async toggle_reaction(channel, message, emoji) -> Result`, as desktop's `toggle_reaction`:
    - a per-(message, emoji) in-flight guard, so a second tap waits;
    - `has_reacted` decides add or remove;
    - `would_exceed_reactions` refuses a new emoji on a full message with `FULL_REACTIONS_TEXT`, without a request;
    - an optimistic `apply_react`, then `react`/`unreact`, undone with `reaction_failure_text` on failure.
- `ios/stoat-ffi/src/controller.rs`: `Shared.reacting`.
- `ios/stoat-ffi/tests/actions.rs`: 5 reaction tests, 10 in all:
  - toggling adds (PUT) and shows at once, the `MessageReact` echo leaves the count at 1, and toggling again removes it (DELETE), each `expect(1)`;
  - a 400 `InvalidOperation` is undone with desktop's text;
  - a message with 20 emoji refuses a 21st without a request, and its picker offers its own 20 with a note;
  - the picker offers only the emoji the author allowed (`interactions.restrict_reactions`), and the usual ones otherwise;
  - search finds `:thumbsup` → 👍, and, after an `EmojiCreate`, a custom emoji by name (which the picker also offers last).
- Swift:
  - `ios/RuSToat/Chat/ReactionsView.swift` (new):
    - `EmojiView` (Unicode text, or a custom image from `EmojiImages`).
    - `ReactionsView`: capsule chips with emoji and count, tinted with a border when yours. Tap toggles; long-press shows who ("You, Ada and 3 others" from `who(_:)`). A trailing smiley button adds.
    - `FlowLayout`, a wrapping `Layout`.
  - `ios/RuSToat/Chat/ReactionPicker.swift` (new): `ReactionPickerView`, a medium/large sheet.
    - The note, then a grid of the choices, with the ones you've used tinted.
    - `.searchable` over `searchEmoji` unless the message is full or restricted.
    - Picking toggles and closes.
  - `ios/RuSToat/Chat/MessageRowView.swift`: `MessageActions.react` and `pickReaction`, "Add Reaction" in the menu, and reactions under the message.
  - `ios/RuSToat/Chat/ChatView.swift`: `reactingTo` with `.sheet(item:)`, `react` through `run` (errors shown under the conversation), and `MessageItem: Identifiable`.
- `ios/RuSToatTests/RichTextTests.swift`: `ReactionsTests` for the "who reacted" text.
- `tests/fixtures/messages_rich.json`: reactions on one message, some yours, one a custom emoji.

### Divergences
- **Long-pressing a chip uses a context menu** to show who reacted (iOS has no hover tooltip).
- **The picker uses the default skin tone,** as the plan assumed.

### Verification Performed
- `cargo test -p stoat-ffi`: 69 passed (actions 10).
- Swift: 19 tests in 7 suites passed, no warnings.
- Desktop: `cargo test`: 1499 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Simulator against `fake_stoat`: six chips wrapping onto a second row, the custom emoji's image in its chip, 👍 and 🚀 tinted as yours, and the add button.

### Manual Validation
Deferred to the end: react and unreact between the phone and desktop; the 20-emoji limit.

## Chunk 6: Edit and delete your own messages

### Changes Made
- Core, so both apps share desktop's handling:
  - `src/api/error.rs`: `ApiError::edit_failure_text()` and `delete_failure_text()`, moved from private functions in `src/ui/chat.rs`. Also `is_not_found()`.
  - `src/draft.rs`: `pub const EMPTY_EDIT_TEXT`, moved from `src/ui/composer.rs`.
  - Desktop uses all of these now; the text is unchanged.
- `ios/stoat-ffi/src/actions.rs`:
  - `async edit(channel, id, text) -> Result`, as desktop's `edit_message`:
    - `draft::check_send`: blank is refused with "To delete the message, use Delete.", over-long with the count.
    - Refused for a message that isn't the user's own; an unchanged text does nothing.
    - `apply_local_edit` (a timeline change), then `PATCH`, then `undo_local_edit` with `edit_failure_text` on failure.
  - `async delete(channel, id) -> Result`, as desktop's `delete_message`:
    - Own messages only.
    - `remove_local` (lists and timeline change), then `DELETE`; `NotFound` counts as done.
    - Other failures call `restore_local` and return `delete_failure_text`.
  - `is_mine` checks the loaded message's author.
- `ios/stoat-ffi/src/controller.rs`: `Shared::changed_lists_and(channel)`.
- `ios/stoat-ffi/tests/actions.rs`: 7 edit/delete tests, 17 in all:
  - an edit PATCHes the content (trailing whitespace trimmed) and shows at once, `edited`;
  - `CannotEditMessage` brings the old text back with desktop's text;
  - unchanged or blank edits send nothing (blank gets the Delete hint);
  - others' messages can't be edited or deleted;
  - a delete removes at once, and its echo changes nothing more;
  - a 500 puts the message back in place;
  - `NotFound` counts as deleted.
- Swift:
  - `ios/RuSToat/Chat/Composer.swift`: an `editing` mode with a glass "Editing message" chip (✕ cancels) in place of the reply chip, a checkmark Save button, and focus on entry.
  - `ios/RuSToat/Chat/MessageRowView.swift`: Edit and Delete Message in the menu for your own sent, non-system messages. `MessageActions.edit` and `delete`.
  - `ios/RuSToat/Chat/ChatView.swift`:
    - `startEditing` stashes the draft and fills the box with the message's text as typed; `finishEditing` restores the draft.
    - Saving leaves edit mode at once, as desktop does, and runs `edit`, so a refusal shows its reason and the old text returns. A blank edit stays in the box, with the hint.
    - Typing isn't reported while editing.
    - A "Delete this message?" confirmation dialog.

### Divergences
- **Edit mode ends on Save, not on Stoat's answer,** as desktop's composer does. The plan didn't say which.
- **Edit mode's visuals weren't screenshotted.** It's only reachable by long-pressing a message, and `simctl` can't tap. The logic is covered by the Rust tests, and the Swift build is warning-free.
- **Core gained `ApiError::is_not_found()`.** The FFI crate can't depend on `revolt-result` directly, because `main` moved the `revolt-*` pins to 0.16.4.

### Verification Performed
- `cargo test -p stoat-ffi`: 76 passed (actions 17).
- Swift: 19 tests in 7 suites passed, no warnings.
- Desktop: `cargo test`: 1499 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Signed device build: succeeded.

### Manual Validation
Deferred to the end: edit and delete from the phone and see desktop update; edit on desktop while the phone shows the channel; Airplane-mode rollbacks.

## Summary

_Completed: 2026-11-14 (code); manual checks pending._

### Chunks Completed
1. Rich text blocks: parsed in Rust with desktop's Markdown parser and options, and cached by render key; paragraphs, headings, quotes, code blocks with copy, lists, rules, maths source and tables drawn in SwiftUI.
2. Mentions as pills (roles in colour), timestamps in local time (relative ones refreshed each minute), tap-to-reveal spoilers, and a warning before opening disguised links.
3. Custom emoji, inline and large, from a once-per-URL image cache.
4. The message menu (Reply, Copy Text, Copy Link, Pin/Unpin, Retry/Discard) and pins, plus the build fix that lets Xcode build straight after core API changes.
5. Reactions: chips with tap-to-toggle and who-reacted; a picker with search that respects allowed emoji and the 20-emoji limit.
6. Editing (in the composer) and deleting (after confirmation) your own messages, with rollback.

### Divergences
- Chunk 1: a flat span list per block instead of a nested inline tree. UniFFI handled the nested `RichBlock` as is.
- Chunk 2:
  - Custom text renderers don't draw on selectable text, so paragraphs with pills or spoilers aren't selectable (Copy Text covers them).
  - Guarded links keep their `stoat:link?…` URL.
  - `prepare` already guards links.
- Chunk 3: no separate emoji id/name fields; 48 pt jumbo.
- Chunk 4:
  - Pins follow desktop (shown when Stoat's update arrives) rather than flipping optimistically; the plan was amended.
  - Unplanned build fix: the core is read from the XCFramework's slices via search paths, the bindings are declared as the script's output and as an optional source, and the README build steps are simpler.
- Chunk 5: long-press on a chip shows who reacted through a context menu.
- Chunk 6:
  - Edit mode ends on Save, as on desktop.
  - Edit mode wasn't screenshotted.
  - `ApiError::is_not_found()` was added.
- Throughout: desktop's private user-facing texts (pin, reaction, edit, delete, the full-reactions and empty-edit texts) moved into core (`ApiError` methods, `store`, `draft`) so both apps show the same words. Desktop's behaviour is unchanged.

### Definition of Done
- [x] `cargo test && cargo clippy --all-targets -- -D warnings && scripts/ios/test.sh` passes (1499 desktop, 76 stoat-ffi, 19 Swift).
- [x] Messages render paragraphs, headings, quotes, code blocks (with copy), lists, rules, tables and math source, parsed by `markdown::to_mdast` with desktop's options in `stoat-ffi` and cached by `render_key`.
- [x] Mentions draw as pills (roles in their colour), timestamps in local time, spoilers hidden until tapped, and guarded links ask before opening. Unit-tested and screenshotted; the taps themselves are pending you.
- [x] Custom emoji render inline and jumbo, each URL loaded once.
- [x] The message menu offers Reply, Copy Text, Copy Link (when the instance has a web app), Pin/Unpin (when `can_pin`), and Retry/Discard for failed messages.
- [x] Reactions show, toggle optimistically with rollback, list who reacted, and the picker respects allowed emoji and the 20-emoji limit.
- [x] Your own messages can be edited in the composer and deleted after confirmation, both updating at once and rolling back on failure; others' can't.
- [x] Every new action is covered by a wiremock test in `ios/stoat-ffi/tests/actions.rs`.

### Remaining Manual Steps
1. Compare a real channel with code blocks, quotes, lists and a table against desktop.
2. Mentions as pills (a coloured role if your server has one); a `<t:…:R>` timestamp updating.
3. Tap a spoiler; tap a disguised link and see the warning.
4. Custom emoji inline and in emoji-only messages on a real server.
5. React and unreact both ways with desktop; long-press a reaction to see who; try the picker's search.
6. Pin and unpin on a server where you have Manage Messages; Copy Link and open it.
7. Edit and delete your own messages; in Airplane mode, see each roll back with a message.
8. Light and dark mode: pills, spoiler bars, code blocks and reaction chips stay legible.
