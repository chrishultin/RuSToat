# Plan: Attachments and autocomplete in the message box (RuSToat milestone 4)

## Summary
Milestone 3 made RuSToat a chat client, but its message box takes only typed text. This milestone finishes the message box with three things:
- **Attachments:** files from an attach button, paste or drag-and-drop. They upload when the message is sent, with progress.
- **Autocomplete:** `@people`, `#channels` and `:emoji:`, with mentions shown as atomic chips in the box.
- **An emoji button:** a full emoji picker with categories and search.

## Approach
Keep the milestone 1–3 architecture:
- **Pure model, thin views:** rules live in GPUI-free modules (`ChatStore`, `draft.rs`, new `complete.rs` and `emoji.rs`). `ChatView` and `Composer` only wire them up.
- **Backend types:** wire types come from `revolt-models`.
- **Optimistic writes:** writes go through milestone 3's pending flow. A sent message appears at once as a pending row, and is confirmed by matching its `Idempotency-Key` to the created message's `nonce`.

**Context from milestone 3** (read `docs/plans/sending-and-interaction.md` and `docs/plans/sending-and-interaction-implementation.md` before starting):
- **`src/ui/composer.rs`:** `Composer` owns a `TextareaState`.
  - It's built with `auto_grow(1, 8)` and `submit_on_enter(true)`.
  - It emits `ComposerEvent::{Send { text, replies }, Edit, EditLast, Input { empty }}`.
  - It keeps a `Draft { text, replies }` per channel, and an `EditStash` that sets the draft aside during edits.
  - It catches Up with `capture_action::<MoveUp>` and Esc with `on_action::<Escape>`. The textarea propagates `Escape` but not `MoveUp`.
- **`src/store.rs`:**
  - `PendingMessage { key, nonce, channel, content, replies, state, sent_ms, nonce_used }`, with `add_pending`, `fail_pending(…, answered)`, `retry_pending(…, fresh_nonce)` and `discard_pending`.
  - `apply_message` confirms by `nonce`. Pending rows are built through `pending_message()` → `pending_as_message()` and the shared `message_row()`.
- **`src/ui/chat.rs`:**
  - `send_message` → `post_pending` handles `DuplicateNonce` as "already sent".
  - A retry keeps the nonce after a network failure and uses a fresh one after an error response. Stoat spends the key as soon as the request arrives.
- **`src/draft.rs`:** `check_send`, `Drafts`, `EditStash` and `new_nonce`.

**Facts this plan relies on** (checked against Stoat 0.16.2 and the pinned crates):
- **Uploads (Autumn).** The file server is `features.autumn.url` from `GET /` (`RootConfig::file_server()`; live: `https://cdn.stoatusercontent.com`; `GET` there answers `{"autumn":"Hello, I am a file server!","version":"0.16.2"}`).
  - An upload is `POST {autumn}/attachments`, multipart, with the file in the `file` field and the `x-session-token` header. The response is `{"id": "<file id>"}`.
  - **This upload shape comes from Stoat's clients and isn't in the pinned crates. Confirm it in chunk 1** with the first real upload (manual check), before building on it.
- **Limits (live `GET /`):** `features.limits.{default,new_user}.file_upload_size_limits.attachments = 20000000` (note the plural key, unlike `Revolt.toml`), and `message_attachments = 5`. Use the smaller of `default` and `new_user`, falling back to 20 MB and 5 if they're missing.
- **Sending with attachments:** `DataMessageSend.attachments: Option<Vec<String>>` (file IDs). A message may have empty `content` if it has attachments (`revolt-database` `models/messages/model.rs`, the `EmptyMessage` check). More than the limit gives `TooManyAttachments { max }`.
- **A file can be claimed by only one message** (`File::use_attachment` → `find_and_use_attachment`). Claiming happens after the idempotency key is spent and before the message is created. So after Stoat **answers** a send with an error, the uploaded IDs may already be claimed and must be uploaded again. After a **network** failure they're kept: the retry reuses the same key, and `DuplicateNonce` then means it was sent.
- **reqwest 0.13** needs its `multipart` and `stream` features. Progress comes from a counting stream: `multipart::Part::stream_with_length(Body::wrap_stream(…), len)`.
- **GPUI (gpui-pre 0.3.11) and gpui-component 0.9:**
  - File picker: `cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt })`, which returns a `oneshot` of `Result<Option<Vec<PathBuf>>>`.
  - Drag-and-drop: `.on_drop::<ExternalPaths>(…)` and `.drag_over::<ExternalPaths>(…)`.
  - Paste: `Textarea::on_paste(|&ClipboardItem, …| -> bool)`, where `true` consumes the paste. Its entries are `ClipboardEntry::{String, Image(Image { format, bytes }), ExternalPaths}`.
  - Image previews from bytes: `img(Arc::new(Image::from_bytes(format, bytes)))`. From a file: `img(path)`.
