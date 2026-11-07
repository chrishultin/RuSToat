# Plan: Your profile (RuSToat milestone 15)

## Summary
Milestone 15 of the [roadmap](roadmap.md) lets you edit your own profile, which today you can only see:
- **Your global profile:** display name, avatar, banner, bio and pronouns, on a new Profile page in the Settings window.
- **A per-server identity:** a nickname, avatar and pronouns for one server, from that server's right-click menu or from the Profile page.

Stoat's web client (`stoatchat/for-web`, read 2026-11-06) is the reference: `settings/user/profile/UserProfileEditor.tsx`, `EditProfile.tsx` and `modal/modals/ServerIdentity.tsx`. So is the backend (`stoatchat/stoatchat`, `crates/delta/src/routes/users/edit_user.rs` and `servers/member_edit.rs`).

## Approach
**What exists already.**
- **Profiles are read, never written.**
  - `StoatClient::fetch_profile` (`GET /users/{id}/profile`) feeds the profile card. `ChatView.profiles` caches the result per user (`chat.rs` ~337, filled ~3002), and nothing ever refreshes it.
  - The card shows the banner centre-cropped (`profile_card.rs` ~70), and `avatar()` (`chat.rs` ~6703) already draws avatars with `ObjectFit::Cover`.
- **`PATCH /users/@me` exists** (milestone 14): `StoatClient::edit_user(&DataEditUser)` strips nulls and an empty `remove`, and has tests in `tests/api_users.rs`. Display name, avatar and pronouns live on `User`, so the `UserUpdate` echo already updates them through `ChatStore::apply_user_update`. Bio and banner live on the profile, which isn't part of `User`.
- **Uploads.** `StoatClient::upload_attachment` always posts to `{file_server}/attachments`. Avatars must go to the `avatars` tag, and profile banners to `backgrounds` (the web client's choice; `banners` is for servers).
  - `RootConfig` reads only the `attachments` size limit. Live `GET /` also has `avatars` (4 MB) and `backgrounds` (6 MB), but `tests/fixtures/root.json` has no `backgrounds`.
  - `AttachmentLimits::from_root` reaches the composer at `chat.rs` ~484.
  - The composer picks files with `cx.prompt_for_paths` (`composer.rs` ~271), and `mime_guess` is already a dependency (`draft.rs` ~123).
- **Members.**
  - The user's own memberships come in `Ready`. Live `ServerMemberUpdate` goes through `apply_member_update` (`chat.rs` ~986), which clears `Nickname`, `Avatar` and `Pronouns`.
  - The store already shows the server nickname and avatar on messages (`author_name` and `avatar_url`, `store.rs` ~2088–2126) and the server pronouns on the card.
  - There's no `PATCH /servers/{id}/members/{user}`.
- **Permissions.** `src/permissions.rs` works out channel permissions with `revolt_permissions::calculate_channel_permissions`. The crate also has `calculate_server_permissions`, which isn't used yet.
- **Settings window** (milestone 13):
  - `SettingsView::app_pages()` (`src/ui/settings.rs`) lists pages as `fn(&SettingsView, &mut App) -> SettingPage`.
  - `SettingsView` keeps a `WeakEntity<AppRoot>` "for sections about the account on screen", and `AppRoot::shown_account()` (`app_root.rs` ~236) names it.
  - The window opens through `SettingsWindow::open_or_focus` (`main.rs` ~318).
  - `SettingField::render` takes any element. gpui-component's `Settings` has `default_selected_index`.
- **Menus** (milestone 14): the server menu is `ChatView::server_menu` (a `MenuBuilder`), and the status menu is `ChatView::status_menu`.

**The profile editor is a Settings page** (user decision).
- **Placement:** "Profile" is first in the sidebar. It edits the account the main window shows, and says which ("Editing @name#1234").
- **The form:** a `ProfileEditor` entity per account, held by `SettingsView` in a `HashMap<account id, Entity<ProfileEditor>>`. Unsaved edits for one account survive switching to another while the window is open. `SettingsView` observes the `AppRoot`, so switching accounts redraws the page.
- **Opening it:** "Edit profile…" in the status menu opens Settings on the Profile page.
- **Loading:** the editor reads display name, pronouns and avatar from the account's `ChatStore`, and the bio and banner from `fetch_profile`. Until the profile has loaded, Save is disabled, so an empty bio can't overwrite the real one.

**One Save for the whole form** (user decision), as the web client does.
- **Save and Reset:**
  - Save sends one `PATCH /users/@me` with only the changed fields. A field cleared to blank goes in `remove` (`DisplayName`, `Pronouns`, `ProfileContent`, `Avatar`, `ProfileBackground`).
  - New images are uploaded first, and the `PATCH` follows only if every upload worked.
  - Reset puts the form back to what was loaded.
- **After saving:** Stoat's answer (the `User`) updates the store at once. The editor's copy of the bio and banner, and `ChatView.profiles` for the user, are updated from what was sent.
- **Errors** become a toast, with a pure helper reused by chunks 2 and 3:
  - `FailedValidation` shows the field's rule.
  - `RateLimited` says "Stoat is limiting changes. Try again in N s." Status changes and profile edits share Stoat's user-edit limit, as manual testing of milestone 14 showed.
  - Anything else says "Couldn't save your profile."
- **Rejected alternative: saving each field on its own,** like the settings switches. It's awkward for text, and means a request per field against a rate-limited endpoint.

**Images are uploaded unchanged** (user decision).
- **What goes up:**
  - RuSToat checks the type (PNG, JPEG, GIF or WebP, from the extension through `mime_guess`) and the size: avatars at most `avatars` (default 4 MB) and banners at most `backgrounds` (default 6 MB), from `GET /`.
  - GIFs stay animated.
- **Previews** draw the local file the way every client will show it: the avatar as a centre-cropped circle, and the banner as a centre-cropped 232:100 strip (the web client's banner ratio).
- **Rejected alternative: centre-cropping and re-encoding before upload.** It needs an image crate, drops GIF animation, and other clients already centre-crop on display.

**The per-server identity is a dialog** (user decision: the recommendations).
- **Opening it:** "Edit identity…" in a server's right-click menu, and a "Server identities" list of your servers on the Profile page, which opens the same dialog in the main window.
- **The dialog:**
  - It's titled "Change identity on {server}", and has a server avatar, Nickname (placeholder: your display name) and Pronouns (placeholder: your global pronouns).
  - Each field can be cleared, which falls back to your global profile (`remove: ["Nickname" | "Avatar" | "Pronouns"]`).
  - It sends `PATCH /servers/{server}/members/{your id}` with a `DataMemberEdit` holding only the changes.
- **Permissions:**
  - Nickname needs `ChangeNickname` and avatar needs `ChangeAvatar` (`member_edit.rs` lines 64–82), worked out with `calculate_server_permissions`. Without them the field is disabled with a note.
  - The backend allows your own pronouns without any permission (lines 72–76).
  - The server's owner has every permission.

## Assumptions
1. **Validation follows Stoat's rules** (`revolt-models` 0.16.2), counted in characters after trimming:
   - display name 2–32, with no line breaks or zero-width spaces (`RE_DISPLAY_NAME`)
   - pronouns 1–24 (global and per server)
   - bio up to 2000; it isn't trimmed, as in the web client
   - nickname 1–32

   Blank means "remove". The form shows counters and disables Save while any field breaks a rule, so Stoat's `FailedValidation` is a fallback.
2. **The bio is plain text in a multi-line field** (`InputState` with `auto_grow`), with no Markdown preview. The card already renders it as Markdown.
3. **Usernames aren't here.** Changing your username is milestone 17. The page has a line saying so, as the web client does.
4. **Fresh profiles.** Any `UserUpdate` for a user forgets their cached profile in `ChatView.profiles`, so their card fetches it again next time. Stoat's `UserUpdate` carries no bio or banner, so a change from another client would otherwise never show.
5. **Opening Settings on the Profile page:** Profile is the first page, so a freshly opened window lands on it. If the window is already open on another page, gpui-component's `Settings` may not switch pages. The plan tries giving `Settings` a new element ID with `default_selected_index(0)` when "Edit profile…" asks for it. If that misbehaves, the item just brings the window forward.
6. **One request at a time per form.** Save is disabled while saving. A second account's editor is independent.

## Plan

### Chunk 1: Profile page: display name, pronouns and bio
**Goal:** Settings → Profile edits your display name, pronouns and bio, synced to Stoat, for the account on screen.
**Files likely touched:**
- `src/profile_edit.rs` (new, pure, no GPUI):
  - `DISPLAY_NAME_MAX = 32`, `DISPLAY_NAME_MIN = 2`, `PRONOUNS_MAX = 24`, `BIO_MAX = 2000`, `NICKNAME_MAX = 32`
  - `check_display_name`, `check_pronouns` and `check_bio`, returning `FieldCheck { Ok(trimmed) | Clear | TooShort(n) | TooLong(n) | Invalid(&'static str) }`
  - `ProfileForm { display_name, pronouns, bio }` with `changes(&loaded, &edited) -> DataEditUser`, holding only what changed, with blanks in `remove`
  - `save_error_text(&ApiError) -> String`, for `FailedValidation`, `RateLimited` (with its seconds) and others
- `src/ui/profile_editor.rs` (new):
  - The `ProfileEditor` entity, for one account: an `Entity<ChatView>` handle (for its client, store and `profiles`), `InputState`s for the three fields, the loaded values, and `saving` and `loaded` flags.
  - `render` draws the fields with counters and Save/Reset.
  - `save` builds the `DataEditUser`, calls `client.edit_user`, applies the returned `User` (`ChatStore::add_user`, or a new `apply_own_user`), and updates `ChatView.profiles` for the user.
- `src/ui/settings.rs`:
  - `profile_page`, first in `app_pages()`, with `SettingPage::resettable(false)`.
  - `SettingsView` gains `profile_editors: HashMap<String, Entity<ProfileEditor>>` and observes `AppRoot`. Without a shown account, the page says "Sign in to edit your profile."
- `src/ui/app_root.rs`: `pub fn shown_chat(&self) -> Option<(String, Entity<ChatView>)>`.
- `src/ui/chat.rs`:
  - "Edit profile…" in `status_menu`, which dispatches a new `OpenProfileSettings` action.
  - `UserUpdate` forgets `profiles[id]`.
  - Expose what `ProfileEditor` needs: the client, the store and the profile cache.
- `src/main.rs` and `src/ui/actions.rs`: an `OpenProfileSettings` action, handled by `SettingsWindow` (open, or focus and select the Profile page; see Assumption 5).
- `src/ui/mod.rs`: the new module.

**Depends on:** None.
**Risks:**
- **Reaching an account's view from the Settings window.** `SettingsView` only has a `WeakEntity<AppRoot>`. If `AppRoot` doesn't hold `Entity<ChatView>` per account in a way that can be handed out, `shown_chat` needs a small refactor there. Keep it read-only access.
- **The page re-renders every frame from a `fn` pointer.** `ProfileEditor` must be an entity, created once per account and kept in `SettingsView`, or its inputs lose focus and text on each render.
- **Overwriting the bio.** If `fetch_profile` fails, Save must stay disabled for the bio. Simplest: the page shows "Couldn't load your profile" with Retry, and Save is disabled until it loads.
- **Rate limits.** Stoat's user-edit limit is tight (milestone 14 hit it with a few status changes). The toast must say how long to wait.

**How to verify:** `cargo test`:
- `src/profile_edit.rs` unit tests:
  - each `check_*` at its limits: display name 1, 2, 32 and 33 characters, with a line break, and with multi-byte characters; pronouns 24 and 25; bio 2000 and 2001; blank and whitespace
  - `changes` sends nothing when nothing changed, only the changed field, `remove: ["DisplayName"]` for a cleared name, `remove: ["ProfileContent"]` for a cleared bio, and `profile.content` for a new one
  - `save_error_text` for `RateLimited { retry_after_ms: Some(4090) }`, which says 5 s
- `tests/api_users.rs` (wiremock, exact `body_json`):
  - `{"display_name":"Ada"}`
  - `{"pronouns":"she/her","profile":{"content":"hi"}}`
  - `{"remove":["DisplayName","ProfileContent"]}`
- a store test: applying the returned `User` changes `display_name(me)` at once

**Manual check:**
- ⌘, opens Settings on Profile, which names your account. Change your display name and Save: it changes in the panel and in the web client.
- Set pronouns and a two-line bio: your card shows them, and so does the web client's.
- Clear the display name and Save: your username shows.
- Switch accounts with Settings open: the page follows, and unsaved text for the first account is still there when you switch back.
- "Edit profile…" in the status menu opens Settings on Profile.

**Shippable when:** display name, pronouns and bio can be set and cleared from Settings → Profile for the account on screen, synced both ways, and the tests pass.

### Chunk 2: Avatar and banner
**Goal:** The Profile page sets, previews and removes your avatar and banner.
**Files likely touched:**
- `src/api/client.rs`:
  - `upload_file(file_server, tag, name, content_type, path, on_progress)`; `upload_attachment` becomes a wrapper for `"attachments"`.
  - `FileSizeLimits` gains `avatars` and `backgrounds`.
  - `RootConfig::avatar_size_limit()` and `background_size_limit()`, the stricter of the two account limits, else 4 MB and 6 MB.
- `src/profile_edit.rs`:
  - `IMAGE_TYPES` (PNG, JPEG, GIF, WebP)
  - `check_image(path, size, limit) -> Result<content_type, ImageProblem>` (wrong type, too big with the limit in MB)
  - `ImageChange { Keep | Replace(path) | Remove }` in `ProfileForm`, mapped to `avatar`, `profile.background` or `remove`
    - _Amended in implementation:_ `ProfileImage { None | Current(file id) | New(path) }` instead, so the form knows whether there's an image to remove, and an upload's ID slots in as `Current(id)`.
- `src/draft.rs` (or `profile_edit.rs`): `ImageLimits { avatar, background }::from_root`, passed to `ChatView` beside `AttachmentLimits` at `chat.rs` ~484.
- `src/ui/profile_editor.rs`:
  - The avatar row: a centre-cropped circle preview (the new file, else the current avatar), with Change… and Remove.
  - The banner row: a 232:100 centre-cropped preview, with Change… and Remove.
  - Pickers use `cx.prompt_for_paths` (files, single), and problems show under the row.
  - Save uploads changed images first (`avatars` and `backgrounds`), shows "Uploading…", then sends the `PATCH`.
- `src/assets.rs`: any icon that isn't bundled (check `IconName` first, as in milestone 14).

**Depends on:** Chunk 1.
**Risks:**
- **Local previews.** GPUI's `img` must load a local path (`img(PathBuf)`). If it can't draw WebP or animated GIF previews, show the file name instead. The upload is unaffected.
- **Autumn rejects a file** (wrong type despite its extension, or a limit that changed): the upload's `ApiError` goes through `save_error_text`, and nothing is half-saved, because the `PATCH` is skipped.
- **Banner propagation.** The banner isn't on `User`, so other open cards only update through Assumption 4's cache forgetting. Check your own card shows the new banner straight after Save.

**How to verify:** `cargo test`:
- `profile_edit.rs` unit tests:
  - `check_image`: `.png`, `.JPG`, `.gif` and `.webp` pass; `.bmp` and `.heic` are the wrong type; 4 000 001 bytes is too big for an avatar but fine for a banner
  - `changes` with `Replace` (after upload IDs are filled in), `Remove` (`remove: ["Avatar"]` and `["ProfileBackground"]`) and `Keep`
- `tests/api_upload.rs`: `upload_file` posts to `/avatars` and `/backgrounds`, and `upload_attachment` still posts to `/attachments`.
- `tests/api_instance.rs` (or the root config tests): the `avatars` and `backgrounds` limits are read, and default when missing (the fixture has no `backgrounds`).
- `tests/api_users.rs`: `{"avatar":"file-1","profile":{"background":"file-2"}}`.

**Manual check:**
- Pick a wide photo as your avatar: the preview is its centre square in a circle. Save: your avatar changes in the panel, on your messages and in the web client.
- Pick a GIF: it animates in the web client.
- Set a banner: your card shows it centre-cropped. Remove both, and the defaults come back.
- Pick a 10 MB file: the row says it's over the limit, and Save stays disabled.

**Shippable when:** avatar and banner can be set and removed from the Profile page, images are checked before upload, and the tests pass.

### Chunk 3: Per-server identity
**Goal:** "Edit identity…" on a server, and a list on the Profile page, set your nickname, avatar and pronouns for that server.
**Files likely touched:**
- `src/api/client.rs`: `edit_member(server, user, &DataMemberEdit) -> Result<Member, ApiError>`, a `PATCH /servers/{server}/members/{user}` with nulls and an empty `remove` stripped, as in `edit_user`.
- `src/permissions.rs`: `server_permissions(store, user, server) -> u64` with `calculate_server_permissions`, then `can_change_nickname` and `can_change_avatar`.
- `src/profile_edit.rs`: `check_nickname`, and `IdentityForm { nickname, avatar: ProfileImage, pronouns }` with `changes -> DataMemberEdit` (blanks in `remove`).
- `src/ui/identity_dialog.rs` (new, or in `chat.rs` beside the status editor):
  - The dialog, opened with `window.open_dialog` and titled "Change identity on {server}".
  - Avatar row (preview, else your global avatar), Nickname and Pronouns fields, with counters.
  - Fields are disabled without permission: "You can't change your nickname on this server." / "…your avatar…".
  - Save uploads a new avatar (`avatars`), then calls `edit_member`, applies the returned `Member` (`ChatStore::apply_member` or `add_member`), and closes.
- `src/ui/chat.rs`: "Edit identity…" in `server_menu`, after "Mark as read".
- `src/ui/profile_editor.rs`: a "Server identities" group listing your servers (icon and name). Clicking one asks the account's `ChatView` to open the dialog in the main window, and brings that window forward.

**Depends on:** Chunk 2 (avatar picking, checks and uploads).
**Risks:**
- **Server permissions.** `calculate_server_permissions` needs the same `PermissionQuery` as channels, minus the channel. `StoreQuery` has `channel` as a required field: it may need to become optional, or a second query type. Keep `channel_permissions`'s behaviour and tests unchanged.
- **The dialog from the Settings window.** Opening a dialog in the main window from another window means updating `AppRoot` and then `ChatView` with the main window's `Window`. Use the existing `MainWindow::update` path in `main.rs`, and defer it, as `open_or_focus` does.
- **The returned `Member`.** If the response's shape differs from `Member` (for example `joined_at`), fall back to applying the change locally and waiting for the `ServerMemberUpdate` echo. Note which in the implementation record.

**How to verify:** `cargo test`:
- `profile_edit.rs` unit tests: `check_nickname` (1, 32, 33, blank), and `IdentityForm::changes` (only changes; `remove: ["Nickname","Avatar","Pronouns"]` when all are cleared)
- `tests/api_members.rs` (new, wiremock):
  - `PATCH /servers/srv/members/u-me` with exact bodies `{"nickname":"Ada"}` and `{"remove":["Avatar"]}`
  - the `Member` parses
  - a 403 `MissingPermission` is an `Api` error
- `tests/permissions.rs`:
  - the owner can change their nickname and avatar
  - a member whose roles deny `ChangeNickname` can't
  - the default server permissions allow both (Stoat's default)

**Manual check:**
- Right-click a server → Edit identity…: set a nickname and server avatar. Your messages there show them, other servers don't, and the web client agrees.
- Clear the nickname: your display name comes back on that server.
- On a server whose roles deny Change Nickname, the field is disabled with its note.
- Settings → Profile → Server identities → a server opens the same dialog in the main window.

**Shippable when:** your nickname, avatar and pronouns can be set and cleared per server from its menu and the Profile page, permissions are respected, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`. Never run it on a `mod.rs` or `lib.rs`, since it formats their child modules too.
- A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines (from `rustfmt --edition 2024 --check <file>`) hasn't grown. Record each touched file's starting count before chunk 1. At the end of milestone 14 they were `src/ui/chat.rs` 17, `src/store.rs` 15, `src/api/client.rs` 3, `src/api/events.rs` 3, and 0 in `src/ui/settings.rs`, `src/permissions.rs`, `src/draft.rs` and `src/assets.rs`.

### Unit Tests
- **`profile_edit.rs`:** every `check_*` at its limits (characters, not bytes; trimming; line breaks in a display name), `check_image` (types, case, limits), `ProfileForm::changes` and `IdentityForm::changes` (only changes, blanks to `remove`, images to IDs), and `save_error_text`.
- **`RootConfig`:** the `avatars` and `backgrounds` limits, and their defaults.

### Integration Tests
- **`tests/api_users.rs`:** `PATCH /users/@me` bodies for display name, pronouns, bio, avatar and banner, and removals.
- **`tests/api_upload.rs`:** uploads go to the right tag.
- **`tests/api_members.rs`** (new): `PATCH /servers/{s}/members/{u}` bodies, the parsed `Member`, and a permission error.
- **`tests/permissions.rs`:** server-level `ChangeNickname` and `ChangeAvatar` (owner, default, denied by a role).
- **Store:** the returned `User` and `Member` apply at once, and a `UserUpdate` forgets the cached profile. That last one is a `ChatView` concern, so give it a thin store-side helper, or test it through the profile cache type if one is extracted.

### Manual Tests
You need an `api.stoat.chat` account, the web client open on the same account, a second account in RuSToat, and a server where a role denies Change Nickname (one you own is easiest to set up in the web client).
1. Run every chunk's manual check.
2. With two accounts, the Profile page always edits the account on screen, and saving one never touches the other.
3. Change your bio and banner in the web client, then open your card in RuSToat: it shows the new ones.
4. Save several times quickly: a rate limit says how long to wait, and nothing is half-saved.
5. Light and dark themes: the previews, counters, disabled-field notes and the identity dialog are readable.

## Definition of Done
- [ ] Settings → Profile (first in the sidebar, naming the account on screen) edits display name (2–32), pronouns (1–24), bio (up to 2000), avatar and banner. Save sends one `PATCH /users/@me` with only the changed fields and blanks as `remove`, and Reset restores what was loaded.
- [ ] Avatar and banner are uploaded unchanged to `avatars` and `backgrounds`, after checking the type (PNG, JPEG, GIF, WebP) and the size limits from `GET /` (4 MB and 6 MB by default). Previews are centre-cropped (a circle and a 232:100 strip).
- [ ] "Edit profile…" in the status menu opens Settings on the Profile page.
- [ ] "Edit identity…" in a server's menu, and the Profile page's Server identities list, set or clear that server's nickname, avatar and pronouns with `PATCH /servers/{s}/members/{you}`. Nickname and avatar are disabled without `ChangeNickname` and `ChangeAvatar`.
- [ ] Changes show in RuSToat at once (panel, messages, card), and changes from other clients show in the card the next time it opens.
- [ ] Save errors say why, and a rate limit says how long to wait.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/your-profile-implementation.md`.
