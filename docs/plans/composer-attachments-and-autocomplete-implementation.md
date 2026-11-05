# Implementation Record: Attachments and autocomplete in the message box (milestone 4)

_Plan: docs/plans/composer-attachments-and-autocomplete.md_
_Started: 2026-11-05_

Supervision: continuous. Pre-existing dirty files kept out of every commit: `.gitignore` (a duplicated `.humanlayer/tasks/` entry) and `.shadow/`. The plan file itself was untracked and is committed with chunk 1.

## Chunk 1: Attach files with a button, upload on send

### Changes Made
- `Cargo.toml`: reqwest features `["json", "multipart", "stream"]`; `mime_guess = "2"` (already in the tree).
- `src/api/client.rs`:
  - `RootFeatures.limits: RootLimits { default, new_user: AccountLimits { message_attachments, file_upload_size_limits: FileSizeLimits { attachments } } }`, all `#[serde(default)]`.
  - `RootConfig::attachment_size_limit()` and `max_attachments()`: the smaller of `default` and `new_user`, else `DEFAULT_ATTACHMENT_SIZE_LIMIT` (20 000 000) and `DEFAULT_MAX_ATTACHMENTS` (5). Both constants are re-exported from `api`.
  - `upload_attachment(file_server, name, content_type, bytes, on_progress)`: `POST {autumn}/attachments`, multipart field `file` with the file name and content type, plus `x-session-token`. The body is a stream of 64 KB chunks that calls `on_progress(sent, total)`. It has its own 10-minute timeout, and logs the response at debug level.
  - `send_message(channel, content, attachments, replies, key)`: `content` is left out when empty, and `attachments` when there are none.
- `src/draft.rs`:
  - `AttachmentSource::{Path, Bytes}`.
  - `DraftAttachment { local_id, name, size, content_type, kind: FileKind, dimensions, source }`, with `new`, `from_path` and `with_dimensions`. The type comes from `mime_guess`, and `local_id` from a global counter.
  - `AttachmentLimits { max_bytes, max_count }`, with `Default` and `from_root`.
  - `AttachError::{TooBig { name, limit }, TooMany { max }}`, whose `Display` is the toast text (limits in decimal MB, as Stoat states them).
  - `Draft.attachments`, `add_attachment(file, &limits)` and `remove_attachment(local_id)`. `is_empty` counts files, and `check_send(text, has_attachments)` gives `Ready("")` when there are files.
- `src/attachment.rs`:
  - `AttachmentView::Local { name, size_label, kind, image: Option<(AttachmentSource, w, h)>, progress (thousandths) }`.
  - `local_image_size(path)` reads the header through `image::ImageReader`. GPUI already turns on the png, jpeg, gif, webp and tiff decoders.
- `src/store.rs`:
  - `PendingAttachment { file: DraftAttachment, upload: UploadState::{Waiting, Uploading { sent, total }, Uploaded(id)} }`, with `uploaded_id()`. `PendingMessage.attachments` holds them.
  - `set_upload_progress` and `set_uploaded`. `fail_pending` resets a cut-off `Uploading` to `Waiting`, and `retry_pending` resets every file to `Waiting` when the nonce was spent (an answered failure).
  - `timeline()` draws pending rows' files as `AttachmentView::Local`. Images use `fit_within(dimensions)`, or `FALLBACK_IMAGE_SIZE` when the size is unknown.
