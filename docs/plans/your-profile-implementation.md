# Implementation Record: Your profile (milestone 15)

_Plan: docs/plans/your-profile.md_
_Started: 2026-11-06_

Run in continuous mode on branch `milestone-15-your-profile`. Every chunk's manual check is batched for the end (marked pending below until then). Pre-existing dirty files left out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt drift (`Diff in` count from `rustfmt --edition 2024 --check`): `src/ui/chat.rs` 17, `src/store.rs` 15, `src/api/client.rs` 3, `src/api/events.rs` 3; 0 in `src/ui/settings.rs`, `src/permissions.rs`, `src/draft.rs`, `src/assets.rs`, `src/ui/app_root.rs`, `src/main.rs`, `src/ui/actions.rs`, `src/ui/profile_card.rs`.

## Chunk 1: Profile page: display name, pronouns and bio

### Changes Made
- `src/profile_edit.rs` (new, pure): the limits (`DISPLAY_NAME_MIN/MAX`, `PRONOUNS_MAX`, `BIO_MAX`, `NICKNAME_MAX`); `FieldCheck` and `check_display_name` / `check_pronouns` / `check_bio` / `check_nickname` (characters, trimmed except the bio, blank = `Clear`, line breaks and U+200B invalid in a display name); `ProfileForm { display_name, pronouns, bio }` with `is_valid` and `ProfileForm::changes(&loaded, &edited) -> DataEditUser` (only changed fields, blanks in `remove`, fields breaking their rule left out); `empty_edit` / `is_empty_edit` (`DataEditUser` has no `Default`); `save_error_text` (rate limit with seconds rounded up, `FailedValidation` mapped to the field's rule by the field name in Stoat's message, else "Couldn't save your profile."). 15 unit tests.
- `src/store.rs`: `ChatStore::apply_own_user(User)` — applies Stoat's answer to `PATCH /users/@me`, keeping the user's status, `online` and relationship (the answer says `online: false` for Invisible, so `add_user` would wrongly flip the presence).
- `src/ui/profile_editor.rs` (new): the `ProfileEditor` entity for one account — weak `ChatView` handle, `InputState`s for display name and pronouns, a `TextareaState` (auto-grow 3–10 rows) for the bio, the loaded `ProfileForm`, and load/saving state. Display name and pronouns come from the store (observing the chat until the user is known); the bio from `fetch_profile`, with "Couldn't load your bio and banner." and Retry on failure. Save is disabled until both have loaded, while saving, while any field breaks its rule, or with no changes. Save sends one `edit_user`, then resets the form to Stoat's answer, calls `ChatView::own_profile_saved`, and toasts "Profile saved." (or `save_error_text`). Reset restores the loaded values. Counters per field (red over the max) and the rule broken under it. "Editing @username#1234" at the top, and "Your username can't be changed here yet." note.
- `src/ui/settings.rs`: `profile_page` first in `app_pages()` (`CircleUser` icon, `resettable(false)`, search keywords); `SettingsView` gains `profile_editors: HashMap<account id, Entity<ProfileEditor>>` (made in `render`, which has the window; replaced if the account signed out and in again, via `ProfileEditor::is_for`), observes the `AppRoot`, and `show_profile()` bumps a `page_reset` counter used in the `Settings` element ID so the sidebar state restarts on page 0. Without a signed-in account on screen the page says "Sign in to edit your profile."
- `src/ui/app_root.rs`: `pub fn shown_chat() -> Option<(String, Entity<ChatView>)>`.
- `src/ui/chat.rs`: `client()`, `store()`, `cache_profile()` and `own_profile_saved(user, bio, cx)` (applies the user, updates the cached profile's bio, re-syncs panes showing the user, member lists and suggestions); a `UserUpdate` now forgets `profiles[id]`; "Edit profile…" (`UserPen` icon) after a separator at the end of the status menu, dispatching `OpenProfileSettings`.
- `src/ui/actions.rs`: `OpenProfileSettings` action.
- `src/main.rs`: `SettingsWindow` now holds the window and a weak `SettingsView`; `open_or_focus(profile, cx)` — with `profile`, an open window is brought forward and switched to Profile; a new one starts there anyway.
- `src/assets.rs`: bundles `UserPen`, added to the status-menu icon test.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- Tests: `tests/api_users.rs` (three exact-body `PATCH /users/@me` tests built through `ProfileForm::changes`), `tests/store_live.rs` (`stoats_answer_to_a_profile_change_applies_at_once`).

### Divergences
- The bio field is a `TextareaState`, not an `InputState` with `auto_grow`: in gpui-component 0.9 multi-line input is the separate `TextareaState`/`Textarea` (as the composer uses). Same behaviour.
- `ChatStore::apply_own_user` is new rather than reusing `add_user` (the plan allowed either): `add_user` would take the answer's `online: false` and show the user as Invisible.
- Choosing the Profile page in an open window uses a changing `Settings` element ID (Assumption 5's approach). Whether it switches pages is a manual check.

### Verification Performed
- `cargo test --lib profile_edit`: 15 passed.
- `cargo test --test api_users --test store_live`: all passed (19 and 37).
- Full suite `cargo test`: 778 passed, 0 failed, 5 ignored (pre-existing ignores). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/ui/chat.rs` 17 and `src/store.rs` 15 (unchanged); `settings.rs`, `main.rs`, `app_root.rs`, `actions.rs`, `assets.rs`, and the new files 0.

### Manual Validation
Pending (batched for the end of the milestone):
- ⌘, opens Settings on Profile, naming your account. Change your display name and Save: it changes in the panel and in the web client.
- Set pronouns and a two-line bio: your card shows them, and so does the web client's.
- Clear the display name and Save: your username shows.
- Switch accounts with Settings open: the page follows, and unsaved text for the first account is still there when you switch back.
- "Edit profile…" in the status menu opens Settings on Profile.

## Chunk 2: Avatar and banner

### Changes Made
- `src/api/client.rs`: `upload_file(file_server, tag, name, content_type, path, on_progress)` posts to `{file_server}/{tag}`; `upload_attachment` is now a wrapper for `"attachments"`. `FileSizeLimits` gains `avatars` and `backgrounds`; `RootConfig::avatar_size_limit()` and `background_size_limit()` take the stricter of the two account limits (shared `stricter_size_limit` helper, also used by `attachment_size_limit`), else `DEFAULT_AVATAR_SIZE_LIMIT` (4 MB) and `DEFAULT_BACKGROUND_SIZE_LIMIT` (6 MB), re-exported from `src/api/mod.rs`.
- `src/profile_edit.rs`: `IMAGE_TYPES` (PNG, JPEG, GIF, WebP); `ImageProblem { WrongType, TooBig { limit } }` with its message ("That image is too big. It can be up to 4 MB."); `check_image(path, size, limit) -> Result<content type, ImageProblem>` (type from the extension through `mime_guess`, any case); `ImageLimits { avatar, background }` with `Default` and `from_root`; `ProfileImage { None, Current(file id), New(path) }`; `ProfileForm` gains `avatar` and `banner`, `changes` maps them to `avatar` / `profile.background` / `remove: Avatar | ProfileBackground` (a `New` image is left out until its upload ID replaces it as `Current`), and `has_changes` also counts picked files. `save_error_text` now covers the file server's refusals (`FileTooLarge`, `FileTypeNotAllowed`, `ImageProcessingFailed`) and unreadable files (`ApiError::File`). 6 more unit tests.
- `src/draft.rs`: `decimal_size_label` made `pub` (reused for the image limits).
- `src/ui/chat.rs`: `ChatView` keeps `image_limits` from `GET /` (beside the composer's `AttachmentLimits`) with an `image_limits()` getter; `own_profile_saved` gains `banner_changed`, which forgets the cached profile so the card fetches the new banner.
- `src/ui/profile_editor.rs`: Avatar and Banner rows above the text fields — preview (avatar: 80 px centre-cropped circle, the default avatar when there's none; banner: 348×150, i.e. 232:100, centre-cropped, "No banner" strip when there's none; a picked file is drawn from disk with `img(PathBuf)`, falling back to its file name if it can't be drawn), Change… (`prompt_for_paths`, one file) and Remove, and under them either the problem with the last pick or "PNG, JPEG, GIF or WebP, up to N MB." A bad pick keeps the old image and disables Save until it's replaced, removed or reset. Save uploads new images (`avatars`, `backgrounds`) one after the other while the button says "Uploading…", then sends the `PATCH` only if every upload worked. Reusable pieces for chunk 3 are `pub(crate)`: `ImageKind`, `ImageSlot`, `prompt_for_image`, `upload_image`, `Preview`, `avatar_preview`, `render_image_row`, `field`.
- Tests: `tests/api_upload.rs` (`avatars_and_banners_go_to_their_own_tags`, `root_config_gives_the_avatar_and_banner_limits` — the fixture has no `backgrounds`, so 6 MB); `tests/api_users.rs` (`{"avatar":"file-1","profile":{"background":"file-2"}}`). `upload_attachment` posting to `/attachments` stays covered by the existing upload tests.

### Divergences
- `ProfileImage { None | Current(id) | New(path) }` instead of the plan's `ImageChange { Keep | Replace(path) | Remove }`: the form needs to know whether there's a current image (removing nothing isn't a change), and an upload's ID slots in as `Current(id)` before `changes` runs. Same outcome. Plan amended (chunk 2's bullet notes it; chunk 3's `IdentityForm` now names `ProfileImage`).
- `ImageLimits` lives in `profile_edit.rs` (the plan allowed `draft.rs` or `profile_edit.rs`).
- After a banner change the card's cached profile is forgotten rather than updated from what was sent: the upload returns only an ID, not the `File` the card's `UserProfile` holds. The card fetches the profile each time it opens anyway.
- `save_error_text` gained the file server's error types and `ApiError::File`, so an upload failure says why rather than "Couldn't save your profile."

### Verification Performed
- `cargo test --lib profile_edit`: 21 passed.
- Full suite `cargo test`: 787 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/ui/chat.rs` 17, `src/store.rs` 15, `src/api/client.rs` 3 (unchanged; new code hand-formatted); `src/draft.rs` 0; `tests/api_upload.rs` 10 (pre-existing, unchanged); `tests/api_users.rs` 0; new files 0.

### Manual Validation
Pending (batched for the end of the milestone):
- Pick a wide photo as your avatar: the preview is its centre square in a circle. Save: your avatar changes in the panel, on your messages and in the web client.
- Pick a GIF: it animates in the web client.
- Set a banner: your card shows it centre-cropped. Remove both, and the defaults come back.
- Pick a 10 MB file: the row says it's over the limit, and Save stays disabled.
