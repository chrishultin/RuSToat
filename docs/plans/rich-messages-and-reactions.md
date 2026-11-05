# Plan: Rich messages and reactions (RuSToat milestone 2)

## Summary
Make messages look like Stoat messages instead of plain text, so the client is pleasant to read and closer to judging GPUI as a real foundation. This milestone adds:
- avatars
- Markdown formatting
- custom emoji
- inline image attachments and file cards
- link and bot embeds
- reactions: shown under messages, live-updated, and toggled with a picker

Sending, replies, edit/delete, typing and unreads are milestone 3, which gets its own plan afterwards.

## Approach
Keep the milestone 1 architecture:
- **Pure display model:** `ChatStore::timeline()` builds plain display rows with no GPUI types, so every rule is unit-testable.
- **Thin rendering:** `ChatView` renders those rows inside gpui-component's `MessageScroller`.
- **Backend types:** all wire types continue to come from `revolt-models`.

This milestone widens `MessageRow` from text into a small display model: avatar URL, Markdown source, attachment views, embed views and reaction chips.

**Rendering pieces:**
- **Markdown:** gpui-component's `TextView::markdown(id, source)`. Its parse state lives in `window.use_keyed_state(id)`, so it's parsed once per element ID, not per frame. Documents under 4 KB (`MAX_SYNC_FULL_REPLACE_BYTES`) parse synchronously, which gives an exact height on first layout. Stoat messages are capped at 2000 characters, so rows won't jump.
- **Stoat-specific syntax** is converted into standard Markdown before rendering:
  - `<@user>` becomes a styled link
  - `<#channel>` becomes a styled link
  - `:ULID:` (custom emoji) becomes an inline image resolved through `TextView::image_source`
  - `||spoiler||` gets a marker style

  `MarkdownPlugin`s can't do this, because they replace whole mdast nodes and can't split a sentence's text node.
- **Images** reuse the `img()` + `ReqwestHttpClient` path from milestone 1 (server icons).

**Media facts, verified on 2026-11-04 against the live service:**
- **Uploaded files** come from `{cdn}/{tag}/{id}`, where `cdn` is `features.autumn.url` (`https://cdn.stoatusercontent.com`). The CDN **ignores resize parameters**: `?max_side=`, `?width=` and `?size=` all return the original bytes. So every image arrives at full size and GPUI scales it.
- **Users without an avatar:** `GET {api}/users/{id}/default_avatar` returns a PNG and needs **no auth**, which matters because GPUI's image loader doesn't send our session header.
- **External images** (embeds, masquerade avatars, embed icons) go through Stoat's proxy, January (`features.january.url` = `https://proxy.stoatusercontent.com`), as `{january}/proxy?url=<percent-encoded>`. It returned `200 image/webp` for a public PNG. This is what Stoat's own clients do, so remote sites never see the user's IP.
- **Custom emoji** are at `{cdn}/emojis/{id}`. Their names come from `Ready.emojis`, which means adding `emojis` to the requested `Ready` fields.
- **Link previews arrive after the message,** as a `MessageAppend { id, channel, append: { embeds } }` event. That event has to be handled or previews never show live.
- **Reactions:**
  - Add: `PUT /channels/{id}/messages/{msg}/reactions/{emoji}`
  - Remove: `DELETE` on the same path
  - Live events: `MessageReact` / `MessageUnreact` / `MessageRemoveReaction` (fields `id`, `channel_id`, `user_id`, `emoji_id`)
  - `Message.reactions` is `IndexMap<emoji, IndexSet<user_id>>`
  - `Message.interactions` has `reactions` (a preset list) and `restrict_reactions`

**Rejected alternatives:**
- **Hand-written Markdown rendering:** too much work for a POC, and `TextView` already handles lists, code, quotes, links and selection.
- **A `MarkdownPlugin` per Stoat syntax:** plugins can't split text nodes.
- **A third-party emoji-picker crate:** none found in the gpui ecosystem. A small custom grid in a gpui-component `Popover` is enough.

