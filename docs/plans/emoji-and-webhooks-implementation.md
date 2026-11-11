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
