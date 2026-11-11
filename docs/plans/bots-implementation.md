# Implementation Record: Bots (milestone 29)

_Plan: [`bots.md`](bots.md)_
_Started: 2026-11-11_

Run in continuous mode. Manual checks are batched at the end, as the user asked. The plan and its roadmap link (uncommitted before chunk 1) were committed with chunk 1, as before.

Rustfmt drift at the start (`Diff in` hunks): `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3. Every other touched file was clean.

## Chunk 1: My Bots: list, create, delete

### Changes Made
- `src/bots.rs` (new, no GPUI):
  - `BOT_USERNAME_MIN`/`MAX` (2, 32) and `DEFAULT_BOT_LIMIT` (5);
  - `BotRow { id, username, display_name, avatar_url, public }` and `bot_rows(bots, users, media)`: users matched by ID, by display name (case-insensitive), then ID. A bot without its user shows its ID; a blank display name falls back to the username;
  - `check_bot_username` → `Result<String, UsernameProblem>` (trimmed; 2–32 characters; letters in any script, digits, `_`, `.`, `-`; not `admin`, `revolt` or `stoat`). `UsernameProblem` displays its text;
  - `bot_limit(root)`, `at_limit`, `count_line` ("{n} of {max} bots"), `delete_prompt`, and `bot_error_text(err, owned)` (Assumption 8).
- `src/api/client.rs`: `AccountLimits.bots`; `fetch_owned_bots()` (`GET /bots/@me`), `create_bot(name)` (`POST /bots/create`, `{"name"}`), `delete_bot(id)` (`DELETE /bots/{id}`).
- `src/store.rs`: `add_bot_user(user) -> bool`. Stoat sends a bot's user as it sees itself (`relationship: User`), so the store keeps the relationship it has (none for a new user), and says whether a known user's name or avatar changed.
- `src/ui/chat.rs`: `bot_limit` from root config (`bot_limit()`), and `bot_users_loaded(users)`, which keeps them and re-syncs panes showing a changed one (and drops its cached profile).
- `src/ui/bots_page.rs` (new):
  - `BotsPage`: fetches once per account ("Loading…", or the reason and Try again), the count line with the `Bot` icon, Create bot… (disabled at the limit, with a note), and a row per bot (avatar, display name, `@username`, Public/Private, chevron). The bots, with their tokens, stay in the page; their users go to the store;
  - `CreateBotDialog`: the username with its problem shown once something's typed, "You can give it a display name and avatar after.", Create (opens the new bot);
  - `BotView` (minimal, grown in chunks 2–3): "‹ My Bots", the avatar, name and `@username#tag`, the ID with Copy ID, and Delete bot… (confirmed with an alert; success toasts "Bot deleted." and goes back to the list). It emits `BotEvent::{Back, Deleted}`.
- `src/ui/settings.rs`: `bots_views`, `sync_bots_view` / `shown_bots_view`, and `bots_page` ("My Bots", icon `Bot`) after Sessions.
- `src/assets.rs`: `Bot` bundled, and checked in the settings icons test.
- `src/lib.rs`, `src/ui/mod.rs`: the modules, by hand.
- Tests:
  - `tests/bots.rs` (new, 5 tests): rows (matched by ID with users out of order, a missing user, a blank display name, public/private, order, avatar URL); the username check (valid, trimmed, other scripts, too short, too long, 32 multi-byte characters, spaces, punctuation, emoji, reserved); the limit; the store keeping its relationship with a bot and reporting changes; the texts;
  - `tests/api_bots.rs` (new, 4 tests): `GET /bots/@me` parses bots (tokens, public, permissions, interactions URL) and users; `POST /bots/create` sends exactly `{"name"}`; `DELETE /bots/{id}`; `ReachedMaximumBots` and `InvalidUsername` read plainly.

