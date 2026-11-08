# Plan: Server list and accounts (RuSToat milestone 21)

## Summary
Milestone 21 of the [roadmap](roadmap.md) lets you arrange the two left-hand columns:
- **Server order syncs with the web client again.** The web client now saves the rail's order in a field RuSToat doesn't read, so RuSToat's order can be stale. RuSToat reads and writes both fields.
- **Server folders,** in the web client's format so they sync both ways: a closed folder shows a 2×2 preview of its servers; an open one lists them on a tinted strip. "Add to folder" and "Remove from folder" in the server menu; "Edit folder" (name and colour) and "Delete folder" in the folder's menu.
- **Drag to reorder servers and folders,** and to move servers into and out of folders, as in the web client.
- **Drag to reorder accounts** in the switcher. ⌘1–9 already exist (milestone 9) and follow the new order.

The references are Stoat's server (`stoatchat/stoatchat` v0.16.2: `crates/delta/src/routes/sync/{get_settings,set_settings}.rs`, `crates/core/database/src/models/user_settings/*`, `crates/core/database/src/events/client.rs` (`UserSettingsUpdate`), `crates/bonfire/src/config.rs`) and the web client (`stoatchat/for-web` `stoat-for-web-v0.16.3`: `components/state/stores/{Sync,Ordering,ServerFolders,index}.ts`, `components/state/SyncWorker.tsx`, `src/interface/navigation/servers/{ServerList.tsx,railDrag.ts}`, `components/app/menus/{ServerContextMenu,ServerFolderContextMenu}.tsx`, `components/modal/modals/EditServerFolder.tsx`, `components/ui/components/design/ColourPicker.tsx`). Both were read on 2026-11-07.

## Approach
**What the API does:**
- **Fetch:** `POST /sync/settings/fetch` with `{"keys": [...]}` returns `{ key: [timestamp_ms, "<JSON string>"] }` for keys that exist.
- **Set:** `POST /sync/settings/set?timestamp=<ms>` with `{ key: "<JSON string>" }`. No validation; **the last write wins per key**, with no comparison against the stored timestamp. A missing or future timestamp becomes the server's now.
- **`UserSettingsUpdate { id, update: { key: [ts, value] } }`** goes to all your sessions, the writer included.
- **Ready has no server order.** `user_settings` in Ready is filled only for keys requested as `ready=user_settings[<key>]`, and the key must match `\w+`, so **`server-folders` (with a hyphen) can't be requested that way.**

**What the web client does:**
- **Synced keys:** `notifications`, `ordering`, `release-notes`, `server-folders`. On connect it fetches them with `/sync/settings/fetch`.
- **`ordering`:** `{ servers: [id], serverSidebar?: [id] }`.
  - `servers` is the legacy order, **read only**; it's the starting point until `serverSidebar` exists.
  - `serverSidebar` is a flat list of server and folder IDs, each folder ID followed by its servers' IDs. Only `serverSidebar` is written now, but the whole object is sent back.
- **`server-folders`:** `{ folders: [{ id: "folder-<ULID>", name, colour?, collapsed?, servers: [id] }] }`.
  - The folder list's own order means nothing; position comes from `serverSidebar` (or the first member's position).
  - A server is in one folder at most (later claims dropped). `collapsed` is stored only when true, and **syncs.** Folders left with no servers are dropped when written.
  - It's a separate key so older clients rewriting `ordering` as `{ servers }` don't wipe folders.
- **Merging:** a remote value newer than the local one replaces it; an older one that differs makes the client push its own. Every local change writes **both keys with one timestamp.**
- **Display order** (`orderedEntries`): walk `serverSidebar` (or `servers`); each ID becomes a folder (by ID, or as a member) or a server; servers in neither list go at the end (new servers go to the bottom); unknown IDs are skipped; folders with no known servers aren't drawn.
- **Folder look:**
  - Closed: a 42 px circle with a 2×2 preview of up to four members' icons, on the folder colour at 30%, with the members' mention count as a badge and the unread or selected pill on the left.
  - Open: a folder icon tinted with the colour, then the members on a rounded strip of the colour at 18% (or a neutral tint).
  - Clicking toggles it. The tooltip is the name, or "Folder".
- **Drag** (pointer events, from 5 px of movement): the dragged item fades, a ghost follows, and a 4 px line marks the drop point.
  - Over the middle half of a top-level server, the drop **makes a folder** ("New Folder", holding the target then the dragged server), or adds to a folder if the target is one. A ring shows this.
  - Near the top or bottom edge, the drop inserts before or after.
  - Folders can be moved but not put into anything. Servers in an open folder reorder within it or are dragged out; dropping among a folder's members puts a server in that folder. Servers in a closed folder can't be dragged.
  - It cancels with Esc or far outside the rail.
