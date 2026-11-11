# Plan: Emoji and webhooks (RuSToat milestone 28)

## Summary
Milestone 28 of the [roadmap](roadmap.md) adds two management pages:
- **Server emoji:** an Emojis page in Server Settings ([`server-settings.md`](server-settings.md)) to upload, rename and delete a server's custom emoji.
- **Webhooks:** a Webhooks page in Channel Settings ([`channel-settings-and-order.md`](channel-settings-and-order.md)), for server channels and groups, to create, rename (and change the avatar of), copy the URL of, and delete a channel's webhooks.

## Approach
**What Stoat allows.** Read from `/tmp/stoatchat-0.16.2` (tag `v0.16.2`) on 2026-11-11. Paths are under `crates/`.
- **Emoji** (`delta/src/routes/customisation/`, mounted at `/custom`):
  - **Create:** first upload the image to the file server's **`emojis`** tag. Then `PUT /custom/emoji/{file_id}` with `DataCreateEmoji { name, parent: { type: "Server", id }, nsfw: false }`, and the file's ID becomes the emoji's ID.
    - The name is 1–32 characters matching `^[a-z0-9_]+$` (`RE_EMOJI`), else `FailedValidation`.
    - It needs **Manage Customisation** across the server, and at most `limits.global.server_emoji` (100) per server (`TooManyEmoji { max }`).
    - `animated` is set when the file is a GIF. `Revolt.toml` caps `emojis` uploads at 500 KB.
    - The answer is the `Emoji`, and `EmojiCreate` follows.
  - **Rename:** `PATCH /custom/emoji/{id}` with `DataEditEmoji { name }`. It needs Manage Customisation (the creator isn't exempt). The answer is the `Emoji`, and **`EmojiUpdate { id, data: { name } }`** follows.
  - **Delete:** `DELETE /custom/emoji/{id}`. It needs Manage Customisation, **unless the user created that emoji**. `EmojiDelete { id }` follows.
- **Webhooks** (routes mounted only when the instance sets `features.webhooks_enabled`):
  - The official instance has them: on 2026-11-11, an unauthenticated `GET https://api.stoat.chat/webhooks/x` was 401, where an unmounted route is 404. Root config doesn't say, so a self-hosted instance without them shows up as a 404 on the list.
  - **List:** `GET /channels/{id}/webhooks` gives `Vec<Webhook>`, **with tokens** (`channels/webhook_fetch_all.rs`). It needs Manage Webhooks in the channel.
  - **Create:** `POST /channels/{id}/webhooks` with `CreateWebhookBody { name (1–32), avatar: Option<file id> }` (`channels/webhook_create.rs`).
    - It works for a server channel or a group (else `InvalidOperation`), and needs Manage Webhooks there.
    - The answer is the `Webhook` with its token. Its permissions are `DEFAULT_WEBHOOK_PERMISSIONS`.
  - **Edit:** `PATCH /webhooks/{id}` with `DataEditWebhook { name, avatar, permissions, remove: [Avatar] }` (`webhooks/webhook_edit.rs`). It needs Manage Webhooks, and the answer is the `Webhook`. Avatars are uploaded to the **`avatars`** tag (`File::use_webhook_avatar`).
  - **Delete:** `DELETE /webhooks/{id}` (`webhooks/webhook_delete.rs`). It needs Manage Webhooks.
    - **In a group it deletes the webhook, then panics** (`channel.server().expect("Webhook created on non server channel")` for the audit log), so the client gets a 500 for a delete that worked.
  - **Events** (`core/database/src/models/channel_webhooks/model.rs`), sent to the channel's subscribers:
    - `WebhookCreate(Webhook)`, **with the token removed**;
    - `WebhookUpdate { id, channel_id, data: PartialWebhook, remove }`;
    - `WebhookDelete { id, channel_id }`.
  - **The URL** to post to is `{api}/webhooks/{id}/{token}`, as the web client copies it.

**What the web client does** (`/tmp/for-web-0.16.3`, `packages/client/components/app/interface/settings/`):
- **`server/emojis/EmojiList.tsx`** is an "Emojis" page under "Customisation" in Server Settings:
  - an upload form: Emoji (image, cropped square, `emojis` size limit) and "Emoji Name" ("Lowercase letters, numbers and underscores only."), then Create, with "N emoji slots remaining";
  - then a list (newest first), each with the image, `:name:`, the creator's avatar and name, and Edit / Delete (each in a modal).
- **`channel/webhooks/WebhooksList.tsx`:** "Create Webhook" (a modal asking for a name), then each webhook (avatar, name, ID) opening its page.
- **`channel/webhooks/ViewWebhook.tsx`:** "Webhook Icon" (uploaded to `avatars`), "Webhook Name", Reset and Save, "Copy webhook URL", and "Delete webhook" (no confirmation).

**What exists in RuSToat** (as of 2026-11-11, after milestone 27):
- **Emoji:**
  - `ChatStore` keeps the user's servers' emoji (`add_emoji`, `remove_emoji`, `emoji`, private `server_emoji(channel)` sorted by name, `custom_emoji_for`);
  - `MediaUrls::emoji(id)` (`file_by_id("emojis", …)`);
  - `api/events.rs` decodes `EmojiCreate` and `EmojiDelete`, but **not `EmojiUpdate`** (it's `Unknown`), so renames elsewhere aren't followed.
- **Webhooks:** messages from webhooks already render (`message.webhook` name and avatar). `WebhookCreate`, `WebhookUpdate` and `WebhookDelete` decode as `Unknown`.
- **Server Settings** (`src/server_settings.rs` `Access { overview, roles, invites, bans, ownership }`; `src/ui/server_settings.rs` `PAGES: [_; 5]`):
  - the Invites and Bans pages fetch on first show (`load_once`, `Load::{Idle, Loading, Loaded, Failed}`);
  - `confirm_member_action_then` and alert-dialog confirmations;
  - image slots (`ImageSlot`, `prompt_for_image`, `render_image_row`, `upload_image(client, file_server, tag, path)`).
- **Channel Settings** (`src/channel_settings.rs` `ChannelAccess { overview, permissions }`; `src/ui/channel_settings.rs` `PAGES: [_; 2]`), with milestone 27's list-then-editor page (`src/ui/channel_permissions.rs`: `list_row`, `chevron`, `editor_bar`).
- **Limits:** `RootConfig` (`src/api/client.rs`) reads `limits.global` (invite duration, group size) and per-account `file_upload_size_limits` (attachments, avatars, backgrounds, icons, banners), but **not `server_emoji` or `emojis`**. `ImageLimits` has no emoji limit.
- **Copying:** `cx.write_to_clipboard(ClipboardItem::new_string(…))`, as "Copy message link" does.

**The chosen shape** (user's choices on 2026-11-11):
- **Webhooks in groups too,** like server channels. A group delete that comes back with an error re-fetches the list, and "gone" counts as success (Stoat's panic).
- **Webhooks can be renamed and given an avatar,** as in the web client. A webhook's own permissions aren't edited (the web client doesn't either).
- **Logic and views are split,** as milestones 25–27 did:
  - `src/custom_emoji.rs` (new, no GPUI; `src/emoji.rs` is already Unicode emoji) holds the rows, the name rule, who may do what, and the texts;
  - `src/webhooks.rs` (new, no GPUI) holds the rows, the URL, the forms and the texts;
  - `src/ui/emoji_page.rs` and `src/ui/webhooks_page.rs` (new) hold the views. Rejected: growing `src/ui/server_settings.rs` (3,650 lines) or `src/ui/channel_settings.rs`.
- **Webhooks live in the store, per channel, once fetched** (`ChatStore::webhooks(channel)`), so the events keep an open page current. Emoji are already there.

## Assumptions
1. **The Emojis page** shows with **Manage Customisation** across the server (`Access.emojis`), under its own sidebar entry ("Emojis", icon `Smile` if bundled, else `Sticker`).
   - Creators without it can't reach their own emoji there. Deleting your own emoji without the permission could come later from the emoji's preview, but isn't part of this milestone.
2. **The emoji list** is in name order (as the picker shows them), each row with:
   - the image (32 px, contained, not cropped);
   - `:name:`;
   - the creator's name, from the store, or "Unknown user";
   - and buttons Rename… and Delete… (icon buttons with tooltips).

   A heading line says "{n} of {max} emoji", with `max` from root config, else 100.
3. **Uploading:**
   - an "Upload emoji…" button picks an image (PNG, JPEG, GIF or WebP, up to the `emojis` limit, else 500 KB), then opens a dialog with a preview and a name field;
   - the name is **prefilled from the file name**: lowercased, other characters replaced with `_`, runs of `_` squeezed, trimmed to 32;
   - Create uploads, then sends the `PUT`, and the emoji shows at once (`add_emoji` from the answer);
   - **no crop editor**, as milestone 15 decided: a non-square image is stored as it is and shown contained;
   - at the limit, the button is disabled with "This server has {max} emoji, the most Stoat allows."
4. **Emoji names:** `check_emoji_name(text)` trims and requires 1–32 characters of `a–z`, `0–9` and `_`. Problems read "An emoji needs a name." and "Use lowercase letters, numbers and underscores only." Uppercase isn't lowercased silently, so the user sees what they'll get.
5. **Rename** is a small dialog (name field with the same check, Cancel and Rename). The answer is applied at once (`ChatStore::rename_emoji`), and `EmojiUpdate` does the same for changes made elsewhere.
6. **Delete** asks "Delete :{name}:?", "Messages and reactions that use it won't show it any more.", Cancel and Delete (danger). The emoji is removed at once on success.
7. **The Webhooks page** shows with **Manage Webhooks** in the channel or group (`ChannelAccess.webhooks`).
   - It fetches the list the first time it's shown, and shows "Loading…", or on failure the reason and Try again.
   - A 404 reads "This Stoat instance doesn't have webhooks." (and Create is hidden).
   - The page is "Create webhook…" then a row per webhook (avatar, name, muted ID, chevron), sorted by name. Each row opens its editor on the same page, with "‹ Webhooks".
8. **Create webhook** is a dialog with a name (1–32) only, as in the web client. On success it opens the new webhook's editor.
9. **The webhook editor** (chunk 4 makes it editable):
   - avatar (`avatars` tag, the avatar size limit), name, and Reset / Save in the top bar;
   - then "Webhook URL", shown as `{api}/webhooks/{id}/` followed by `••••••`, with **Copy URL**, which writes the full URL and toasts "Copied.";
   - then **Delete webhook…**, which confirms with "Delete {name}?" and "Anything posting to its URL will stop working."
   - If the token isn't known (the webhook arrived by event, which strips it), Copy re-fetches the list first.
10. **Live events:** `WebhookCreate`, `WebhookUpdate` and `WebhookDelete` change a channel's list only if it has been fetched. A created webhook without a token keeps a token already known for it.
11. **Errors:**
    - **emoji:** `TooManyEmoji { max }` "This server has {max} emoji, the most Stoat allows."; `FailedValidation` "Emoji names are 1–32 lowercase letters, numbers and underscores."; `MissingPermission` "You don't have permission to manage this server's emoji."; `NotFound` "That emoji was deleted."; else `error_text`;
    - **webhooks:** `MissingPermission` "You don't have permission to manage webhooks here."; `FailedValidation` "Webhook names are 1–32 characters."; `NotFound` "That webhook was deleted."; `InvalidOperation` "Webhooks can't be made here."; else `error_text`.
12. **Messages already posted by a deleted webhook keep its name and avatar,** as Stoat stores them on the message.

## Plan

### Chunk 1: The Emojis page: list and delete
**Goal:** someone with Manage Customisation sees a server's custom emoji in Server Settings, and can delete one; renames made elsewhere show at once.
**Files likely touched:**
- `src/custom_emoji.rs` (new, no GPUI):
  - `EmojiRow { id, name, url, animated, creator, creator_name }` and `emoji_rows(store, server)` (name order);
  - `can_manage_emoji(store, server)`;
  - `emoji_limit(root)`;
  - `delete_prompt(name)` and `emoji_error_text`.
- `src/store.rs`: `server_emojis(server) -> Vec<&Emoji>` (public, by server ID) and `rename_emoji(id, name)`.
- `src/api/events.rs`: decode `EmojiUpdate { id, data: { name } }` to `ServerEvent::EmojiUpdate { id, name: Option<String> }`.
- `src/ui/chat.rs`: handle it (`rename_emoji`), and `ChatView::emoji_deleted(id)` (applies `remove_emoji` at once).
- `src/api/client.rs`: `delete_emoji(id)`; `RootConfig`: `limits.global.server_emoji` and `file_upload_size_limits.emojis` (with `emoji_size_limit()`, default 500 KB).
- `src/server_settings.rs`: `Access.emojis` (Manage Customisation, a known membership or ownership), and `any()`.
- `src/ui/emoji_page.rs` (new): `EmojiPage` (list, count line, Delete… with confirmation, toasts).
- `src/ui/server_settings.rs`: `emojis_page` in `PAGES` (after Roles).
- `src/assets.rs`: the page and button icons in the icons test.
- `src/lib.rs`, `src/ui/mod.rs`: the modules, by hand.
- `tests/custom_emoji.rs` (new), `tests/server_settings.rs`, `tests/api_emoji.rs` (new), `tests/store_emoji.rs`, the event decode tests in `src/api/events.rs`.

**Depends on:** None
**Risks:**
- **`server_emoji(channel)` is keyed by channel** and treats "no server" as "every server's". The new by-server function must not change the picker's behaviour, so leave the old one alone and add the new one beside it.
- **The creator may be unknown** (not cached). Show "Unknown user" rather than fetching users here.

**How to verify:**
- `cargo test --test custom_emoji`: `emoji_rows` (order, URLs, creator names known and unknown, other servers' emoji left out); `can_manage_emoji` for the owner, a holder and a plain member; the prompt; the error texts.
- `cargo test --test server_settings`: `Access.emojis`.
- `cargo test --test api_emoji`: `DELETE /custom/emoji/{id}` with the token; a refusal's text.
- `cargo test --test store_emoji`: `rename_emoji`; an `EmojiUpdate` frame decodes and renames.
- Then the full suite.

**Manual check:** the Emojis page lists the server's emoji like the web client. Deleting one removes it here and from the picker. A rename in the web client shows at once.

**Shippable when:** Server Settings lists a server's emoji and deletes them, for anyone with Manage Customisation.

### Chunk 2: Upload and rename emoji
**Goal:** add a server emoji from an image file, and rename one.
**Files likely touched:**
- `src/custom_emoji.rs`: `check_emoji_name`, `name_from_file(path)`, `ImageLimits`-style `emoji_limit_bytes(root)` and the create and rename request bodies.
- `src/api/client.rs`: `create_emoji(file_id, server, name) -> Emoji` (`PUT /custom/emoji/{id}`) and `rename_emoji(id, name) -> Emoji` (`PATCH`).
- `src/profile_edit.rs`: `ImageLimits.emoji`.
- `src/ui/emoji_page.rs`:
  - "Upload emoji…" → `prompt_for_image` → `UploadEmojiDialog` (preview, name, Create, uploading state);
  - `RenameEmojiDialog`;
  - both apply answers at once (`add_emoji` / `rename_emoji` through `ChatView`).
- `tests/custom_emoji.rs`, `tests/api_emoji.rs`.

**Depends on:** Chunk 1
**Risks:**
- **The upload's ID must be used as the path.** If the `PUT` fails after the upload, the file is orphaned on the file server, which is harmless and what the web client does too. Say so in the error toast only if it helps; don't retry.
- **GIFs and large images:** the file server rejects files over its `emojis` limit. Check the size before uploading, as profile images do (`check_image`).

**How to verify:**
- `cargo test --test custom_emoji`: `check_emoji_name` (valid, blank, too long, uppercase, spaces, punctuation, trimming); `name_from_file` ("My Cat!.png" → `my_cat`, an empty stem, 40 characters → 32).
- `cargo test --test api_emoji`: `PUT /custom/emoji/{id}` sends exactly `{"name","parent":{"type":"Server","id"},"nsfw":false}` and parses the `Emoji`; `PATCH` sends `{"name"}`; `TooManyEmoji` and `FailedValidation` read as their texts.
- Then the full suite.

**Manual check:** upload a PNG and a GIF; both show in the list and the picker, and the GIF animates where emoji do. Rename one, and the web client follows. At 100 emoji (or a test instance's lower limit) the button is disabled.

**Shippable when:** emoji can be uploaded and renamed from Server Settings, and show at once.

### Chunk 3: The Webhooks page: list, create, copy the URL, delete
**Goal:** someone with Manage Webhooks sees a channel's (or group's) webhooks, creates one, copies its URL and deletes it.
**Files likely touched:**
- `src/webhooks.rs` (new, no GPUI):
  - `WebhookRow { id, name, avatar_url, token_known }` and `webhook_rows(store, channel)` (by name);
  - `can_manage_webhooks(store, channel)` (Manage Webhooks in `channel_permissions`, a server channel or group);
  - `webhook_url(api, id, token)` and `shown_url(api, id)` (token hidden);
  - `check_webhook_name`, `delete_prompt` and `webhook_error_text` (a 404 on the list reads as Assumption 7's text).
- `src/store.rs`: `webhooks: HashMap<String, Vec<Webhook>>`, with `webhooks(channel)`, `set_webhooks`, `add_webhook`, `apply_webhook_update`, `remove_webhook` (Assumption 10).
- `src/api/events.rs`: decode `WebhookCreate`, `WebhookUpdate` and `WebhookDelete`.
- `src/ui/chat.rs`: handle them; and `webhook_saved` / `webhook_deleted` for the user's own changes.
- `src/api/client.rs`: `fetch_webhooks(channel)`, `create_webhook(channel, name)`, `delete_webhook(id)`.
- `src/channel_settings.rs`: `ChannelAccess.webhooks`, and `any()`.
- `src/ui/webhooks_page.rs` (new): `WebhooksPage` (`Load` state, list, Create dialog, and a read-only `WebhookView` with the URL, Copy URL and Delete…). A group's failed delete re-fetches, and success is "it's gone".
- `src/ui/channel_settings.rs`: `webhooks_page` in `PAGES` (after Permissions).
- `tests/webhooks.rs` (new), `tests/api_webhooks.rs` (new), `tests/channel_settings.rs`, the event tests.

**Depends on:** None (it doesn't touch chunks 1–2's files beyond `client.rs`, `store.rs` and `events.rs`)
**Risks:**
- **Tokens:** only the list and create answers carry them. Never put a token in a log line (log the webhook ID), and don't show it on screen.
- **The group delete panic:** the 500 must not read as failure when the webhook is gone. Re-fetch, then decide. If the re-fetch fails too, say "Couldn't tell whether the webhook was deleted." and keep it in the list.
- **Event decoding:** `WebhookUpdate.data` is a `PartialWebhook` (name, avatar, permissions, but never the token). Unknown fields must not fail the decode.

**How to verify:**
- `cargo test --test webhooks`: rows (order, avatar URL, token known or not); `can_manage_webhooks` (owner, holder, override-only, plain member, group owner, a group member with default permissions, which lack it); the URLs; the name check; the texts, including the 404.
- `cargo test --test api_webhooks`: `GET /channels/{id}/webhooks` parses tokens; `POST` sends exactly `{"name"}`; `DELETE /webhooks/{id}`; refusals read as their texts.
- Store and events: set then create, update and delete events; a token survives an event without one; events for an unfetched channel are ignored.
- Then the full suite.

**Manual check:** create a webhook in a channel and a group, copy the URL, and post with `curl -X POST <url> -H 'Content-Type: application/json' -d '{"content":"hi"}'`: the message shows with the webhook's name. Delete it in the channel and in the group: both leave the list, with no error toast.

**Shippable when:** a channel's or group's webhooks can be listed, created, copied and deleted from Channel Settings.

### Chunk 4: Edit a webhook's name and avatar
**Goal:** rename a webhook and change or remove its avatar.
**Files likely touched:**
- `src/webhooks.rs`: `WebhookForm { name, avatar: ProfileImage }` with `of`, `is_valid`, `new_file`, `changes -> Option<DataEditWebhook>` (`remove: [Avatar]` for a removed avatar), and `empty_webhook_edit()`.
- `src/api/client.rs`: `edit_webhook(id, &DataEditWebhook) -> Webhook` (through `edit_body`).
- `src/ui/webhooks_page.rs`: `WebhookView` becomes an editor (avatar row, name field, Reset and Save in the top bar, upload-then-`PATCH`, live-follow through `filled`), applying the answer through `webhook_saved`.
- `tests/webhooks.rs`, `tests/api_webhooks.rs`.

**Depends on:** Chunk 3
**Risks:**
- **The `PATCH` answer may lack the token** (Stoat's `Webhook` answer includes it here, but check). Keep the known token when applying it.

**How to verify:**
- `cargo test --test webhooks`: `WebhookForm::changes` (a trimmed name, an avatar removed → `remove`, replaced → the new ID, unchanged → `None`, invalid → `None`).
- `cargo test --test api_webhooks`: the `PATCH` body has only what's set, and the answer parses.
- Then the full suite.

**Manual check:** rename a webhook and give it an avatar, then post with `curl`: the new name and avatar show. Remove the avatar.

**Shippable when:** a webhook's name and avatar can be changed from its page, and show at once.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–27):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3. Check any other file's count with `git show HEAD:<file> | rustfmt --edition 2024 --check | grep -c '^Diff in'` before formatting it whole.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/custom_emoji.rs` (new): rows, access, the name check, names from files, prompts and error texts.
- `tests/webhooks.rs` (new): rows, access (channels and groups), URLs, the name check, the form's changes, prompts and error texts.
- Event decoding (`src/api/events.rs` tests): `EmojiUpdate`, `WebhookCreate`, `WebhookUpdate`, `WebhookDelete`.
- Store: emoji rename; webhooks set, events, token kept.

### Integration Tests
wiremock tests (`tests/api_emoji.rs`, `tests/api_webhooks.rs`, new) against the exact routes, headers and bodies:
- `PUT`, `PATCH` and `DELETE /custom/emoji/{id}`;
- `GET` and `POST /channels/{id}/webhooks`;
- `PATCH` and `DELETE /webhooks/{id}`.

Failure cases that become plain toasts: `TooManyEmoji`, `FailedValidation`, `MissingPermission`, `NotFound`, `InvalidOperation`, and a 404 list ("no webhooks here").

### Manual Tests
Run with two accounts and the web client, on a test server owned by the first account, plus a group. Batch them at the end if the user asks:
1. **Emoji list:** matches the web client; the count line is right.
2. **Upload:** a PNG and a GIF, named from their files; both show in the picker and in messages at once.
3. **Rename and delete:** both show at once, here and in the web client; a rename there shows here.
4. **Access:** without Manage Customisation, there's no Emojis page.
5. **Webhooks:** create one in a channel; copy its URL; post to it with `curl`; the message shows with its name. Rename it and give it an avatar; post again. Delete it.
6. **Group webhooks:** the same in a group; deleting shows no error.
7. **Live:** a webhook created or deleted in the web client while the page is open shows or goes, and Copy URL still works for it (re-fetching the token).
8. **Themes:** both pages readable in light and dark.

## Definition of Done
- [ ] Server Settings has an Emojis page for Manage Customisation, listing the server's emoji with their creators, and a count against the limit.
- [ ] Emoji upload to `emojis` then `PUT /custom/emoji/{id}`, rename with `PATCH`, and delete with `DELETE`, each showing at once. `EmojiUpdate` from elsewhere renames.
- [ ] Emoji names are checked as Stoat does (1–32 of `a–z0–9_`), prefilled from the file name.
- [ ] Channel Settings has a Webhooks page for Manage Webhooks in a server channel or group: list, create (name), copy URL (token never shown or logged), and delete (with the group-panic workaround).
- [ ] A webhook's name and avatar can be edited; the answer and webhook events keep the list current, and a known token isn't lost.
- [ ] Refusals read plainly, including a 404 for instances without webhooks.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan.