## Assumptions
1. **Code blocks** render monospace without syntax highlighting. Highlighting needs gpui-component's `tree-sitter-*` features, a large build cost for a POC.
2. **Spoilers** (`||text||`) render with a distinct muted background, but **visibly**. Click-to-reveal is deferred, because it needs per-span interaction state that a Markdown preprocessor can't express.
3. **Video and audio** attachments and embeds show as cards (name, size, open in browser). There's no in-app playback.
4. **Image size guard:** because the CDN only serves originals, images over **8 MB** (from `File.size`) show as a file card with a "Show image" button instead of loading automatically. Smaller images load as they scroll into view, since the list is virtualized and only visible rows render.
5. **Display size for inline images:** fit within 400×300 logical px, keeping the aspect ratio. Space is reserved up front from `File.metadata` (`Image { width, height }`), so rows never change height when the bytes arrive.
6. **Link clicks:** http(s) links in messages and embeds open in the default browser (`cx.open_url`) with no "leaving Stoat" confirmation. `stoat:` links (mentions) do nothing yet.
7. **Avatar precedence** follows the author-name order from milestone 1:
   - masquerade avatar (external URL, proxied)
   - webhook avatar
   - server member avatar
   - user avatar
   - `default_avatar`

   Avatars appear only on header rows. Continuation rows keep an empty gutter of the same width.
8. **Emoji picker:** about 40 common Unicode emoji plus the current server's custom emoji. No search and no full Unicode catalog. When `interactions.restrict_reactions` is set, the picker shows only `interactions.reactions`.
9. **Reaction permissions** aren't precomputed with `revolt-permissions`. If the server refuses (`MissingPermission`), the optimistic change is rolled back and a short error shows in the row.
10. **Animated emoji, GIFs and animated WebP** render however GPUI's `img()` handles them (animated if supported, otherwise the first frame). No custom animation work.
11. **Unicode emoji** in text render through the system font (Apple Color Emoji). Shortcodes like `:smile:` aren't converted, because Stoat clients already send Unicode.

## Open Questions
- *(Status)* **Paused after chunk 3** (2026-11-04) at the user's request so that `docs/plans/unreads-and-notifications.md` can run first. Resume at chunk 4 (images and file cards) afterwards.
- *(Deferred, not blocking)* Milestone 3 ("interaction": send, reply, edit/delete own messages, typing indicators, unreads and mark-as-read) needs its own plan once this one is done.

## Plan

### Chunk 1: Avatars and media URLs
**Goal:** Show a round avatar next to each message group (and in the DM list and user panel), using correct, privacy-preserving URLs for every image source. Every later chunk reuses these URL helpers and the new row layout.
**Files likely touched:**
- `src/api/client.rs`: `RootFeatures.january`, plus `RootConfig::proxy_server()`
- `src/media.rs` (new): `MediaUrls { api, files, proxy }` with:
  - `file(&File)`
  - `default_avatar(user_id)`
  - `proxied(external_url)`, percent-encoded using the `url` crate already in the tree
  - `emoji(id)`
- `src/store.rs`:
  - `ChatStore::set_media(MediaUrls)`
  - `avatar_url(&Message) -> Option<String>`
  - `MessageRow.avatar`
  - `ChannelEntry.avatar` for DMs
- `src/ui/chat.rs`: avatar gutter in `render_message_row`, DM row avatars, user-panel avatar
- `tests/media_urls.rs`
**Depends on:** None (builds on milestone 1)
**Risks:**
- Masquerade avatar URLs are external strings, so they must be proxied, not passed to `file()`.
- `User.avatar` can be stale in `Ready` for long sessions, but `UserUpdate` events are out of scope, so accept it.
- About 50 visible avatars fetched at once on first open is fine over HTTP/2, and GPUI's asset cache dedups repeat authors.
**How to verify:**
- `cargo test --test media_urls` covers:
  - file URLs
  - default-avatar URL
  - proxied URL encoding (spaces, `&`, unicode)
  - avatar precedence for masquerade, webhook, member, user and default
