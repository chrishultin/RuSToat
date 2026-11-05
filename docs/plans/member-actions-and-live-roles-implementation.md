# Implementation Record: Member actions and live role edits (milestone 7)

_Plan: docs/plans/member-actions-and-live-roles.md_
_Started: 2026-11-05_

**Supervision:** continuous. The run stops for significant divergences and for test failures that can't be explained. After chunk 1, the user chose to batch the manual checks: each chunk is committed once its automatic checks pass, and every chunk's manual check is walked through together at the end.

**Pre-existing dirty files, kept out of every commit:** `.gitignore` (the same Riptide entry added twice, by tooling) and `.shadow/` (untracked). The plan file was committed on its own first (`797dfd6`), as milestone 6's was.

**Formatting:** `cargo fmt --check` is part of the plan's full-suite command, but it already fails on a clean `HEAD` (`3b89e68`). The drift predates this milestone and is spread over many files (`store.rs`, `chat.rs`, `client.rs`, `events.rs`, `emoji.rs`, `markdown.rs`, `complete.rs`…). The packaging milestone set the precedent: new files go through `rustfmt`, and new code in existing files is laid out as `rustfmt` wants it, but existing files aren't reformatted wholesale. Each chunk checks that the number of `rustfmt --check` hunks in every file it touched is no higher than at `HEAD`.

## Chunk 1: Live role and server edits

### Changes Made
- `src/api/events.rs`:
  - New `ServerEvent` variants: `ServerUpdate { id, data: Box<PartialServer>, clear: Vec<FieldsServer> }`, `ServerRoleUpdate { server, role_id, data: Box<PartialRole>, clear: Vec<FieldsRole> }`, `ServerRoleDelete { server, role_id }` and `ServerRoleRanksUpdate { server, ranks }`.
  - Decoding for each in `decode()`, with `#[serde(default)] clear` as in `ChannelUpdate`. The role is the top-level `role_id`, never `data`'s ID.
  - `session()` forwards the four new events to the UI.
  - Tests: the "unknown event" test now uses `WebhookCreate`, since `ServerRoleUpdate` is decoded now. New parse tests:
    - `ServerUpdate`: with `clear`, and a `default_permissions`-only update.
    - `ServerRoleUpdate`: a role-creation payload carrying the whole role, an edit with `clear: ["Colour"]`, and a permissions-only edit.
    - `ServerRoleDelete` and `ServerRoleRanksUpdate`.
- `src/store.rs`:
  - `apply_server_update(id, data, clear)` merges with `Server::apply_options`, then clears `Description`, `Categories`, `SystemMessages`, `Icon` and `Banner` as listed.
  - `apply_role_update(server, role_id, data, clear)`:
    - A known role is merged with `Role::apply_options`, and `Colour` and `Icon` are cleared as listed.
    - An unknown role is built from the partial. `name` is required; without it the update is logged and ignored (Assumption 9). `permissions` defaults to `{a:0,d:0}`, `rank` to the role count, and `hoist` to false.
    - The role's `id` is always set to the map key.
  - `apply_role_delete(server, role_id)` removes the role from `server.roles`, from the `roles` of every cached member of that server, and from the `role_permissions` of that server's channels.
  - `apply_role_ranks(server, ranks)` sets `rank = index` for each listed role.
  - New `shows_channel(channel)`: a server channel is shown if the user can view it (`permissions::can_view`), or while the user's own membership of its server is unknown (Assumption 8). Other channels are always shown. `server_channels` (both uncategorized channels and categories) and the rail's unread and mention counts in `servers()` both use it.
- `src/permissions.rs`: the module doc no longer says role edits only arrive at reconnect. It lists the live events that are applied.
- `src/ui/chat.rs`:
  - `on_connection_event` applies each of the four events. When the store reports a change, it calls a new `server_permissions_changed(server)`, which does four things:
    1. `forget_member_lists()`.
    2. `sync_pane` on every loaded channel of that server, since `can_pin` on rows depends on permissions.
    3. `drop_stale_selection()`, then `remember_places()` if a selection was dropped.
    4. `ensure_current_loaded`.
  - `drop_stale_selection` now also drops selections of channels that `shows_channel` hides, so a channel that becomes hidden while open is replaced by the server's first visible channel.