- `src/ui/composer.rs`:
  - A paperclip button left of the box (disabled while editing) opens `prompt_for_paths`.
  - `add_files(paths)` (public, for chunk 2's drops) reads the size with `fs::metadata`, reads an image's dimensions, applies the limits, and shows a toast per distinct refusal.
  - The tray under the box shows a chip per file: a 36 px thumbnail from the path or a file icon, the name, the size and ×.
  - `take_draft` carries the whole draft (text, replies and files) through channel switches and edits.
  - `ComposerEvent::Send` gains `attachments`, and `set_limits` takes the limits.
  - `file_icon(kind)` is shared with the chat view.
- `src/ui/chat.rs`:
  - The root config also sets the composer's limits.
  - `send_message` takes `Vec<PendingAttachment>`.
  - `post_pending` now runs as one UI task. It uploads each file without an ID through `upload_file` (bytes read with `spawn_blocking` on the IO runtime), then posts with the IDs. Progress comes back over an unbounded `mpsc`, and `futures::select!` applies it at most every 100 ms (the final `(len, len)` always).
  - An upload failure fails the message as "Couldn't upload {name}: {reason}" with `answered = false`, since nothing was posted.
  - `render_attachment` draws `Local` images (`img(path)` at the fitted size) and file cards, each with an absolutely positioned 3 px progress bar, so the row height doesn't change.
- `src/assets.rs`: registers `Paperclip`.
- Tests:
  - `tests/api_upload.rs` (new, 8 tests): the multipart upload, progress, 413 and API errors, the network error, root limits with the stricter-of-two rule and fallbacks, attachments without content, attachments with a caption, and `TooManyAttachments`.
  - `tests/store_pending.rs`: 9 new tests (file types, limits and toast text, empty text with files, the edit stash, pending views and progress, fitted image size, confirmation by nonce, and both retry paths).
  - `tests/fixtures/root.json` gains a live-shaped `limits`.
  - `tests/store_edit.rs` and `tests/api_send.rs` are updated for the new fields and signatures.

### Divergences
- **File kinds:** `DraftAttachment.kind` reuses the existing `attachment::FileKind` (which also has `Text`) rather than a new `Image | Video | Audio | File` enum.
- **Content type:** `content_type` is a field of `DraftAttachment`, not part of `AttachmentSource::Bytes`.
- **Pending attachments:** `PendingAttachment` wraps the `DraftAttachment` (its `source` doubles as the preview) instead of copying its fields and adding a `preview` field.
- **Fallback box:** the fixed box for images without a known size is the existing `FALLBACK_IMAGE_SIZE` (300×200), not 240×180. Image dimensions are read once at attach time and stored on the attachment.
- **Upload timeout:** uploads get a 10-minute timeout. The client's 30 s total timeout would cut off large uploads.
- **In-memory previews:** pending previews of `Bytes` sources (pasted images) aren't drawn as pictures yet. They fall back to a file card. Chunk 2 is the first to create `Bytes` attachments and handles it (see the plan's chunk 2 note).
- **Upload API confirmed:** the plan's open question is resolved. A real upload from the app was `POST https://cdn.stoatusercontent.com/attachments`, answered with `HTTP 200 OK: {"id":"aKofAJqRKSpXt1MoqiT2p_vtKA0v7dslNEu2oWcWxu"}`, then `sent 01M6PFN9D0AS8BBW5C6D9SVQFS`. No change was needed.

