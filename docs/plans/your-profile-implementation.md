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
