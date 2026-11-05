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
