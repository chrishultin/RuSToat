# Implementation Record: Attachments and images (milestone 30)

_Plan: [`attachments-and-images.md`](attachments-and-images.md)_
_Started: 2026-11-12_

Run in continuous mode. Manual checks are batched at the end, as the user asked, and the work is merged into `main` at the end. The plan and its roadmap link (uncommitted before chunk 1) were committed with chunk 1, as before.

Rustfmt drift at the start (`Diff in` hunks): `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3. Every other touched file was clean.

## Chunk 1: File actions on attachments

### Changes Made
- `src/file_actions.rs` (new, no GPUI):
  - `FileTarget { name, url, link, picture }`, `FileAction { Save, CopyImage, CopyLink, OpenInBrowser }` with `label()`, and `actions(&FileTarget)` (Copy image for pictures only);
  - `display_name` (drops a leading `spoiler_` in any case while something is left; byte-safe), `suggested_save_name` (the shown name, with `/`, `:` and NUL replaced, leading dots and spaces dropped, "file" if empty), `save_directory(home)` (`~/Downloads`, else home);
  - `clipboard_image(bytes)` → `(ClipboardFormat::{Png, Jpeg}, bytes)`: the format is sniffed from the bytes. PNG and JPEG pass through; anything else is decoded and re-encoded as PNG (an animated GIF gives its first frame). `COPY_IMAGE_MAX_BYTES` (50 MB);
  - `download_failure(&ApiError)` (404 "it's no longer there", 401/403 "the server refused it", network "couldn't reach the server", other statuses "the server answered HTTP {n}"), and the texts: "Saved {name}.", "Couldn't save {name}: {reason}.", "Couldn't copy the image: {reason}.", "Image copied.", "Link copied.", "Show in Finder".
- `src/api/client.rs`: `download(url, max)`: a `GET` without the session token (the file server and proxy are public), with the upload's 10-minute timeout. With `max`, it reads the body as a stream and stops after `max` bytes. Error statuses become `ApiError` (a 404 is `Unexpected { status: 404 }`).
- `src/attachment.rs`: `AttachmentView::file_target()`: the file server's original for `Image` and `File` (the name shown without a spoiler prefix); `None` for `Local`, or before the file server is known.
- `src/ui/chat.rs`:
  - `RowAction::File(FileAction, FileTarget)`, run by `file_action`:
    - `save_file`: the save panel (`prompt_for_new_path`, in Downloads, with the name filled in), then the download and write on the IO runtime, then a toast "Saved {name}." with a "Show in Finder" button (`reveal_path`), or the failure;
    - `copy_image`: the download (capped at 50 MB + 1 byte, to tell a picture that's too big), `clipboard_image`, then `ClipboardItem::new_image`, and "Image copied.";
    - Copy link copies the link and toasts "Link copied."; Open in browser goes through `open_external`.
  - The row's right-click items move into `MessageMenu` (built once per row, `None` for search results and messages still sending), and `MessageMenu::items` adds them to any menu.
  - `render_attachment` takes the message's menu. It draws through `attachment_element`, then `with_file_menu` adds a right-click menu: `file_menu_items` (Save…, Copy image, Copy link, Open in browser, with icons), a separator, then the message's items. The menu goes on inline images, "Show image" cards and file cards. It isn't on uploads, search results or messages still sending.
  - `render_embed` and `render_embed_card` pass the message's menu down, so a bot embed's uploaded file gets the same menu.
- `Cargo.toml`: the `image` dependency lists the `png`, `jpeg`, `gif` and `webp` features (the decoders GPUI already enables), since the crate now decodes and encodes with them itself.
- `src/lib.rs`: the module, by hand.
- Tests:
  - `tests/file_actions.rs` (new, 10 tests): actions per target and their labels; shown names (prefix in any case, nothing left, `spoiler.png`, `my_spoiler_…`, a multi-byte character at the boundary); save names; the save folder; PNG and JPEG passed through; GIF and WebP turned into PNG of the same size; an animated GIF's first frame; non-pictures refused; failure texts; attachments' targets (spoiler name stripped, file-server URL, no target without a file server).
  - `tests/api_files.rs` (new, 4 tests, wiremock): the body comes back without the session token; a cap stops at the cap (and a bigger cap gives it all); a 404 reads as "it's no longer there"; an unreachable server reads as "couldn't reach the server".

### Divergences
- **`download(url, max)` keeps the first `max` bytes rather than refusing a bigger file.** That's what chunk 5's text previews need (Assumption 10: "the first 50,000 bytes kept"). Copy image asks for one byte over its 50 MB limit and refuses a picture that's longer ("it's too big"). The chunk's test is "a cap stops at the cap", not "refuses past `max`".
- **The clipboard format is sniffed from the bytes, not taken from the content type.** Embed pictures have no content type, and a server's label can be wrong. So `clipboard_image` takes only the bytes, and `AttachmentView` didn't need to carry the content type. `AttachmentView::file_target()` builds the target instead.
- **Open in browser goes through `open_external`.** Attachments are on the instance's own file server, so they open directly. Chunk 2's embed pictures on other sites will get the usual link-safety prompt.
- **The row's menu became `MessageMenu`.** Its items are reused at the end of each file's menu, as Assumption 1 asks.
- **Download tests are in a new `tests/api_files.rs`,** one of the two places the plan offered.
- **Another session in this worktree briefly reverted `src/ui/chat.rs`** (its `git restore` while it fixed notification settings, now on `main` as 66de472). It restored part of this chunk's hunks from a patch, and the rest was redone and checked here. Nothing of its work remains in this branch.

Look-ahead: chunk 2 can reuse `with_file_menu`, `file_menu_items` and `RunFileAction` (the viewer runs actions directly on the view, not through a row). Chunk 5's `download(url, Some(TEXT_AUTO_LOAD))` matches the truncating cap. No plan amendments are needed.

### Verification Performed
- `cargo test --test file_actions --test api_files`: 10 and 4 passed.
- Full suite, `cargo test && cargo clippy --all-targets -- -D warnings`: 1,441 passed, 0 failed; clippy clean.
- Rustfmt: `src/ui/chat.rs` 14 (down from 17: reformatting this chunk's regions also cleared older hunks inside them), `src/api/client.rs` 3, `src/attachment.rs` 0; the new files are clean.

### Manual Validation
Batched at the end (Manual Test 1).

## Chunk 2: The viewer and embed pictures

### Changes Made
- `src/embed.rs`: `EmbedImage.source`, the picture's own address (not proxied), set in `EmbedImage::new`. That covers every embed picture: image links, cards' images and thumbnails, GIF links and Gifbox stills.
- `src/file_actions.rs`:
  - `FileTarget.page`: where Open in browser goes instead of `link` (a GIF still's page);
  - `embed_target(&EmbedImage, page)`: downloaded through the proxy (`url`), linked by `source`, named by `embed_name`;
  - `embed_name(url)`: the URL's last non-empty path segment, percent-decoded (a `+` stays a plus), else "image".
- `src/attachment.rs`: `file_target()` sets `page: None`.
- `src/ui/chat.rs`:
  - `ViewedImage.target`. An attachment's viewer title is now its shown name (`target.name`).
  - The viewer's bar has Save…, Copy image, Copy link and Open in browser (ghost buttons with icons), then Close. The picture has a right-click menu with the same four. Both run `ChatView::file_action` through a `RunFileAction` holding a weak handle on the view.
  - `with_file_menu` takes `Option<&Rc<MessageMenu>>` and adds no menu without one, so callers don't check.
  - `viewable_embed_image(id, image, title, …, message)` returns the picture with its menu. Its viewer title is `title`, else the picture's name (it was "Image").
  - The menu is on standalone embed pictures, card thumbnails and large pictures, and GIF stills. A GIF still and a video's play picture keep their click (opening the page) and get `page` set, so Open in browser goes there and Copy link copies the picture's own address.
  - `file_action`'s Open in browser opens `page`, else `link`, through `open_external` (so an outside site gets the usual link-safety prompt).
- `Cargo.toml`: `percent-encoding = "2"` (already in the tree, through `url`).
- Tests:
  - `tests/store_embeds.rs`: every `EmbedImage` expected has its `source`: an image link, a card's image, a GIF link and Gifbox stills (from a video embed and from a website embed's poster).
  - `tests/file_actions.rs` (+2 tests): embed names (query and fragment dropped, percent-decoding, trailing slash, no path, not a URL, `+`); embed targets (proxy to download, own address to link, four actions, and a GIF's page for Open in browser).

### Divergences
- **`FileTarget.page` was added** for the plan's risk "a GIF poster's link is its page": Copy link copies the still's address and Open in browser opens the page. A video's play picture in a card works the same way.
- **`embed_target(image, page)` takes the page, not the title.** Assumption 6 names an embed picture by its URL, so a title isn't needed for the name. The card's title is still the viewer's heading for its large picture.
- **Open in browser on an outside picture asks first** (`open_external`), like any other link from a message. The old viewer button opened the proxied address directly; it now opens the picture's own address.

Look-ahead: no remaining chunk is affected.

### Verification Performed
- `cargo test --test store_embeds --test file_actions`: 16 and 12 passed.
- Full suite, `cargo test && cargo clippy --all-targets -- -D warnings`: 1,443 passed, 0 failed; clippy clean.
- Rustfmt: `src/ui/chat.rs` 14 (unchanged from chunk 1); `src/embed.rs`, `src/file_actions.rs` and the tests are clean.

### Manual Validation
Batched at the end (Manual Test 2).

## Chunk 3: Spoiler attachments

### Changes Made
- `src/file_actions.rs`:
  - `is_spoiler(name)`: the name starts with `spoiler_` in any case. A name that's only the prefix counts, as in Stoat's SDK.
  - `suggested_save_name` no longer strips the prefix itself: it's given the shown name, which already lost it. Stripping twice would turn `spoiler_spoiler_x` into `x`.
- `src/attachment.rs`:
  - `AttachmentView::Image.spoiler`: covered, not shown yet. `AttachmentView::File.spoiler`: the tag.
  - Both views' `name` is now the shown name (`display_name`), so the card, the viewer and the file actions all use it. `file_target()` takes it as is.
  - `uncovered(shown)` clears an image's cover.
- `src/store.rs`: `revealed_attachments` (by file ID) and `reveal_attachment(file_id)`. `message_row` applies it to attachments, and passes it to `EmbedView::new` for a bot embed's file.
- `src/embed.rs`: `EmbedView::new` takes `shown` (spoilers the user clicked to see) beside `revealed`, for a bot embed's file.
- `src/ui/chat.rs`:
  - `RowAction::RevealAttachment(file_id)` reveals it and re-syncs the pane.
  - `covered_image`: at the image's display size, its ThumbHash under a 60% black overlay (or the image backdrop and the overlay without one), with an `EyeOff` + "Spoiler" pill. Clicking it reveals the image; right-clicking gives the file menu (Chunk 1), which works while it's covered.
  - `spoiler_tag` on a spoiler file's card and on a large spoiler image's "Show image" card. "Show image" loads it, and it's still covered until clicked.
- `src/assets.rs`: `file_icons_load` checks Download, Copy, Link, ExternalLink and EyeOff.
- Tests:
  - `tests/store_attachments.rs` (+4): `is_spoiler` (the plan's names, plus `spoiler_`, an empty name and a multi-byte character); spoiler images covered and revealed by file ID (another spoiler in the same message stays covered; same size; the row is remeasured); a spoiler file's tag and shown name (and a name that's only the prefix); a large spoiler stays covered after "Show image";
  - `tests/store_history.rs`, `tests/store_embeds.rs`, `tests/file_actions.rs`: the new fields and `EmbedView::new`'s argument.

### Divergences
- **`AttachmentView::new` keeps its signature;** `.uncovered(shown)` applies a reveal. That leaves the 14 existing callers alone.
- **`EmbedView::new` takes the reveal set too,** so a spoiler file in a bot embed can be revealed like a message's own.
- **The shown name is set once, in `AttachmentView`,** and `suggested_save_name` no longer strips the prefix (see above).
- **There's no extra blur pass.** The ThumbHash under the overlay reads as covered, as the plan's risk allowed.
- **Reformatting `on_row_action`** (for the new arm) also cleared one older rustfmt hunk there (`RowAction::Delete { id, confirmed: true }`).

Look-ahead: chunk 4 can draw a marked pending image with `covered_image`. Chunk 5's "spoiler text preview hidden until revealed" can use `revealed_attachments`. No plan amendments are needed.

### Verification Performed
- `cargo test --test store_attachments --test file_actions`: pass.
- Full suite, `cargo test && cargo clippy --all-targets -- -D warnings`: 1,448 passed, 0 failed; clippy clean.
- Rustfmt: `src/ui/chat.rs` 13, `src/store.rs` 14 (unchanged); every other touched file is clean.

### Manual Validation
Batched at the end (Manual Test 3).

## Chunk 4: Mark an upload as a spoiler

### Changes Made
- `src/draft.rs`:
  - `DraftAttachment.spoiler`. `from_path` starts it marked when the file's name already starts with `spoiler_` (any case).
  - `upload_name()`: a marked file gets `SPOILER_` once. A marked file already named as a spoiler keeps its name. An unmarked one named `spoiler_…` loses the prefix, so it isn't sent as a spoiler.
  - `Draft::toggle_spoiler(local_id)`.
  - Unit tests: `a_marked_file_uploads_with_the_spoiler_prefix_once` and `files_in_the_tray_are_marked_and_unmarked_by_id`.
- `src/ui/composer.rs` (`render_tray`): each chip has an eye button. It shows `EyeOff` with "Mark as spoiler", or `Eye` with "Unmark spoiler" when marked. A marked chip's thumbnail is dimmed under a closed eye, and its second line reads "Spoiler · {size}".
- `src/ui/chat.rs`:
  - `upload_file` uploads under `file.upload_name()`. It's the one path for first sends and retries.
  - A marked pending image is drawn with `covered_image` (no ThumbHash yet, so the dark box and pill) with its upload bar. A marked pending file's card has the tag.
- `src/attachment.rs`: `AttachmentView::Local.spoiler`.
- `src/store.rs`: `PendingAttachment::view` passes `file.spoiler`. The flag lives on the `DraftAttachment` the pending message carries, so `PendingAttachment` needed no field of its own. The draft keeps it, and so does an edit stash (it stashes the whole `Draft`).
- Tests:
  - `tests/store_pending.rs` (+1): a marked image is covered at its fitted size while sending, and a marked file's card is tagged. After a failure and a retry, the files' upload names are still `SPOILER_photo.png`, `SPOILER_notes.pdf` and `plain.png`.
  - `tests/api_upload.rs` (+1): the multipart part is `filename="SPOILER_photo.png"` for a marked file.

### Divergences
- **A file already named `spoiler_…` starts marked, and unmarking it drops the prefix on upload.** The name is what makes a spoiler on Stoat, so the chip's flag must decide it. Otherwise an unmarked chip could still arrive covered.
- **The marked chip shows a closed eye over the dimmed thumbnail, and "Spoiler" goes in its size line** ("Spoiler · 1.2 MB"). The plan put "Spoiler" over the thumbnail, but the thumbnail is 36 px, too small to read it there.
- **`PendingAttachment` gained no field.** It already carries the `DraftAttachment`.
- **Reformatting `render_tray` also cleared one older rustfmt hunk there** (composer drift 14 → 13).

Look-ahead: chunk 5 is unaffected.

### Verification Performed
- `cargo test --lib draft`, `--test store_pending`, `--test api_upload`: pass.
- Full suite, `cargo test && cargo clippy --all-targets -- -D warnings`: 1,452 passed, 0 failed; clippy clean.
- Rustfmt: `src/ui/composer.rs` 13 (from 14), `src/ui/chat.rs` 13, `src/store.rs` 14, `tests/store_pending.rs` 18 and `tests/api_upload.rs` 10 (unchanged); `src/draft.rs` and `src/attachment.rs` clean.

### Manual Validation
Batched at the end (Manual Test 4).

## Chunk 5: Text-file previews

### Changes Made
- `src/attachment.rs`:
  - `TEXT_AUTO_LOAD` (50,000);
  - `TextLoad { Loading, Loaded(Arc<str>), Failed }`, which the store keeps by file ID;
  - `TextPreview { NotLoaded, Loading, Loaded(Arc<str>), Failed, Hidden }`, what the row shows;
  - `text_preview(file, shown, load)`, for files whose metadata is `Text` only: `Hidden` for a spoiler not shown yet; otherwise as far as `load` has got; a small file not asked for yet is `Loading` (it's fetched on the sync that builds it); a big one is `NotLoaded`;
  - `loads_at_once(file)`;
  - `preview_text(bytes)`: at most 50,000 bytes, lossy UTF-8, CRLF made LF and tabs four spaces (they don't draw);
  - `AttachmentView::File` gains `id` and `preview`, and `with_preview` sets it.
- `src/store.rs`:
  - `text_previews: HashMap<String, TextLoad>` and `set_text_preview(file_id, load)`;
  - `text_previews_wanted(channel)` → `(file ID, URL)`: the small text files in the channel's loaded messages, not hidden, and not asked for yet;
  - `message_row` applies `text_preview` once there's a file server to fetch from.
- `src/ui/chat.rs`:
  - `sync_pane` ends with `fetch_text_previews`. `load_text(channel, file_id, url)` marks the file `Loading`, fetches `download(url, Some(TEXT_AUTO_LOAD))` on the IO runtime, stores `Loaded` or `Failed`, and re-syncs, so the row is remeasured through the normal diff.
  - `RowAction::LoadText { id, url }` is "Load file ({size})" and Try again.
  - `text_preview_block` draws under the card:
    - "Spoiler: click to show" (it reveals the file, through `RevealAttachment`);
    - the Load file button;
    - "Loading…";
    - "Couldn't load the file." with Try again;
    - or the text in Menlo `text_xs`, wrapped, at most 120 px tall and scrolling inside.
  - A text file with a preview is a 432 px column: the card (widened to match), then the block.
  - `TextScrolls` keeps a `ScrollHandle` per file ID on the view, handed to rows through `Pickers`. It keeps a preview's place across redraws, and tells whether it overflows: only then does a scroll over it stop there rather than reach the timeline.
  - Search results and bot embeds get no preview (`render_attachment`'s `scrolls` is `None` there).
- Tests:
  - `tests/store_attachments.rs` (+6): a small file is wanted once (not again while loading), a big one not until asked; loaded text reaches the row and the row is remeasured; a failure offers Try again and isn't fetched again by itself; a spoiler's preview stays hidden, and unfetched, until revealed; `text/*` files the server didn't call Text, and files without a file server, get none; `preview_text` (tabs, CRLF, invalid UTF-8, the cap);
  - `tests/store_history.rs`, existing `store_attachments` literals: the new fields.
  - The download cap at 50,000 bytes was already covered by chunk 1's `tests/api_files.rs`.

### Divergences
- **The store's state is `TextLoad`, and `text_preview` combines it with the reveal set into the view's `TextPreview`.** The plan named it `TextPreviewState`. Text is held as `Arc<str>`, so the rows rebuilt on every sync don't copy it.
- **`AttachmentView::File` gained `id`,** which "Load file" and the spoiler's reveal need.
- **A small text file shows "Loading…" from the first build of its row,** since it's fetched on that same sync. "Not loaded" is for big files only.
- **There are no previews in search results or bot embeds.** They aren't in a channel's loaded history, so nothing would fetch them.
- **Overflow is read from a `ScrollHandle` per file ID kept on `ChatView`,** passed down through `Pickers`. A handle made fresh on each render would reset the scroll position.
- **Previews also normalise CRLF and tabs.**
- **Fetches aren't throttled.** Each small text file is one request, made once per session.
- **Reformatting `sync_pane` also cleared one older rustfmt hunk** (a mis-indented `fetch_missing_emojis` line).

### Verification Performed
- `cargo test --test store_attachments --test store_history`: 21 and 28 passed.
- Full suite, `cargo test && cargo clippy --all-targets -- -D warnings`: 1,458 passed, 0 failed; clippy clean (after replacing `or_insert_with(ScrollHandle::new)` with `or_default()`).
- Rustfmt: `src/ui/chat.rs` 12, `src/store.rs` 14; `src/attachment.rs` and the tests are clean.

### Manual Validation
Batched at the end (Manual Test 5).

## Summary

_Completed: 2026-11-12_

### Chunks Completed
1. File actions on attachments: Save…, Copy image, Copy link and Open in browser from an inline image's or a file card's right-click menu, followed by the message's own items.
2. The viewer and embed pictures: the same actions in the image viewer's bar and right-click menu, and on link-preview pictures, card images and GIF stills.
3. Spoiler attachments: spoiler images covered until clicked, spoiler files tagged, and names shown without the prefix.
4. Mark an upload as a spoiler: the tray's eye button, `SPOILER_{name}` uploads, and covered pending images.
5. Text-file previews: a scrolling monospace block, loaded at once up to 50,000 bytes, else on "Load file"; spoiler previews hidden until clicked.

Before the merge into `main`, `main` (66de472, the notification-settings fixes) was merged into this branch. The full suite passed on the result: 1,462 tests, 0 failed, and clippy clean.

### Divergences
- Chunk 1: `download(url, max)` keeps the first `max` bytes rather than refusing (Copy image asks for one byte over 50 MB to refuse a bigger picture). The clipboard format is sniffed from the bytes. Open in browser goes through `open_external`. The row's menu became `MessageMenu`. Another session's `git restore` briefly reverted `chat.rs`; the lost hunks were redone and checked.
- Chunk 2: `FileTarget.page`, so a GIF still's Open in browser goes to its page while Copy link copies the still. `embed_target` takes the page, not the title. Outside pictures open through the link-safety prompt.
- Chunk 3: `AttachmentView::uncovered(shown)` instead of a new constructor argument. `EmbedView::new` takes the reveal set. The shown name is set once in `AttachmentView`. There's no extra blur pass.
- Chunk 4: files already named `spoiler_…` start marked, and unmarking drops the prefix on upload. A marked chip has a closed eye over the thumbnail, and "Spoiler" goes in its size line. `PendingAttachment` needed no field.
- Chunk 5: `TextLoad` in the store and `TextPreview` in the view, holding `Arc<str>`. `AttachmentView::File.id`. Small files show "Loading…" from the start. No previews in search results or bot embeds. A `ScrollHandle` per file ID is kept on the view. CRLF and tabs are normalised. Fetches aren't throttled.
- Small rustfmt cleanups inside reformatted functions cleared older hunks: `chat.rs` 17 → 12, `composer.rs` 14 → 13.

### Definition of Done
- [x] Inline images, embed pictures and file cards have a right-click menu with Save…, Copy image (pictures), Copy link and Open in browser, followed by the message's own items.
- [x] The image viewer's bar and its right-click menu offer the same actions, for attachments, link previews and GIF links. Gifbox stills keep opening their page and aren't viewed, but have the menu in the message.
- [x] Save… uses the macOS save panel (Downloads, with the name filled in) and offers Show in Finder; Copy image writes PNG or JPEG. *Pasting into Preview is still to be checked by hand (Manual Test 1).*
- [x] Attachments named `spoiler_…` (any case) are covered until clicked (images) or tagged (other files), and their names are shown without the prefix.
- [x] Files marked in the tray upload as `SPOILER_{name}` and show covered here. *Showing covered in the web client is still to be checked by hand (Manual Test 4).*
- [x] Text attachments preview in a scrolling monospace block: at once up to 50,000 bytes, else on "Load file"; spoiler previews stay hidden until clicked.
- [x] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew (`chat.rs` 12, `store.rs` 14, `client.rs` 3, `events.rs` 3, `composer.rs` 13), and the new files are clean.
- [x] The roadmap links this plan.

### Remaining Manual Steps
The plan's manual tests, batched as the user asked. Run them with the web client in a test channel:
1. **Attachments' menu:** on an image and a PDF, try Save… (the panel opens in Downloads, then "Show in Finder"), Copy image (paste into Preview with File › New from Clipboard, and into Messages), Copy link and Open in browser.
2. **The viewer:** try each bar button and the right-click menu for an attachment, a link preview and a GIF link. Also right-click a Gifbox GIF in a message, where Open in browser should open its page. Check the menu draws above the viewer, and that clicking an item doesn't close the viewer.
3. **Spoilers in:** a spoiler image from the web client arrives covered and shows on click; a spoiler PDF has its tag.
4. **Spoilers out:** mark a picked file and a pasted image with the tray's eye button; they show covered while sending, and arrive covered here and on the web.
5. **Text files:** a small `.txt` shows at once; a large `.log` shows on "Load file"; a spoiler `.txt` shows on click. Scroll inside a long preview, and past a short one.
6. **Themes:** the covered image, the tag, the tray's spoiler chip and the preview block read well in light and dark.