- **Inline tokens (gpui-base 0.9):** `TextareaState` supports atomic tokens.
  - API: `InlineToken::new(id, text).with_label(label)`, `replace_range_with_token(range, token, …)`, `content()` and `tokens()`, and `set_value(InputContent::new(text).with_token(range, token)?)`.
  - A token's `text` is what's in `value()`, and `label` is what's drawn. `Textarea::token(|ctx, …| element)` draws tokens as custom elements.
  - So a mention token has the text `<@ID>` and the label `@Ada`. `value()` is already the wire text, and the chip deletes as one unit.
- **There's no completion menu for `TextareaState`.** Completion providers exist only on `EditorState` (`TextareaMode`'s `Extras` is `()`). The popup is ours.
  - Text editing for it: `cursor()`, `selected_range()`, `set_selected_range(range)` then `replace(text)`, and `insert(text)`.
  - Keys to intercept while it's open: `MoveUp`, `MoveDown`, `Enter` (with `submit_on_enter` the textarea propagates it), `IndentInline` (Tab, bound to `"tab"`), and `Escape` (propagated).
- **Members:** `GET /servers/{id}/members` returns `AllMemberResponse { members, users }`. There's no search endpoint in 0.16.2, so this is the whole list.
- **Unicode emoji data** isn't in the tree. Add the `emojis` crate (MIT/Apache-2.0, no dependencies). It provides groups, names and GitHub/gemoji shortcodes. Check its license and API (`emojis::iter()`, `Emoji::shortcodes()`, `Emoji::group()`, `get_by_shortcode`) when chunk 5 starts.

**Rejected alternatives:**
- **Uploading as soon as a file is attached:** sending would be instant, but it wastes uploads for abandoned drafts and needs a second progress UI. Uploading on send reuses the pending row, Retry and Discard.
- **Mentions as plain `<@id>` text** (no tokens): fragile and unreadable while typing. Tokens are built in.
- **The editor's completion menu:** it's only available on the code editor, not the textarea.
- **Searching members on the server:** Stoat 0.16.2 has no search endpoint. A lazily fetched list per server is what Stoat's web client does.

## Assumptions
1. **Files upload when the message is sent (user decision).**
   - Attached files wait in a tray under the box, each removable with ×. Enter creates the pending message at once, with its attachments and per-file progress. The files then upload one at a time, then the message is posted.
   - Retry continues from the first file not yet uploaded. After an error response from Stoat it uploads them all again (see Facts).
2. **Three ways to attach:** the attach (paperclip) button, paste (images and copied files), and drag-and-drop onto the conversation.
3. **Limits:** files over the size limit or past the count limit are refused at attach time with a toast. Text can be empty when there are attachments.
4. **Mentions cover people and `#channels` (user decision).** Roles, `@everyone` and `@online` are out of scope: they need permissions this client doesn't compute yet.
5. **@ candidates in servers come from the full member list (user decision)**, fetched the first time `@` is typed in that server and cached for the session. People already cached locally (recent authors in the channel, DM and group recipients) show at once.
6. **The emoji button opens a full picker (user decision):** search, Unicode emoji by category, then the server's custom emoji. It's a new picker. It reuses the reaction grid's cell rendering, not the 40-emoji reaction set.
7. **Custom emoji** picked or completed become inline tokens: the text is `:ID:` and the image is drawn in the box. Unicode emoji are inserted as plain characters.
8. **Typing the closing `:` of an exact shortcode** (`:smile:`) replaces it with the emoji, like Slack. Text that doesn't match is left as typed.
9. **Autocomplete opens** for `@`, `#` and `:` at the start of the text or after whitespace or opening punctuation, and not inside inline code or a fenced code block. `:` needs at least 2 characters after it. At most 8 suggestions are shown.
10. **Attachments can't be edited** (Stoat edits text only). While editing, the tray is set aside with the rest of the draft (`EditStash` already stores the whole `Draft`).
11. **Drafts store wire text.** Tokens are rebuilt from the text when a draft or an edit is loaded (`<@id>`, `<#id>` and `:id:` become chips), so drafts and edits show chips with no token storage.
12. **Previews come from the local file:** a pending image shows from the file or pasted bytes. Its size is read locally if possible (see chunk 1 risks); otherwise it's fitted into a fixed box.