### Verification Performed
- `cargo test --test api_upload`: 8 passed.
- `cargo test --test store_pending`: 27 passed (9 new).
- `cargo test --test store_edit --test api_send`: all passed.
- Full suite: `cargo test` gives 285 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings` is clean, after fixing `is_multiple_of` and `slice::from_ref`.

### Manual Validation
The user ran the build (`RUST_LOG=stoat_gpui=debug`, log in `/tmp/rustoat-chunk1.log`), attached a file with the paperclip and sent it. They said "I like it so far". The log shows the limits loaded (20 000 000 bytes, 5 files), the upload of a 3157-byte SVG (`image/svg+xml`) answered with `{"id": …}`, and the message sent with it.

Not exercised in this session: the over-20 MB toast and the offline Retry. Both are covered by unit tests and carried to the plan's Manual Tests 3 and 4.

## Chunk 2: Paste and drag-and-drop

### Changes Made
- `src/draft.rs`:
  - `AttachmentSource` is removed. `DraftAttachment` now has `path: PathBuf`, and `from_path` is its only constructor.
  - `save_pasted_image(dir, extension, bytes)` writes the image as `pasted-image-{n}.{ext}` (with a per-process counter) and reads its dimensions. TIFF and BMP are converted to PNG through `image` (`to_png`). Undecodable data is saved unchanged.
- `src/ui/composer.rs`:
  - `Textarea::on_paste` calls `Composer::paste` through a weak entity.
  - `ExternalPaths` entries are attached, and the paste is consumed. `Image` entries are saved with `save_pasted_image` into `pasted_dir()` (`$TMPDIR/rustoat-pasted-<pid>`) and attached. Anything else falls through as text.
  - Pasting while editing falls through.
  - `add_files`, `paste` and the picker share `attach(Vec<Result<DraftAttachment, String>>)` (limits, plus one toast per distinct refusal) and `read_file(path)`, which skips folders and reads an image's size.
- `src/attachment.rs`: `AttachmentView::Local.image` holds a `PathBuf`.
- `src/store.rs` and `src/ui/chat.rs` now read files from `path`.
- `src/ui/chat.rs`: the conversation column is a drop target:
  - It's `.relative().group(DROP_GROUP)` with `.on_drop::<ExternalPaths>` forwarding to `Composer::add_files`. Drops are ignored while the image viewer is open, and there's no target without an open channel.
  - `render_drop_overlay` is an absolute, normally invisible child that becomes visible through `group_drag_over::<ExternalPaths>`. It shows a dashed primary border, a translucent background, a paperclip and "Drop to attach".
- `tests/store_pending.rs`: 3 new tests for pasted images (naming, type, size, dimensions, unique names; TIFF and BMP to PNG and the undecodable fallback; limits applying to pasted and dropped files). Existing tests are updated for `path`.

### Divergences
- **Pasted images are saved to a temp file instead of kept in memory (`Bytes`).**
  - Every attachment is then a path. `img(path)` previews use GPUI's path-keyed image cache, which settles the plan's risk about in-memory `Image`s being re-hashed every frame. Uploads read from disk as for picked files.
  - The files sit in `$TMPDIR/rustoat-pasted-<pid>/` and aren't deleted by the app. macOS clears `$TMPDIR` on its own.
- **Clipboard formats (plan risk):** TIFF and BMP are converted to PNG before attaching, so Stoat can preview them.
- **Mixed clipboards (plan risk):** no rule of ours was needed. GPUI's macOS pasteboard (`gpui-pre-macos` `pasteboard.rs`) returns a plain-text entry instead of any image when both exist. Copied files arrive as `ExternalPaths` plus a string of their paths, and we consume those as files.
- **Drop target (plan risk):** drops are ignored while the image viewer is open, so they never land behind it.

### Verification Performed
- `cargo test --test store_pending`: 30 passed (3 new).
- Full suite: `cargo test` gives 288 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings` is clean.

### Manual Validation
Deferred by the user ("we'll test it all later"). The chunk's checks (pasting a screenshot, pasting copied Finder files, dropping onto the message list with the overlay, pasting text unchanged) move to the plan's Manual Test 2 at completion.

## Chunk 3: @mention autocomplete with chips

