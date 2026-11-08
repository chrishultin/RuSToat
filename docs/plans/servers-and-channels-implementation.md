# Implementation Record: Servers and channels (RuSToat milestone 19)

_Plan: docs/plans/servers-and-channels.md_
_Started: 2026-11-07_

Pre-existing uncommitted changes, kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt `Diff in` counts (2026-11-07): `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15. Zero in `src/permissions.rs`, `src/main.rs`, `src/ui/actions.rs`, `src/ui/invites.rs`, `src/invites.rs`, `src/ui/profile_card.rs`, `tests/store_servers.rs`, `tests/permissions.rs` and `tests/store_ready.rs`.

Supervision: continuous, unattended overnight; manual checks batched at the end.

## Chunk 1: Create a server

### Changes Made
- `src/api/client.rs`: `create_server(name) -> CreateServerLegacyResponse` (`POST /servers/create`), with a private body holding only `name`. `CreateServerLegacyResponse` added to the hand-formatted import list.
- `src/server_edit.rs` (new, pure):
  - `NAME_MAX` (32) and `AUP_URL`
  - `valid_name(text)`: trimmed, 1–32 characters by `chars().count()`
  - `create_server_error_text(&ApiError)`: `TooManyServers` and `CantCreateServers` in their own words, else `profile_edit::error_text` with "Couldn't create the server. Try again."
  - 2 unit tests.
- `src/ui/server_dialogs.rs` (new): `CreateServerDialog` and `CREATE_SERVER_DIALOG_WIDTH` (420).
  - A "Server name" field with a "{n}/32" counter (red past 32), the error line, Close and Create (`loading` while creating, disabled until the name is valid). Enter creates.
  - On the official instance (`web_app` is `https://stoat.chat`), "By creating this server, you agree to the Acceptable Use Policy.", with the policy's name a link (`InteractiveText`) to `https://stoat.chat/aup`.
  - Success hands the response to `ChatView::server_created` and closes the dialog; no toast.
- `src/ui/chat.rs`:
  - `server_added(server, channels, cx)`: `add_server(server, channels, [])`, then `fetch_own_member` if the membership isn't known. `invite_joined`'s server branch now uses it.
  - `open_create_server(window, cx)` (title "Create a server", the field focused) and `server_created(response, cx)`, which adds the server and opens its first channel in sidebar order (General), or the server if there's none.
  - `fetch_own_member` skips a server whose fetch is already on its way (`own_member_fetches`), so the response and its `ServerCreate` ask once. This applies to invite joins too.
  - The rail's "+" is now `rail-add` (tooltip "Create or join a server"). A left click opens a menu beside the rail: "Create a server…" (`Plus`) and "Join a server…" (`LogIn`).
    - `rail_button`'s round face is factored out as `rail_icon(id, content, tooltip, selected, cx) -> Stateful<Div>`; `rail_button` is unchanged for callers.
    - `RailMenuButton` wraps that face so it can be a `DropdownMenu` trigger (it implements `Selectable`, `Styled`, `InteractiveElement` and `IntoElement`), and keeps the hover look while the menu is open. The menu is anchored `Anchor::LeftCenter`, which opens it to the right of the button.
- `src/ui/actions.rs`: the `CreateServer` action. `src/main.rs`: "Create Server…" before "Join Server…" in the Stoat menu, routed through `root.shown_chat()` and doing nothing without one.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- `tests/api_servers.rs` (new): create sends exactly `{"name":"Lounge"}` with the token and parses a server with one "General" channel and no categories; a 400 `TooManyServers { max: 100 }` stays an `Api` error.
- `tests/store_servers.rs`: a create response (no `categories` key) then its `ServerCreate`, and the reverse. Both give one rail entry, at the end, with General.

### Divergences
- **The "+" isn't a ghost `Button`.** gpui-component's `DropdownMenu` needs a `Selectable` trigger, which a `div` isn't, so (rather than the plan's fallback of a `Button` styled like the rail) a small `RailMenuButton` wrapper makes the rail's own face selectable. The "+" looks and behaves exactly like the other rail buttons, including the hover morph from circle to rounded square.
- **`fetch_own_member` now dedupes in-flight fetches.** The plan's Risks wanted one fetch for the response and the event. Before, both could start one, because the membership isn't known until the first returns.
- **The dialog's title is "Create a server"** (the web client says "Create server"), to match the menu item and "Join a server".
- **The test fixtures carry `approximate_member_count`,** which `revolt-models`' `Server` requires. Stoat sends it in the real response.