## Plan

### Chunk 1: Attach files with a button, upload on send
**Goal:** An attach button adds files to a tray under the message box. Sending creates the pending message at once, with its files and per-file progress, then uploads, then posts. Failures can be retried or discarded as in milestone 3.

**Files likely touched:**
- `Cargo.toml`: reqwest features `["json", "multipart", "stream"]`.
- `src/api/client.rs`:
  - `RootConfig` gains `limits` (`features.limits`), using `#[serde(default)]` structs for `file_upload_size_limits.attachments` and `message_attachments`, plus helpers `attachment_size_limit()` and `max_attachments()` with the fallbacks.
  - `upload_attachment(file_server, name, content_type, bytes, on_progress: impl Fn(u64, u64)) -> String` (the file ID) does `POST {autumn}/attachments` with the session header.
  - `send_message` gains `attachments: &[String]`, and `content` becomes optional (omitted when empty).
- `src/draft.rs`:
  - `Draft` gains `attachments: Vec<DraftAttachment { local_id, name, size, kind: Image | Video | Audio | File, source: Path(PathBuf) | Bytes(Arc<[u8]>, mime) }>`.
  - `add_attachment(…, limits) -> Result<(), AttachError::{TooBig, TooMany}>`.
  - `check_send(text, has_attachments)` allows empty text when there are attachments.
  - `Draft::is_empty` counts attachments.
- `src/store.rs`:
  - `PendingMessage` gains `attachments: Vec<PendingAttachment { local_id, name, size, kind, preview, upload: Waiting | Uploading { sent, total } | Uploaded(id) }>`.
  - `set_upload_progress` and `set_uploaded` update a file's upload state. `retry_pending` clears `Uploaded` IDs when `nonce_used` (an answered failure).
  - Pending rows draw their attachments from local previews, through a new `AttachmentView::Local { … }` (or a `pending` flag on the existing variants) with a progress fraction.
- `src/ui/composer.rs`:
  - A paperclip button left of the box opens `prompt_for_paths`. Files are read with `std::fs::metadata` for size, and the kind comes from the extension via `mime_guess` (already in the tree, so add it to `Cargo.toml`).
  - The tray row shows a chip per file (an image thumbnail or file icon, the name, the size and ×).
  - `ComposerEvent::Send` gains the attachments.
- `src/ui/chat.rs`:
  - `post_pending` uploads each file without an `Uploaded` ID, one at a time on the IO runtime (`spawn_io`). Progress goes back through a `futures::channel::mpsc` to a UI task, which updates the store at most every 100 ms.
  - It then posts with the IDs. An upload failure fails the pending message with "Couldn't upload {name}: {reason}".
  - The pending row draws each attachment with a thin progress bar.
- `src/assets.rs`: register `Paperclip` if it isn't a default icon (check `gpui-kit-assets` `default-icons.txt`).
- Tests: `tests/api_upload.rs` (new, wiremock), `tests/store_pending.rs` (attachments), `tests/composer_rules.rs` or the existing `store_pending.rs` (draft attachment rules).

**Depends on:** None (builds on milestone 3).

**Risks:**
- **The upload API shape is not in the pinned crates.** Spike it first:
  - Do one real upload from the app (the manual check) and log the response.
  - If `/attachments` or the `{id}` response differ, adjust before building the UI.
  - If Autumn rejects requests without a `Content-Type` per part, set it from `mime_guess`.
- **Progress through reqwest:** a counting stream measures bytes handed to the HTTP stack, not bytes acknowledged by the server. That's good enough for a bar. Throttle UI updates (every 100 ms, or on each 1%) so rows aren't re-measured hundreds of times. Progress lives in a fixed-height bar, so the row height doesn't change.
- **Local image size:** `image` is built with `default-features = false`. Formats only decode if GPUI's own `image` features cover them (features are shared across the build). Try `image::ImageReader::open(path)?.with_guessed_format()?.into_dimensions()`. If PNG and JPEG don't decode, fall back to a fixed 240×180 box with `ObjectFit::Contain`, and accept a re-measure when the real message arrives.
- **Memory:** files are read fully into memory (at most 20 MB each, 5 per message). That's acceptable. Streaming from disk can come later.
- **Pending previews from `Bytes`** need `img(Arc<Image>)`. Check that GPUI caches these by ID (`Image::from_bytes` hashes the bytes) so they don't decode on every frame.