### Divergences
- **The limit is the established accounts' (`default`), else `new_user`, else 5,** not the stricter of the two. On Stoat's official config the stricter is 2, which would show "3 of 2" and block established accounts that may have 5. Stoat refuses new accounts itself, and that refusal reads plainly. The plan's Assumption 1 is amended.
- **`bot_error_text(err, owned)` takes how many bots the user has,** so `ReachedMaximumBots` says the true count ("You have 5 bots…", "1 bot"), rather than the limit from root config (which may not be the one that applies). The plan's Assumption 8 is amended.
- **`ChatStore::add_bot_user`** was added (the plan's risk: "check `add_user` replaces sensibly, or merge"). `add_user` replaces outright, and a bot's user arrives with `relationship: User`.
- **`bot_rows` takes the bots and users as slices**, not `&OwnedBotsResponse`, so the page can build rows from its list and the store's users after a create or a change.
- **The username check also refuses `admin`, `revolt` and `stoat`** (Stoat's blocked names), and the Create dialog shows the problem as you type.

Look-ahead: chunk 2 needs a `BotEvent::Saved` so the page's list follows a save (and a token reset), and the bot view's state grows. Chunks 3–5 are unaffected.

### Verification Performed
- `cargo test --test bots --test api_bots`: 5 and 4 passed (after fixing one expected order in the test).
- Full suite: `cargo test` gave 1,411 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` 17, `src/store.rs` 14 and `src/api/client.rs` 3 are unchanged. `src/ui/settings.rs` and `src/assets.rs` stay clean. The new files are clean.

### Manual Validation
Deferred to the end (batched): My Bots lists your bots like the web client; create one, copy its ID, delete it.

## Chunk 2: A bot's settings and token

### Changes Made
- `src/bots.rs`:
  - `INTERACTIONS_URL_MAX` (2048);
  - `BotForm { public, interactions_url, default_permissions }` with `of`, `is_valid` (URL length) and `changes -> Option<DataEditBot>` (only what changed; the URL trimmed, and a blank one `remove: [InteractionsURL]`; `None` stored permissions read as 0);
  - `username_change(typed, current)`, `rename_edit(name)`, `token_reset_edit()` (`remove: [Token]` only), `reset_prompt(name)` and `invite_url(app, id)` (`{app}/bot/{id}`).
- `src/api/client.rs`: `edit_bot(id, &DataEditBot) -> BotWithUserResponse` (`PATCH /bots/{id}`, through `edit_body`).
- `src/ui/bots_page.rs`:
  - `BotView` now has the bar (‹ My Bots, Reset, Save), the header, **Settings** (the username with its own Save and its problem shown; Public as a switch; Interactions URL with a count; Requested permissions as the matrix in Values mode, server context, every option enabled), **Token** (`••••••`, Copy token, Reset token… confirmed), **Share** (the ID, Copy ID, and Copy invite URL when the instance's web client is known), and Delete bot…;
  - every change goes through one `send(change, edit)`: the answer replaces the page's bot, its user goes to the store (`bot_users_loaded`), and `BotEvent::Saved` keeps the section's list current. Settings refill from the answer ("Saved."), a rename refills the username ("Renamed."), a token reset closes its alert ("Token reset."). A refusal toasts `bot_error_text` and keeps the edits;
  - `open_confirm`, a shared alert for Reset token and Delete whose buttons wait for the answer.
- Tests:
  - `tests/bots.rs`: 2 new tests. `BotForm` (filled, nothing changed, public, URL trimmed/changed/blank → remove, too long, permissions, none stored); rename, token reset, the prompt, invite URLs;
  - `tests/api_bots.rs`: 2 new tests. The exact `PATCH` bodies (`{"public":true}`, `{"remove":["Token"]}`, `{"default_permissions":24}`, `{"name"}`, `{"remove":["InteractionsURL"]}`) and the parsed answers (a new token, the user); `UsernameTaken` reads plainly.

### Divergences
- **`rename_edit`** was added (the rename's `PATCH`), beside `username_change`.
- **Copy ID moved to Share**, with the ID shown there, as Assumption 3 lays out.
- **A failed token reset keeps its alert open** (with a toast), as Delete does, rather than closing.

The token is held only in `BotView.bot` and the section's list; it's copied, never drawn, and no log line formats a `Bot` (only IDs and `ApiError`s).

Look-ahead: chunk 3 adds a Profile section to `BotView`; chunk 4 adds "Add to a server or group…" to Share. No amendments.

### Verification Performed
- `cargo test --test bots --test api_bots`: 7 and 6 passed.
- Full suite: `cargo test` gave 1,415 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/api/client.rs` stays at 3; the new files are clean.

### Manual Validation
Deferred to the end (batched): make a bot public, set its interactions URL and requested permissions, reset its token (the old one stops working with a bot client), copy the new one, and rename the bot.

## Chunk 3: A bot's profile

### Changes Made
- `src/ui/profile_editor.rs`:
  - `ProfileEditor::new` still edits the account (unchanged behaviour); `ProfileEditor::for_bot(chat, bot)` edits a bot's user. Both go through `editing(chat, user, own)`;
  - the `me` field is now `user`, plus `own`. Filling from the store, fetching the bio and banner (`fetch_profile(user)`, which Stoat allows for any bot), the avatar preview's default, and caching the profile all use `user`;
  - Save sends `edit_user` (own) or `edit_bot_user(user)`, and hands the answer to `own_profile_saved` or the new `bot_profile_saved`;
  - for a bot: no "Editing @…" line (its page names it), no "Your username can't be changed here yet." (it's in Settings), the bio's placeholder is "Tell people about the bot", and the failure texts say "the bot's". Log lines name the user rather than "own".
- `src/api/client.rs`: `edit_bot_user(id, &DataEditUser) -> User` (`PATCH /users/{id}`, through `edit_body`).
- `src/ui/chat.rs`: `bot_profile_saved(user, bio, banner_changed)`, as `own_profile_saved` does for the account: the user goes to the store (`add_bot_user`), the cached profile follows (or is dropped when the banner changed), and panes showing the bot are re-synced.
- `src/ui/bots_page.rs`: `BotView` holds a `ProfileEditor::for_bot`, shown as a **Profile** section under the header. The settings' Reset and Save moved from the top bar to under the Settings section.
- `tests/api_bots.rs`: 1 new test. `PATCH /users/b1` sends exactly what `ProfileForm::changes` gives (display name, avatar ID, bio) and parses the bot's `User`.

### Divergences
- **The settings' Reset and Save sit under the Settings section,** not in the top bar: the Profile section has its own Save, and two unrelated Saves at opposite ends of the page would mislead. The plan's Assumption 3 is amended.
- **`ChatView::bot_profile_saved`** was added beside `own_profile_saved`, rather than applying the answer with `add_user`/`apply_user_update` directly, so the cached profile (bio, banner) follows too.

Look-ahead: chunks 4–5 are unaffected.

### Verification Performed
- `cargo test --test profile` (unchanged): 38 passed. `cargo test --test api_bots`: 7 passed.
- Full suite: `cargo test` gave 1,416 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` 17 and `src/api/client.rs` 3 are unchanged; `src/ui/profile_editor.rs` stays clean; the new files are clean.

### Manual Validation
Deferred to the end (batched): give a bot an avatar, banner, display name and bio; its messages and profile card show them. Your own Profile page behaves as before.
