# Implementation Record: Invites (RuSToat milestone 18)

_Plan: docs/plans/invites.md_
_Started: 2026-11-07_

Pre-existing uncommitted changes, kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt `Diff in` counts (2026-11-07): `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15, `src/markdown.rs` 1. Zero in `src/api/error.rs`, `src/links.rs`, `src/permissions.rs`, `src/main.rs`, `src/ui/actions.rs`, `src/assets.rs`, `src/shortcuts.rs`, `tests/links.rs`, `tests/permissions.rs` and `tests/store_servers.rs`.

Supervision: continuous. The user asked for every manual check to be batched at the end.

## Chunk 1: Join a server from a code or link

### Changes Made
- `src/api/client.rs`:
  - `fetch_invite(code) -> InviteResponse` (`GET /invites/{code}`)
  - `join_invite(code) -> InviteJoinResponse` (`POST /invites/{code}`, no body)
  - The two models added to the hand-formatted import list.
- `src/invites.rs` (new, pure):
  - `InviteHosts` (`new(app)`, `is_official`, `placeholder`), `is_official_app`, `is_code`
  - `code_from_link`: the own host (port included) always, and `stt.gg` / `rvlt.gg` / the Stoat and Revolt hosts on the official instance only. It rejects user-info URLs, ports on the official hosts, and `discover`.
  - `parse_input`, with the `NOT_AN_INVITE` and `OTHER_INSTANCE` texts
  - `member_count_text`
  - `InvitePreview` / `InviteTarget` (from either `InviteResponse` shape), with `detail()` and `server_id()`
  - `already_joined(&ApiError)` and `join_error_text(&ApiError)`
  - 12 unit tests.
- `src/ui/invites.rs` (new): the `JoinDialog` entity (input, lookup, preview, Join/Open, errors under the field, "Joining as @name#tag"), `JOIN_DIALOG_WIDTH`, `render_preview`.
  - Editing the field drops a stale preview, and a lookup that returns for a code no longer typed is ignored.
  - Enter looks up, or joins once a preview is shown.
- `src/ui/chat.rs`:
  - `invite_hosts()`, `open_join_dialog(code, window, cx)`, `is_invite_member`, `open_invite` and `invite_joined`
    - A server join is added with `add_server(server, channels, [])`, then `fetch_own_member` if needed.
    - A group join adds only users not already known, then `add_channel`.
  - The rail gets a "+" `rail_button` ("Join a server") after the servers, behind a second divider when there are servers.
- `src/ui/actions.rs`: the `JoinServer` action.
- `src/main.rs`: "Join Server…" in the Stoat menu after Add Account…. Its handler opens the dialog for `root.shown_chat()`, and does nothing without one.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- `tests/api_invites.rs` (new): server and group lookups, `NotFound`, server join with an empty request body, group join, and `AlreadyInServer` as an `Api` error.
- `tests/store_servers.rs`: a join response then its `ServerCreate`, and the reverse. Both give one rail entry, with the emoji.

### Divergences
- **"Joining as @name#tag" always shows** (Assumption 12 said only with several accounts). `ChatView` doesn't know how many accounts are signed in, and the line is harmless with one.
- **The rail's second divider** only shows when there are servers, to avoid two dividers in a row.
- **The JoinDialog is its own entity** (like `IdentityEditor`) rather than fields on `ChatView`, which keeps `chat.rs` smaller. The plan allowed either.

### Verification Performed
- `cargo test --lib invites`: 12 passed.
- `cargo test --test api_invites --test store_servers`: 6 and 6 passed. The first run of `joining_a_server_sends_no_body` failed on an incomplete test fixture (`approximate_member_count` missing), which was fixed in the fixture.
- `cargo test`: 875 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after fixing `manual_is_multiple_of` and `let_and_return`.
- rustfmt `Diff in` counts:
  - `src/api/client.rs` 3 and `src/ui/chat.rs` 17: unchanged.
  - `src/main.rs` 0 and `src/ui/actions.rs` 0.
  - New files 0, and `tests/store_servers.rs` 0.

### Manual Validation
Deferred: the user asked for every manual check to be run together at the end.