**How to verify:**
- `cargo test --test api_upload`:
  - The multipart `POST /attachments` to the mock file server: a `file` part with the file name and content type, and the `x-session-token` header. It returns the ID.
  - The progress callback reaches `(len, len)`.
  - 413 or an error body maps to `ApiError`.
  - `RootConfig` parses `features.limits` from the live-shaped fixture (update `tests/fixtures/root.json` to include `limits`), and falls back when it's missing.
  - `send_message` with attachments sends `{"attachments": [...]}` and no `content` key when the text is empty.
- `cargo test --test store_pending`:
  - Attachment limits (too big, sixth file).
  - Empty text allowed with attachments.
  - A pending message with attachments shows rows with progress.
  - `set_uploaded`, then confirmation by nonce, gives one real row.
  - A retry after a network failure keeps uploaded IDs. A retry after an answered failure clears them and gets a fresh nonce.

**Manual check:**
- Attach a PNG and a PDF with the button, type a caption, and send. The pending message should show both, with progress filling, then become the real message, which shows in the web app.
- Log the upload response once to confirm the API shape.
- Attach a file over 20 MB: refused with a toast.
- Send with Wi-Fi off: failed, then Retry once back online sends once.

**Shippable when:** Files can be attached with the button and sent with or without text, showing progress, never duplicated, with failures retryable.

### Chunk 2: Paste and drag-and-drop
**Goal:** Paste an image (a screenshot) or copied files into the message box, or drop files onto the conversation, and they join the tray like picked files.

**Files likely touched:**
- `src/ui/composer.rs`: `Textarea::on_paste`.
  - `ClipboardEntry::Image` becomes a `Bytes` attachment named `pasted-image-{n}.{ext}` (the extension from `ImageFormat`), and the paste is consumed.
  - `ClipboardEntry::ExternalPaths` adds those files, and is consumed.
  - Anything else isn't consumed, so text pastes as usual.
- `src/ui/chat.rs`: `.drag_over::<ExternalPaths>` (a highlighted drop overlay, "Drop to attach") and `.on_drop::<ExternalPaths>` on the conversation area (the message list and box), forwarding to `Composer::add_files`.
- `src/draft.rs`: the shared `add_attachment` limit rules (already there from chunk 1).
- Tests: draft rules for pasted bytes (name, kind and size).

**Depends on:** Chunk 1.

**Risks:**
- **Clipboard formats:** macOS screenshots paste as PNG, but some apps put TIFF on the clipboard. GPUI's `ImageFormat` may include TIFF. If so, either upload it as is (Stoat may not preview TIFF), or convert to PNG if `image` can decode it. Record which.
- **Mixed clipboards:** a clipboard with both text and an image (for example from a browser) shouldn't swallow the text. Consume the paste only when there's no `String` entry, or when the user pasted from a screenshot tool. Decide by testing: copy an image in Safari, and copy text in Notes.
- **Drop target:** a drop must reach the active channel's composer, and not the image viewer when it's open (the viewer occludes the conversation).

**How to verify:** `cargo test` covers naming and kinds for pasted images, and the limits applying to pasted and dropped files.

**Manual check:**
- Take a screenshot to the clipboard (Cmd+Ctrl+Shift+4) and paste: it should appear in the tray with a thumbnail.
- Copy two files in Finder and paste: both should be added.
- Drag a file onto the message list: an overlay should appear, and dropping should attach it.
- Paste ordinary text: it should paste as text.

**Shippable when:** Screenshots and files can be pasted or dropped into a message, and text pasting is unchanged.

### Chunk 3: @mention autocomplete with chips
**Goal:** Typing `@` opens a suggestion list of people above the box. Picking one inserts an `@Name` chip that sends as `<@id>`. Mentions in drafts and edits show as chips too.

**Files likely touched:**
- `src/complete.rs` (new, pure):
  - `completion_query(text, cursor) -> Option<Query { kind: Mention | Channel | Emoji, range, query }>` (Assumption 9; not inside code).
  - `rank(query, candidates) -> Vec<Candidate>`: prefix of the display name, nickname or username, then word prefix, then substring. Recent authors rank first, and the list is cut at 8.
  - `mention_spans(text) -> Vec<(Range, TokenRef::User(id) | Channel(id) | Emoji(id))>`, used to rebuild chips from wire text.