### Verification Performed
- `cargo test --lib server_edit`: 2 passed. `cargo test --test api_servers --test store_servers`: 2 and 8 passed. The first run of `creating_a_server_sends_only_its_name` failed on a fixture missing `approximate_member_count`, which was fixed in the fixture.
- `cargo test`: 906 passed, 0 failed (900 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts:
  - `src/api/client.rs` 3 and `src/ui/chat.rs` 17: unchanged, after hand-formatting `create_server` and moving the new `use` into order.
  - `src/main.rs` 0 and `src/ui/actions.rs` 0.
  - New files (`src/server_edit.rs`, `src/ui/server_dialogs.rs`, `tests/api_servers.rs`) 0, and `tests/store_servers.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Click the rail's "+": a menu with "Create a server…" and "Join a server…" opens to the right of the rail, not clipped by the window edge (try with the window short enough that the "+" is near the bottom).
2. Create a server: it appears at the end of the rail, General opens, and you can send a message.
3. Stoat → Create Server… opens the same dialog. With the login view on screen, it does nothing.
4. "+" → Join a server… still opens Join a server, and joining still works.
5. The Create button is disabled for an empty or blank name and past 32 characters; the counter shows the count and turns red past 32.
6. On stoat.chat, the Acceptable Use Policy line shows and its link opens `https://stoat.chat/aup` in the browser. On another instance, it doesn't show.
7. Light and dark themes: the dialog, the counter and the link are readable.

## Chunk 2: Delete a channel, and follow deletions made elsewhere

### Changes Made
- `src/api/events.rs`: `ServerEvent::ChannelDelete { id }`, parsed and forwarded, with a parse test.
- `src/api/client.rs`: `delete_channel(channel)` (`DELETE /channels/{id}`, no body, returns `()`).
- `src/permissions.rs`: `can_manage_channel(store, user, channel)`: a server channel, and Manage Channels in `channel_permissions`. `src/store.rs`: the `ChatStore::can_manage_channel(channel)` wrapper.
- `src/store.rs`:
  - `remove_channel(id) -> bool`, for any kind of channel: takes it out of `channels`, its server's `channels` and every category's `channels` (the server is read from the removed channel), and drops its history, read state and pending messages. `false` if it wasn't known, so the second of the response and the event does nothing.
  - The per-channel part of `remove_server` is now the private `drop_channel_state(id)`, shared by both.
- `src/server_edit.rs`: `delete_channel_error_text(&ApiError)`: `MissingPermission` is "You can't delete this channel.", else `profile_edit::error_text` with "Couldn't delete the channel. Try again." One unit test.
- `src/ui/chat.rs`:
  - `forget_channel(channel, cx)`: what `leave_server` cleared per channel (pane, loading flags and errors, jump, banner, `pending_ack`). `leave_server` now calls it for each removed channel.
  - `channel_deleted(channel, cx)`: `store.remove_channel`, then (if it was known) `forget_channel`, `drop_stale_selection`, `remember_places`, `ensure_current_loaded`. The `ChannelDelete` handler and the user's own delete both use it.
  - "Delete channel…" at the end of `channel_menu`, before Copy channel ID's separator, for server channels where `can_manage_channel`. It's drawn like "Delete message" (danger colour, `Trash` icon).
  - `confirm_delete_channel(channel, window, cx)`: "Delete {name}?", "Once it's deleted, there's no going back.", Cancel and a Danger "Delete". Shift doesn't skip it.
  - `delete_channel(channel, window, cx)`: sets `deleting_channel`, so the dialog's buttons are disabled (Delete shows a spinner) and Escape doesn't close it. On 204 or `NotFound` it runs `channel_deleted` and closes the dialog; otherwise the dialog is enabled again and a toast says why.
- `tests/store_channels.rs` (new, from `ready.json`): deleting a categorised channel takes it out of the server's list, its category, the sidebar and its history, and the category stays; a second removal returns false; other servers, channels and categories are untouched; DMs and groups can be removed too.
- `tests/api_servers.rs`: `DELETE /channels/c1` with the token and an empty body; a 403 `MissingPermission` stays an `Api` error.
- `tests/permissions.rs`: `can_manage_channel` is false with the default permissions, true for the owner (even where a channel takes it away), true for a role granting it, false when a channel override denies it to that role, and false in DMs, groups and Saved Notes. The shared test server gains a lowest-ranked `builder` role granting Manage Channels.

### Divergences
- **The menu item's icon is `Trash`, not `IconName::Delete`,** and it's drawn in the danger colour, as "Delete message" is. That's the app's existing destructive menu item.
- **The confirmation has its own footer** (Cancel and Delete), like the external-link prompt, rather than `.confirm()`'s default buttons. The default buttons can't be disabled or show a spinner, which Assumption 13 needs. Escape is also off while the request runs, so the dialog can't be closed under it.
- **A failed delete leaves the confirmation open,** enabled again, beside the toast, as in the web client (which shows the error in the dialog). Cancel closes it.
- **The title uses the bare channel name** ("Delete general?"), as the web client does.
- **The store's per-channel helper is `drop_channel_state`,** so it isn't confused with `ChatView::forget_channel`.

### Verification Performed
- `cargo test --test store_channels`: 4 passed. `--test api_servers`: 4 passed. `--test permissions`: 33 passed. `cargo test --lib server_edit` and the `channel_delete_decodes` events test pass.
- `cargo test`: 916 passed, 0 failed (906 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts:
  - `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17 and `src/store.rs` 15: unchanged, after hand-formatting `delete_channel` and the dialog's `busy` check.
  - `src/permissions.rs`, `src/server_edit.rs`, `tests/api_servers.rs` and `tests/permissions.rs` 0. New file `tests/store_channels.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. As a server's owner, right-click a channel: "Delete channel…" is last before Copy channel ID, in red. Choose it: "Delete {name}?" asks. Cancel keeps the channel.
2. Delete it: Delete shows a spinner while the request runs, then the dialog closes and the channel disappears from the sidebar. If it was open, the sidebar's first channel opens.
3. Delete the last channel of a category: the category disappears (folded or not) until chunk 3, which shows empty categories. No empty header is left behind.
4. Delete a channel from the web client: it disappears from RuSToat within a moment, and if it was open there, another channel opens with no pane or spinner left behind.
5. As a member without Manage Channels, the channel menu has no Delete. DMs and groups never show it.
6. With a channel open in a second account in RuSToat, delete it from the first: it disappears in both.
7. Light and dark themes: the red menu item and the dialog are readable.
