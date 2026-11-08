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
