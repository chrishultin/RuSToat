# Plan: Multiple accounts and instances (RuSToat)

## Summary
Let the user be signed in to several Stoat accounts at once, on the official instance and on self-hosted ones, possibly with several accounts on the same instance. They switch between accounts the way Slack switches workspaces. Today the client talks to one instance, chosen with `STOAT_API_URL`, and holds one session, so anyone who uses a self-hosted server and stoat.chat has to quit and relaunch to switch.

## Approach
**One `ChatView` per account, and `AppRoot` manages the list.** A `ChatView` (`src/ui/chat.rs`) is already a complete session: its own `StoatClient`, `ChatStore`, events websocket, `MediaUrls`, panes, acks and banners. The websocket URL and the file and proxy servers come from that client's own `GET /`, so a self-hosted instance needs nothing special. `AppRoot` (`src/ui/app_root.rs`) changes from "one client, one `Screen`" to "a list of accounts, each with its own state, plus which one is shown". A narrow switcher column on the far left shows one icon per account and a **+** button.

**Every account stays connected,** not just the one on screen, so unreads, notifications and the Dock badge cover all of them. Hidden `ChatView`s aren't rendered, but they keep processing events.

**Three things are app-wide today and have to move up to `AppRoot`:**
- **The banner-click handler.** GPUI's `cx.on_system_notification_response` keeps a single handler ("subsequent registrations replace the handler", `gpui-pre-0.3.11/src/app.rs:1994`). If each `ChatView` registered its own, only the last account's clicks would work. `AppRoot` registers it once and routes clicks by a tag that names the account and the channel.
- **The Dock badge.** It's set inside `ChatView::render` today. Hidden views don't render, and several views would overwrite each other. `AppRoot` sums `attention_count()` across accounts instead.
- **Marking messages read.** `maybe_ack_current` only checks `window.is_window_active()`, and it also runs from each view's window-activation observer. Hidden views would ack whatever channel they last had open. Each `ChatView` gets an `active` flag that gates acks and counts as "not viewing" for `notify::should_notify`.

**Where accounts are stored.** The account list goes in a small JSON file:
- Location: `dirs::config_dir()/chat.stoat.rustoat/accounts.json` (on macOS, `~/Library/Application Support/…`).
- Each entry holds the instance, user ID, display name, and the entry's Keychain account name. The file also remembers which account was active.
- Tokens stay in the Keychain. New accounts use the Keychain account name `"{api_url}#{user_id}"`, so two accounts on one instance get separate items.
- **Migration:** the existing item, keyed by bare API URL, is adopted as-is. It becomes a first entry whose Keychain name is the bare URL and whose user ID is filled in after its first successful restore. No token moves, so a migration while offline can't lose the session.

**Choosing the instance at login.** The login screen gets an instance field, defaulting to `STOAT_API_URL` or `https://api.stoat.chat`. A new `api::resolve_instance(input)` works out the API base URL from what the user typed:
1. Add `https://` if no scheme was typed. `http://` is kept only when typed explicitly, e.g. for a local dev server.
2. Remove a trailing slash.
3. Try `GET {url}/` and parse it as a `RootConfig`.
4. If that fails, try `GET {url}/api`, because self-hosted instances usually serve the API there.

