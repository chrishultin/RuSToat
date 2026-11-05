# Implementation Record: Multiple accounts and instances (RuSToat)

_Plan: docs/plans/multiple-accounts.md_
_Started: 2026-11-05_

_Pre-existing uncommitted changes at start: `.gitignore` (Riptide artifacts lines) and the untracked `.shadow/`. Neither belongs to this work, and both are kept out of every commit. `docs/plans/multiple-accounts.md` (the plan, new) is committed with chunk 1._

## Chunk 1: Account list and per-account tokens

### Changes Made
- `Cargo.toml`: `dirs = "6"` (the account list's location) and dev-dependency `tempfile = "3"`, both already in the tree.
- New `src/accounts.rs`:
  - `keychain_name(instance, user_id)` gives `"{instance}#{user_id}"`.
  - `AccountEntry { id, instance, user_id: Option, name: Option }`. `id` is the Keychain name.
  - `AccountList { accounts, active }`, with `get`, `find`, `active_entry` (the active account, else the first), `add_or_update` (dedupes on `(instance, user_id)`), `set_user` (fills in a migrated entry), `set_active`, and `remove` (the active account moves to the next account, else the previous one).
  - `AccountsFile` trait:
    - `FileAccounts` writes JSON at `dirs::config_dir()/chat.stoat.rustoat/accounts.json` through a temp file and a rename. `load` returns `Ok(None)` when the file is missing.
    - `MemoryAccounts` is for tests.
  - `migrate_legacy(tokens, instance)`: if a token exists under the bare API URL, returns a one-entry list with `id = instance` and `user_id: None`. Keychain errors count as no saved session.
  - Unit tests for the list operations.
- `src/session.rs`:
  - `TokenStore` methods take a `key` (the Keychain name).
  - `restore(store, client, key)`, `remember(store, key, token)` and `log_out(store, client, key)` take the key explicitly.
  - `blocking` is `pub(crate)` so `accounts` can use it.
- `src/ui/app_root.rs`:
  - Holds the `AccountsFile`, the `AccountList` and `current` (the ID of the account on screen).
  - At startup it loads the list synchronously:
    - missing file: first launch, run `migrate_legacy` and save the result
    - corrupt file: log it, start with an empty list, and don't migrate or overwrite the file
  - Then it restores `active_entry()`.
  - A successful restore runs `set_user` with the user's display name, and saves if that changed anything.
  - `NeedsLogin` for an entry with no `user_id` removes it, because there's nothing to show for an account we never identified.
  - Login runs `add_or_update` with the default instance, sets it active, saves, and remembers the token under the account's ID.
  - Log out removes the account and clears its Keychain item.
  - The list is saved synchronously on the main thread: it's small, and this keeps writes in order.
- `src/main.rs`: passes `FileAccounts::in_config_dir()` to `AppRoot`, falling back to `MemoryAccounts` with a warning.
- `tests/session_restore.rs`: uses explicit keys. New tests:
  - `two_accounts_on_one_instance_keep_their_own_tokens`
  - `legacy_session_is_adopted_and_restores`
  - `nothing_to_adopt_gives_an_empty_list`
- New `tests/accounts.rs`:
  - a file round trip, including a config directory that doesn't exist yet and no leftover temp file
  - a missing file loads as `None`
  - an emptied list is not treated as a first launch
  - a corrupt file is an error and is left untouched
  - unknown and missing fields are tolerated
- `README.md`: the self-hosted and Keychain sections now describe the `{API URL}#{user ID}` naming, adopting an old session, and the account list file.

### Divergences
- **Corrupt account file.** The plan said "treated as empty, not overwritten until the next save". The implementation also skips the legacy migration in that case, so a corrupt file isn't immediately replaced by a migration save.
- **`STOAT_API_URL` with a saved account.** Until chunk 2 adds the instance field, a saved account is restored whatever `STOAT_API_URL` says. The variable only picks the instance for a fresh login. Documented in the README.
- **Found while doing the manual check:** `main` is 10 commits ahead of this branch. Its changes include `src/places.rs`, which remembers the open server and channels per account in the same Application Support folder, plus about +1200 lines in `src/ui/chat.rs`. A parallel branch, `let-s-continue-the-implementation-from-where-we-4ae9` (commit 9a63302, not yet on `main`), renames the Keychain service to `com.chrishultin.rustoat` and moves the Application Support folder. Its token migration is per key name, and it moves the folder as a whole, so both compose with this chunk. Once it lands, `FileAccounts::in_config_dir` should use `places::app_dir()`, and the `dirs` dependency can go.

### Verification Performed
- `cargo test`: 179 passed, 0 failed (ignored: the live and Keychain opt-in tests).
- `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Run by Claude, with the user's approval, against the user's real saved session. `RUST_LOG=stoat_gpui=debug cargo run` logged:
1. `adopting the session saved for https://api.stoat.chat as the first account`
2. `restoring https://api.stoat.chat`
3. `restored saved session for spedione`

The chat opened with no login prompt. `~/Library/Application Support/chat.stoat.rustoat/accounts.json` then contained one entry with `id` and `instance` set to `https://api.stoat.chat`, `user_id` `01KH4Q74NX640MPRYJF506Y71W`, `name` `spedione`, and `active` set to that entry.

The optional log-out-and-relaunch step was not run, to keep the user's session.

## Merge: main into this branch (550a67b)

After chunk 1, `main` (10 commits: attachments, embeds, reactions, and `places.rs` remembering the open server and channels per account) was merged at the user's request, before the chunks that rework `src/ui/chat.rs`. It merged without conflicts; 224 tests passed, and clippy and fmt were clean. Look-ahead found the code that chunks 2–5 touch (banners, Dock badge, acks, login) unchanged in shape, so the plan needed no amendments.

## Chunk 2: Choose the instance at login

### Changes Made
- New `src/api/instance.rs`:
  - `normalize_instance(input)`:
    - trims the input
    - adds `https://` when there's no `://`
    - accepts only `http` and `https`, with a host
    - lowercases (via `url::Url`)
    - drops the query, fragment and trailing slash
  - `resolve_instance(input) -> (StoatClient, RootConfig)`:
    - tries `GET {url}/`
    - a network error stops there (`Unreachable`)
    - otherwise tries `GET {url}/api/` unless the URL already ends in `/api`
    - finds neither: `NotStoat { url }`
  - `InstanceError` has user-facing messages.
  - `SUPPORTED_VERSION_PREFIX = "0.16."`: other versions resolve but log a warning.
  - Unit tests for normalization.
- `src/api/mod.rs`: `pub mod instance`, and re-exports `InstanceError`, `normalize_instance` and `resolve_instance`.
- `src/ui/login.rs`:
  - `LoginView::new(instance, locked, …)`, with a **Server** input above Email. Enter moves on to Email.
  - Locked mode shows the URL as text and uses it as it is, with no resolution. Locked mode is for chunk 4's re-login.
  - The credentials step resolves the instance and then logs in, in one IO task. Results are `Result<(StoatClient, ResponseLogin), LoginError>`, where `LoginError` wraps `InstanceError` or `ApiError`.
  - On a response, the view keeps that client, so 2FA continues on the same instance. The Server field shows the resolved URL.
  - `LoginEvent::LoggedIn { session, instance }`.
- `src/ui/app_root.rs`: `client` becomes `default_instance: String`. `login_screen` is a method that passes the default instance. `logged_in` takes the instance from the event.
- New `tests/api_instance.rs` (7 tests): the API at `/`, the API at `/api` with HTML at `/`, a typed `/api` isn't doubled, `NotStoat` with the URL in the message, `Unreachable` without trying `/api`, a 0.17 version still resolves, and junk input is rejected.
- `README.md`: the "Self-hosted instances" section describes the Server field. `STOAT_API_URL` now only sets its default.

### Divergences
- The plan mentioned showing the resolved host under the field. Instead, the field is replaced with the resolved API URL after a successful attempt. Same purpose, less UI.
- **Known limitation:** on the official instance, `https://stoat.chat/api/` also serves the API (checked live: 200, JSON). So typing `stoat.chat` gives the instance `https://stoat.chat/api`, which differs from `https://api.stoat.chat`. The same user added both ways would be two accounts. Not special-cased.

### Verification Performed
- `cargo test`: 235 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- Live read-only check: `GET https://stoat.chat/` returns HTML, and `GET https://stoat.chat/api/` returns the root config. Bare-host resolution works for the official instance.

### Manual Validation
Deferred at the user's request. Testing the login screen now would mean logging out of their only real session. Chunk 4's manual check (adding an account with **+**) covers logging in through the Server field.

## Chunk 3: App-level notification routing and Dock badge

### Changes Made
- `src/notify.rs`:
  - `banner_tag(account, channel)` gives `"{channel} {account}"`.
  - `parse_banner_tag(tag) -> (Option<account>, channel)`. A tag with no space is treated as legacy: just the channel, with no account.
- `src/ui/chat.rs`:
  - `ChatView::new(client, me, account_id, …)`. It no longer calls `banners::request_permission()` or registers `on_system_notification_response`.
  - New fields: `account_id`, and `active` (default `true`). The `dock_badge` field is removed.
  - `ChatEvent::Activate` is emitted when a toast is clicked, after `open_channel`.
  - Toast IDs and banner tags use `banner_tag(account_id, channel)`.
  - `pub fn sync_outside(&mut self, cx) -> usize` replaces `dismiss_read_banners` plus the Dock code in `render`. It dismisses banners for channels read since, and returns `attention_count()`. `pub fn log_attention()` logs the breakdown.
  - `pub fn set_active(bool, cx)`: going inactive drops `pending_ack`, and coming back calls `notify` (render then acks what's on screen).
  - `maybe_ack_current` returns early when inactive.
  - The live-message `Viewing.channel` is `None` when inactive.
- `src/ui/app_root.rs`:
  - Calls `banners::request_permission()` once, and registers the app's single `on_system_notification_response`. That handler activates the window and calls `open_notice(tag)`, which routes by `parse_banner_tag`. A legacy tag goes to the account on screen, and a tag for an account that isn't on screen is ignored for now (chunk 4 switches to it).
  - `Screen::Chat` keeps a `cx.observe(&view)` subscription that calls `AppRoot::sync_outside`. That sums `view.sync_outside()` over the chat views and sets the Dock badge when the total changes, with a debug log and the per-view breakdown.
  - Log out calls `sync_outside` after replacing the screen, which clears the badge.
  - `ChatEvent::Activate` is a no-op until chunk 4.
- `tests/notify_rules.rs`: `banner_tags_round_trip_with_the_account` and `banner_tags_from_before_accounts_are_just_the_channel`.

### Divergences
- Banner dismissal also moved out of `render`, not just the Dock badge. Hidden views don't render, so their banners would never be taken away. Both now run from the `AppRoot` observer, which fires whenever any view notifies.

### Verification Performed
- `cargo test`: 237 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- Bundled run (`scripts/run-bundled.sh`, debug logs):
  - `banners: allowed`
  - login, `events: Ready (5 servers, 76 channels, …, 81 unreads)`
  - live messages handled
  - No `dock badge:` line: the count stayed 0, because the only unread mention is in a closed DM and the servers are mentions-only.

### Manual Validation
**Not done.** The bundled app was killed partway through: the parallel worktree's session ran `pkill -f "RuSToat.app/Contents/MacOS/stoat-gpui"`, which matches this branch's bundle too. At the user's request, the banner, toast and badge checks move to the end of the plan, together with chunks 4 and 5.

**Found during the run (environment, not this chunk):** the parallel branch's first launch renamed `~/Library/Application Support/chat.stoat.rustoat` to `com.chrishultin.rustoat`, taking chunk 1's `accounts.json` with it. It also moved the bare-URL Keychain item to its new service. So this branch's next launch saw a first launch with nothing to adopt, and showed the login screen. The user logged in through the new Server field. This covers chunk 2's deferred login check on the default instance: `instance: https://api.stoat.chat (API 0.16.2)`, then `logged in as user 01KH…`, and `accounts.json` gained the per-user Keychain name `https://api.stoat.chat#01KH4Q74NX640MPRYJF506Y71W`. When the two branches meet, `FileAccounts` must use `places::app_dir()`, so the account list moves with the folder.

## Chunk 4: Several accounts at once, with a switcher

### Changes Made
- `src/ui/app_root.rs`, rewritten:
  - **State.** `AccountState { Restoring, Offline { token, error }, SignedOut { login }, Chat { view, token, … } }` per account, in `states: HashMap<id, AccountState>`. `Shown { Loading, Account(id), Adding { login, back } }` says what's on screen.
  - **Startup.** `start()` restores every saved account at once (one `spawn_io` each), then `show_next()`: the active account, or the add screen when there are none.
  - **Switching.** `show_account(id)` calls `set_active(false)` on the previous view and `set_active(true)` on the new one, and saves `active`.
  - **Adding.** `add_account()` opens a login view, cancellable when other accounts exist. `cancel_adding()` returns to `back`.
  - **Restore results:**
    - `SignedIn`: `set_user`, then `start_chat`. A new view starts hidden unless it's the account on screen.
    - `NeedsLogin`: `forget_account` for an unidentified adopted entry, otherwise `sign_out`.
    - `Unverified`: `Offline`.
    - a task error: `sign_out`.
  - **Signed out.** `sign_out(id, message)` gives `SignedOut`, with a login view locked to the account's instance.
  - **Rejected session.** `session_ended(id)` forgets the token (new `session::forget`), and the account shows "Your session has ended. Please log in again."
  - **Login.** `logged_in()` (assumption 3):
    - same instance and user, already `Chat`: ends the new session (`end_session`), switches to the existing account, and shows a toast "You're already signed in to that account."
    - `Offline`: ends the old unchecked session, and the new token replaces it
    - otherwise: `add_or_update`, save, remember the token, `start_chat`, show
  - **Log out.** `log_out(id)` ends the session on the server, clears the token, and calls `forget_account`. An account with no token (signed out) is only forgotten. `log_out_current()` is for the menu.
  - **Forgetting.** `forget_account(id)` drops the state (closing its websocket), removes the entry and saves. If the account was on screen, the next one is shown.
  - **Banners.** `open_notice(tag)` switches to the banner's account, then opens the channel.
  - **Sync.** `sync_outside()` runs from every chat view's observer. It sums attention counts for the Dock badge, dismisses banners that have been read, saves display names seen in `Ready` (`AccountList::set_name`), and redraws.
  - **Render.** The switcher column (only once there's at least one account), plus the main area: chat, login, or a status page (Signing in…, or Offline with Retry and Log out for that account).
- New `src/ui/switcher.rs`:
  - `render_switcher(items, adding, on_select, on_log_out, on_add)`.
  - `SwitcherItem { id, name, host, avatar, status, selected, unread, mentions }`.
  - `AccountStatus { Loading, SignedIn, Offline, SignedOut }`. Offline and signed-out accounts are drawn at 50% opacity, and the tooltip reads "Name — host (offline / signed out)".
  - Right-click shows "Log Out". **+** sits at the bottom.
  - `host_of(instance)`.
- `src/ui/chat.rs`:
  - `avatar`, `rail_button` and `mention_badge` are `pub(crate)`.
  - `rail_button`'s menu parameter becomes `Option<(&'static str, MenuAction)>` (it was "Mark as read" only).
  - New `me_summary() -> Option<(name, avatar_url)>`.
- `src/ui/login.rs`:
  - `cancellable(bool)` builder, `LoginEvent::Cancelled`, and a Cancel button on the credentials step.
  - The title reads "Add an account" when cancellable.
  - `LoginEvent::LoggedIn.session` is boxed (clippy `large_enum_variant`).
- `src/session.rs`: `forget(store, key)` clears a token without contacting the server.
- `src/accounts.rs`: `set_name(id, name) -> bool`, with a test.
- `src/main.rs`: an **Add Account…** menu item (`AddAccount` action). Log Out calls `log_out_current`.
- `src/ui/mod.rs`: `pub mod switcher`.
- `README.md`: new "Multiple accounts" section, and a note that each account has its own Keychain item.

### Divergences
- **The switcher is drawn by `AppRoot`, not a separate entity.** `render_switcher` is a function, which is simpler, because `AppRoot` owns all the state.
- **Name on fresh login.** A freshly logged-in account's `name` is filled in from `Ready` (`sync_outside` with `set_name`), not only on the next restore. The plan left it unknown until relaunch.
- **List-logic tests.** These were planned for `tests/accounts.rs`. They're unit tests in `src/accounts.rs` (chunk 1, plus `set_name`), covering removal from the middle, last and only positions, dedupe, and keeping the active account.
- **Rejected sessions.** These used to also try a server logout. Now they only forget the token, since the server has already ended the session.

### Verification Performed
- `cargo test`: 238 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after boxing the login event's `Session`.
- `cargo fmt --check`: clean.

### Manual Validation
Deferred to the end at the user's request (see chunk 3).

## Chunk 5: Switcher unread badges and labelled notices

### Changes Made
- `src/store.rs`: `account_summary() -> (bool, usize)` folds `servers()` into `home_summary()`. It shows a dot if Home or any server shows one, and sums all mentions. Muted channels add mentions but no dot, matching the rail.
- `src/ui/chat.rs`:
  - `account_summary()` passthrough.
  - `account_label: Option<String>`, with `set_account_label()`.
  - `labelled()` appends ` · {label}` to toast and banner titles.
- `src/ui/switcher.rs`:
  - `notice_labels(&[(host, name)])`: no labels with fewer than 2 signed-in accounts, otherwise the host, or `host · name` when several accounts share the host.
  - Unit tests for `notice_labels` and `host_of`. The test module imports items by name, because `super::*` brings in GPUI's `test` attribute through `gpui_kit::*`, which fails with "recursion limit reached while expanding `#[test]`".
- `src/ui/app_root.rs`:
  - The switcher items' `unread` and `mentions` come from each chat view's `account_summary()`. The account on screen shows neither, because `render_switcher` hides them when selected.
  - `sync_outside` computes `notice_labels` over the signed-in accounts and sets each view's label.
- `tests/store_unreads.rs`:
  - `account_summary_covers_home_and_servers`
  - `account_summary_is_clear_when_everything_is_read`
  - `an_empty_store_has_nothing_to_show`
- `tests/notify_rules.rs`: the muted-channel test also checks that the account summary counts the muted mention.

### Divergences
- **Labels for accounts sharing a host.** The plan said `@username`. The implementation uses `host · display name`, because the account entry stores the display name, which is also what the switcher shows.

### Verification Performed
- `cargo test`: 245 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean.

### Manual Validation
Deferred to the end at the user's request (see chunk 3).
