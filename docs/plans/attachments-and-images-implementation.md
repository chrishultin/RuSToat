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
