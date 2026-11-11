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
