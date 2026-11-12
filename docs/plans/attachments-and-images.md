# Plan: Attachments and images (RuSToat milestone 30)

## Summary
Milestone 30 of the [roadmap](roadmap.md) adds:
- **file and image actions:** Save…, Copy image, Copy link and Open in browser, from an attachment's or an embed image's right-click menu and from the image viewer (its bar and its own right-click menu);
- **spoiler uploads:** mark a file in the message box as a spoiler; spoiler images arrive blurred and show when clicked;
- **text-file previews:** a text attachment shows its contents in the message, loaded at once when small, or with "Load file" when not.

## Approach
**What Stoat does** (0.16.4, from `/tmp/stoatchat-0.16.4`, `/tmp/for-web-0.16.3` and `stoatchat/javascript-client-sdk`, read on 2026-11-12):
- **Spoilers aren't a field.** The backend never mentions them. A spoiler is a file whose **filename starts with `spoiler_`, in any case**: the SDK's `File.isSpoiler` is `filename.toLowerCase().startsWith("spoiler_")`, and the web client uploads a marked file as `SPOILER_{name}` (`components/state/stores/Draft.ts`).
- **The web client's spoiler** (`components/ui/components/utils/Spoiler.tsx`) covers images and videos with a dark blur and "Click to show spoiler"; one click removes it. Other files aren't covered.
- **Text files** have `metadata.type == "Text"` (the file server decides). The web client (`elements/TextFile.tsx`):
  - loads files of at most 50,000 bytes at once, and offers "Load file ({size})" for bigger ones;
  - shows a `pre` block, 120 px tall at most, scrolling, wrapped.
- **The web client's file menu** (`app/menus/MessageContextMenu.tsx`) has Open file, Copy link to file, Save file and Copy file. Files are served from `{autumn}/attachments/{id}`, with no resizing.