- `src/store.rs`: `mention_candidates(channel) -> Vec<Candidate { id, name, username, avatar, recent }>`. These are the DM or group recipients, or the server's cached members plus recent authors in the channel, newest first, excluding `me`.
- `src/ui/composer.rs`:
  - After each `Change`, compute the query and set the popup state.
  - The popup is a list above the box, full width of the box, at most 8 rows, each with an avatar, name and `@username`.
  - Keys while it's open, all by `capture_action` and stopping propagation: `MoveUp`/`MoveDown` move the selection, `Enter` and `IndentInline` (Tab) accept, `Escape` closes (before the edit and reply handling).
  - Accepting calls `replace_range_with_token(range, InlineToken::new("user:{id}", "<@{id}>").with_label("@{name}"))` and adds a space.
  - `set_channel` and `start_edit` load text through `InputContent` with tokens rebuilt by `mention_spans` and labelled from the store.
  - `Textarea::token(...)` draws chips: a rounded pill in the primary tint.
- `src/ui/chat.rs`: provides candidates to the composer (a callback, or a `Composer::set_candidates` call when the query changes).
- Tests: `tests/complete.rs` (new).

**Depends on:** None (independent of chunks 1–2).

**Risks:**
- **Key interception order:** `Enter` must accept a suggestion and not send. With `submit_on_enter`, the textarea's `enter` handler propagates, but `capture_action::<Enter>` on the composer runs first. Stop propagation only while the popup is open. Check that Shift+Enter still adds a new line while it's open.
- **Token text vs label:** `InputContent::with_token` requires the range to contain exactly the token's text. Build the content from the wire text and its spans, never from labels.
- **IME composition:** don't open the popup while text is marked (composing). Check `ime_marked_range` exposure, or ignore `Change` while composing if a public accessor exists.
- **Labels of unknown users** show `@Unknown user` until known. Re-label on the next load; don't chase it live.

**How to verify:** `cargo test --test complete` covers:
- The query: at the start, after a space, after `(`, not after letters (`email@x`), not inside `` `code` `` or a fence, and the cursor in the middle of a word.
- Ranking and the cut at 8.
- `mention_spans` round-trips `<@id>`, `<#id>` and `:ID:` but not `<@notanid>`.
- `mention_candidates` for DMs, groups and servers, excluding `me`, with recent authors first.

**Manual check:**
- In a DM, type `@` and pick the other person with the arrow keys and Enter. A chip should appear.
- Send it: in the web app the mention should ping. Backspace over a chip should delete it whole.
- Edit a message with a mention: it should show as a chip.

**Shippable when:** People can be @mentioned from suggestions, mentions show as chips everywhere in the box, and sending is unchanged.

### Chunk 4: Server members and #channels
**Goal:** In servers, @ suggestions cover every member (fetched once per server), and `#` suggests the server's channels.

**Files likely touched:**
- `src/api/client.rs`: `fetch_members(server) -> AllMemberResponse` (`GET /servers/{id}/members`).
- `src/store.rs`: `add_members(server, response)` caches members and users. `channel_candidates(channel)` gives the text channels of the channel's server, in sidebar order.
- `src/ui/chat.rs`: the first `@` query in a server starts `fetch_members`, at most once per server per session, tracked like `reply_fetches`. When it arrives, the suggestions refresh. While it loads, the list shows local candidates and a "Loading members…" row.
- `src/ui/composer.rs`: `#` queries. Accepting inserts `InlineToken::new("channel:{id}", "<#{id}>").with_label("#{name}")`.
- Tests: `tests/api_send.rs` (`fetch_members`) and `tests/complete.rs` (channel candidates, member ranking with nicknames).

**Depends on:** Chunk 3.

**Risks:**
- **Large servers:** the member list may be thousands of entries. Fetch it lazily, cache it, and rank in memory (cheap). If the response is very slow, local candidates still show.
- **Member nicknames vs display names:** rank on both, and show the nickname with `@username` under it.

**How to verify:**
- `cargo test --test api_send`: `GET /servers/{id}/members` with the session header, parsed.
- `cargo test --test complete`: `#` queries, channel candidates in order, and a nickname match ranking above a username substring match.

