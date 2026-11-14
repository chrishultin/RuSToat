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

## Chunk 3: The Webhooks page: list, create, copy the URL, delete

### Changes Made
- `src/webhooks.rs` (new, no GPUI):
  - `WEBHOOK_NAME_MAX` (32) and `WEBHOOK_AVATAR_TAG` (`"avatars"`);
  - `WebhookRow { id, name, avatar_url, token_known }` and `webhook_rows(store, channel)` (`None` until fetched; by name), plus `avatar_url`;
  - `can_manage_webhooks` (Manage Webhooks in `channel_permissions`, for a server channel or group);
  - `webhook_url(api, id, token)` and `shown_url(api, id)` (the token as `••••••`);
  - `check_webhook_name` and `delete_prompt`;
  - `webhooks_unavailable(err)`: an HTML 404, which arrives as `ApiError::Unexpected { status: 404 }`, as opposed to Stoat's own JSON `NotFound`;
  - `list_error_text` and `webhook_error_text`.
- `src/store.rs`: `webhooks: HashMap<channel, Vec<Webhook>>`, with `webhooks`, `set_webhooks`, `add_webhook` (only into a fetched list; keeps a known token when the new copy has none), `apply_webhook_update` (name, avatar, permissions, `remove: [Avatar]`; never the token) and `remove_webhook`. `apply_ready` clears them, so a page fetches again after a reconnect.
- `src/api/events.rs`: `WebhookCreate(Box<Webhook>)`, `WebhookUpdate { id, channel, data, remove }` and `WebhookDelete { id, channel }`. The old test that `WebhookCreate` stays unknown now uses a made-up event type.
- `src/ui/chat.rs`: handles the three events (and redraws), and adds `webhooks_fetched`, `webhook_saved` and `webhook_deleted`.
- `src/api/client.rs`: `fetch_webhooks(channel)`, `create_webhook(channel, name)` (a body of only `{"name"}`) and `delete_webhook(id)`.
- `src/channel_settings.rs`: `ChannelAccess.webhooks`, included in `any()`.
- `src/ui/webhooks_page.rs` (new):
  - `WebhooksPage`: fetches when the store has no list for the channel (first shown, or after a reconnect). It shows "Loading…", the reason with Try again, or "This Stoat instance doesn't have webhooks." (and no Create);
  - a line explaining webhooks, "Create webhook…" (`Plus`), and the rows: avatar, name, muted ID and chevron;
  - `CreateWebhookDialog` (a name, then Create, which adds it with its token and opens it);
  - `WebhookView`: "‹ Webhooks", the avatar, name and ID, then "Webhook URL" (shown with the token hidden, monospace) with Copy URL, then "Delete webhook…" under a border.
    - Copy writes the full URL to the clipboard and toasts "Copied.". Without a known token it fetches the list first, and toasts "Couldn't get the webhook's URL." if that fails.
    - Delete confirms; in a group a failed `DELETE` is followed by a fetch, and the webhook being gone counts as success (Stoat's panic). It goes back to the list when the webhook is deleted, here or elsewhere.
- `src/ui/channel_settings.rs`: `webhooks_page` (icon `Webhook`) in `PAGES: [_; 3]`. The file was formatted whole (it was clean).
- `src/assets.rs`: `Webhook` added to the extra Lucide icons; the icons test checks it and `Copy`.
- `src/lib.rs`, `src/ui/mod.rs`: the modules, by hand.
- Tests:
  - `tests/webhooks.rs` (new, 7 tests): who may manage them (owner, holder, override-only, plain, group owner and a default group member), and `ChannelAccess.webhooks`; rows `None` until fetched, then by name with avatar URLs and whether the token is known; create and update events keep the list and its tokens, and a delete removes; events for an unfetched channel are ignored; a reconnect forgets the lists; the URLs (the token never in the shown one); names, the prompt, and every text, with an HTML 404 telling "no webhooks" apart from Stoat's `NotFound`;
  - `tests/api_webhooks.rs` (new, 5 wiremock tests): the list parses tokens; an HTML 404 reads as "no webhooks"; `POST` sends exactly `{"name":"CI"}` and the answer carries the token; `DELETE /webhooks/{id}`; `InvalidOperation` reads plainly;
  - `tests/channel_settings.rs`, `tests/channel_permissions.rs`: the `ChannelAccess` literals gain `webhooks`.

### Divergences
- **Fetching is done by the page's render** (`load_once` from the list view), rather than the window's, since the page knows whether the store has the list.
- **The group-delete check happens inside the request task** (delete, then fetch on failure), so the view sees one result.
- **Token safety:** tokens are only in the store and the clipboard. Logs name webhooks by ID, and the shown URL hides the token.

Look-ahead: chunk 4 turns `WebhookView`'s heading into an editor (avatar row, name field, Reset and Save in the bar). No amendments.

### Verification Performed
- `cargo test --test webhooks --test api_webhooks --test channel_settings --test channel_permissions`: 7, 5, 22 and 21 passed.
- Full suite: `cargo test` gave 1,398 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` 17 and `src/api/events.rs` 3 (one new hunk each, hand-formatted), `src/store.rs` 14 and `src/api/client.rs` 3 are unchanged. The new files and the other touched files are clean.

### Manual Validation
Deferred to the end (batched): create a webhook in a channel and in a group, copy the URL, post with `curl`, then delete both (no error in the group).

## Chunk 4: Edit a webhook's name and avatar

### Changes Made
- `src/webhooks.rs`: `WebhookForm { name, avatar: ProfileImage }` with `of`, `is_valid`, `new_file`, `has_changes` and `changes` (a trimmed new name; a new avatar's file ID, or `remove: [Avatar]`; a picked file waits for its upload; `None` when unchanged or invalid), and `empty_webhook_edit()`.
- `src/api/client.rs`: `edit_webhook(id, &DataEditWebhook) -> Webhook`, a `PATCH /webhooks/{id}` through `edit_body`.
- `src/ui/webhooks_page.rs`, `WebhookView`:
  - the bar has Reset and Save beside "‹ Webhooks";
  - under the muted ID come an Avatar row (`render_image_row` with `avatar_preview`, the avatar size limit, uploaded to `avatars`) and a Name field (count, "A webhook needs a name."). Enter saves;
  - `filled` with per-field live-follow, `reset`, `pick_avatar`, and `save` (upload, then a `PATCH` built against the webhook as stored then; the answer goes through `webhook_saved`, which keeps the known token; "Saved."), with `save_failed`;
  - the URL and Delete sections are as in chunk 3.
- Tests:
  - `tests/webhooks.rs`: 2 new tests (9 in all). The editor sends only what changed (name, avatar removed, replaced, picked, blank name); a saved webhook keeps its token;
  - `tests/api_webhooks.rs`: 2 new wiremock tests (7 in all). A rename with a removed avatar sends exactly `{"name","remove":["Avatar"]}`; a new avatar is sent by file ID and the answer's avatar parses.

### Divergences
- **The heading with the avatar and name is replaced by the editor** (the avatar row and the name field), with the webhook's ID kept above them as muted text.

### Verification Performed
- `cargo test --test webhooks --test api_webhooks`: 9 and 7 passed.
- Full suite: `cargo test` gave 1,402 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/api/client.rs` is still 3. The touched new files are clean.

### Manual Validation
Deferred to the end (batched): rename a webhook and give it an avatar, post with `curl`, and see the new name and avatar. Remove the avatar.

## Summary

_Completed: 2026-11-11_

### Chunks Completed
1. The Emojis page: list and delete (`8516372`)
2. Upload and rename emoji (`d51357f`)
3. The Webhooks page: list, create, copy the URL, delete (`739e608`)
4. Edit a webhook's name and avatar (this commit)

### Divergences
- Chunk 1: the event decode test is in `tests/store_emoji.rs`. The page icon is `FaceSlightlySmiling`. Emoji events now redraw the chat view.
- Chunk 2: the emoji size limit is on `ChatView`, not `ImageLimits`. The page has its own file prompt (to open the dialog with a window). Upload and create are one task.
- Chunk 3: the page fetches from its own render. The group-delete check is in the request task. Tokens are kept off screen and out of logs.
- Chunk 4: the webhook's heading became the editor.
- Before starting, the live API had moved to Stoat 0.16.4 (`1f568f4`); no emoji or webhook route changed.

### Definition of Done
- [x] Server Settings has an Emojis page for Manage Customisation, listing the server's emoji with their creators, and a count against the limit.
- [x] Emoji upload to `emojis` then `PUT /custom/emoji/{id}`, rename with `PATCH`, and delete with `DELETE`, each showing at once. `EmojiUpdate` from elsewhere renames.
- [x] Emoji names are checked as Stoat does (1–32 of `a–z0–9_`), prefilled from the file name.
- [x] Channel Settings has a Webhooks page for Manage Webhooks in a server channel or group: list, create (name), copy URL (token never shown or logged), and delete (with the group-panic workaround).
- [x] A webhook's name and avatar can be edited; the answer and webhook events keep the list current, and a known token isn't lost.
- [x] Refusals read plainly, including an HTML 404 for instances without webhooks.
- [x] `cargo test` (1,402) and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [x] The roadmap links this plan (committed with chunk 1).

### Remaining Manual Steps
The plan's Manual Tests 1–8, batched at the end as the user asked: the emoji list; upload (PNG and GIF); rename and delete; access; webhooks in a channel (create, copy, post with `curl`, rename, avatar, delete); group webhooks (delete without an error); live changes (including Copy URL for a webhook made elsewhere); themes.
