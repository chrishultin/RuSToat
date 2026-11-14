# Plan: RuSToat for iPhone, milestone 2 (rich messages)

## Summary
Make messages on the iPhone look and act as they do on desktop. That means full Markdown (code blocks, quotes, lists, headings), mentions as chips, timestamps in local time, tap-to-reveal spoilers and custom emoji. It also adds reactions, a message menu (copy text or link, reply, pin), and editing and deleting your own messages. This is milestone iOS 2 of the [iOS roadmap](ios-app.md#ios-roadmap), building on milestone 1 ([plan](ios-app.md), [record](ios-app-implementation.md)).

## Approach
**Markdown is parsed in Rust, with desktop's parser, and drawn natively in SwiftUI.**
- Messages already arrive as the Markdown `markdown::prepare` produces: mentions, timestamps and spoilers as `stoat:` links, and custom emoji as `<img>`.
- A new `ios/stoat-ffi/src/rich_text.rs` parses that with the same `markdown` crate and options desktop uses (`ParseOptions::gfm()` plus math; see `src/markdown.rs:210`).
- It turns the mdast tree into plain records: blocks (paragraph, heading, quote, code, list, rule, math, table), each holding inline runs (text with styles, link, mention, timestamp, spoiler word, custom emoji, line break). Internal links are decoded with `markdown::parse_internal_link`.
- `MessageItem` gains `rich: Vec<RichBlock>`. `body` stays, for copying and for the system-message fallback.
- SwiftUI draws blocks as views. Inline runs become one `Text` per paragraph, built from an `AttributedString` with custom attributes. A `TextRenderer` (iOS 18+) draws mention pills and spoiler bars behind or over their runs. Custom emoji are `Text(Image(uiImage:))` from a small image cache.
- Taps on mentions, spoilers and links go through the `OpenURLAction` milestone 1 set up, using the run's `stoat:` URL.

**Actions follow desktop's optimistic pattern in the controller:**
- Reactions: `has_reacted`, then `apply_react`, then `react`/`unreact`, undoing on failure (`src/ui/chat.rs:3494`).
- Pin: `set_pinned`, then `pin_message`/`unpin_message` (`chat.rs:2847`).
- Edit: `apply_local_edit`, then `edit_message`, then `undo_local_edit` on failure (`chat.rs:2931`).
- Delete: `remove_local`, then `delete_message`, then `restore_local` on failure (`chat.rs:3217`).

**Rejected alternatives:**
- **Apple's `swift-markdown` package.** A new dependency and a different parser (cmark-gfm), so iOS and desktop could split the same message differently, and it can't be tested from Rust.
- **`AttributedString(markdown:)` with `.full` syntax.** It gives block intents, but rebuilding blocks from runs is fiddly. It doesn't pass through the `<img>` HTML for custom emoji, and it still parses differently from desktop.
- **Moving `markdown::prepare` into Swift.** That duplicates the dialect logic (names, roles, spoilers), which is exactly what the shared core exists to avoid.

## Assumptions
1. **Math shows as its TeX source in a monospaced block or span.** Desktop draws it with `ui::math` (GPUI), which iOS can't reuse; rendering it natively is a later step.
2. **Edit and delete are for your own messages only,** as on desktop (`chat.rs:13142`). Moderators deleting others' messages is out of scope.
3. **Mention taps do nothing yet.** Profile cards and opening channels are milestone iOS 4 ("People and places"). Channel mentions still show as chips.
4. **External links whose text differs from their address** (`stoat:link?url=…&text=…`) ask before opening, showing the real address, as desktop warns. Plain links open directly.
5. **The reaction picker is a sheet.** It has a row of the channel's usual choices (`ChatStore::reaction_choices`: common Unicode emoji, then the server's custom emoji) and search over everything (`emoji::search_all`). When the author limited reactions, or the message is full (`MessageRow::picker_choices`), only those choices are offered. There's no skin-tone setting on iOS yet, so the default tone is used.
6. **"Copy Text" copies the Markdown as typed** (`ChatStore::raw_content`), as desktop's "Copy text" does. **"Copy Link"** uses `links::message_link` with the instance's web app (`root.app` from `GET /`), and is hidden when the instance has none.
7. **No pins list.** Pinning and unpinning work from the menu, and pinned messages show a pin. Browsing a channel's pins is left for a later milestone.
8. **Editing happens in the composer:** an "Editing message" chip, the text prefilled, Save and Cancel, with the draft stashed and restored. This is desktop's model (`composer.rs`'s `EditStash`), and it's simpler on a phone than editing in place.
9. **Custom emoji show their first frame.** Animated GIF/WebP emoji move later, with the media milestone.
10. **Desktop's "Quote", "Mark as unread" and "Copy message ID" stay off the iOS menu for now.** They're cheap to add later.