**Manual check:**
- In a server, type `@` and the name of someone who hasn't posted recently. They should appear after a moment.
- Type `#gen` and pick `#general`. It should send as a channel link that the web app shows as `#general`.

**Shippable when:** Anyone in a server can be mentioned, and channels can be linked with `#`.

### Chunk 5: :emoji: autocomplete
**Goal:** Typing `:sm` suggests emoji: Unicode by shortcode, plus the server's custom emoji by name. Typing a complete `:shortcode:` turns it into the emoji.

**Files likely touched:**
- `Cargo.toml`: the `emojis` crate (check its license and API first).
- `src/emoji.rs` (new, pure):
  - `search(query, custom: &[Emoji]) -> Vec<EmojiChoice>`: shortcode prefix first, then the name's word prefix. Custom emoji are matched by name. At most 8.
  - `by_shortcode(code)` finds an exact match.
- `src/complete.rs`: `:` queries (Assumption 9: at least 2 characters), and closing-colon detection (`:name:` right before the cursor, matching exactly).
- `src/ui/composer.rs`:
  - The popup rows show the emoji and `:shortcode:`.
  - Accepting a Unicode emoji does `set_selected_range(range)` then `replace(emoji)`. Accepting a custom emoji inserts a token, `InlineToken::new("emoji:{id}", ":{id}:").with_label(":{name}:")`, which the token renderer draws as its image.
- `src/store.rs`: `custom_emoji_for(channel)`, the same set as `reaction_choices`' custom part.
- Tests: `tests/emoji.rs` (new) and `tests/complete.rs` (`:` queries, closing colon).

**Depends on:** Chunk 3 (the popup and token rendering).

**Risks:**
- **Shortcode set:** the `emojis` crate uses gemoji shortcodes, while Stoat's web client may use different names for some emoji. That's acceptable: what's sent is the Unicode character, not the shortcode.
- **Times and ratios** like `12:30` or `1:2` must not open the popup. The rule that `:` must follow whitespace or the start, and that 2 letters must come after it, handles this. Test it.
- **Custom emoji tokens** must round-trip through drafts and edits (`mention_spans` already covers `:ID:` from chunk 3).

**How to verify:** `cargo test --test emoji --test complete` covers:
- `:smi` finds 😄 among the first results.
- A custom emoji is found by name.
- `12:30` and `http://x` don't trigger.
- `:smile:` completed by its closing colon is replaced.
- An unknown `:nope:` is left as typed.

**Manual check:**
- Type `:thu`, pick 👍, and send.
- In a server with custom emoji, type part of one's name, pick it, and send. The web app should show the custom emoji.
- Type `:tada:`: it should become 🎉 as you type the final colon.

**Shippable when:** Emoji can be found and inserted by typing, Unicode and custom.

### Chunk 6: Emoji button with a full picker
**Goal:** An emoji button at the right of the message box opens a picker: a search field, recently used emoji, then every Unicode emoji by category, then this server's custom emoji. Picking one inserts it at the cursor.

**Files likely touched:**
- `src/emoji.rs`: `groups() -> Vec<(group name, Vec<EmojiChoice>)>`, a recently used list (in memory, at most 16, most recent first), and `search` reused for the search field.
- `src/ui/emoji_picker.rs`:
  - A new `emoji_browser(...)` reuses the cell rendering from `emoji_grid`, with a search `InputState` at the top.
  - Category headers sit between rows.
  - It's virtualized with `uniform_list` (rows of 8 emoji, with headers as rows), because there are roughly 1,900 emoji.
- `src/ui/composer.rs`: a smiley button (`IconName::Smile`, registered if not a default) in a `Popover` anchored above the box. Picking inserts at the cursor (Unicode as text, custom as a token, as in chunk 5). The picker stays open after picking only with Shift held.
- Tests: `tests/emoji.rs` (group order, recently used ordering and cap, search reuse).

**Depends on:** Chunk 5 (the emoji data and insertion).

**Risks:**
- **Rendering cost:** about 1,900 text cells in one popover. Virtualize by rows from the start; don't measure it afterwards.
- **Focus:** the picker's search field takes focus. After a pick, focus returns to the message box with the cursor after the inserted emoji.
- **Skin tones:** out of scope. Insert the default emoji.

**How to verify:** `cargo test --test emoji` covers:
- Groups appear in Unicode order, with every group non-empty.
- Recently used: most recent first, no duplicates, capped at 16.
- Search over all emoji returns the same results as `:` completion.