- **Menus:**
  - Server: "Add to folder" (each folder's name, or "Unnamed folder", then "New folder") and "Remove from folder" (when in one), before the notification items.
  - Folder: "Edit folder" ("Edit folder", "Name", "Folder colour", Cancel and Save) and "Delete folder" (destructive, no confirmation; its servers stay where they are).
- **Colours:** a palette of 33 hex swatches plus a custom colour.

**What exists already** (line numbers as of 2026-11-07, before milestones 19 and 20):
- **Settings sync** (milestone 14):
  - `StoatClient::set_settings(&[(&str, String)], timestamp_ms)` (`src/api/client.rs` ~714). **No fetch method.**
  - `READY_FIELDS` (`src/api/events.rs` ~38) asks for `user_settings[ordering]` and `user_settings[notifications]`. `server_order_from_settings` (~735) **reads only `ordering.servers`**, dropping the timestamp.
  - `ServerEvent::UserSettingsUpdate` (~106) decodes only `notifications`.
  - The pattern to copy: `ChatStore.notifications` plus `notifications_updated` (`src/store.rs` ~561), `set_notification_settings(updated, settings)` ignoring older copies (~1367), `restore_notification_settings` (~1411), and `ChatView::update_notifications` (`src/ui/chat.rs` ~3333: `updated = now_ms().max(before + 1)`, an optimistic store change, `spawn_io(set_settings)`, rollback and a toast on failure, and a write counter so stale failures are ignored).
  - `NotificationSettings` (`src/notify.rs` ~123) keeps a `raw` map so unknown fields survive. `ordering` needs the same.
- **Order:** `ChatStore.server_order` (`src/store.rs` ~536), set in `apply_ready` (~651: saved order first, then the rest in Ready order). `add_server` appends; `remove_server` drops. `servers()` (~3193) gives `ServerEntry { id, name, initials, icon, unread, mentions }` to the rail.
- **Rail:** `render_rail` (`src/ui/chat.rs` ~5546; milestone 19 changes its "+"). `rail_button` (~9354, `pub(crate)`, also used by the switcher) returns a `Div` with an inner 48 px button that has the `id`, click, tooltip and `context_menu`, a left pill (selected or unread) and a mention badge. `server_icon` (~9250). `move_server` (~5380) is ⌘⌥↑/↓ over `servers()`.
- **Drag in gpui** (`gpui-pre` 0.3.11 through `gpui_kit::*`): `on_drag(value, |v, offset, window, cx| -> Entity<Preview>)` (needs an `id`), `on_drag_move::<T>`, `drag_over::<T>`, `on_drop::<T>`, `can_drop`, `cx.has_active_drag()`. A drag starts after 2 px with the left button and cancels the click; right-click menus are unaffected. Examples: gpui-component's `dock/tab_panel.rs` (~597) and `table/state.rs` (~2070). The app has no in-app drag yet; only file drops (`chat.rs` ~6233).
- **Accounts:** `AccountList { accounts: Vec<AccountEntry>, active }` (`src/accounts.rs` ~42), saved to `accounts.json` atomically (`FileAccounts`, ~170) through `AppRoot::save_accounts` (`src/ui/app_root.rs` ~183). The switcher is `src/ui/switcher.rs` `render_switcher` (~46), fed in Vec order by `AppRoot::render_switcher` (~733). `show_account_at` (~288) and `account_names` (~295) follow the same Vec, and `MainWindow::sync_menus` (`src/main.rs` ~249) rebuilds the Accounts menu when names change.
- **Shortcuts:** ⌘1–9 → `SwitchAccount1..9` (`src/ui/actions.rs` ~60); listed in `src/shortcuts.rs`.

**Decisions** (user, 2026-11-07):
1. **Folders follow the web client** (look, drag, menus, no confirmation for deleting a folder), stored in its `ordering.serverSidebar` and `server-folders` format, so they sync both ways.
2. **Folder colours** are a fixed palette of 12 plus "No colour"; no custom value. A colour set elsewhere is still drawn.
3. **⌘⌥↑/↓ skips servers in closed folders.**
4. **⌘1–9 already exist** and follow the new account order.

**Rejected alternatives.**
- **RuSToat-only folders** (stored locally). They wouldn't follow you between clients, and the web client's format already exists.
- **Reading `server-folders` through Ready.** The hyphen doesn't match Bonfire's `\w+`. A fetch on connect works for any key, as in the web client.
- **Rearranging the rail live while dragging.** The web client keeps the rail still and draws a drop line; that's simpler and avoids items jumping under the cursor.
- **Account order in `prefs.json`.** `accounts.json` already holds the list in order; reordering the Vec is enough.

## Assumptions
1. **A pure module, `src/rail.rs`, owns the model:** `Ordering { servers, server_sidebar: Option, raw }`, `ServerFolders { folders, raw }`, `Folder { id, name, colour, collapsed, servers, raw }`, `RailEntry::{Server(id), Folder { folder, servers }}`, and `entries(ordering, folders, known_servers_in_ready_order) -> Vec<RailEntry>` matching `orderedEntries`. All edits (move, fold, add to folder, remove from folder, rename, recolour, collapse, delete) are pure functions returning new values, plus `to_sidebar(entries)` to write `serverSidebar`.
2. **Unknown fields survive.** Both keys keep their raw JSON objects (as `NotificationSettings` does), so the web client's fields and any future ones aren't lost. A RuSToat write never removes `ordering.servers`.
3. **Reading:** on connect (after Ready), fetch `ordering` and `server-folders` with `/sync/settings/fetch`, with their timestamps. Ready's `user_settings[ordering]` stays for the first draw, so the rail doesn't jump when the fetch lands a moment later.
4. **Merging** follows the notification settings: a value older than the one held is ignored; a newer one replaces it. RuSToat doesn't push its own copy back on seeing an older one (unlike the web client); it only writes on your changes.
5. **Writing:** every change writes both keys at one timestamp (`now_ms().max(previous + 1)`), with an optimistic store update, rollback and an error toast "Couldn't save the server list. Try again." on failure.
6. **Collapse syncs,** as in the web client. Folding or unfolding writes `server-folders`.
7. **New servers** (created or joined) appear at the end, outside folders, and aren't written until you next change the order. That's the web client's behaviour (unlisted servers go last).
8. **Folder IDs** are `folder-` plus a ULID (milestone 19 adds the `ulid` crate). A new folder is named "New Folder", as in the web client.
9. **Badges:** a closed folder shows the summed mentions of its servers and the unread pill if any is unread; the selected pill if the open server is inside. An open folder's servers draw their own.
10. **Folder palette** (12, chosen to read on light and dark): red `#E5484D`, orange `#F76B15`, amber `#FFB224`, yellow `#F5D90A`, lime `#99D52A`, green `#30A46C`, teal `#12A594`, cyan `#05A2C2`, blue `#3E63DD`, indigo `#6E56CF`, pink `#D6409F`, grey `#8B8D98`. The Edit folder dialog shows them as 24 px swatches with a ring on the chosen one, plus "No colour". A colour from elsewhere (any CSS hex) is drawn if it parses as `#rgb` or `#rrggbb`, else ignored.
11. **Dragging in the rail:**
    - Any server or folder can be dragged (not Home or "+"). Servers in a closed folder can't, as in the web client.
    - The preview is the item's icon at 48 px, slightly transparent.
    - While dragging, the item's rail slot fades to 35%, and the drop target is shown: a 3 px accent line between items, or a 2 px accent ring around a server or folder for "fold into".
    - Over a top-level server, the middle half means fold (or add to that folder); the top and bottom quarters mean before and after. Over a folder's icon, the middle means add to it. Over a server inside an open folder, only before and after, within the folder.
    - Dropping a server at the top level takes it out of its folder. A folder can't be dropped into anything.
    - Dropping where it already is does nothing and writes nothing. Esc cancels (gpui cancels a drag on Esc; check).
12. **Dragging in the switcher:** accounts reorder with before and after lines only; the order is saved to `accounts.json` and the Accounts menu and ⌘1–9 follow at once.
13. **⌘⌥↑/↓** walks the rail's visible servers: top-level servers and servers in open folders, skipping closed folders (Decision 3).
14. **No new keyboard shortcuts,** and no keyboard dragging (the web client's Space/arrow drag is left out).

## Plan

### Chunk 1: Read and write the server order the web client's way
**Goal:** RuSToat's server order matches the web client's, including `serverSidebar`, and survives a write without losing fields.
**Files likely touched:**
- `src/rail.rs` (new, pure): `Ordering`, `ServerFolders`, `Folder`, `RailEntry`, `entries(...)`, parsing from the settings' JSON strings (tolerant: missing or malformed gives empty), and `to_json`.
- `src/api/client.rs`: `fetch_settings(keys) -> HashMap<String, (i64, String)>` (`POST /sync/settings/fetch`).
- `src/api/events.rs`: Ready keeps the raw `ordering` with its timestamp; `UserSettingsUpdate` also decodes `ordering` and `server-folders` (with timestamps).
- `src/store.rs`: `ordering` and `server_folders` with their timestamps replace `server_order`; `servers()` (or a new `rail_entries()`) goes through `rail::entries`; `set_ordering(ts, ordering)` and `set_server_folders(ts, folders)` ignore older copies.
- `src/ui/chat.rs`: fetch both keys after Ready; handle `UserSettingsUpdate`; `update_rail(change)` writes both keys (Assumption 5). The rail draws folders as plain lists of servers for now (chunk 2 draws them properly).

**Depends on:** None.
**Risks:**
- **Ready-order fallback.** Tests in `tests/store_ready.rs` (~47) and `tests/store_servers.rs` rely on `server_order`. Keep their meaning (saved order first, then the rest in Ready order) or update them deliberately.
- **`UserSettingsUpdate` echo.** Our own write comes back with our timestamp; equal timestamps must change nothing.
- **Clock skew.** `now_ms().max(previous + 1)` keeps writes newer than what we hold, as for notifications.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/rail.rs`:
  - `entries` with only `servers`; with `serverSidebar`; with a folder whose ID is listed and one positioned by its first member; unknown IDs skipped; unlisted servers appended in Ready order; a server claimed by two folders goes to the first; a folder with no known servers isn't drawn.
  - Round trip: parsing the web client's JSON and writing it back keeps `servers`, unknown fields, and folders' unknown fields.
- `tests/api_settings.rs`: `POST /sync/settings/fetch` sends `{"keys":["ordering","server-folders"]}` and parses the tuple map.
- `src/api/events.rs`: `UserSettingsUpdate` with `ordering` and `server-folders` parses with timestamps.

**Manual check:** Reorder servers in the web client: RuSToat follows within a moment, and after a restart.

**Shippable when:** the rail's order matches the web client's, and the tests pass.

### Chunk 2: Server folders
**Goal:** Folders draw as in the web client, open and close (synced), and can be made, edited and deleted from menus.
**Files likely touched:**
- `src/rail.rs`: `add_to_folder(server, folder)`, `new_folder_with(server, id)`, `remove_from_folder(server)`, `rename_folder`, `recolour_folder`, `toggle_folder`, `delete_folder` (servers stay in place), `parse_colour(&str) -> Option<Rgba>`, `PALETTE`.
- `src/ui/chat.rs`:
  - `render_rail` draws `RailEntry::Folder`: closed (2×2 icon preview on the tinted circle, badges per Assumption 9) or open (a tinted folder icon, then members on a tinted strip). Click toggles.
  - `server_menu` gains "Add to folder" (submenu: folders, then "New folder") and "Remove from folder".
  - `folder_menu`: "Edit folder…" and "Delete folder".
  - `move_server` follows Assumption 13.
- `src/ui/server_dialogs.rs` (from milestone 19): `EditFolderDialog` ("Edit folder", "Name", the palette, Cancel and Save).

**Depends on:** Chunk 1.
**Risks:**
- **`rail_button`** is shared with the switcher. Add folder drawing beside it, not inside it; reuse its pill and badge parts if they can be split out.
- **The 2×2 preview** with servers that have no icon: draw their initials at the small size, or a coloured dot. Check legibility at 18 px.
- **Submenus** in `PopupMenu`: milestone 14's Mute submenu shows they work.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/rail.rs`: each edit, including adding a server already in another folder (moves it), deleting a folder (servers stay in order), removing the last server (the folder disappears on write), toggle, and `parse_colour` for `#abc`, `#AABBCC`, `red` (None) and `""`.

**Manual check:**
- "Add to folder → New folder" makes a folder; add a second server; close it: the preview shows both icons.
- Edit its name and colour: the web client shows them.
- Fold it in the web client: it folds in RuSToat.
- An unread server inside a closed folder lights the folder's pill; a mention shows its badge.
- Delete the folder: its servers stay in place.

**Shippable when:** folders show, sync, and can be managed from menus, and the tests pass.

### Chunk 3: Drag servers and folders in the rail
**Goal:** Drag servers and folders to reorder them, fold servers together, and move them into and out of folders.
**Files likely touched:**
- `src/rail.rs`: `DropTarget::{Before(id), After(id), Into(id)}`, `target_for(entry, pointer_y_fraction, inside_open_folder) -> DropTarget` (Assumption 11), and `apply_drop(entries, dragged, target, new_folder_id) -> Option<(Ordering, ServerFolders)>` (`None` when nothing changes).
- `src/ui/chat.rs`: `on_drag` on each server and folder (a `RailDrag { id }` value and a small preview entity), `on_drag_move` on the rail to compute the target from the hovered item's bounds, the drop line and ring, `on_drop` calling `apply_drop` and `update_rail`.

**Depends on:** Chunks 1 and 2.
**Risks:**
- **Finding the hovered item.** `on_drag_move` gives the dragged item and the element's bounds; put the listener on each rail item (its own bounds) or record each item's bounds during prepaint. Pick one and keep the drop target in `ChatView` state, cleared on drop and when the drag ends outside.
- **Clicks after drags.** gpui cancels the click once a drag starts, but check a tiny drag (under 2 px) still clicks.
- **Scrolling rails.** With many servers the rail scrolls; dropping near its ends should still work (no auto-scroll; acceptable).

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/rail.rs`: `target_for` at 0.1, 0.5 and 0.9 for top-level servers, folders and in-folder servers; `apply_drop` for before, after, into a server (new folder with target first), into a folder, out of a folder, a folder before a server, a folder onto a server (treated as before/after), and a no-op drop (`None`).

**Manual check:**
- Drag a server between two others; onto another (a folder appears); out of a folder; reorder inside an open folder; drag a folder.
- The web client shows the same order.
- Clicking servers still works; right-click menus still open.

**Shippable when:** the rail can be arranged by dragging, and the tests pass.

### Chunk 4: Drag accounts in the switcher
**Goal:** Reorder accounts by dragging; the Accounts menu and ⌘1–9 follow.
**Files likely touched:**
- `src/accounts.rs`: `AccountList::move_account(id, before: Option<&str>) -> bool`.
- `src/ui/switcher.rs`: `on_drag`, `on_drag_move` and `on_drop` on account buttons, with a before/after line; a callback `on_reorder(id, before)`.
- `src/ui/app_root.rs`: handle it: `move_account`, `save_accounts`, notify so `sync_menus` runs.

**Depends on:** Chunk 3's drag approach (shared preview and line drawing if it's cleanly reusable).
**Risks:** `sync_menus` compares name lists; a reorder changes the order, so it should rebuild. Check it does.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`: `tests/accounts.rs` (or the existing accounts tests): `move_account` to the front, the end, before itself (no change), and an unknown ID (false); the order round-trips through `MemoryAccounts`.

**Manual check:** With three accounts, drag the third to the top: ⌘1 opens it, the Accounts menu lists it first, and the order survives a restart.

**Shippable when:** accounts can be reordered by dragging, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`. Formatting rules as in milestone 19's plan: format only touched files with `rustfmt --edition 2024 <file>`, never `mod.rs`, `lib.rs` or `main.rs`; don't let a drifting file's `Diff in` count grow (record counts at the start).

### Unit Tests
- **`src/rail.rs`:** ordering and folders (parsing, `entries`, round-tripping unknown fields), every edit, colours, drop targets and drops.
- **`src/api/events.rs`:** `UserSettingsUpdate` with the two keys.
- **`src/accounts.rs`:** `move_account`.

### Integration Tests
- **`tests/api_settings.rs`:** fetch; a write of both keys at one timestamp.
- **`tests/store_ready.rs` / `tests/store_servers.rs`:** order from `serverSidebar`, from `servers`, with new servers appended; older settings ignored.

### Manual Tests
You need the web client open on the same account, at least five servers, and two or three RuSToat accounts.
1. Run every chunk's manual check.
2. Arrange folders in RuSToat, then in the web client: each follows the other, and nothing is lost (names, colours, collapsed state).
3. Light and dark themes: folder tints, previews, drop lines and rings are visible.

## Definition of Done
- [ ] The rail follows `ordering.serverSidebar` (falling back to `servers`) and `server-folders`, fetched on connect and updated live; unknown fields survive RuSToat's writes.
- [ ] Folders draw closed (2×2 preview, tinted, badges) and open (tinted strip); folding syncs.
- [ ] The server menu has "Add to folder" (with "New folder") and "Remove from folder"; the folder menu has "Edit folder…" (name and a 12-colour palette plus none) and "Delete folder".
- [ ] Servers and folders can be dragged to reorder, fold together, and move into and out of folders; a no-op drop writes nothing.
- [ ] ⌘⌥↑/↓ skips servers in closed folders.
- [ ] Accounts can be dragged in the switcher; the order persists, and the Accounts menu and ⌘1–9 follow.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/server-list-and-accounts-implementation.md`.