## Open Questions
- **Deferrable:** should channel mentions open the channel now (it needs cross-tab navigation), or wait for milestone iOS 4 with the other "go to" actions?
- **Deferrable:** a quick-reaction strip above the context menu, like iMessage's Tapbacks. It needs a custom preview layout; the sheet covers the function first.

## Plan

### Chunk 1: Rich text blocks
**Goal:** Messages render their full block structure (paragraphs with bold, italics, strikethrough, inline code and links; headings; block quotes; fenced code blocks with a copy button; ordered and unordered lists; rules; tables; math as source), parsed by the same Markdown parser desktop uses.
**Files likely touched:**
- `ios/stoat-ffi/src/rich_text.rs` (new):
  - Types: `RichBlock` (enum), `RichInline` (enum), `InlineStyle` (bold, italic, strike, code flags).
  - `pub fn blocks(prepared: &str) -> Vec<RichBlock>`, using `markdown::to_mdast` with desktop's options.
  - Unhandled mdast nodes fall back to their plain text, never to nothing.
- `ios/stoat-ffi/src/timeline.rs`: `MessageItem.rich`. Embed descriptions that are Markdown get `rich` too.
- `ios/stoat-ffi/src/controller.rs`: cache parsed blocks by `render_key`, so `timeline()` doesn't reparse every row on every change.
- `ios/stoat-ffi/tests/rich_text.rs` (new).
- Swift:
  - `ios/RuSToat/Chat/RichTextView.swift` (new): block views, plus inline runs → `AttributedString`.
  - `ios/RuSToat/Chat/MessageText.swift`: replaced by `RichTextView`, with `attributed(_:)` kept for system rows and reply snippets.
  - `ios/RuSToat/Chat/MessageRowView.swift`.
- `ios/RuSToatTests/RichTextTests.swift` (new), replacing `MessageTextTests.swift`.

**Depends on:** None (milestone 1).
**Risks:**
- **Prepared Markdown escapes.** `prepare` writes `\<`, hard breaks as two trailing spaces, and `<img>` HTML. The tree must turn `Html` nodes that aren't emoji `<img>`s back into literal text, or `<b>` would vanish. Chunk 3 handles the emoji ones; until then they show as `:name:`.
- **Performance.** Parsing every loaded message on each `timeline()` call is the obvious trap; the `render_key` cache avoids it. Watch for jank on a 300-row channel on device.
- **Text selection.** Selection across several `Text` blocks isn't possible in SwiftUI, so each block is selectable on its own. That's acceptable; "Copy Text" (chunk 4) covers the whole message.

**How to verify:**
- `cargo test -p stoat-ffi --test rich_text`, covering:
  - every block kind;
  - nested quote → list → code;
  - inline styles combined (`***bold italic***`, `~~**x**~~`);
  - `<not a mention>` staying literal;
  - hard breaks;
  - an unterminated fence;
  - a GFM table;
  - `$$x^2$$` as math;
  - the cache returning the same blocks for the same `render_key`.
- `scripts/ios/test.sh`, with `RichTextTests` checking that paragraph runs build the expected `AttributedString` attributes.
- Simulator: open #general on `fake_stoat` (`SIMCTL_CHILD_RUSTOAT_DEV_OPEN=ch-general`) after adding a fixture message with every block kind (a new `tests/fixtures/messages_rich.json`, served by `fake_stoat` for a `ch-rich` channel), then screenshot.

**Manual check:** Open a real channel with code blocks and quotes and compare it with desktop.
**Shippable when:** Every block kind renders on iOS, matching desktop's structure, with no reparsing on scroll.

### Chunk 2: Mentions, timestamps, spoilers and guarded links
**Goal:**
- User, role, channel, `@everyone` and `@online` mentions draw as pills (roles in their colour).
- `<t:…>` timestamps show in local time, as desktop formats them.
- Spoilers show as bars until tapped, then stay revealed for the session.
- Links whose text hides their address ask before opening.

