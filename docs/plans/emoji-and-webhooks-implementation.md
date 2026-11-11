# Implementation Record: Emoji and webhooks (milestone 28)

_Plan: [`emoji-and-webhooks.md`](emoji-and-webhooks.md)_
_Started: 2026-11-11_

Run in continuous mode. Manual checks are batched at the end, as the user asked. The plan and its roadmap link (uncommitted before chunk 1) were committed with chunk 1, as before.

**Stoat version.** The plan was read from 0.16.2's source. Since then the live API went to 0.16.4 and RuSToat's Stoat crates were updated to `=0.16.4` (`1f568f4`). Between the two, no emoji or webhook route changed (only `webhook_execute_github.rs`), so the plan stands.

Rustfmt drift at the start (`Diff in` hunks): `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3. Every other touched file was clean.

## Chunk 1: The Emojis page: list and delete

### Changes Made
- `src/custom_emoji.rs` (new, no GPUI):
  - `DEFAULT_SERVER_EMOJI` (100);
  - `EmojiRow { id, name, url, animated, creator, creator_name }` and `emoji_rows(store, server)` (by name; an uncached creator is "Unknown user");
  - `can_manage_emoji` (owner, or a known member with Manage Customisation);
  - `count_line`, `delete_prompt` and `emoji_error_text` (Assumption 11).
- `src/store.rs`: `server_emojis(server)` (by name; the channel-keyed `server_emoji` the picker uses is unchanged) and `rename_emoji(id, name)`.
- `src/api/events.rs`: `ServerEvent::EmojiUpdate { id, name }`, decoded from `{ id, data: PartialEmoji }`.
- `src/api/client.rs`:
  - `delete_emoji(id)` (`DELETE /custom/emoji/{id}`);
  - `GlobalLimits.server_emoji`, `FileSizeLimits.emojis`, `DEFAULT_EMOJI_SIZE_LIMIT` (500 KB, exported from `api/mod.rs`) and `RootConfig::emoji_size_limit()`.
- `src/ui/chat.rs`:
  - `EmojiCreate`, `EmojiDelete` and the new `EmojiUpdate` now redraw (they returned early before), so an open Emojis page follows them;
  - `emoji_deleted(id)`;
  - `emoji_limit()` (from root config's `server_emoji`, else 100).
- `src/server_settings.rs`: `Access.emojis` (Manage Customisation), included in `any()`.
- `src/ui/emoji_page.rs` (new): `EmojiPage`. A count line ("{n} of {max} emoji") with the `FaceSlightlySmiling` icon, an empty state, and a row per emoji: the image (32 px, contained), `:name:`, "Uploaded by {name}", and a Trash button ("Delete emoji"). Delete confirms with an alert dialog ("Delete :name:?") whose buttons wait for the answer; success drops the emoji at once and closes the dialog, and a refusal toasts.
- `src/ui/server_settings.rs`: `emojis_page` (icon `FaceSlightlySmiling`) in `PAGES: [_; 6]` after Roles, and the view holds the page.
- `src/assets.rs`: the icons test also checks `FaceSlightlySmiling`.
- `src/lib.rs`, `src/ui/mod.rs`: the modules, by hand.
- Tests:
  - `tests/custom_emoji.rs` (new, 3 tests): rows (order, URLs, the animated flag, known and unknown creators, another server's left out); who may manage them, and `Access.emojis`; the texts;
  - `tests/api_emoji.rs` (new, 2 tests): the `DELETE` with the token; a refusal's text;
  - `tests/store_emoji.rs`: 2 new tests. `EmojiUpdate` decodes (with and without a name) and renames; `server_emojis` by name, per server;
  - `tests/server_settings.rs`: the owner's `Access` includes `emojis`.

### Divergences
- **The event decode test is in `tests/store_emoji.rs`** (through `parse_frame`), not `src/api/events.rs`'s own tests.
- **The page's icon is `FaceSlightlySmiling`** (already bundled), as Assumption 1 allowed.
- **Emoji events now redraw the chat view.** They're rare, and the page needs it.

Look-ahead: chunk 2 adds the upload dialog and rename dialog to `EmojiPage`, `ImageLimits.emoji`, and client calls. No amendments.

### Verification Performed
- `cargo test --test custom_emoji --test api_emoji --test store_emoji --test server_settings`: 3, 2, 5 and 33 passed.
- Full suite: `cargo test` gave 1,380 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3 and `src/api/events.rs` 3 are unchanged. `src/ui/server_settings.rs` was formatted whole (it was clean), and the new files are clean.

### Manual Validation
Deferred to the end (batched): the page lists a server's emoji as the web client does; deleting removes it here and from the picker; a rename in the web client shows at once.

## Chunk 2: Upload and rename emoji

### Changes Made
- `src/custom_emoji.rs`:
  - `EMOJI_NAME_MAX` (32) and `EMOJI_TAG` (`"emojis"`);
  - `check_emoji_name` (trimmed, 1–32 of `a–z0–9_`; "An emoji needs a name.", "Use lowercase letters, numbers and underscores only.", "Emoji names are at most 32 characters.");
  - `name_from_file` (the stem lowercased, anything else `_`, runs squeezed, none at the ends, cut to 32);
  - `at_limit(count, max)`.
- `src/api/client.rs`: `create_emoji(id, server, name) -> Emoji` (`PUT /custom/emoji/{id}` with `DataCreateEmoji`, `nsfw: false`) and `rename_emoji(id, name) -> Emoji` (`PATCH` with `DataEditEmoji`).
- `src/ui/chat.rs`: `emoji_saved(Emoji)` (applies the answer at once) and `emoji_size_limit()` (root config's `emojis` limit, else 500 KB).
- `src/ui/emoji_page.rs`:
  - "Upload emoji…" in the count line (disabled at the limit, with the reason as its tooltip). It runs its own file prompt (`pick`), so the dialog can open with the window;
  - `UploadEmojiDialog`: the picked image (64 px, contained) or why it can't be used (type or size, through `check_image`), a name field prefilled from the file name, with the count and the name problem, then Cancel and Create ("Uploading…"). Create uploads to `emojis`, sends the `PUT`, applies the answer, closes, and toasts "Added :name:.". A refusal shows in the dialog;
  - `RenameEmojiDialog`: the name prefilled, Cancel and Rename. An unchanged name just closes;
  - each row gains a Pencil "Rename emoji" button.
- `src/ui/server_settings.rs`: `open_focused_dialog` is `pub(crate)`.
- Tests:
  - `tests/custom_emoji.rs`: 3 new tests (6 in all). The name check (valid, the 32 edge, blank, uppercase, spaces, dashes, accents, too long); names from files (punctuation, runs, ends, accents, nothing left, cut at 32 with no trailing `_`, and suggestions pass the check); the limit;
  - `tests/api_emoji.rs`: 3 new wiremock tests (5 in all). The `PUT` body is exactly `{"name","parent":{"type":"Server","id"},"nsfw":false}` and the `Emoji` parses; the `PATCH` sends only `{"name"}`; `TooManyEmoji` and `FailedValidation` read as their texts.

### Divergences
- **No `ImageLimits.emoji`:** the emoji size limit is `ChatView::emoji_size_limit()`, read from root config next to the server emoji limit, since only this dialog uses it.
- **The page has its own file prompt** rather than `prompt_for_image`, whose callback has no window to open the dialog with.
- **Upload and create are one task:** a failed `PUT` after a good upload leaves the file orphaned on the file server, as the plan accepted. The dialog shows the refusal.

Look-ahead: chunks 3–4 (webhooks) are independent. No amendments.

### Verification Performed
- `cargo test --test custom_emoji --test api_emoji`: 6 and 5 passed.
- Full suite: `cargo test` gave 1,386 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` 17 and `src/api/client.rs` 3 are unchanged. `src/ui/server_settings.rs` and the new files are clean.

### Manual Validation
Deferred to the end (batched): upload a PNG and a GIF (named from their files); both show in the list and the picker, and the GIF animates. Rename one; the web client follows. At the limit, the button is disabled.