### Changes Made
- `src/complete.rs` (new, pure):
  - `completion_query(text, cursor) -> Option<Query { kind: Mention | Channel | Emoji, range, query }>`. The trigger must start the text or follow whitespace or `( [ { " ' “ ‘`. The cursor must be at the end of the word. Code (an odd number of backticks earlier on the line, or inside a ``` or ~~~ fence) doesn't count. `:` needs at least 2 shortcode characters `[alnum _ + -]`.
  - `rank(query, items, names, recent)` is generic: tier 0 is a prefix, 1 a word prefix, 2 a substring, ignoring case. Within a tier, recent items come first, then the given order. The result is cut at `MAX_SUGGESTIONS` (8). `rank_people` ranks on nickname, display name and username.
  - `Candidate { id, name, display_name, username, avatar, recent }`.
  - `TokenRef::{User, Channel, Emoji}`, with `wire_text()` (`<@id>`, `<#id>`, `:id:`), `token_id()` (`user:id`…) and `from_token_id`.
  - `mention_spans(text)` finds the byte ranges of wire mentions whose IDs are real ULIDs (26 Crockford base-32 characters).
  - `Suggestion { key, title, detail, image, glyph, insert: Insertion::{Token { token, label }, Text} }`, and `Suggestion::person`.
- `src/store.rs`:
  - `mention_candidates(channel)`: recent authors in the loaded history, newest first, skipping system and webhook messages. Then DM or group recipients, or the server's known members sorted by name. Each person appears once, never the user, and only known users.
  - `token_label(channel, token)`: `@Name` (the nickname in servers), `#channel` or `:emoji:`, with fallbacks for unknown ones.
- `src/ui/composer.rs`:
  - `ComposerEvent::Complete(Query)`, `set_suggestions(query, suggestions, loading)` (ignored if the query moved on; the selected row is kept by key), and `completion_query()`.
  - On `Change`, the query is recomputed unless the IME is composing (`EntityInputHandler::marked_text_range`). The popup closes on `Blur`.
  - Capture-phase keys, which stop propagation only while the list is open: `MoveUp` and `MoveDown` (wrapping), `Enter` without Shift and `IndentInline` (Tab) accept, and `Escape` closes before the edit and reply handling.
  - Accepting a token calls `replace_range_with_token(range, InlineToken::new(token_id, wire).with_label(label))`, then inserts a space. Text uses `set_selected_range` and `replace` (for chunk 5).
  - The popup is absolute above the input row, the row's full width: up to 8 rows of 32 px with an avatar (or glyph), the name, a muted `@username`, and an optional "Loading members…" row. Mouse-down accepts, so focus stays in the box.
  - Chips are drawn by `Textarea::token(render_chip)`: a pill in a 15 % primary tint, using the selection colour when selected.
  - Text is loaded with chips: `set_text` builds `InputContent` from `mention_spans` and labels. `set_channel` and `start_edit` take a `Labeler` (`&dyn Fn(&TokenRef) -> String`), and labels are cached by token ID, so a draft restored after an edit (or on Esc) keeps its labels.
- `src/ui/chat.rs`: `ComposerEvent::Complete` goes to `suggest(channel, query)`, which ranks `store.mention_candidates`, maps them through `Suggestion::person`, and calls `set_suggestions` (channels and emoji give nothing yet). `sync_composer` and `start_edit` pass `store.token_label` as the labeler.
- `src/lib.rs`: `pub mod complete`.
- `tests/complete.rs` (new, 15 tests): triggers and non-triggers (start, space, `(`, `"`, `email@x`, inline code, fences, after a chip, mid-word, bad offsets), query ranges in bytes, `:` rules (`12:30`, `1:2`, `http://x`, `:)`), ranking tiers, recent first, nicknames and display names, the cut at 8, the person suggestion's chip, `mention_spans` round trips and rejections, `mention_candidates` for DMs, groups (recent first), servers (recent, then members with nicknames) and saved messages, and chip labels.

### Divergences
- **Candidates come by event, not callback.** The composer emits `Complete(query)` and `ChatView` answers with `set_suggestions`. The same path will refresh the list when members arrive (chunk 4).
- **Labels are passed in, not read by the composer.** `set_channel` and `start_edit` take a labeler argument, and the composer caches labels by token ID. It can't read the store itself: `ChatView` is mid-update when it calls these, so reading it would panic.
- **`Candidate` gained `display_name`**, so a nickname and a display name can both be ranked (the plan had this in chunk 4's risk).
- **Ranking:** "recent authors first" applies within each match tier. With an empty query (`@` alone), every name matches, so recent authors lead.
- **No completion on cursor-only moves.** The popup is recomputed on text changes and closed on blur, not on cursor moves without typing (arrow keys left and right).

### Verification Performed
- `cargo test --test complete`: 15 passed. One expectation was corrected: the fixture's Ada has the display name "Ada L.".
- Full suite: `cargo test` gives 303 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings` is clean.

### Manual Validation
Deferred by the user ("we'll test it all later"). The chunk's checks (`@` in a DM with arrows and Enter, a chip deleting as one unit, the mention pinging in the web app, an edit showing chips) move to the plan's Manual Tests 5 and 6.

## Chunk 4: Server members and #channels

### Changes Made
- `src/api/client.rs`: `fetch_members(server) -> AllMemberResponse` (`GET /servers/{id}/members`).
- `src/store.rs`:
  - `add_members(response)` caches the users and members, keyed by (server, user).
  - `channel_server(channel)` (public).
  - `channel_candidates(channel)`: the server's sidebar channels (`server_channels`, so only those the user can see) without voice channels, in sidebar order. None outside servers.
- `src/complete.rs`: `Suggestion::channel(id, name)` has the glyph `#` and inserts `TokenRef::Channel` with the label `#name`.
- `src/ui/chat.rs`:
  - `member_fetches` (asked this session) and `members_loading`.
  - `suggest(channel, query, fetch)`: in a server, `@` calls `fetch_members` the first time, and the list shows "Loading members…" (`loading = true`) with local candidates meanwhile. `#` ranks `channel_candidates` by name.
  - When members arrive, `refresh_suggestions` re-ranks the open query. A failed fetch is forgotten (retried on the next `@` typed), and its refresh doesn't fetch, so it can't loop.
- Tests:
  - `tests/api_send.rs`: `fetching_members_gets_the_whole_list_with_users` (path, session header, parsing).
  - `tests/complete.rs`: 4 new tests (a fetched list makes a quiet member mentionable, shown by nickname with `@username`, only in that server; a nickname match above a username substring; channel candidates in sidebar order without voice channels and none in DMs; the channel chip).

### Divergences
- **`add_members` takes only the response.** The member IDs carry the server, so the plan's `server` parameter isn't needed.
- **Voice channels aren't offered for `#`**, following the plan's "text channels". Stoat models them as `TextChannel` with `voice`, and they're filtered out.
- **The nickname shows beside `@username` on the same row**, not under it, keeping rows at a fixed 32 px.
- **A failed member fetch is retried on the next `@`.** The plan says once per server per session; that holds for successful fetches.

### Verification Performed
- `cargo test --test complete --test api_send`: 19 and 10 passed.
- Full suite: `cargo test` gives 308 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings` is clean.

### Manual Validation
Deferred by the user ("we'll test it all later"). The chunk's checks (a member who hasn't posted appearing after a moment, `#gen` → `#general` showing as a link in the web app) move to the plan's Manual Test 5.

## Chunk 5: :emoji: autocomplete

### Changes Made
- `Cargo.toml` and `Cargo.lock`: `emojis = "0.9.1"`, licensed `(MIT OR Apache-2.0) AND Unicode-3.0`, with one dependency, `phf 0.13`. The API was checked: `iter()` (CLDR order, default skin tones only), `get_by_shortcode`, `Emoji::{as_str, name, group, shortcode, shortcodes}`, and `Group::iter()`.
- `src/emoji.rs` (new, pure):
  - `CustomEmoji { id, name, url }` and `EmojiChoice::{Unicode { emoji, shortcode }, Custom(CustomEmoji)}`.
  - `EmojiChoice::suggestion()`: a Unicode emoji shows the glyph and `:code:` and inserts the character as text. A custom one shows its image and `:name:` and inserts a chip, `TokenRef::Emoji(id)` labelled `:name:`.
  - `search(query, custom)` ranks in tiers: 0 an exact shortcode or name, 1 a prefix, 2 a later word (split on `_ - space`). Within a tier, custom emoji come first, then CLDR order. The result is cut at 8.
  - `by_shortcode(code)`.
- `src/complete.rs`: `closed_shortcode(text, cursor)` gives the `:code:` closed by the `:` just before the cursor, with the same start and code rules as `completion_query`.
- `src/store.rs`:
  - `server_emoji(channel)` is factored out of `reaction_choices` (its server's emoji, or all servers' in DMs and groups, sorted by name).
  - `custom_emoji_for(channel) -> Vec<CustomEmoji>` (with image URLs).
- `src/ui/chat.rs`: `suggest` answers `QueryKind::Emoji` with `emoji::search(query, store.custom_emoji_for(channel))`. `set_media` also gives the composer the `MediaUrls`.
- `src/ui/composer.rs`:
  - `replace_closed_shortcode` runs on every `Change`. When exactly one `:` was typed at the cursor (`typed_char(last_text, text, cursor, ':')`, so pasted or loaded text is left alone) and it closes a known shortcode, the range is selected and replaced with the emoji. Unknown codes are left as typed. `last_text` is kept on changes and `set_text`.
  - `render_chip` draws custom emoji chips (`emoji:{id}`) as their image at 90 % of the line height, falling back to the label. Mentions keep the pill. `set_media` provides the URLs.
- `src/lib.rs`: `pub mod emoji`.
- Tests:
  - `tests/emoji.rs` (new, 6 tests): `:smi` finds 😄 in the first 3; 👍 for `thu` and `+1`; 🎉 for `TADA`; exact matches first (😄 for `smile`, 🐱 for `cat`); later-word matches (🚒 for `engine`); custom emoji by name and tier order; suggestion insertions; `by_shortcode`.
  - `tests/complete.rs`: `a_closing_colon_finds_the_shortcode_it_ends` covers `:smile:`, `:tada:` after a space, `(:+1:`, an unknown `:nope:` left alone, and the non-triggers `12:30:`, `a:smile:`, code, spaces and custom-emoji wire text.

### Divergences
- **`emojis` has one dependency**, `phf 0.13` (another `phf` version, 0.11, was already in the tree). The plan said it had none. Its licence includes `Unicode-3.0` for the emoji data, which is permissive.
- **Ranking adds an exact-match tier** (so `:smile` puts 😄 before 😃 `smiley`, which comes earlier in CLDR order). Words are matched after the first one only, since the first is covered by the prefix tier.
- **Closing colons only convert Unicode shortcodes.** A custom emoji's exact `:name:` isn't converted. Custom emoji are inserted from the list.
- **The closing colon only acts on a typed `:`.** It compares the previous text with the new one, so drafts, edits and pastes that contain `:smile:` aren't changed.

### Verification Performed
- `cargo test --test emoji`: 6 passed. One expectation was corrected: `parrot` is 🦜's exact shortcode, so 🦜 rightly ranks above the custom `:smile_parrot:`.
- `cargo test --test complete`: 20 passed.
- Full suite: `cargo test` gives 315 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings` is clean.

### Manual Validation
Deferred by the user ("we'll test it all later"). The chunk's checks (`:thu` → 👍, a custom emoji from the list, `:tada:` converting on the final colon) move to the plan's Manual Test 7.

## Chunk 6: Emoji button with a full picker

### Changes Made
- `src/emoji.rs`:
  - `groups()`: every Unicode emoji (default skin tones) by `emojis::Group`, in Unicode order, with display names.
  - `RecentEmoji` (`used`, `list`; most recent first, no duplicates, `MAX_RECENT = 16`).
  - `search_all(query, custom, limit)`, with `search` = `search_all(…, 8)`, so the picker's first results are exactly the `:` completion's.
  - `picker_sections(query, recent, custom)`: "Recently used", the 9 categories and "Custom", or "Results" (up to 64) for a search (colons trimmed), dropping empty sections.
  - `PickerRow::{Header, Emoji}` and `picker_rows(sections, columns)`.
- `src/ui/emoji_picker.rs`:
  - The cell rendering is factored out of `emoji_grid` into `emoji_cell` (34 px square, hover, chosen tint, tooltip) and `custom_face` (an image with a loading skeleton). The reaction grid now uses them.
  - New `EmojiBrowser` view: a search `InputState` (focused on open, cleanable, search icon) over a `uniform_list` of `PickerRow`s, all 34 px high, 9 rows tall. Changing the search rebuilds the rows and scrolls to the top, and Enter picks the first result.
  - Clicking a cell calls `OnBrowsePick(choice, shift)`.
- `src/ui/composer.rs`:
  - A smiley button (`IconName::FaceSlightlySmiling`) right of the box, in a `Popover` anchored bottom-right and controlled by `emoji_picker.is_some()`. Opening emits `ComposerEvent::OpenEmojiPicker`, and `ChatView` answers with `open_emoji_picker(store.custom_emoji_for(channel))`.
  - `insert_emoji` records the emoji as recently used and inserts it at the cursor (Unicode with `insert`, custom with `replace_with_token`, a chip). It closes the picker and refocuses the box unless Shift was held.
  - While the picker is open, `on_move_up` and `on_escape` leave the box alone (its search field's keys pass through the composer), and Esc only closes the picker.
- `src/ui/chat.rs`: handles `ComposerEvent::OpenEmojiPicker`.
- `src/assets.rs`: registers `FaceSlightlySmiling`.
- `tests/emoji.rs`: 5 new tests (groups in Unicode order, all non-empty, more than 1800 emoji, no skin tones; recently used order, deduplication and cap; picker search equal to completion and then more; sections for empty, recent and custom, search and no results; rows of 8 under headers, fewer than 300 rows in all).

### Divergences
- **Icon:** the button uses `IconName::FaceSlightlySmiling`. Lucide 1.43 (bundled by gpui-kit-assets) renamed `smile` to `face-slightly-smiling`, so `IconName::Smile` doesn't exist.
- **The browser is its own view** (`EmojiBrowser`, holding the search state, the rows and the scroll handle) rather than an `emoji_browser(...)` function, because the search field and the virtualized list need state. Cells are shared with `emoji_grid` through `emoji_cell` and `custom_face`.
- **Custom emoji come by event.** The composer asks with `OpenEmojiPicker` and `ChatView` answers, as `Complete` does for suggestions.
- **Picker search shows up to 64 results.** Its first 8 are exactly the `:` completion's, which is what the plan's "same results" check tests.
- **Recently used isn't per server.** A custom emoji from one server can show as recent elsewhere. Stoat lets custom emoji be used anywhere.
- **Smoke check:** the final build launched with no panics. It stopped at the Keychain prompt for the rebuilt binary, so the signed-in UI wasn't drawn in this session.

### Verification Performed
- `cargo test --test emoji`: 11 passed.
- Full suite: `cargo test` gives 320 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings` is clean.

### Manual Validation
Deferred by the user ("we'll test it all later"). The chunk's checks (search "cat" and pick 🐱 at the cursor, smooth scrolling through all categories, picking a custom emoji, recently used first on reopening) move to the plan's Manual Test 8.

## Summary

_Completed: 2026-11-05 (code). Manual testing was deferred by the user to the end and is still to be done._

### Chunks Completed
1. **Attach files with a button, upload on send:** the paperclip, the tray, limits from `GET /`, pending rows with per-file progress, Autumn uploads, and retry rules for uploaded IDs.
2. **Paste and drag-and-drop:** screenshots and copied files paste into the tray (images saved to a temp file, TIFF and BMP converted to PNG), and a "Drop to attach" overlay on the conversation.
3. **@mention autocomplete with chips:** pure `complete.rs`, a keyboard-driven suggestion list, mention chips, and chips rebuilt from wire text in drafts and edits.
4. **Server members and #channels:** a lazy `GET /servers/{id}/members` once per server, "Loading members…", and `#` channel chips.
5. **:emoji: autocomplete:** the `emojis` crate, pure `emoji.rs` search, custom emoji chips drawn as images, and `:shortcode:` converting on the typed closing colon.
6. **Emoji button with a full picker:** a virtualized `EmojiBrowser` with search, recently used, 9 categories and custom emoji, inserting at the cursor (Shift keeps it open).

### Divergences
- **Chunk 1:**
  - `FileKind` is reused.
  - `content_type` is on `DraftAttachment`.
  - `PendingAttachment` wraps the draft file.
  - The image fallback box is 300×200.
  - Uploads have a 10-minute timeout.
  - The upload API shape was confirmed live (`{"id": …}`).
- **Chunk 2:**
  - Pasted images go to a temp file, so every attachment is a path (no in-memory `Bytes`).
  - TIFF and BMP become PNG.
  - Mixed clipboards are already resolved to text by GPUI's macOS pasteboard.
  - Drops are ignored while the image viewer is open.
- **Chunk 3:**
  - Candidates come through a `Complete` event and `set_suggestions`, not a callback.
  - Labels are passed to `set_channel` and `start_edit` (the composer can't read the store) and cached by token ID.
  - `Candidate.display_name` was added.
  - Recent authors rank first within each match tier.
  - The list doesn't follow cursor-only moves.
- **Chunk 4:**
  - `add_members` takes only the response.
  - Voice channels aren't offered for `#`.
  - The nickname and `@username` share one row.
  - A failed member fetch is retried on the next `@`.
- **Chunk 5:**
  - `emojis` depends on `phf`.
  - Ranking adds an exact-match tier.
  - Closing colons convert Unicode shortcodes only, and only for a typed `:`.
- **Chunk 6:**
  - The button uses the `FaceSlightlySmiling` icon (Lucide renamed `smile`).
  - `EmojiBrowser` is its own view, with cells shared with the reaction grid.
  - Custom emoji come through an `OpenEmojiPicker` event.
  - Picker search shows up to 64 results.
  - Recently used is global.
- **At completion:** `tests/api_login.rs`'s live `GET /` check now also asserts that `limits` parse from the live shape for both account types.
- **Process:** the first chunk 2 commit timed out waiting for signing approval (Secretive or Touch ID: `communication with agent failed`). Work went on in the working tree, saving each chunk's files at its green state. Chunks 2–6 were then committed one per chunk from those snapshots, all signed: `19570c5`, `68a50a0`, `f4a4ca2`, `d77382f` and `ee84622`. Each intermediate commit was checked out on its own and passed `cargo test` (288, 303, 308 and 315 tests), and `ee84622` is the final, verified tree (320).

### Definition of Done
- [x] **Attaching:** the attach button, paste (images and files) and drag-and-drop add files to a tray with thumbnails and ×. Files over the size limit (20 MB) or past the count limit (5) are refused with a toast. Text pastes unchanged. Implemented and unit-tested. Only the button was used live (chunk 1); paste, drop and the toasts **still need manual checks**.
- [x] **Sending files:** pending at once with per-file progress, uploaded on send to `POST {autumn}/attachments`, posted with `attachments`, confirmed by nonce with no duplicates (tested). Confirmed live in chunk 1 (an SVG upload, then the send).
- [x] **File failures:** the reason with Retry and Discard. After a network failure the uploaded IDs and the key are kept; after an answered failure everything is uploaded again with a fresh key (tested). **The live offline Retry still needs a manual check.**
- [x] **@mentions:** DM and group recipients and recent authors at once, all server members after a single lazy fetch. Picking inserts an `@Name` chip sent as `<@id>`, with arrows, Enter or Tab, and Esc. Chips show in drafts and edits. Implemented and unit-tested; **still needs a manual check** (pinging in the web app, deleting a chip as one unit).
- [x] **#channels:** `#` suggests the server's text channels and inserts a chip sent as `<#id>`. Implemented and unit-tested; **still needs a manual check.**
- [x] **:emoji:** `:` with 2 or more characters suggests Unicode and custom emoji, a complete `:shortcode:` converts, and times and URLs don't trigger (tested). **Still needs a manual check.**
- [x] **Emoji picker:** virtualized, with search, recently used, every category and custom emoji, inserting at the cursor. Implemented and unit-tested (pure parts); **still needs a manual check** (scrolling, focus, custom emoji).
- [x] **Checks:** `cargo test && cargo clippy --all-targets -- -D warnings` gives 320 passed, 0 failed, 5 ignored, and clippy is clean. `cargo test -- --ignored`: all 5 live checks pass (Keychain round trip, live `GET /` against the pinned models, now including `limits`, the events handshake, the default avatar, the media proxy).

### Remaining Manual Steps
1. **Plan Manual Tests**, all deferred by the user:
   1. Attach a PNG, a PDF and a short video with a caption, and send. Watch the progress and compare with the web app.
   2. Paste a screenshot (⌘⌃⇧4), paste copied Finder files, drag a file onto the message list (the overlay should show), and paste plain text (it should stay text).
   3. Send with Wi-Fi off, then Retry once back online. It should send exactly once, with its files. (Also outstanding from milestone 3.)
   4. Try a 25 MB file and a sixth file. Each should get a toast.
   5. `@` in a DM and in a server, including a quiet member (shown after "Loading members…"). Link a channel with `#`. Both should ping or link in the web app.
   6. Edit a message with a mention and a custom emoji. Both should show as chips. Save it unchanged, then with a change.
   7. `:thu` → 👍, a custom emoji from the list, typing `:tada:` in full, and `12:30` (nothing should open).
   8. The emoji picker: search "cat" → 🐱, scroll every category, pick a custom emoji, reopen (recently used should come first), Shift-click (it should stay open), and Esc (it should close without clearing replies).
   9. Light and dark mode: the tray, the progress bars, the suggestion list, chips, the drop overlay and the picker.