**What exists in RuSToat** (as of 2026-11-12, after milestone 29, `750657d`):
- **`src/attachment.rs`:** `AttachmentView` comes in three shapes. `Image` is drawn inline at a size fixed by its metadata, with a ThumbHash while it loads and `too_big` behind "Show image". `File` is a card that opens in the browser. `Local` is a file still uploading. There are also `FileKind` (including `Text`) and `file_kind`.
- **`src/ui/chat.rs`:**
  - `render_attachment` draws them;
  - `inline_image`, `viewable_embed_image` and the GIF/card pictures open the viewer through `RowAction::ViewImage(ViewedImage { url, name, original })`;
  - `render_viewer` has a bar with "Open in browser" and Close;
  - the message row has one right-click menu (`row.context_menu`), and elements inside rows already carry their own (an author's avatar opens the person menu), so nested menus work.
- **Rows:** they're rebuilt from the store (`message_row`), and `diff_rows` remeasures a row whose data changed. Anything that changes a row's height (a loaded text preview, a revealed spoiler) must therefore be row data held by the store, like `revealed_images` and `revealed_spoilers`.
- **Embeds:** `src/embed.rs` `EmbedImage { url (proxied), width, height, original }`. It doesn't keep the image's own address.
- **The message box:**
  - `src/draft.rs` `DraftAttachment { local_id, name, size, content_type, kind, dimensions, path, temporary }`;
  - `src/ui/composer.rs` `render_tray` draws a chip per file, with ×;
  - `ChatView::upload_file` sends `file.name` to `StoatClient::upload_attachment`.
- **GPUI** (`gpui-pre` 0.3.11) has the pieces:
  - `cx.prompt_for_new_path(dir, Some(name))` shows the macOS save panel;
  - `ClipboardItem::new_image(&Image::from_bytes(ImageFormat::Png, bytes))` copies an image;
  - `cx.open_url` and `cx.reveal_path` cover Open in browser and Show in Finder.
  - The `image` crate (0.25) is a dependency, and its PNG, JPEG, GIF and WebP decoders are already built (GPUI enables them).

**The chosen shape** (user's choices on 2026-11-12):
- **Save… uses the macOS save panel,** starting in Downloads with the file's name filled in (not straight to Downloads).
- **Every image the viewer can show gets the actions:** attachments, link-preview images and GIFs. An embed's image has no filename, so its name comes from its URL's last segment (else "image"), and Copy link copies its own address, not the proxy's.
- **Spoilers:**
  - spoiler images are blurred until clicked, as on the web;
  - a spoiler text file's preview stays hidden until clicked too;
  - other spoiler files get a "Spoiler" tag on their card.
- **Text previews are plain monospace text,** with no syntax highlighting.
- **Logic and views are split:**
  - `src/file_actions.rs` (new, no GPUI): what can be done with a file or picture, names, which clipboard format, spoiler names;
  - the text preview's states live in the store and `src/attachment.rs`;
  - `src/ui/chat.rs` draws everything and runs the actions.

## Assumptions
1. **The menu** appears on right-clicking an inline image, an embed's picture, or a file card. Its items, in order:
   - "Save…";
   - "Copy image" (pictures only);
   - "Copy link";
   - "Open in browser";
   - a separator, then the message's own items (Reply, Quote, …), so the row's menu isn't lost.

   Left-clicking still opens the viewer (pictures) or the browser (files).
2. **The viewer's bar** has Save…, Copy image, Copy link, Open in browser and Close. Right-clicking the picture gives the same four actions.
3. **Save…:**
   - The panel starts in `~/Downloads` (else the home folder), with the file's name filled in. A spoiler's `SPOILER_` prefix is left out.
   - The file is downloaded after the panel closes, then written to the chosen path. The toast says "Saved {name}." with "Show in Finder"; a failure toasts "Couldn't save {name}: {reason}."
   - Attachments are downloaded from the file server's original. Embed pictures come through the proxy (the address the client already uses).
4. **Copy image** downloads the picture. PNG and JPEG bytes are copied as they are; anything else (GIF, WebP, …) is decoded and copied as PNG, since many macOS apps can't paste WebP. An animated GIF copies its first frame. The toast says "Image copied."
5. **Copy link:**
   - for an attachment, its file-server URL;
   - for an embed picture, the picture's own address, kept in a new `EmbedImage.source`.

   The toast says "Link copied."
6. **Names:**
   - shown names drop a leading `spoiler_` (any case), as long as something is left;
   - an embed picture's name is its URL's last path segment, percent-decoded, else "image".
7. **Spoiler images** (spoiler-named attachments with image metadata) are drawn at their normal size:
   - before they're shown: their ThumbHash, blurred further and darkened (or a plain dark box without one), with a "Spoiler" pill in the middle;
   - the first click shows them for the session (`ChatStore::reveal_attachment`, by file ID); after that they behave like any image;
   - the menu's actions work while they're covered. Copying and saving reveal nothing on screen.
8. **Spoiler files** that aren't images get a small "Spoiler" tag on their card. A spoiler text file's preview is replaced by "Spoiler: click to show" until clicked (the same reveal set).
9. **Marking a spoiler:**
   - each chip in the tray gets an eye button ("Mark as spoiler" / "Unmark spoiler"), and a marked chip shows "Spoiler" over a dimmed thumbnail;
   - the flag is `DraftAttachment.spoiler`, kept with the draft and through an edit stash;
   - the upload is named `SPOILER_{name}`, unless it already starts with `spoiler_`;
   - the pending row draws a marked image covered, as the sent one will be.
10. **Text previews** are for attachments whose metadata is `Text`:
    - a file of at most 50,000 bytes loads as soon as its row is built; a bigger one shows "Load file ({size})";
    - the text is read as UTF-8 (lossy), with the first 50,000 bytes kept if the file is longer than it said;
    - it's shown in a monospace block under the card, wrapped, at most 120 px tall, then scrolling inside;
    - while loading: "Loading…"; on failure: "Couldn't load the file." with Try again.

    The contents live in the store by file ID, for the session, so the row's height changes through the normal diff and remeasure.
11. **Downloads** go through one new client call, `StoatClient::download(url, max: Option<u64>) -> Vec<u8>`, using the API client's `reqwest` (no session token: the file server and proxy are public). Text previews and Copy image use a cap; Save doesn't.

## Plan

### Chunk 1: File actions on attachments
**Goal:** right-click an inline image or a file card to Save…, Copy image, Copy link or Open in browser.
**Files likely touched:**
- `src/file_actions.rs` (new, no GPUI):
  - `FileTarget { name, url, link, picture: bool }` and `actions(&FileTarget) -> Vec<FileAction>`;
  - `display_name` (Assumption 6), `suggested_save_name`, `clipboard_image(bytes, content_type) -> (ImageFormat-like, bytes)` (PNG/JPEG pass through, the rest becomes PNG);
  - the texts (toasts, errors).
- `src/api/client.rs`: `download(url, max)`.
- `src/attachment.rs`: `AttachmentView::Image` and `File` carry what a `FileTarget` needs (the content type).
- `src/ui/chat.rs`:
  - a `context_menu` on inline images and file cards, with the four actions then the message's items;
  - `RowAction::FileAction(FileAction, FileTarget)`, run by `save_file` (save panel, download, write, toast with "Show in Finder"), `copy_image`, `copy_link` and `open_file`.
- `src/lib.rs`.
- `tests/file_actions.rs` (new), `tests/api_upload.rs` (or a new `tests/api_files.rs`).

**Depends on:** None
**Risks:**
- **Nested right-click menus:** the attachment's menu must win over the row's. Avatars already do this inside rows; follow how `person_target` attaches its menu. If GPUI opens both, attach the row menu to the bubble's text only.
- **The clipboard image** has to paste into other apps (Preview, Messages). Check PNG after conversion; if GPUI writes an unexpected pasteboard type on macOS, always convert to PNG.
- **Big files on Save** are held in memory once. That's acceptable for Stoat's 20 MB attachment limit; note it, don't stream.

**How to verify:**
- `cargo test --test file_actions`:
  - names (spoiler prefix stripped, kept when nothing would be left, a URL's last segment decoded, "image" fallback);
  - which actions a picture or file gets;
  - the clipboard format choice (PNG and JPEG pass through, GIF and WebP fixtures become PNG with the right size).
- wiremock: `download` returns the body, refuses past `max`, and turns a 404 into an error with a plain reason.
- Then the full suite.

**Manual check:** save an image and a PDF through the panel; copy an image and paste it into Preview (File › New from Clipboard); copy a link; open in browser.

**Shippable when:** attachments' right-click menu saves, copies and opens them.

### Chunk 2: The viewer and embed pictures
**Goal:** the same actions from the image viewer and on link-preview pictures and GIFs.
**Files likely touched:**
- `src/embed.rs`: `EmbedImage.source` (the image's own URL), set wherever an `EmbedImage` is made.
- `src/ui/chat.rs`:
  - `ViewedImage` gains the `FileTarget`;
  - the viewer's bar has Save…, Copy image, Copy link, Open in browser and Close, with a right-click menu on the picture;
  - embed pictures (`viewable_embed_image`, card images, GIF stills) get the right-click menu.
- `src/file_actions.rs`: `embed_target(&EmbedImage, title)`.
- `tests/file_actions.rs`, `tests/store_embeds.rs`.

**Depends on:** Chunk 1
**Risks:**
- **The viewer is in front of everything:** its menu must render above it (the viewer uses `deferred` with priority; check the menu's layer).
- **A GIF poster's link is its page,** not a picture: Copy link copies the picture's source, and Open in browser opens the page, as clicking it does now.

**How to verify:**
- `cargo test --test store_embeds`: `source` is the embed's own URL (not proxied) for an image link, a card's image and a Gifbox still.
- `tests/file_actions.rs`: embed targets' names and links.
- Then the full suite.

**Manual check:** open an attachment and a link preview in the viewer; use each bar button and the right-click menu; right-click a GIF in a message.

**Shippable when:** every picture the viewer shows can be saved, copied, linked and opened from the viewer and from its message.

### Chunk 3: Spoiler attachments
**Goal:** attachments named as spoilers arrive covered, and show when clicked.
**Files likely touched:**
- `src/file_actions.rs`: `is_spoiler(name)`.
- `src/attachment.rs`: `AttachmentView::Image` gains `spoiler: bool` (covered and not revealed yet); `File` gains `spoiler: bool` (the tag).
- `src/store.rs`: `revealed_attachments` and `reveal_attachment(file_id)`; `message_row` passes it.
- `src/ui/chat.rs`:
  - a covered image: the blurred, darkened ThumbHash (or a dark box) with a "Spoiler" pill, and a click revealing it (`RowAction::RevealAttachment`);
  - a file card with a "Spoiler" tag;
  - names shown without the prefix.
- `tests/store_attachments.rs`, `tests/file_actions.rs`.

**Depends on:** None (Chunk 1 for the menu on covered images)
**Risks:**
- **The row must keep its height** when revealed: the covered image is drawn at the same size.
- **The ThumbHash blur:** `thumbhash` gives a tiny image already; darkening it (an overlay) may be enough. Don't add a blur pass if the overlay reads as covered.

**How to verify:**
- `cargo test --test file_actions`: `is_spoiler` (`SPOILER_a.png`, `spoiler_a.png`, `Spoiler_A.PNG`, not `spoiler.png`, not `my_spoiler_a.png`).
- `cargo test --test store_attachments`:
  - a spoiler image's row is covered, and revealed by file ID (another image in the same message stays covered);
  - a spoiler file has the tag;
  - shown names lose the prefix;
  - the width and height don't change when revealed.
- Then the full suite.

**Manual check:** a spoiler image sent from the web client arrives covered; clicking shows it; a spoiler PDF has the tag.

**Shippable when:** spoiler attachments sent from any client are covered until clicked.

### Chunk 4: Mark an upload as a spoiler
**Goal:** mark a file in the message box as a spoiler before sending.
**Files likely touched:**
- `src/draft.rs`: `DraftAttachment.spoiler`, `upload_name()` (`SPOILER_{name}` once).
- `src/ui/composer.rs`: the tray chip's eye button and its "Spoiler" look.
- `src/ui/chat.rs` / `src/store.rs`:
  - `upload_file` sends `upload_name()`;
  - the pending `Local` image is covered when marked (`PendingAttachment` gains `spoiler`).
- `tests/store_pending.rs`, a draft test (`tests/composer` tests live in `src/draft.rs`'s tests or `tests/store_pending.rs`), `tests/api_upload.rs`.

**Depends on:** Chunk 3
**Risks:**
- **The name reaches several places:** the upload, the pending row, and a retried send. Use `upload_name()` everywhere a marked file is named, and test the retry path.
- **`composer.rs` formatting drift:** check its count before editing (`git show HEAD:src/ui/composer.rs | rustfmt --edition 2024 --check | grep -c '^Diff in'`).

**How to verify:**
- Unit: `upload_name` (marked, unmarked, already prefixed in any case).
- `cargo test --test api_upload`: the multipart filename is `SPOILER_photo.png` for a marked file.
- `cargo test --test store_pending`: a marked pending image is covered.
- Then the full suite.

**Manual check:** mark a pasted image and a picked file; send; they arrive covered here and in the web client.

**Shippable when:** spoilers can be sent from RuSToat and show as spoilers everywhere.

### Chunk 5: Text-file previews
**Goal:** a text attachment shows its contents in the message.
**Files likely touched:**
- `src/attachment.rs`:
  - `TEXT_AUTO_LOAD` (50,000);
  - `TextPreview { NotLoaded, Loading, Loaded(String), Failed, Hidden }`;
  - `AttachmentView::File` gains `preview: Option<TextPreview>` (Text files only).
- `src/store.rs`:
  - `text_previews: HashMap<String, TextPreviewState>`, `set_text_preview` and `text_previews_wanted(channel)` (small Text files not asked for yet);
  - a spoiler's preview stays `Hidden` until revealed.
- `src/ui/chat.rs`:
  - after `sync_pane`, fetch the wanted previews (`download(url, Some(TEXT_AUTO_LOAD))`), store them, and re-sync;
  - "Load file ({size})" and Try again as row actions;
  - the monospace block (max 120 px, scrolling, wrapped).
- `tests/store_attachments.rs`, `tests/api_upload.rs` (or `tests/api_files.rs`).

**Depends on:** Chunk 1 (`download`), Chunk 3 (spoiler reveal)
**Risks:**
- **Scrolling inside a list row:** the block's scroll must not fight the timeline's. Stop propagation once the block can scroll, as the profile card does (`on_scroll(|_, _, cx| cx.stop_propagation())`), but only when it overflows, or the timeline can't scroll past it.
- **Fetch storms:** a channel with many text files must not fetch them all at once on every sync. Mark each `Loading` before fetching, so a re-sync doesn't ask again.
- **Text that isn't text:** a binary file the server called Text shows replacement characters. That's acceptable; cap it at 50,000 bytes.

**How to verify:**
- `cargo test --test store_attachments`:
  - a small text file is wanted once, and not again while loading;
  - a big one isn't wanted until asked for;
  - loaded text is in the row (the row's data changes, so it's remeasured);
  - a failure keeps Try again;
  - a spoiler's preview stays hidden until revealed.
- wiremock: `download` with a cap stops at 50,000 bytes.
- Then the full suite.

**Manual check:** send a small `.txt` and a large `.log`; the small one shows at once, the large one on "Load file"; a spoiler `.txt` stays hidden until clicked; scroll inside a long preview and past it.

**Shippable when:** text attachments preview in the message.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–29):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3. Check any other file's count with `git show HEAD:<file> | rustfmt --edition 2024 --check | grep -c '^Diff in'` before formatting it whole.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/file_actions.rs` (new): names, spoiler detection, which actions each target has, the clipboard format choice (with small PNG, JPEG, GIF and WebP fixtures), embed targets.
- `tests/store_attachments.rs`: spoiler covering and reveal, the tag, names, text preview states and what's wanted.
- `tests/store_embeds.rs`: `EmbedImage.source`.
- `tests/store_pending.rs` and `src/draft.rs`: the spoiler flag and upload name.

### Integration Tests
wiremock (`tests/api_upload.rs`, or a new `tests/api_files.rs`):
- `download` returns the body, stops at a cap, and reads a 404 plainly;
- a marked upload's multipart filename is `SPOILER_{name}`.

### Manual Tests
Run with the web client in a test channel. Batch them at the end if the user asks:
1. **Attachments' menu:** Save… (panel, Downloads, Show in Finder), Copy image (paste into Preview and Messages), Copy link, Open in browser, on an image and on a PDF.
2. **The viewer:** each bar button and the right-click menu, for an attachment, a link preview and a GIF.
3. **Spoilers in:** a spoiler image from the web client is covered and shows on click; a spoiler file has its tag.
4. **Spoilers out:** mark a picked file and a pasted image; they arrive covered here and on the web.
5. **Text files:** a small `.txt` shows at once, a large `.log` on "Load file", a spoiler `.txt` on click; scrolling inside and past a long preview.
6. **Themes:** the covered image, the tag, the tray's spoiler chip and the preview block read well in light and dark.

## Definition of Done
- [ ] Inline images, embed pictures and file cards have a right-click menu with Save…, Copy image (pictures), Copy link and Open in browser, followed by the message's own items.
- [ ] The image viewer's bar and its right-click menu offer the same actions, for attachments, link previews and GIFs.
- [ ] Save… uses the macOS save panel (Downloads, the name filled in) and offers Show in Finder; Copy image pastes into Preview.
- [ ] Attachments named `spoiler_…` (any case) are covered until clicked (images) or tagged (other files), and their names are shown without the prefix.
- [ ] Files marked in the tray upload as `SPOILER_{name}` and show covered here and in the web client.
- [ ] Text attachments preview in a scrolling monospace block: at once up to 50,000 bytes, else on "Load file"; spoiler previews stay hidden until clicked.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan.