- `cargo test --test store_history` still passes, with avatar fields added.
- Opt-in live test (`cargo test --test live_media -- --ignored`): `GET default_avatar` and `GET {january}/proxy?url=…` both return `200 image/*`.
**Manual check:** Avatars match the web app for a few authors, including someone with no avatar (default) and, if available, a bridge or bot with a masquerade.
**Shippable when:** Message groups, DMs and the user panel show the right avatars, and the URL tests pass.

### Chunk 2: Markdown message bodies
**Goal:** Render message content as Markdown (bold, italics, links, lists, quotes, inline and block code), keeping mentions readable and clickable-looking. This is the biggest visual change.
**Files likely touched:**
- `src/markdown.rs` (new): pure `prepare(content, &ChatStore) -> String`:
  - `<@id>` becomes `[@Name](stoat:user/id)`
  - `<#id>` becomes `[#name](stoat:channel/id)`
  - `||x||` is wrapped for the spoiler style
  - user text is escaped where a replacement could inject Markdown
- `src/store.rs`: `MessageRow.markdown` replaces `body` for normal messages. `render_content` folds into `markdown::prepare`; system messages stay plain text.
- `src/ui/chat.rs`:
  - `TextView::markdown(row_key, source)`, keyed by `"{message_id}:{edited}"` so an edit re-parses
  - `.selectable(true)`
  - `on_link_click`: http(s) goes to `cx.open_url`, `stoat:` is ignored
  - a style pass so text size and colors match the rest of the UI