- `tests/store_roles.rs` (new, 11 tests):
  - A rename keeps the colour. This confirms that `apply_options` leaves a field alone when the partial's field is `None`, which was a plan risk.
  - A colour change, then a colour clear (Bob falls back to his next role's colour).
  - Hoisting off and on.
  - The whole pipeline: a created role → hoisted and coloured → given to a member through `ServerMemberUpdate` → its section and colour show → a ranks update moves it to the top → a delete puts the member back under Online.
  - An unnamed unknown role, and an unknown server, are ignored.
  - A new role without a rank goes last.
  - A delete strips members and overrides.
  - A ranks update changes both the sections and which colour wins.
  - A role permission edit flips `can_pin` and then `can_view`.
  - `default_permissions` through `ServerUpdate` flips `can_view`.
  - A `ServerUpdate` merges and clears fields.
  - `server_channels` hides channels after a role edit denies ViewChannel, and shows them again when it's undone.
  - The `Ready` fixture guard: every server channel in `tests/fixtures/ready.json` is still shown and listed.
- `tests/fixtures/ready.json`: both servers' `default_permissions` changed from `0` to `2276038022144`, which is `DEFAULT_PERMISSION_SERVER`.

### Divergences
- **The `Ready` fixture's servers had `default_permissions: 0`**, which no real server has. With the new `server_channels` filter, the permission maths correctly worked out that the user (who doesn't own `srv-alpha`) couldn't view any of its channels. Real `Ready` payloads only contain channels the user can view, so the fixture was unrealistic, not the maths. The fix sets both servers to Stoat's default server permissions. Tests that read the fixture (pins, unreads, notifications, completion) still pass: the new value grants no Manage Messages, so `can_pin` stays false in server channels. This counts as minor: a file outside the plan's list, with the same approach and outcome.
- **`servers()` uses `shows_channel` too**, so the rail's unread dot and mention count ignore channels a role edit has hidden. The plan didn't list this, but it keeps the rail consistent with the channel list.
- **`ensure_current_loaded` runs after every server or role change**, not only when a selection was dropped. When the open channel is the server's default first channel rather than a picked one, hiding it makes another channel current without any selection being dropped. The call does nothing if the channel is already loaded.
- **Look-ahead:** chunk 2's `add_server` relies on the "membership unknown" exception in `shows_channel`, which exists now as planned. No other chunk is affected.

### Verification Performed
- `cargo test --test store_roles`: 11 passed.
- `cargo test --lib events`: 24 passed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test` (full): 505 passed, 0 failed.
- `rustfmt --check` hunks in the touched files are the same as at `HEAD` (`events.rs` 3, `store.rs` 15, `chat.rs` 19, `permissions.rs` 0). The new `tests/store_roles.rs` is rustfmt-clean.

### Manual Validation
Deferred to the end of the milestone (manual checks are batched).

## Chunk 2: Joining and deleted servers, live

### Changes Made
- `src/api/events.rs`:
  - New `ServerCreate { server: Box<Server>, channels: Vec<Channel>, emojis: Vec<Emoji> }`. Channels and emojis are read with `lenient_list`, so an unparseable channel costs only itself. `voice_states` is ignored.
  - New `ServerDelete { id }`.
  - Both are forwarded to the UI.
  - Parse tests: a `ServerCreate` with an unknown channel type skipped, one with no lists, and `ServerDelete`.
- `src/api/client.rs`: `fetch_member(server, user) -> Member` (`GET /servers/{s}/members/{u}`).
- `src/store.rs`: `add_server(server, channels, emojis) -> bool` (true if new).
  - It inserts or replaces the server and adds each channel (through `add_channel`) and emoji.
  - A new server is appended to `server_order` (Assumption 10).
  - Histories and members aren't touched, so a repeated `ServerCreate` keeps loaded history.
- `src/ui/chat.rs`:
  - On `ServerCreate`: `add_server`, then, if the user's own membership isn't known yet, `fetch_own_member`. That fetches `GET /servers/{id}/members/{me}`, applies it with `apply_member_join`, and calls `server_permissions_changed`, so the chunk 1 channel filter now applies. If the server was left before the membership arrived, the result is dropped. A failure is logged and left for the next reconnect.
  - On `ServerDelete`: the existing `leave_server`.
- `tests/store_servers.rs` (new, 4 tests):
  - A joined server goes at the end of the rail, with its channels and emojis.
  - Channels aren't filtered until the membership is known, and a role-restricted channel disappears once it is.
  - A repeated `ServerCreate` replaces the server (renamed, not duplicated) and keeps history.
  - `ServerDelete` → `remove_server` removes the channels.
- `tests/api_users.rs` (new, 2 tests): `fetch_member`'s path and token, parsing a bare `Member`, and a 404 → `ApiError::Api(NotFound)`.

### Divergences
None. The rail-order risk was accepted as planned: the server appears last, and the saved ordering isn't re-read live.

### Verification Performed
- `cargo test --test store_servers --test api_users`: 6 passed.
- `cargo test --lib events`: 26 passed.
- `cargo test` (full): 513 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --check` hunks are unchanged from `HEAD` in `events.rs`, `store.rs`, `chat.rs` and `client.rs`. The new test files are rustfmt-clean.

### Manual Validation
Deferred to the end of the milestone (manual checks are batched).