**Manual check:**
- Open the picker, search "cat", pick 🐱, and it should be inserted at the cursor.
- Scroll through all categories smoothly.
- Pick a custom emoji in a server.
- Reopen the picker: the emoji used most recently should be first.

**Shippable when:** Any emoji, Unicode or custom, can be inserted from a picker in the message box.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`. Optional live checks: `cargo test -- --ignored`, which runs the Keychain, API drift, events handshake and media checks. The API drift check parses the live `GET /`, so it will cover the new `limits` fields.

### Unit Tests
- **Draft attachments:** size and count limits, from config with fallbacks. Empty text allowed with files. `Draft::is_empty` counts files. Attachments are set aside during edits.
- **Pending uploads:** progress and uploaded states. A retry keeps IDs after a network failure and clears them after an answered failure (with a fresh nonce). Confirmation by nonce with attachments gives one row.
- **Completion queries:** `@`, `#` and `:` triggers and non-triggers (inside code, mid-word, times, URLs). The closing-colon replacement.
- **Ranking:** prefix, word prefix, substring. Recent authors first. Nicknames. The cut at 8.
- **Token spans:** `mention_spans` round-trips `<@id>`, `<#id>` and `:ID:`, and rejects text that isn't an ID.
- **Emoji:** shortcode search, custom emoji by name, group order, recently used.

### Integration Tests
- **`wiremock`:**
  - The multipart upload (headers, part name, file name, content type, progress).
  - `send_message` with `attachments` and no `content`.
  - `fetch_members`.
  - Refusals: 413, `TooManyAttachments`, `MissingPermission`.
- **`RootConfig`:** the live-shaped fixture, with `features.limits`.
- **Store pipelines:**
  - Pending message with attachments, then uploads, then the echo with the nonce: one real row with the real attachments.
  - A member list fetched, then `mention_candidates` includes them.

### Manual Tests
1. Attach a PNG, a PDF and a short video with the button, add a caption, and send. Progress should show, and the result should match the web app.
2. Paste a screenshot. Drag a file from Finder. Paste text: it should still paste as text.
3. Send with Wi-Fi off, then Retry once back online. It should send exactly once, with its files.
4. Try a 25 MB file, and a sixth file. Both should be refused with a clear toast.
5. Mention someone with `@` in a DM and in a server, including someone who hasn't posted recently. Link a channel with `#`. Mentions should ping in the web app.
6. Edit a message with a mention and a custom emoji: both should show as chips. Save it unchanged, then save a change.
7. Use `:` completion for a Unicode emoji and a custom one. Type `:tada:` in full. `12:30` shouldn't open anything.
8. Use the emoji picker: search, scroll every category, pick a custom emoji, and check that recently used works.
9. Check light and dark mode: the tray, progress bars, the popup, chips and the picker should all be readable.

## Definition of Done
- [ ] **Attaching:** the attach button, paste (images and files) and drag-and-drop add files to a tray with thumbnails and ×. Files over `file_upload_size_limits.attachments` (20 MB) and past `message_attachments` (5) are refused with a toast. Text pastes unchanged.
- [ ] **Sending files:** a message with files (and optional text) appears at once as pending with per-file progress. It uploads on send to `POST {autumn}/attachments`, then posts with the `attachments` IDs. It's confirmed by nonce, with no duplicates (tested).
- [ ] **File failures:** a failed upload or send shows the reason with Retry and Discard. A retry after a network failure keeps uploaded IDs and the key. After an answered failure it uploads again with a fresh key (tested).
- [ ] **@mentions:** `@` suggests people: DM and group recipients and recent authors at once, and all server members after a single lazy fetch. Picking inserts an `@Name` chip that sends as `<@id>`. Keyboard: arrows, Enter or Tab, Esc. Chips show in drafts and edits.
- [ ] **#channels:** `#` suggests the server's channels and inserts a chip sent as `<#id>`.
- [ ] **:emoji:** `:` (with at least 2 characters) suggests Unicode and custom emoji. A complete `:shortcode:` turns into the emoji. Times and URLs don't trigger it (tested).
- [ ] **Emoji picker:** the button opens a virtualized picker with search, recently used, every category and custom emoji. Picking inserts at the cursor.
- [ ] **Checks:** `cargo test && cargo clippy --all-targets -- -D warnings` passes, and `cargo test -- --ignored` passes (live checks, including `GET /` parsing with `limits`).