**Files likely touched:**
- `ios/stoat-ffi/src/rich_text.rs`: new inlines.
  - `Mention { kind, label, colour }`.
  - `Time { label, full }`: the label from `time::format_timestamp`, the full date for accessibility.
  - `SpoilerWord { index, text, hidden }`.
  - `Link { url, external_text }`.
- `ios/stoat-ffi/src/controller.rs`: `reveal_spoiler(channel, message, index)`, wrapping `ChatStore::reveal_spoiler` and reporting a timeline change.
- Swift:
  - `ios/RuSToat/Chat/RichTextView.swift`: custom `AttributedString` attributes and a `TextRenderer` that draws pills and spoiler bars.
  - `ios/RuSToat/Chat/ChatView.swift`: the `OpenURLAction` handles `stoat:spoiler/…` (reveal) and `stoat:link?…` (a confirmation alert with the real URL), and ignores other `stoat:` links (assumption 3).

**Depends on:** Chunk 1.
**Risks:**
- **`TextRenderer` is new, and it moves runs.** Drawing a pill behind a run means padding it without changing its layout; the pill is drawn inset with a negative outset, not padded, so line heights don't jump. Prototype it first. If it can't look right, fall back to a tinted foreground with bold weight and no background.
- **Tapping a single spoiler word.** Each word is its own link (that's how `prepare` writes them), so a tap reveals that spoiler's index, and every word with the index reveals together.
- **Relative timestamps** ("in 3 hours") go stale. Redraw the open channel each minute while it has any (`markdown::has_relative_time`), as desktop does.

**How to verify:**
- `cargo test -p stoat-ffi --test rich_text`: the inline kinds; a role colour carried through; a revealed spoiler becoming styled text; a guarded link carrying both URL and text.
- `cargo test -p stoat-ffi --test reading`: `reveal_spoiler` changes the row's `render_key`, and its blocks no longer contain `SpoilerWord { hidden: true }`.
- Simulator screenshot of a fixture message with each kind.

**Manual check:** Tap a spoiler on a real message; tap a disguised link and see the warning.
**Shippable when:** Mentions, timestamps, spoilers and guarded links behave as on desktop, apart from mention taps.

### Chunk 3: Custom emoji
**Goal:** Custom emoji show as images inline in text, large in emoji-only ("jumbo") messages, from a shared cache so a channel full of the same emoji loads each image once.
**Files likely touched:**
- `ios/stoat-ffi/src/rich_text.rs`: `Emoji { id, name, url, jumbo }`, from the `<img>` HTML `prepare` writes. Unknown emoji stay as `:id:` text.
- Swift:
  - `ios/RuSToat/Shared/EmojiImages.swift` (new): an `@Observable` cache of `UIImage`s by URL, loaded with `URLSession`, scaled to 20 pt (inline) or 48 pt (jumbo), with `[weak]`-safe in-flight de-duplication.
  - `ios/RuSToat/Chat/RichTextView.swift`: `Text(Image(uiImage:))` once loaded, else the `:name:` placeholder in secondary colour.

**Depends on:** Chunk 1.
**Risks:**
- A `Text` built from images only redraws when the cache changes, so views must read the cache so Observation tracks it; otherwise emoji stay as placeholders until the next scroll.
- Baseline alignment of inline images: offset with `.baselineOffset` to sit like an emoji glyph.

**How to verify:**
- `cargo test -p stoat-ffi --test rich_text`: an `<img>` → `Emoji` with its URL; jumbo; an `<img>` that isn't an emoji stays literal.
- Swift `EmojiImagesTests`: two requests for one URL load it once, against a local `URLProtocol` stub.
- Simulator screenshot (`fake_stoat` serving an emoji PNG).

**Manual check:** A server with custom emoji: they appear inline and in jumbo messages.
**Shippable when:** Custom emoji render inline and jumbo, loaded once per URL.

### Chunk 4: The message menu and pins
**Goal:** Long-pressing a message offers Reply, Copy Text, Copy Link and Pin/Unpin (where allowed). Pinned messages show a pin. Retry and Discard stay for failed messages.
**Files likely touched:**
- `ios/stoat-ffi/src/actions.rs` (new, a second `#[uniffi::export] impl AccountController`):
  - `raw_text(channel, id)`.
  - `message_link(channel, id) -> Option<String>`, using `links::message_link` and the web app from root.
  - `can_pin(channel)`.
  - `set_pinned(channel, id, pinned)`, optimistic with rollback, reporting a timeline change.
- `ios/stoat-ffi/src/controller.rs`: keep `root.app` from `fetch_media`.
- `ios/stoat-ffi/src/timeline.rs`: `MessageItem.pinned`.
- Swift: `ios/RuSToat/Chat/MessageRowView.swift` (the menu, a pin badge) and `ChatView.swift`.
- `ios/stoat-ffi/tests/actions.rs` (new).

**Depends on:** None, but it lands after chunk 1 so the menu's Copy Text matches what's drawn.
**Risks:** A live `MessageUpdate` with `pinned` from another client can race the optimistic flip. The store's `set_pinned` is idempotent, and the server's echo wins, which matches desktop.
**How to verify:**
- `cargo test -p stoat-ffi --test actions`: pin calls `PUT /channels/{c}/messages/{m}/pin` and flips `pinned`; a 403 flips it back and returns the error text; `message_link` uses root's `app`; `raw_text` is the source Markdown.

**Manual check:** Pin a message on the phone and see it pinned on desktop; Copy Link and paste it on desktop.
**Shippable when:** The menu works and pins sync both ways.

### Chunk 5: Reactions
**Goal:**
- Reactions show as chips under a message (emoji or custom emoji, count, highlighted when yours).
- Tapping a chip toggles your reaction; long-pressing it lists who reacted.
- "Add Reaction" in the menu opens a picker sheet with the channel's choices and search.

**Files likely touched:**
- `ios/stoat-ffi/src/timeline.rs`: `MessageItem.reactions: Vec<ReactionItem { key, emoji, url, count, mine, names }>` and `picker: PickerItem`, from `MessageRow::picker_choices`.
- `ios/stoat-ffi/src/actions.rs`:
  - `toggle_reaction(channel, id, emoji)`: optimistic `apply_react`, then `react`/`unreact`, rolled back on error; refused up front when `would_exceed_reactions`.
  - `reaction_choices(channel, id)`.
  - `search_emoji(channel, query) -> Vec<EmojiChoiceItem>`, over `emoji::search_all` and the server's custom emoji.
- Swift: `ios/RuSToat/Chat/ReactionsView.swift` (new, chips in a wrapping `Layout`) and `ios/RuSToat/Chat/ReactionPicker.swift` (new, a sheet with `.searchable` and a grid).
- `ios/stoat-ffi/tests/actions.rs`.

**Depends on:** Chunk 4 (the menu), and chunk 3 for custom emoji images, which it reuses.
**Risks:**
- Percent-encoding the emoji in the URL path for custom emoji versus Unicode: the core's `react` already does it; test it through the FFI.
- Live `MessageReact` echoes double-counting the optimistic one: `apply_react` is idempotent per user, so check it stays at count 1.
- The emoji search set is large (~1,900). Return at most 200 and search in Rust, so Swift never holds the whole list.

**How to verify:**
- `cargo test -p stoat-ffi --test actions`:
  - toggling adds (PUT) and removes (DELETE);
  - the live echo leaves the count at 1;
  - a 400 rolls back;
  - a full message refuses a new emoji without a request;
  - the picker offers only allowed emoji when the author restricted them;
  - search finds `:thumbsup:` and a custom emoji by name.

**Manual check:** React from the phone and see it on desktop; react on desktop and watch the chip update on the phone.
**Shippable when:** Reactions display, toggle and sync, and the picker respects Stoat's limits.

### Chunk 6: Edit and delete your own messages
**Goal:**
- "Edit" puts your message in the composer, prefilled; Save updates it at once, showing "(edited)", and Cancel restores your draft.
- "Delete" asks first, then removes the message at once.
- Both roll back with a message if Stoat refuses.

**Files likely touched:**
- `ios/stoat-ffi/src/actions.rs`:
  - `async edit(channel, id, text) -> Result<(), CoreError>`: `apply_local_edit` (reported as a timeline change before the request), then `edit_message`, then `undo_local_edit` and the error text on failure.
  - `async delete(channel, id) -> Result<(), CoreError>`: `remove_local`, then `delete_message`, then `restore_local` on failure.
  - These are the same shape as chunk 4's `set_pinned` and chunk 5's `toggle_reaction`: the local change is immediate, the `await` covers the request, and Swift shows any error it returns.
- Swift:
  - `ios/RuSToat/Chat/Composer.swift`: edit mode, with an "Editing message" chip and a Save (checkmark) button.
  - `ios/RuSToat/Chat/ChatView.swift`: `editing` state, the draft stash, a delete confirmation, and a brief error banner for a failed action.
  - `ios/RuSToat/Model/AccountModel.swift`.
- `ios/stoat-ffi/tests/actions.rs`.

**Depends on:** Chunk 4.
**Risks:**
- Editing to empty text. Desktop refuses it (an empty edit would need delete); refuse it in `edit` with a clear message.
- An edit whose local `render_key` differs from the server echo's causes one extra redraw. That's harmless.
- A `MessageDelete` echo arriving after `remove_local` is a no-op, so check it doesn't resurrect the message on rollback.

**How to verify:**
- `cargo test -p stoat-ffi --test actions`:
  - edit PATCHes `/channels/{c}/messages/{m}` with the content, and the row shows the new text immediately and `edited`;
  - a 403 restores the old text and returns the error text;
  - delete removes the row immediately, and a 500 restores it in place;
  - editing to blank is refused without a request;
  - others' messages can't be edited or deleted (the controller checks `mine`).

**Manual check:** Edit and delete from the phone and watch desktop update; edit on desktop while the phone has the channel open.
**Shippable when:** You can fix or remove your own messages from the phone, with failures visible and undone.

## Test Plan

**Full-suite command:**
```bash
cargo test && cargo clippy --all-targets -- -D warnings && scripts/ios/test.sh
```
`scripts/ios/test.sh` runs the no-default-features clippy and iOS check, `cargo test -p stoat-ffi`, its clippy, the XCFramework build and the Swift tests on a simulator (first available iPhone, or `IOS_SIMULATOR=…`).

### Unit Tests
- `rich_text.rs`:
  - each block and inline kind;
  - nesting;
  - escaped `<` staying literal;
  - unknown mdast nodes falling back to text;
  - mention kinds and role colours;
  - spoiler words hidden or revealed;
  - guarded links;
  - custom emoji from `<img>`;
  - the `render_key` cache.
- Swift:
  - inline runs → `AttributedString` attributes (styles, link URLs, custom attributes);
  - `EmojiImages` de-duplication;
  - the reaction chip layout wrapping at the row's width (a `Layout` unit test).

### Integration Tests
`ios/stoat-ffi/tests/actions.rs`, against wiremock and the fake events socket from `tests/support`:
- pin, then unpin, then a 403 rollback;
- reaction toggle, its echo, rollback, and the full-message refusal;
- the picker's allowed-only choices, and emoji search;
- edit, then its echo, then a 403 undo;
- delete, then a 500 restore;
- `reveal_spoiler` changing the row.

### Manual Tests
1. A channel with code blocks, quotes, lists and a table looks like it does on desktop.
2. Mentions are pills (roles in colour); a `<t:…:R>` timestamp updates over time.
3. A spoiler stays hidden until tapped, and a disguised link warns before opening.
4. Custom emoji show inline and large in emoji-only messages.
5. React and unreact both ways between phone and desktop; a message at 20 different emoji won't take a 21st.
6. Pin and unpin; Copy Link opens the message on desktop's web app.
7. Edit and delete your own messages; in Airplane mode, see them roll back with a message.
8. Light and dark mode: pills, spoiler bars and code blocks stay legible.

## Definition of Done
- [ ] `cargo test && cargo clippy --all-targets -- -D warnings && scripts/ios/test.sh` passes.
- [ ] Messages render paragraphs, headings, quotes, code blocks (with copy), lists, rules, tables and math source, parsed by `markdown::to_mdast` with desktop's options in `stoat-ffi`, and cached by `render_key`.
- [ ] Mentions draw as pills (roles in their colour), timestamps in local time, spoilers hidden until tapped, and guarded links ask before opening.
- [ ] Custom emoji render inline and jumbo, each URL loaded once.
- [ ] The message menu offers Reply, Copy Text, Copy Link (when the instance has a web app), Pin/Unpin (when `can_pin`), and Retry/Discard for failed messages.
- [ ] Reactions show, toggle optimistically with rollback, list who reacted, and the picker respects allowed emoji and the 20-emoji limit.
- [ ] Your own messages can be edited in the composer and deleted after confirmation, both updating at once and rolling back on failure; others' can't.
- [ ] Every new action is covered by a wiremock test in `ios/stoat-ffi/tests/actions.rs`.