- `tests/markdown_prepare.rs`
**Depends on:** Chunk 1 (row layout)
**Risks:**
- **Keyed state and edits:** `TextView` keeps the parsed document per element ID, and only uses the source when the state is first created. Keying by message ID alone would leave edited messages showing old text. The key must include the edit timestamp (or a content hash). Add a test that the key changes when `edited` changes.
- **Per-row cost:** each row creates a `TextViewState`, and keyed state is dropped once a row stops rendering. Check scrolling stays smooth with 300 or more loaded messages in a busy channel. If not, cache prepared sources per message in the store.
- **Escaping:** a display name containing `]` or `*` must not break the link syntax. Test hostile names.
- **Style:** `TextView`'s default font sizes may not match `text_sm` rows. Adjust with `TextViewStyle` and check light and dark themes.
**How to verify:** `cargo test --test markdown_prepare` covers:
- mentions, including an unknown user, which keeps the raw text
- channel links
- spoilers
- escaping of hostile names
- code spans and blocks left untouched (mentions inside code aren't replaced)
- that the row key changes on edit
**Manual check:** In a channel with formatted messages, bold, italics, links, lists, quotes and code look right. Clicking a link opens the browser. Editing a message from another client updates the formatting live.
**Shippable when:** Formatted messages render as formatted text, links open, and the tests pass.

### Chunk 2b: Message bubbles and grouping *(inserted at the user's request after chunk 2)*
**Goal:** Put each message in a chat bubble, built from gpui-component's `Message` / `MessageAvatar` / `MessageHeader` / `MessageContent` / `Bubble` / `BubbleContent` components. Your own messages go on the right; consecutive messages from one sender stack as one unit.
**Files likely touched:** `src/store.rs` (`MessageRow.group_end`, `MessageRow.mine`, `mark_group_ends`), `src/ui/chat.rs` (`render_message_row` and `render_system_row`), `tests/store_history.rs`
**Depends on:** Chunks 1 and 2
**Design (the user chose "Chat bubbles"):**
- Others' messages are left-aligned with `BubbleVariant::Secondary`. Yours are right-aligned with `BubbleVariant::Tinted`; `Tinted` rather than `Filled`, so link colours in Markdown stay readable.
- The header (name and time, or "You") appears only on a group's first bubble. The avatar sits beside the group's last bubble (gpui-component anchors it to the bottom). Earlier bubbles keep an empty `MessageAvatar` slot, so they line up.
- Corners are 16 px, except on the sender's side where bubbles meet (4 px). Each row knows its position from `continuation` (not first) and `group_end` (last). gpui-component's `BubbleGroup`/`MessageGroup` are plain stacks with no corner shaping, and a group can't be one element anyway, because `MessageScroller` renders one row per message.
- System messages are centred, small and muted.
**Shippable when:** Messages render as bubbles with grouping and own-message alignment, and the tests for `group_end` and `mine` pass.

### Chunk 3: Custom emoji
**Goal:** Show server custom emoji inline in messages (`:ULID:`) instead of raw IDs, with larger "jumbo" emoji when a message is only emoji. Chunk 7's picker reuses this.
**Files likely touched:**
- `src/api/events.rs`: add `"emojis"` to `READY_FIELDS`; `Ready.emojis` parsed leniently into `revolt-models` `Emoji`
- `src/store.rs`: `emojis: HashMap<String, Emoji>`, kept up to date from `Ready`. `EmojiCreate`/`EmojiDelete` events are optional; add them if they're simple.
- `src/markdown.rs`: `:ULID:` becomes `![:name:](stoat-emoji:ID)`. Validate the ULID first, so things like `:smile:` and times like `12:30:45` are left alone. Messages that are only emoji are flagged as jumbo.
- `src/ui/chat.rs`: `TextView::image_source` maps `stoat-emoji:ID` to `img(MediaUrls::emoji(id))`, sized to the line height (or 48 px for jumbo)
- `tests/markdown_prepare.rs`: emoji cases
**Depends on:** Chunk 2
**Risks:**
- **Inline image sizing (the main risk):** `TextView` sizes Markdown images from their intrinsic size, and emoji files can be 128 px or more. If `image_source` and `TextViewStyle` can't constrain inline images to the line height, fall back to rendering `:name:` as styled text and note it in the record. Spike this first.
- **Unknown emoji:** emoji from servers the user isn't in aren't in `Ready`. They still render from the CDN by ID, but the name falls back to `emoji`.
**How to verify:** `cargo test --test markdown_prepare` covers:
- a known emoji
- an unknown emoji (still an image, generic name)
- non-ULID shortcodes and times left alone
- jumbo detection (whitespace plus emoji only, at most 10 emoji)
**Manual check:** A message using a server's custom emoji shows the image at text size, and an emoji-only message shows it larger, matching the web app.
**Shippable when:** Custom emoji render inline, or the fallback is in place and recorded, and the tests pass.

### Chunk 4: Image attachments and file cards
**Goal:** Show image attachments inline at a sensible size, and other files as cards. This replaces the `[attachment: name]` placeholder.
**Files likely touched:**
- `src/store.rs`: `AttachmentView` is either:
  - `Image { url, display_w, display_h, too_big }`, with display size from `File.metadata` per Assumption 5
  - `File { name, size_label, url, kind }`, where `kind` is image, video, audio or file
- `src/ui/chat.rs`:
  - `img(url)`, sized exactly to the precomputed dimensions, with a shimmer or blank placeholder while loading
  - *(After chunk 2b)* images go inside the message's bubble surface. A message that's only an image keeps its normal bubble variant but makes the surface invisible (`bg(transparent_black()).p_0()` on `BubbleContent`), so the picture isn't framed by a coloured box. **Don't use `BubbleVariant::Ghost`:** gpui-component stretches Ghost bubbles to the full row width, which left-aligned the user's own emoji-only messages after chunk 3.
  - a file card with icon, name and size, opening in the browser on click
  - "Show image" for images over 8 MB, with per-message "revealed" state in `ChatView`
- `src/assets.rs`: extra Lucide icons for file kinds (`File`, `FileImage`, `FileVideo`, `FileAudio`, `Download`)
- `tests/store_history.rs` and fixtures: image (with metadata), large image, video, generic file
**Depends on:** Chunk 1
**Risks:**
- `File.metadata` may be `Image { width, height }`, `Video { … }`, `File`, `Text` or `Audio`. Missing or zero dimensions need a fallback box (e.g. 300×200) and must never divide by zero.
- Full-size downloads (no CDN resizing) can be several MB each. Virtualization limits this to visible rows, and the 8 MB guard covers the worst cases. Watch memory in a channel full of screenshots.
- The display size must exactly match what's reserved, or row heights shift. Unit-test the sizing maths, including rounding.
**How to verify:** `cargo test` with new store tests covering:
- fit-within-400×300 sizing for wide, tall and tiny images (no upscaling)
- zero and missing dimensions
- the 8 MB guard
- the file-kind classification
- the human-readable size labels (`1.2 MB`)
**Manual check:** A channel with screenshots shows them inline at the right aspect ratio, with no jumping when they finish loading. A PDF or zip shows a card that opens in the browser.
**Shippable when:** Images show inline and other files show as cards, matching the web app's layout closely enough, and the tests pass.

### Chunk 5: Embeds (link previews and bot embeds)
**Goal:** Show link previews and bot embeds under messages, live (they arrive after the message), with every external image going through the proxy.
**Files likely touched:**
- `src/api/events.rs`: `ServerEvent::MessageAppend { id, channel, append: AppendMessage }`
- `src/store.rs`:
  - `apply_append`, which extends `message.embeds`
  - `EmbedView`, built from `revolt-models` `Embed`:
    - `Website`: site name, title (link), description, thumbnail or large image, and the colour bar. `Special` (YouTube, Twitch and so on) is shown as a website card with a link.
    - `Image` / `Video`: an image embed, or a video as a thumbnail card
    - `Text` (bots): title, description (Markdown) and media file
    - `None`: skipped
- `src/ui/chat.rs`: the embed card renderer. Images are sized from the embed's `width`/`height`, using the chunk 4 sizing helper.
- `src/media.rs`: every external `url` / `icon_url` goes through `proxied()`
- Fixtures: one of each embed type, plus a `MessageAppend` event
**Depends on:** Chunks 1, 2 and 4 (proxy helper, Markdown for `Text` descriptions, image sizing)
**Risks:**
- **Live preview path:** previews almost always arrive via `MessageAppend` a moment after the message. Without that event they only appear after a reload. Test the event path explicitly, and run `diff_rows` remeasure on append.
- **Missing fields:** `colour` is a CSS colour string, possibly named or hex. Parse hex only and fall back to the theme border.
- **Variants:** `WebsiteMetadata` fields are all optional, so cards must handle a title-only or image-only preview.
**How to verify:**
- `cargo test` for each `Embed` variant mapping to `EmbedView`, including proxied URLs, missing fields and the colour parse.
- A `MessageAppend` decode test in `events.rs`.
- In `store_live`, `apply_append` adds embeds to a loaded message, and `diff_rows` yields `Remeasure`.
**Manual check:** Post a link (to a news site or YouTube) from another client. The preview appears under the message within a few seconds, without reloading. A bot embed in a server renders its title and description.
**Shippable when:** Link and bot embeds render and appear live, all external images are proxied, and the tests pass.

### Chunk 6: Reactions display (live)
**Goal:** Show reaction chips under messages (emoji plus count, highlighted when you've reacted) and keep them live.
**Files likely touched:**
- `src/api/events.rs`: `MessageReact`, `MessageUnreact`, `MessageRemoveReaction` (note the `channel_id` field name)
- `src/store.rs`:
  - `apply_react(channel, id, user, emoji, add)` and `apply_remove_reaction`
  - `ReactionChip { emoji: Unicode(String) | Custom { id, url, name }, count, mine }`, in `Message.reactions` order
- `src/ui/chat.rs`: reaction chips under the bubble, using gpui-component's `Bubble::reactions(BubbleReactions)` slot (which positions them on the bubble edge) added in chunk 2b. Custom emoji chips use the chunk 3 image path.
- `tests/store_live.rs`: reaction cases
**Depends on:** Chunk 3 (custom emoji rendering)
**Risks:**
- **Field names:** the reaction events use `channel_id`, not `channel` like the other message events. A test with the real field names guards this.
- **Unicode vs custom keys:** emoji keys are either Unicode strings or emoji ULIDs. Tell them apart by validating the ULID, not by length.
- **Empty reactions:** a reaction whose last user is removed must disappear rather than show "0".
**How to verify:** `cargo test` covers:
- decoding all three events
- adding the first reaction
- a second user adding the same emoji (count 2)
- unreacting to zero (chip removed)
- `MessageRemoveReaction` clearing all users
- the `mine` flag following `ChatStore::me()`
- `diff_rows` producing `Remeasure` for the row
**Manual check:** React and unreact to a message from the web app. Chips appear, count and disappear here live.
**Shippable when:** Reaction chips render and update live, and the tests pass.

### Chunk 7: Add and remove reactions (picker)
**Goal:** Let the user toggle their own reactions: click a chip to add or remove theirs, or use "+" to pick a new emoji. This milestone's first write action.
**Files likely touched:**
- `src/api/client.rs`: `react(channel, message, emoji)` (`PUT`) and `unreact(…)` (`DELETE`), with the emoji percent-encoded in the path for Unicode
- `src/ui/chat.rs`:
  - clicking a chip toggles it; a hover "+" on a message opens the picker
  - optimistic store update, then the request; rollback and an inline error on failure
  - de-duplication when the live `MessageReact` echo arrives
- `src/ui/emoji_picker.rs` (new): a gpui-component `Popover` with a grid of about 40 common Unicode emoji plus the current server's custom emoji (from the chunk 3 store), restricted to `interactions.reactions` when `restrict_reactions` is set
- `tests/api_reactions.rs`: `wiremock` tests
**Depends on:** Chunk 6
**Risks:**
- **Echo:** the server echoes our own reaction as a `MessageReact` event. Applying it on top of the optimistic update must be idempotent; `IndexSet` semantics in the store already guarantee that, but test it.
- **Encoding:** Unicode emoji in URL paths must be percent-encoded (`%F0%9F%91%8D`). Test the exact path that's sent.
- **Permissions:** without permission the server returns `MissingPermission`. Roll back and show "You can't react here" without crashing.
- **Hover UI:** hover-only "+" buttons have to work with GPUI's hover tracking inside a virtualized list. Fallback: always show a faint "+" after the chips.
**How to verify:** `cargo test --test api_reactions` covers:
- the `PUT` and `DELETE` paths for a Unicode and a custom emoji, with the session header
- a 403 `MissingPermission` mapped to `ApiError::Api`

Store tests cover the optimistic add followed by the echo (still count 1) and rollback on failure.
**Manual check:** Add a 👍 and a custom emoji to a message here. The web app shows them, and clicking the chip again removes yours. In a channel where reactions are restricted, the picker only offers the allowed ones.
**Shippable when:** Reactions can be toggled from the picker and chips, survive the server echo without double counting, and failures roll back cleanly.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`. Optional live checks: `cargo test -- --ignored`, which runs the existing API drift, events handshake and Keychain checks, plus chunk 1's new `live_media`.

### Unit Tests
- `media.rs`:
  - file, emoji and default-avatar URLs
  - `proxied()` percent-encoding
  - avatar precedence for masquerade, webhook, member, user and default
- `markdown::prepare`:
  - mention and channel links
  - unknown IDs left raw
  - hostile-name escaping
  - spoilers
  - custom emoji (known, unknown, non-ULID left alone, times left alone)
  - jumbo detection
  - nothing replaced inside code
- The row key changes when a message is edited.
- Attachment sizing: fit-within box, no upscaling, zero or missing dimensions, rounding.
- The 8 MB guard, file-kind classification and size labels.
- `Embed` to `EmbedView` for every variant, with proxied URLs, missing fields and the colour parse.
- Reactions:
  - chip building (order, count, `mine`, Unicode vs custom)
  - react, unreact and remove-all
  - an empty reaction disappearing
  - idempotent echo after an optimistic add

### Integration Tests
- `wiremock`:
  - reaction `PUT`/`DELETE` paths, encoding and session header
  - a `MissingPermission` error
- Event decoding (`parse_frame`) for `MessageAppend`, `MessageReact`, `MessageUnreact` and `MessageRemoveReaction`, using the real field names.
- Store pipelines: a fixture page, then `MessageAppend`, then `timeline()` shows the embed. A react event changes `timeline()`, and `diff_rows` gives a `Remeasure`.
- Opt-in live: `default_avatar` and the January proxy return images.

### Manual Tests
1. Open a busy server channel:
   - avatars match the web app
   - formatted text renders
   - custom emoji show at text size
   - screenshots show inline without rows jumping
2. Scroll through 300 or more messages. Scrolling stays smooth and memory stays reasonable (Activity Monitor).
3. Post a link from another client. The preview appears live.
4. React from the web app. The chip appears here. Then react here with 👍 and with a custom emoji, and both appear in the web app. Click your chip to remove it.
5. Try reacting in a channel without permission. The change rolls back with an error.
6. Switch between light and dark mode. Text, code, embed cards and chips stay legible.

## Definition of Done
- [ ] **Avatars:** message group headers show the author's avatar with the precedence in Assumption 7. Users without one get `GET /users/{id}/default_avatar`. External avatar URLs load via `{january}/proxy?url=`.
- [ ] **Markdown:** message bodies render bold, italics, links, lists, quotes, inline code and code blocks. http(s) links open in the default browser. Edited messages re-render with the new text.
- [ ] **Mentions:** `<@id>` and `<#id>` render as styled `@Name` and `#channel`. Hostile names can't break the formatting (tested).
- [ ] **Custom emoji:** `:ULID:` emoji render inline at text size from `{cdn}/emojis/{id}`, and emoji-only messages render them larger. If that proves impossible, a recorded fallback shows `:name:` text.
- [ ] **Image attachments:** images render inline within 400×300, keeping the aspect ratio. Space is reserved from metadata, so rows don't change height on load. Images over 8 MB need a click to load.
- [ ] **Other attachments:** non-image files show a card (icon, name, size) that opens the file URL in the browser.
- [ ] **Website embeds:** render site name, title, description, image and colour bar. Image, video and text embeds render as cards. Every external image URL goes through the January proxy.
- [ ] **Live previews:** a link preview appears without reloading, via `MessageAppend`.
- [ ] **Reaction chips:** emoji (Unicode or custom image) plus count, highlighted when you've reacted. They update live for `MessageReact`, `MessageUnreact` and `MessageRemoveReaction`, and disappear at zero.
- [ ] **Reacting:** clicking a chip toggles your reaction. "+" opens a picker with common emoji and the server's custom emoji, limited to `interactions.reactions` when `restrict_reactions` is set.
- [ ] **Reaction requests:** reactions are sent with the correct percent-encoded `PUT`/`DELETE` paths (tested). The server echo doesn't double count, and refused reactions roll back with a visible error.
- [ ] **Checks:** `cargo test && cargo clippy --all-targets -- -D warnings` passes, and the opt-in live media checks pass.