**Rejected:**
- **Merged rail** (every instance's servers in one rail, all DMs together): `ChatView` and `ChatStore` would have to become multi-session. The user chose the switcher.
- **One window per account:** it fragments notifications and the Dock badge.
- **Storing the account list in the Keychain:** each item is another "Always Allow" prompt after every dev rebuild, and the list isn't secret.
- **Moving the legacy token to a new Keychain name during migration:** it needs the user ID first, which fails offline.

## Assumptions
1. **Logging out of an account removes it from the switcher.** Log Out (menu, user panel, switcher context menu) ends that account's session and deletes its entry and Keychain item. Afterwards the next account in the list is shown, or the login screen with no Cancel if none are left.
2. **A rejected session doesn't remove the account.** When Stoat rejects a session (`ChatEvent::SessionRejected`, or `Restored::NeedsLogin` for a known entry), the account stays in the switcher, dimmed and marked "Signed out". Selecting it shows the login screen with that account's instance filled in and locked, and the message "Your session has ended. Please log in again."
3. **Logging in again to an account that's already in the list:** same instance and user ID.
   - If that account is signed in, the new session is ended on the server (`client.logout()`), and the app switches to the existing account.
   - Otherwise (signed out, or offline), the new token replaces the saved one and the account connects.
4. **The switcher is always visible once at least one account exists,** so **+** can be found. It's hidden on the very first login screen. Flipping it to "only with 2+ accounts" would be a one-line change if it feels wasteful.
5. **The Stoat menu gets "Add Account…"**, and "Log Out" applies to the account on screen. There are no shortcuts for switching accounts yet (see Open Questions).
6. **Banner tags become `"{channel} {account id}"`.** The account ID is the entry's Keychain name; channel IDs are ULIDs, so they contain no spaces. A tag with no space (a banner left over from before this change) routes to the active account.
7. **Notices name the account only when more than one is signed in.** The instance host (or `@username` if two accounts share a host) is appended to the toast or banner title. With one account they look as they do today.
8. **An instance's API version is checked but doesn't block login.** If its `RootConfig.version` isn't on 0.16.x, a warning is logged. The `revolt-*` crates are pinned to 0.16.2, so a much newer or older server may misparse.
9. **TLS uses the system trust store, as now.** Self-signed certificates that aren't trusted in the macOS Keychain will fail to connect, and that's out of scope.

## Open Questions
- *(Deferred)* Keyboard shortcuts to switch accounts (e.g. ⌃1…⌃9, as in Slack). They're cheap to add after chunk 4.
- *(Deferred)* Reordering accounts in the switcher. The order is the order they were added.
- *(Deferred)* Per-account "Mark all as read" in the switcher's context menu.

## Plan

### Chunk 1: Account list and per-account tokens
**Goal:** Store accounts as a list with per-user Keychain items, and migrate the existing saved session, while the UI still shows a single account. Everything later builds on this storage.
**Files likely touched:**
- `Cargo.toml`: `dirs = "6"`, already in the tree via another dependency; `tempfile` as a dev-dependency, also already in the tree.
- New `src/accounts.rs`:
  - `AccountEntry { id /* Keychain account name */, instance, user_id: Option<String>, name: Option<String> }`
  - `AccountList { accounts, active: Option<String> }`, with pure functions:
    - `add_or_update`: dedupe on `(instance, user_id)`
    - `remove(id) -> next active`
    - `set_active`
    - `find(instance, user_id)`
  - `AccountsFile` trait with `FileAccounts` (JSON at `dirs::config_dir()/chat.stoat.rustoat/accounts.json`, written atomically via temp file plus rename) and `MemoryAccounts` for tests, mirroring `TokenStore`/`MemoryStore`
  - `keychain_name(instance, user_id)`
  - `migrate_legacy(list, tokens, default_instance)`: if the list is empty and the Keychain has a bare-URL item for `default_instance`, add an entry with `id = instance` and `user_id: None`
- `src/session.rs`: `restore`, `remember` and `log_out` take the Keychain name explicitly instead of deriving it from `client.base_url()`. `Restored::SignedIn` already returns the `User`, which lets `AppRoot` fill in `user_id` and `name`.
- `src/lib.rs`: `pub mod accounts;`
- `src/ui/app_root.rs`:
  - load the list and migrate at startup
  - restore the active account, or the first one
  - on login, `add_or_update`, save the file, and remember the token under `keychain_name`
  - on log out, remove the entry
  - still one account on screen
- `tests/session_restore.rs`: updated for the explicit-name API
- New `tests/accounts.rs`
- `README.md`: the "Your session and the Keychain" section, with the new Keychain naming and the accounts file

**Depends on:** None
**Risks:**
- Getting the migration wrong logs the user out. Keep the legacy item untouched until it's explicitly logged out, and cover the migration with tests.
- A write to the accounts file can fail (permissions, full disk). Log it and carry on, as `remember` does for the Keychain. The session then just won't be restored next launch.
**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`. New tests cover:
- the list round-trips through `FileAccounts` in a temp dir
- adding the same `(instance, user_id)` twice keeps one entry
- `remove` picks the right next active account
- migration adopts a legacy `MemoryStore` token and is a no-op when the list isn't empty
- `restore` with a migrated entry (`user_id: None`) succeeds and reports the user

**Manual check:** With an existing saved session from before this change, `cargo run` signs in without a login prompt. `accounts.json` then contains one entry with your user ID filled in, and Log Out followed by a relaunch shows the login screen.
**Shippable when:** The app behaves exactly as before for a single account, existing sessions migrate silently, and tokens are stored per user.

### Chunk 2: Choose the instance at login
**Goal:** Log in to any Stoat instance from the login screen, not just through `STOAT_API_URL`.
**Files likely touched:**
- `src/api/client.rs` (or a new `src/api/instance.rs`):
  - `normalize_instance(input) -> Result<Url>`
  - `async resolve_instance(input) -> Result<(StoatClient, RootConfig), ApiError>`, which tries `{url}/` and then `{url}/api` and logs a warning when the version isn't 0.16.x
  - re-export from `src/api/mod.rs`
- `src/ui/login.rs`:
  - an "Instance" field above Email, prefilled with the default and shown with the resolved host under it
  - `LoginView::new(default_instance, locked: bool, …)`
  - the instance is resolved on submit, before `login`, with errors such as "Couldn't find a Stoat server at …"
  - `LoginEvent::LoggedIn` carries the resolved `StoatClient` (or base URL) along with the `Session`
- `src/ui/app_root.rs`: use the instance from the login event when saving the account, instead of `self.client`
- New `tests/api_instance.rs`: wiremock tests
- `README.md`: the "Self-hosted instances" section, with the field (`STOAT_API_URL` now only sets its default)

**Depends on:** Chunk 1 (accounts record their own instance)
**Risks:**
- Self-hosted layouts vary. Some put the API on a subdomain (`api.example.com`), which `/api` discovery won't find. The user can still type the full API URL, so mention that in the error text.
- The 2FA step must keep using the resolved client. Store it on the view, not only in the event.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`. New tests cover:
- `stoat.example.com` becomes `https://stoat.example.com`
- `http://localhost:14702/` keeps `http` and loses the trailing slash
- a mock serving `RootConfig` at `/` resolves to the bare URL
- a mock serving HTML at `/` and `RootConfig` at `/api` resolves to `/api`
- a mock serving neither returns an error

**Manual check:** Log out, enter a self-hosted instance (or the official one by bare host, `api.stoat.chat`), log in, and confirm that servers and images load. Relaunch and confirm it restores that instance with no `STOAT_API_URL` set.
**Shippable when:** A user can log in to a self-hosted instance from the UI, and it's remembered across launches.

### Chunk 3: App-level notification routing and Dock badge
**Goal:** Move the app-wide pieces (banner-click handling, the Dock badge, and when to ack) out of `ChatView` and route them by account, with no visible change for one account. Without this, a second account breaks banners and the badge.
**Files likely touched:**
- `src/notify.rs`: `banner_tag(account_id, channel) -> String` and `parse_banner_tag(tag) -> (Option<&str>, &str)`, with tags that have no separator handled as legacy (assumption 6)
- `src/ui/chat.rs`:
  - `ChatView::new` takes `account_id` and no longer calls `request_permission` or `on_system_notification_response`
  - `show_banner`, `dismiss_read_banners` and the toast ID use `banner_tag`
  - new `active: bool` and `set_active(bool, window, cx)`:
    - inactive cancels `pending_ack`
    - `maybe_ack_current` returns early when inactive
    - `Viewing.channel` is `None` when inactive
  - the Dock badge code leaves `render`; `pub fn attention_count(&self) -> usize` replaces it
  - toast `on_click` also emits a new `ChatEvent::Activate`
- `src/ui/app_root.rs`:
  - call `banners::request_permission()` once
  - register `on_system_notification_response` once, routing via `parse_banner_tag` to the matching account's view (activate the account, then `open_channel`)
  - `cx.observe` the chat view to recompute the summed Dock badge (only touching AppKit when it changes)
  - handle `ChatEvent::Activate`

**Depends on:** Chunk 1 (account IDs)
**Risks:**
- `cx.observe` fires on every `ChatView` notify, which is every event. Keep the observer to summing integers.
- If the Dock badge falls behind, it's because a store change didn't call `cx.notify()`. Today render catches that; check the badge manually.
- Old banners in Notification Center have the old tag format. Assumption 6 handles them.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`. New unit tests cover `banner_tag`/`parse_banner_tag`: round trip, legacy tag, and a channel ID with no account.
**Manual check:** `scripts/run-bundled.sh`, then send yourself a DM from another client while RuSToat is in the background. A banner appears, clicking it opens the DM, reading it removes the banner, and the Dock badge goes up and then clears. With RuSToat in front, a toast appears instead, and clicking it opens the channel.
**Shippable when:** Banners, toasts, the badge and auto-ack behave exactly as before, now driven from `AppRoot`.

### Chunk 4: Several accounts at once, with a switcher
**Goal:** The feature itself. Add more accounts, keep them all connected, and switch between them from a left-hand column.
**Files likely touched:**
- `src/ui/app_root.rs`, which is mostly a rewrite:
  - `Screen` becomes per-account `AccountState { Restoring, Offline { token, error }, SignedOut { login }, Chat { view, token } }`
  - the root holds `accounts: Vec<Account>` and `shown: Shown { Account(id) | Adding { login, cancellable } }`
  - at startup, every entry is restored concurrently (one `spawn_io` each)
  - switching calls `set_active(false)` on the old view and `set_active(true)` on the new one, then saves `active` to the file
  - logging in uses `add_or_update` (assumption 3)
  - Log Out and session rejection follow assumptions 1 and 2
  - the Dock badge sums across all `Chat` accounts
  - banner routing finds the account by ID
- New `src/ui/switcher.rs`:
  - a ~64px column left of the server rail, with one avatar per account (`ChatStore::user_avatar_url` for the user, initials while loading) and a tooltip "Display name — host"
  - a selected marker, as on the rail
  - dimmed with a small icon when Offline or Signed out
  - a **+** button at the bottom
  - right-click → "Log Out"
- `src/ui/login.rs`: an optional Cancel button (used when adding an account while others exist), and the locked-instance mode from chunk 2 for re-login
- `src/ui/chat.rs`: `ChatEvent::LogOutRequested` already goes up to `AppRoot`, which now applies it to that view's account. Expose `pub fn me_summary(&self) -> (name, avatar_url)` for the switcher.
- `src/main.rs`: an "Add Account…" menu item and action, routed to `AppRoot::add_account`. Log Out stays on the active account.
- `src/ui/mod.rs`: `pub mod switcher;`
- `README.md`: a new "Multiple accounts" section

**Depends on:** Chunks 1–3
**Risks:**
- **The largest chunk.** If it grows, ship the `AppRoot` state refactor first (still one account on screen), then the switcher UI.
- Several `ChatView`s in one window all `observe_window_activation`. The `active` gate from chunk 3 is what keeps hidden views from acking, so confirm it manually.
- During development, each account's Keychain item gets its own "Always Allow" prompt after a rebuild. That's expected; note it in the README.
- Two accounts on the same instance means two websockets from the same IP. That should be fine, but watch for rate limiting on `Ready`.
- Logging out the account on screen while its `ChatView` has requests in flight: dropping the view cancels them (`IoTask` drop aborts). That's intended.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, plus unit tests in `tests/accounts.rs` for the pure list logic that drives the UI: next active after removal (middle, last, only), dedupe on re-login, and keeping `active` when an unrelated account is removed.
**Manual check:**
1. Signed in to stoat.chat, press **+**, log in to a second account (a self-hosted instance, or a second stoat.chat user). Both icons appear, and the new one is shown.
2. Switch back and forth. Each keeps its own server and channel selection and scroll position.
3. From another client, DM the hidden account. Its message isn't marked read (the other client still shows it unread), and a toast or banner appears. Clicking it switches accounts and opens the DM.
4. Quit and relaunch. Both accounts restore, and the last one shown is shown again.
5. Revoke one account's session from another client. That account is marked "Signed out" in the switcher, the other keeps working, and re-login restores it.
6. Log out of one account. It disappears, and the other is shown.

**Shippable when:** Two or more accounts across one or more instances work side by side, survive a relaunch, and never ack or notify for the wrong account.

### Chunk 5: Switcher unread badges and labelled notices
**Goal:** Make activity on hidden accounts visible: unread dots and mention counts on the switcher icons, and notices that say which account they're for.
**Files likely touched:**
- `src/store.rs`: `pub fn account_summary(&self) -> (bool /* any unread */, usize /* mentions */)`, built from the existing `home_summary` and the per-server `unread` and `mentions` with muted servers and channels excluded, consistent with the rail
- `src/ui/switcher.rs`: draw the dot and the `mention_badge` (reuse `chat.rs`'s helper by making it `pub(crate)`). The account on screen shows no dot, as Slack does.
- `src/ui/chat.rs`: `set_account_label(Option<String>)`. `show_toast` and `show_banner` append ` · {label}` to the title when it's set.
- `src/ui/app_root.rs`: once 2+ accounts are signed in, set the label on each view to the host, or `@username` for accounts that share a host. Clear it when back to one.
- `tests/store_unreads.rs`: tests for `account_summary`

**Depends on:** Chunk 4
**Risks:**
- Re-rendering the switcher on every event in every account. The summaries are cheap, but if `servers()` turns up in profiles, cache the summary per view and refresh it on store change.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`. New `account_summary` tests cover: unread in a DM counts; a mention in a server counts; a muted server's unread doesn't add a dot but its mentions still count (matching the rail, assumption 3 of the unreads plan); and an empty store gives `(false, 0)`.
**Manual check:** With two accounts, DM and @mention the hidden one. Its switcher icon shows a dot and then a mention count. The toast title ends with that account's host. Switching to it and reading clears both.
**Shippable when:** You can tell which hidden account has activity, and which account each notice belongs to.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings` (from the README; run after every chunk)

### Unit Tests
- `src/accounts.rs` / `tests/accounts.rs`:
  - `keychain_name` format
  - `AccountList` operations: add/dedupe, remove → next active, set_active, find by `(instance, user_id)`
  - JSON round trip through `FileAccounts` in a `tempfile` dir, including a missing file (empty list) and a corrupt file (logged, treated as empty, not overwritten until the next save)
  - `migrate_legacy`
- `src/notify.rs`: `banner_tag`/`parse_banner_tag`, including legacy tags
- `normalize_instance`: scheme added, `http` kept when explicit, trailing slash removed, junk rejected
- `ChatStore::account_summary`

### Integration Tests
- `tests/session_restore.rs`, updated: restore, remember and log out with explicit Keychain names. Two accounts on the same mock instance don't see each other's tokens. A legacy bare-URL item restores through a migrated entry.
- `tests/api_instance.rs`: `resolve_instance` against wiremock, with root at `/`, root at `/api`, neither, and an unexpected version (resolves with a warning).

### Manual Tests
- **Upgrade path:** with a pre-change saved session, launch, get signed in with no prompt, and check `accounts.json`.
- **Add a self-hosted instance** by bare host, and by full API URL.
- **Two accounts on stoat.chat** at once.
- **Background account** notifications, both bundled (banners) and with `cargo run` (toasts). Clicking switches accounts.
- **Dock badge** totals across accounts and clears as each is read.
- **Relaunch** restores all accounts and the last one shown.
- **Offline at launch:** each account shows Offline with Retry, independently.
- **Session revoked elsewhere:** the account shows Signed out, and re-login works.
- **Log out** of the shown account, a hidden account (via right-click), and the last account (back to first-run login with no Cancel).

## Definition of Done
- [ ] A pre-existing saved session signs in after upgrading with no login prompt, and its Keychain item isn't renamed.
- [ ] The login screen has an instance field. `stoat.example.com` resolves to the instance's API at `/` or `/api`, and an unreachable instance shows an error without crashing.
- [ ] Two accounts on different instances, and two accounts on the same instance, can be signed in at once, each with its own Keychain item (`{api_url}#{user_id}`).
- [ ] The switcher shows one icon per account and a **+** button. Clicking an icon shows that account's last server and channel.
- [ ] Hidden accounts stay connected. Their messages produce toasts or banners, and are never marked read while hidden.
- [ ] Clicking a banner or toast for a hidden account switches to it and opens that channel.
- [ ] The Dock badge is the sum of every account's attention count.
- [ ] Relaunching restores every saved account and shows the last active one.
- [ ] Logging out removes only that account. A rejected session marks only that account Signed out.
- [ ] Switcher icons show unread dots and mention counts for hidden accounts. Notice titles name the account when 2+ are signed in.
- [ ] The README documents multiple accounts, the instance field, the accounts file and the Keychain naming.
- [ ] `cargo test && cargo clippy --all-targets -- -D warnings` passes.
