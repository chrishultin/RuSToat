# Implementation Record: Server list and accounts (RuSToat milestone 21)

_Plan: docs/plans/server-list-and-accounts.md_
_Started: 2026-11-08_

Pre-existing uncommitted changes, kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt `Diff in` counts (2026-11-08): `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15. Zero in every other file in scope.

Supervision: continuous, unattended overnight; manual checks batched at the end.

Commits: commit signing (Secretive, through Touch ID) wasn't available overnight, so each chunk is left staged, uncommitted, on top of milestones 19 and 20's staged chunks, and its tree (from `git write-tree`), with its patch against the one before, is recorded in `/tmp/rustoat-chunks/ledger.txt`. They're to be committed one by one in the morning, each from its recorded tree.

## Chunk 1: Read and write the server order the web client's way

### Changes Made
- `src/rail.rs` (new, pure):
  - `ORDERING_KEY` (`ordering`) and `FOLDERS_KEY` (`server-folders`).
  - `Ordering`: `parse(json)`, `servers()` (the legacy order), `server_sidebar()`, `sidebar()` (`serverSidebar`, else `servers`), `set_server_sidebar(ids)`, `to_json()`. Parsing cleans ID lists as the web client's `cleanIds` does (strings only, no blanks, first occurrence kept, an early web client's folder object standing for its servers). The parsed object is kept, less `serverSidebar`, so `servers` and unknown fields are written back unchanged; `servers: []` is added if it was missing, as the web client writes it.
  - `ServerFolders { folders, raw }` and `Folder { id, name, colour, collapsed, servers, raw }`: `parse(json)` as the web client's `clean()` (folders without an ID or with a repeated one dropped; a server's later claims dropped), `folder(id)`, `to_json()` (folders with no servers dropped, `colour` only if set, `collapsed` only if true, unknown fields kept at both levels).
  - `RailEntry::{Server(id), Folder { folder, servers }}` with `servers()`, and `entries(ordering, folders, known)`, the web client's `orderedEntries`: each sidebar ID is a folder (by ID, or as a member) drawn where it first comes, or a server; servers not listed come last in `known` order (bringing their folder); unknown IDs skipped; folders with no known servers not drawn.
  - `to_sidebar(entries)`: each folder's ID then its servers', for `serverSidebar`.
  - 14 unit tests.
- `src/api/client.rs`: `fetch_settings(keys) -> HashMap<String, (i64, String)>` (`POST /sync/settings/fetch` with `{"keys": [...]}`).
- `src/api/events.rs`:
  - `Ready.server_order` is replaced by `Ready.ordering: Option<(i64, Ordering)>`, keeping the timestamp.
  - `ServerEvent::UserSettingsUpdate` also carries `ordering` and `server_folders`, each `Option<(i64, Box<_>)>`.
  - `user_settings(value)` reads the `{key: [ts, json]}` map leniently per key (a malformed key no longer hides the others); `parse_setting(map, key, parse)` takes one out, parsed. They replace `server_order_from_settings` and `notifications_from_settings`.
- `src/store.rs`:
  - `server_order` becomes `ready_order` (servers in `Ready` order, then any joined since), with `ordering` / `ordering_updated` and `server_folders` / `server_folders_updated`. `apply_ready` takes `Ready`'s `ordering` (or none) and keeps the folders, which `Ready` can't carry.
  - `rail_entries()` (through `rail::entries`), `server_entry(id)` (one rail icon's `ServerEntry`, split out of `servers()`), and `servers()` now the rail's servers flattened in rail order.
  - `ordering()`, `ordering_updated()`, `server_folders()`, `server_folders_updated()`; `set_ordering(updated, ordering)` and `set_server_folders(updated, folders)` (older copies ignored; an older identical copy moves the held time back, as for notification settings; returns true only if the value changed, so our own echo changes nothing); `restore_ordering` and `restore_server_folders`. A private `take_newer` holds the shared rule.
- `src/ui/chat.rs`:
  - After every `Ready`, `fetch_rail_settings` fetches both keys and applies them through `apply_rail_settings` (which `UserSettingsUpdate` also uses). A failed fetch is logged; the rail keeps `Ready`'s order.
  - `update_rail(change: impl FnOnce(&mut Ordering, &mut ServerFolders), window, cx)`: the write path for chunks 2 and 3. Nothing is written if the change changes nothing. Otherwise both keys are written at one timestamp, `now_ms().max(max(both held) + 1)`, with the store updated first (holding the values as re-parsed from what's written, so the echo matches), a `rail_write` counter, and on failure each key put back unless a newer copy of it has landed, and the toast "Couldn't save the server list. Try again." (`RAIL_FAILED_TEXT`).
  - `render_rail` walks `rail_entries()`, drawing each folder's servers in its place as plain servers, through the new `render_rail_server(server, cx)` (split out of `render_rail`).
- Tests: `tests/api_settings.rs` (fetch sends `{"keys":["ordering","server-folders"]}` and parses the tuple map; a write of both keys at one timestamp keeps `servers` and unknown fields), `tests/store_servers.rs` (sidebar over legacy order, folders in place, a joined server last outside folders with nothing written, older copies and echoes ignored, a new `Ready` brings the order and keeps folders, a folder whose servers are all gone isn't drawn), `src/api/events.rs` (`UserSettingsUpdate` with both keys and their timestamps; the order from `Ready`'s settings), `tests/store_ready.rs` updated for `Ready.ordering`.

### Divergences
- **`update_rail` has no caller yet,** so it carries `#[expect(dead_code, reason = …)]`. The expectation fails the build once chunk 2 calls it, so it can't be left behind.
- **`update_rail` takes `FnOnce(&mut Ordering, &mut ServerFolders)`** rather than a pure function's result, as `update_notifications` does; pure edits returning new values plug in by assigning them, and a change that changes nothing (a no-op drop) writes nothing because the values are compared.
- **A failed write puts back each key separately,** when its held time is still the write's, since the two keys can be overtaken separately by other clients' copies.
- **Settings in `Ready` and `UserSettingsUpdate` are read per key.** Before, one malformed key made the whole map unreadable (losing the notification settings too).
- **`to_sidebar` and `Ordering::set_server_sidebar` are added now** (Assumption 1, not assigned to a chunk), as the pair the write path needs; chunk 3's drops use them.
- **The legacy `servers` list is written back exactly as stored,** not cleaned as the web client writes it, so older clients see what they last wrote (Assumption 2).

### Verification Performed
- `cargo test --lib rail::`: 14 passed. `cargo test --test api_settings`: 4 passed. `cargo test --test store_servers --test store_ready`: 16 and 14 passed. The `UserSettingsUpdate` and `Ready` settings tests in `src/api/events.rs` pass.
- `cargo test`: 1023 passed, 0 failed (1000 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15, unchanged (hand-formatted; their `--check` output is identical to the staged versions'). `src/rail.rs`, `tests/api_settings.rs`, `tests/store_servers.rs`, `tests/store_ready.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Reorder servers in the web client: RuSToat's rail follows within a moment.
2. Quit and restart RuSToat: the rail opens in the web client's order (no jump once the fetch lands).
3. With folders made in the web client, RuSToat shows each folder's servers together, in the folder's place.
