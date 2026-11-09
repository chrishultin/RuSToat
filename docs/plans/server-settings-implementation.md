# Implementation Record: Server settings (milestone 23)

_Plan: docs/plans/server-settings.md_
_Started: 2026-11-09_

**Mode:** continuous. Manual checks are batched at the end, as in milestone 22 (the user didn't ask for them per chunk).

**Files dirty at the start:** the plan (`docs/plans/server-settings.md`, untracked) and the roadmap (`docs/plans/roadmap.md`, linking the plan). Both are this milestone's, so they go in with chunk 1.

## Chunk 1: Server Settings window with name and description

### Changes Made
- `src/server_settings.rs` (new, no GPUI; registered in `src/lib.rs` by hand):
  - `Access { overview, invites, bans, ownership }` with `any()`.
  - `access(store, server)`: the owner sees everything. Anyone else needs a known membership: Manage Server for Overview and Invites, Ban Members for Bans.
  - `OverviewForm { name, description }`:
    - `of(&Server)` fills it from the server;
    - `is_valid()`: name 1–32 and description ≤1024, both trimmed;
    - `changes(&Server) -> Option<DataEditServer>`: only what changed, trimmed. A blank description becomes `remove: [Description]`. `None` when nothing changed or the form is invalid.
  - `empty_server_edit()`, `DESCRIPTION_MAX`, and `NAME_MAX` re-exported from `server_edit`.
  - `save_error_text`: texts for `MissingPermission`, `FailedValidation` and `NotFound`, else `error_text`.
- `src/ui/server_settings.rs` (new; registered in `src/ui/mod.rs` by hand):
  - `ServerSettingsWindow` (library global):
    - `open(chat, server, cx)` is deferred. It brings the same server's window forward; otherwise it closes the old window and opens a new one, 760×560 and centred.
    - `close(cx)`.
  - `ServerSettingsView`:
    - pages are `ServerPageFn = fn(&ServerSettingsView, &mut App) -> Option<SettingPage>`, in `PAGES`;
    - `shown: Vec<bool>` records which pages were visible at the last render. The sidebar's keyed ID (`page_key`) is bumped only when that set changes;
    - the window title is "Server Settings — ‹name›", updated on rename;
    - it observes the `ChatView`, and closes the window when the server leaves the store or no page is left;
    - `observe_release_in` closes it when the `ChatView` goes (sign-out);
    - it follows macOS appearance, as `SettingsView` does.
  - `overview_page`, and `OverviewPage`:
    - a name input and a description textarea, with counters through `profile_editor::field`;
    - untouched fields follow live changes (`follow`, compared against `filled`);
    - Reset and Save. Save sends one `edit_server`, then `ChatView::server_edited`, refills the form, and toasts "Saved." A refusal toasts `save_error_text`.
- `src/api/client.rs`: `edit_server(server, &DataEditServer) -> Server` (`PATCH /servers/{id}` through `edit_body`). Imports `DataEditServer` and `Server`.
- `src/store.rs`: `apply_server_edit(&Server) -> bool`, built on `apply_server_update`. It takes name, owner, description, icon, banner and system messages from Stoat's answer, and clears those the answer lacks.
- `src/ui/chat.rs`:
  - "Server settings…" (`Settings` icon) in `server_menu`, after "Invite people…", when `access(..).any()`;
  - `ChatView::open_server_settings(server, cx)` and `ChatView::server_edited(&Server, cx)` (applies the edit, then `server_permissions_changed`, then notify);
  - `open_info_dialog` passes a "Server settings…" callback for servers the user may manage.
- `src/ui/server_dialogs.rs`: `render_info` takes `on_settings: Option<OnSettings>` (a new `Rc` alias), drawn as a ghost button beside Close.
- `src/assets.rs`: `server_settings_icons_load` (Settings, Link, Ban, Crown).
- Tests:
  - `tests/server_settings.rs` (new), 9 tests:
    - `access` for the owner, Manage Server, Ban Members only, a plain member, an unknown membership and an unknown server;
    - `OverviewForm`: unchanged gives `None`, including surrounding whitespace; a rename sends only the name; a description is trimmed; a blank description is removed; the 32 and 1024 limits.
  - `tests/api_servers.rs`: `edit_server` sends `{"name":"Lounge","remove":["Description"]}` once; `MissingPermission` gives "You don't have permission to change this server."
  - `tests/store_servers.rs`: a saved edit shows at once, and the matching `ServerUpdate` then leaves the server identical. An unknown server is ignored.

### Divergences
- **`open_server_settings` takes no `window`.** Opening is deferred to a new window, so the main window isn't needed. The plan's signature had one.
- **The window also closes when no page is left** (e.g. Manage Server taken away), not only when the server goes. Chunk 6's risk asked for this; it's done here because the same observer handles it.
- **`apply_server_edit(&Server)` takes no `removed` list.** The answer is the whole server, so a field it lacks is cleared. That gives the same result as passing the edit's `remove`.
- None of the plan's risks materialised:
  - `gpui_kit::open_window` works from the library;
  - `window.remove_window()` only sets a flag, so it's safe inside the observer;
  - no `cx.defer` was needed for closing.
- No look-ahead changes to later chunks.

### Verification Performed
- `cargo test --test server_settings --test api_servers --test store_servers`: 9 + 17 + 15 passed.
- `cargo test`: 1137 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after adding the `OnSettings` alias for `type_complexity`.
- rustfmt:
  - drift unchanged: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/ui/server_dialogs.rs` 0, `src/assets.rs` 0;
  - new files clean: `src/server_settings.rs`, `src/ui/server_settings.rs`, `tests/server_settings.rs`;
  - `tests/api_servers.rs` and `tests/store_servers.rs` are still clean.

### Manual Validation
Deferred to the end (rename, clear the description, switch servers, and a member without permissions).

## Chunk 2: Icon and banner

### Changes Made
- `src/api/client.rs`:
  - `FileSizeLimits` gains `icons` and `banners`;
  - `DEFAULT_ICON_SIZE_LIMIT` (2.5 MB) and `DEFAULT_BANNER_SIZE_LIMIT` (6 MB);
  - `RootConfig::icon_size_limit()` / `banner_size_limit()` take the stricter of the two account limits, else the default.
- `src/api/mod.rs`: re-exports the two defaults (hand-edited).
- `src/profile_edit.rs`: `ImageLimits` gains `server_icon` and `server_banner`, set by `Default` and `from_root`.
- `src/ui/profile_editor.rs`:
  - `upload_image(client, file_server, tag, path)` and `Preview::of(image, tag, media, fallback)` take the file server's tag instead of an `ImageKind`. Profile callers pass `ImageKind::…tag()`.
  - `banner_preview(id, preview, cx)` is `pub(crate)` and takes its element ID. It was already a 232:100 strip.
- `src/ui/identity_dialog.rs`: its two calls pass `ImageKind::Avatar.tag()`.
- `src/server_settings.rs`:
  - `ServerImage { Icon, Banner }` with `tag()` (`icons` / `banners`) and `limit(ImageLimits)`;
  - `OverviewForm` gains `icon` and `banner: ProfileImage`, filled by `of`;
  - `new_files()` and `has_changes(&Server)`;
  - `changes` sends a new uploaded ID as `icon` / `banner` (no remove), a removed image as `remove: [Icon/Banner]`, and leaves out a picked file until it's uploaded.
- `src/ui/server_settings.rs`, `OverviewPage`:
  - `icon` / `banner: ImageSlot` rows above the name, using milestone 15's `render_image_row`, `avatar_preview` (round) and `banner_preview`;
  - Change… goes through `prompt_for_image` with the server limits;
  - live-follow and Reset cover the images;
  - Save uploads new files first ("Uploading…"), then computes `changes` against the server as it is then, and sends one `PATCH`;
  - a failed upload or `PATCH` toasts and keeps the form (`save_failed`).
- Tests:
  - `tests/server_settings.rs`, 6 new: the images from the server; a new icon sends its ID without a remove; removed images go in `remove`; unchanged images are left out; a picked image waits for its upload; tags and limits.
  - `tests/api_upload.rs`: `root_config_gives_the_server_icon_and_banner_limits` (`icons` 2.5 MB from `root.json`, `banners` defaulting to 6 MB, the stricter account limit, and the defaults with no limits). `a_server_icon_is_uploaded_then_saved_by_its_id` uploads to `/icons`, then sends `PATCH /servers/srv` `{"icon":"f-icon"}`. The existing limits test gains the two new fields.
  - `tests/api_servers.rs`: the chunk 1 edit test builds its form with `..OverviewForm::of(..)`.

### Divergences
- **`ImageKind` wasn't widened.** A parallel `ServerImage` lives in `src/server_settings.rs`, as the plan allowed. Widening would have put meaningless server arms in three of the profile editor's matches. Instead, `upload_image` and `Preview::of` now take a tag. Profile banners still go to `backgrounds`; `avatars_and_banners_go_to_their_own_tags` still passes.
- **The limits tests are in `tests/api_upload.rs`**, where the avatar and banner limits already were. The plan said `tests/api_instance.rs`.
- **`tests/api_upload.rs` started with 10 rustfmt hunks**, not 0 as the plan's formatting note implied for unlisted files. I formatted it whole by mistake, then restored it from HEAD and reapplied only this chunk's changes. Its new code is hand-formatted, and the count stays at 10.
- No look-ahead changes. Chunk 3 adds system messages to the same `OverviewForm` and `changes`.

### Verification Performed
- `cargo test --test server_settings --test api_upload`: 15 + 14 passed.
- `cargo test`: 1145 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt:
  - drift unchanged: `src/api/client.rs` 3, `tests/api_upload.rs` 10;
  - clean files still clean: `src/profile_edit.rs`, `src/ui/profile_editor.rs`, `src/ui/identity_dialog.rs`, `tests/api_servers.rs`;
  - new files clean: `src/server_settings.rs`, `src/ui/server_settings.rs`, `tests/server_settings.rs`.

### Manual Validation
Deferred to the end (PNG and animated GIF icons, a banner, removing both, and a 3 MB icon refused).

## Chunk 3: System-message channels

### Changes Made
- `src/server_settings.rs`:
  - `SystemMessageKind { Joined, Left, Kicked, Banned }` with `ALL` and `label()` ("User joined", "User left", "User kicked", "User banned").
  - `SystemChannels { user_joined, user_left, user_kicked, user_banned: Option<String> }`:
    - `of(&Server)` reads the server's channels, with any channel not in `server.channels` shown as off;
    - `existing(&Server)` drops unknown channels;
    - `get` / `set`, and `From<SystemChannels> for SystemMessageChannels`.
  - `OverviewForm` gains `system`. `changes` compares the edited channels (unknown ones dropped) with the server's. A difference sends the whole object as `system_messages`, or `remove: [SystemMessages]` when every kind is off. It goes in the same `DataEditServer` as the other fields.
  - `system_channel_choices(store, server) -> Vec<(id, name)>`: the server's channels in channel-list order (`server_channels`), voice channels included. A chosen channel the list doesn't show the user is added at the end ("Hidden channel" if its name is unknown).
- `src/ui/server_settings.rs`, `OverviewPage`:
  - a `system: SystemChannels` field, covered by `edited`, Reset and live-follow (per kind);
  - a "System messages" section under the description: four rows, each an outline button "#channel ▾" or "Off ▾" with a dropdown (`DropdownMenu`, scrollable, max 320 px): Off, then every choice, the current one ticked;
  - saved by the same Save as the rest of the form.
- `tests/server_settings.rs`:
  - the `srv` fixture gains three channels (`ch-b`; `ch-a` and voice `ch-voice` in a category) and `system_messages` with a deleted `ch-gone`;
  - 6 new tests: a deleted channel shows as off and isn't sent; setting one sends the whole object without deleted channels (JSON `{"user_joined":"ch-a","user_banned":"ch-voice"}`); all off gives `remove: [SystemMessages]`; a channel picked then deleted isn't sent; choices follow channel order and include voice channels; the labels.

### Divergences
- **Voice channels are offered.** Stoat 0.16.2's `server_edit.rs` only checks that each ID is in `server.channels`, and voice channels there are text channels with chat. As the plan said, I kept them and noted it.
- **The plan's `SystemMessagesForm` / `edit()` is `SystemChannels`**, merged into `OverviewForm::changes`, so one `DataEditServer` carries every change.
- **Pickers are dropdown menus on outline buttons** (the `CreateInviteDialog` pattern), not gpui-component's `Select`. No select state is needed.
- No look-ahead changes.

### Verification Performed
- `cargo test --test server_settings`: 21 passed.
- `cargo test`: 1151 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: the new files (`src/server_settings.rs`, `src/ui/server_settings.rs`, `tests/server_settings.rs`) are clean. No other file was touched.

### Manual Validation
Deferred to the end (point "User joined" at a channel and have the second account join; turn "User left" off and have it leave).

## Chunk 4: Invites page

### Changes Made
- `src/api/client.rs`: `fetch_server_invites(server) -> Vec<Invite>` (`GET /servers/{id}/invites`) and `delete_invite(code)` (`DELETE /invites/{code}`).
- `src/server_settings.rs`:
  - `InviteRow { code, creator, creator_name, creator_avatar, channel, uses, expiry }`.
  - `invite_rows(store, invites, now)`:
    - keeps Stoat's order, since codes carry no creation time;
    - skips `Invite::Group`, and invites that have expired by `now`;
    - uses "Unknown user" (no avatar) for an uncached creator, and "Unknown channel" for an unknown channel;
    - expiry is "Never expires", or "Expires in 3 days" through `time::relative_time`.
  - `uses_label`: "1 use", "3 uses", "3 / 10 uses".
  - `timestamp_utc` (a Stoat `Timestamp` to `chrono`, through its JSON string, as `permissions::is_timed_out` does).
  - `invites_error_text` and `revoke_error_text`.
- `src/ui/moderation_dialogs.rs`: `confirm_member_action_then(request, action, after, …)` and `AfterAction`. It is the same confirmation, but Stoat's answer goes to `after` once the dialog has closed, instead of the usual refusal toast. `confirm_member_action` is unchanged for milestone 22's callers; both share `open_confirmation`.
- `src/ui/invites.rs`: `CreateInviteDialog` emits `InviteCreated` when Stoat makes the invite.
- `src/ui/chat.rs`: `first_invitable_channel` is now `pub`, and there's a new `invite_max_days()` accessor.
- `src/ui/server_settings.rs`:
  - a generic `Load<T> { Idle, Loading, Loaded, Failed(text) }`;
  - `invites_page` (Link icon, Manage Server);
  - `InvitesPage`:
    - fetched the first time the page may be shown (`load_once`, from `ServerSettingsView::render`);
    - a header with "Create invite…" (disabled with no channel to invite to) and Refresh;
    - rows: the creator's avatar and name, "#channel · code", uses and expiry, Copy link (a link from `web_app`, else the code, with a toast), and a red Revoke;
    - Revoke confirms with `confirm_member_action_then` ("Revoke invite {code}?"). The row is removed on success or `NotFound`; any other refusal toasts `revoke_error_text`;
    - "Create invite…" opens `CreateInviteDialog` in the settings window for `first_invitable_channel`, and refreshes the list on `InviteCreated`;
    - Loading, Failed with Retry, and "This server has no invites." states.
- Tests:
  - `tests/api_invites.rs`: a list with and without `max_uses` / `expires`; a revoke is one `DELETE /invites/abc`; a `MissingPermission` on the list becomes its text.
  - `tests/server_settings.rs`: rows name the creator and channel (unknown ones too), with uses and relative expiry; expired and group invites are left out; `uses_label`.

### Divergences
- **`confirm_member_action` couldn't report success**, but the page must drop the revoked row. I added `confirm_member_action_then` beside it rather than changing milestone 22's callers.
- **`CreateInviteDialog` needed only an event** (`InviteCreated`). It doesn't depend on the main window.
- **Uses read "3 uses" / "3 / 10 uses"** rather than the plan's bare "3" / "3 / 10".
- **Look-ahead:** chunk 5 (Bans) will reuse `Load<T>`, `load_once` and `confirm_member_action_then`. The plan's approach is unchanged; no amendment needed.

### Verification Performed
- `cargo test --test server_settings --test api_invites`: 24 + 12 passed.
- `cargo test`: 1157 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt:
  - drift unchanged: `src/ui/chat.rs` 17, `src/api/client.rs` 3;
  - still clean: `src/ui/invites.rs`, `src/ui/moderation_dialogs.rs`, `tests/api_invites.rs`;
  - new files clean.

### Manual Validation
Deferred to the end (create from the page, copy and open the link in the web client, revoke, and a Ban-Members-only account doesn't see Invites).
