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

## Chunk 2: Server folders

### Changes Made
- `src/rail.rs`:
  - `NEW_FOLDER_NAME` ("New Folder"), `PALETTE` (Assumption 10's twelve, as (name, hex)), `new_folder_id()` (`folder-` and a ULID), `Folder::new(id, name, servers)`.
  - `ServerFolders::folder_of(server)`, `rename_folder`, `recolour_folder`, `toggle_folder` (the folders only; nothing if the folder's gone).
  - `new_folder_with`, `add_to_folder`, `remove_from_folder` and `delete_folder`, each `(ordering, folders, rail, …)` with `rail` the entries as drawn. They first write the drawn rail into `serverSidebar` (`to_sidebar`), so nothing else moves, then: a new folder goes in its server's place; an added server goes to the end of the folder (and after its servers in the sidebar), out of any other; a removed server lands just below its folder; a deleted folder's servers stay where they were. Folders left empty are dropped, with their place in the sidebar.
  - `parse_colour(&str) -> Option<Rgba>`: `#rgb` or `#rrggbb`, else `None`.
  - 11 unit tests.
- `src/ui/chat.rs`:
  - `render_rail` matches on `RailEntry`. `render_rail_folder`: closed, a 48 px circle (its colour at 30% over the button's own, so hover still shows) with `folder_preview` (up to four servers' icons at 18 px, two by two; a server without an icon shows its initial on a small circle), the summed mentions badge, the unread pill if any is unread, the selected marker if the open server is in it; open, a `FolderOpen` icon in its colour, then its servers, on a strip of the colour at 18% (or the foreground at 10%). Click toggles it (synced); the tooltip is its name, or "Folder". Element ID `rail-<folder ID>`.
  - `rail_button`'s marker and badge are split out as `rail_slot(button, selected, unread, mentions, cx)`, which the folder uses; `rail_button`'s signature (shared with the switcher) is unchanged.
  - `server_menu` gains `folder_items` after Invite people…: an "Add to folder" submenu (the rail's other folders, by name or "Unnamed folder", then "New folder") and "Remove from folder" when in one.
  - `folder_menu`: "Edit folder…" and a red "Delete folder" (no confirmation).
  - `edit_rail(edit)` runs one of `rail`'s edits through `update_rail` with the rail as drawn (its `#[expect(dead_code)]` is gone); `edit_folder(folder, name, colour)` saves the dialog; `open_edit_folder` opens it.
  - `move_server` passes over servers in closed folders (from one, it moves past its folder).
- `src/ui/server_dialogs.rs`: `EditFolderDialog` ("Edit folder"): "Name" (no length counter: folder names have no limit), "Folder colour" (the palette as 24 px swatches with a ring on the chosen one, in two rows of six, plus a swatch for a colour set elsewhere if it can be drawn), "No colour", Cancel and Save (or Enter). Saving closes it at once; the change shows at once and a failed write puts it back with the rail's toast.
- `src/assets.rs`: `FolderMinus` and `FolderPlus` added (`Folder` and `FolderOpen` are gpui-component's); a test that the folder icons load.

### Divergences
- **The structural edits take the rail as drawn** (rather than only the two settings), since they write `serverSidebar` from it first. The web client changes only `server-folders` for these, so, e.g., adding a server listed above a folder to it moves the folder up to the server's place; here the folder stays put. The result reads back the same in the web client.
- **"Add to folder" lists only folders drawn in the rail, and not the server's own** (the web client lists every folder, including ones whose servers you've all left, and its own, which does nothing).
- **Clicking "Add to folder" itself doesn't make a new folder** (the web client's desktop shortcut); "New folder" is in the submenu.
- **The 2×2 preview uses 18 px icons for any number of servers** (the web client uses 16 px for up to two and 13 px past that, in a 42 px circle; RuSToat's rail buttons are 48 px).
- **The open folder's icon keeps the rail buttons' neutral circle** (the web client draws the icon alone), so it reads as a button like Home.
- **"Folder colour" offers a colour set elsewhere as one more swatch** when it isn't in the palette and can be drawn, so it can be chosen back after trying another.

### Verification Performed
- `cargo test --lib rail::`: 25 passed (11 new): a new folder takes its server's place (and from another folder, dropping that one if emptied); adding leaves the folder in place; adding a server in another folder moves it; adding to its own folder or a gone one changes nothing; removing puts the server just below the folder; removing the last server drops the folder (and its sidebar place); deleting keeps the servers in drawn order; toggle (written only when closed), rename, recolour (none drops `colour`); folder IDs; `parse_colour` for `#abc`, `#AABBCC`, `red`, `""` and others, and every palette colour.
- `cargo test --lib assets::`: 8 passed (1 new).
- `cargo test`: 1035 passed, 0 failed (1023 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/ui/chat.rs` 17 (unchanged; hand-formatted, and its `--check` output matches the staged version's), `src/rail.rs`, `src/ui/server_dialogs.rs`, `src/assets.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Right-click a server → Add to folder → New folder: a folder appears in its place. Add a second server to it from its menu; click the folder to close it: the preview shows both icons (a server without an icon shows its initial).
2. Right-click the folder → Edit folder…: change its name and colour and Save: the tooltip and tint change, and the web client shows them.
3. Close or open the folder in the web client: RuSToat follows.
4. With the folder closed, an unread server inside lights the folder's pill; a mention shows the summed badge; opening a server inside shows the tall marker on the folder.
5. ⌘⌥↑/↓ passes over the servers in the closed folder, and walks those in an open one.
6. Remove from folder: the server lands just below the folder. Delete folder: its servers stay in place.
7. Light and dark themes: tints, preview and the palette are visible.

## Chunk 3: Drag servers and folders in the rail

### Changes Made
- `src/rail.rs`:
  - `DropTarget::{Before(id), After(id), Into(id)}`: above or below an item (in its folder if it's in an open one), or onto a server (a new folder of the two) or into a folder (at its end).
  - `target_for(rail, dragged, hovered, fraction) -> Option<DropTarget>` (Assumption 11, with the web client's fold band): over a top-level server or a folder's icon, the middle half is onto it and the top and bottom quarters above and below; below an open folder's icon is above its first server, in it; over a server in an open folder, above or below it by half. A dragged folder only goes above or below top-level items (over another folder's servers, below that folder). `None` over itself (or, for a folder, its own servers), or onto the dragged server's own folder.
  - `apply_drop(ordering, folders, rail, dragged, target, new_folder_id)`, in the same shape as chunk 2's edits (the rail as drawn written into `serverSidebar`, then `settle`): a server beside a top-level item leaves its folder; beside a server in a folder, it joins that folder there; onto a server, a "New Folder" of the two in the target's place, the target first; into a folder, at its end. A folder moves whole. Folders left empty go. A drop that leaves the rail as it was (or can't be made: a folder into anything, an item that's gone) changes nothing, so nothing is written (and a rail still on the legacy order doesn't get a `serverSidebar`).
  - Private helpers `locate`, `is_folder`, `place`, `fold`, `take_from_rail`, `beside`.
  - 12 unit tests.
- `src/ui/chat.rs`:
  - `RailDrag { id }`, the dragged value; `RailDragState { dragged, target, over_rail }` held in `ChatView.rail_drag`, read through `rail_dragging` (only while gpui has a drag going, so one that ended unseen is ignored).
  - Servers' and folders' buttons are draggable (`rail_draggable`: `on_drag` with a `DragPreview`; starting a drag records it). Servers in a closed folder aren't drawn, so can't be dragged; Home and "+" aren't draggable. `render_rail_server` now builds its button from `rail_icon` and `rail_slot` (as the folder does) rather than through `rail_button`, whose signature (shared with the switcher) is unchanged.
  - Finding the target: each item's row has an `on_drag_move` (`rail_drop_slot` → `rail_drag_over`) that, when the pointer is inside its own bounds, asks `rail::target_for` with the pointer's fraction down the row. The rail's own listener (`rail_drag_moved`, which gpui runs before its items') clears the target when the pointer leaves the rail, and items take the pointer only while it's over the rail (an item scrolled out of sight still sees it). Home, and "+" with the space below it (now one column that fills the rest of the rail), mean above the first item and below the last (`rail_drag_over_end`). Between items, the target stays where it was.
  - Drawing: the dragged item's row (an open folder's whole strip) at 35%; `with_drop_mark` draws a 3 px line in the primary colour centred in the 8 px gap above or below an item (an open folder: its whole strip), or a 2 px ring around a 48 px button (an open folder: around its icon).
  - Dropping: `on_drop::<RailDrag>` on the rail (`drop_on_rail`) runs `apply_drop` through `edit_rail` / `update_rail`; a mouse-up off the rail (`on_mouse_up_out`) forgets the drag; nothing happens.
  - Esc: gpui doesn't cancel a drag on Esc (checked in `gpui-pre` 0.3.11: only a mouse-up ends one), so `ChatView::new` adds a keystroke interceptor (`_drag_escape`) that, while a rail drag is going, forgets it, calls `cx.stop_active_drag(window)` and stops the Esc there (so it doesn't also jump to the present).
  - Reusable for the switcher (chunk 4): `DropMark::{Above, Below, Onto}` and `with_drop_mark(item, mark, cx)`; `drag_face(content, bg, cx)` (a 48 px face) and `DragPreview::new(face, cx)` (the preview entity for `on_drag`, at 80%); `DRAGGED_OPACITY`.

### Divergences
- **`target_for` takes the rail, the dragged ID and the hovered ID** (rather than `(entry, fraction, inside_open_folder)`), since what's possible depends on what's dragged (a folder goes into nothing) and where (an open folder's icon, a server's own folder).
- **`apply_drop` edits `(ordering, folders)` in place and changes nothing for a no-op**, as chunk 2's edits do, rather than returning `Option<(Ordering, ServerFolders)>`; `update_rail` already writes nothing when nothing changed.
- **Below an open folder's icon drops above its first server, in the folder**, as in the web client (whose bottom half there inserts before the first member); Assumption 11 has only the middle of a folder's icon meaning "into it".
- **Below the last server in an open folder stays in the folder** (Assumption 11: "only before and after, within the folder"); the web client puts it after the folder. To drop just below an open folder at the top level, use the top quarter of the next item, or the "+" area if it's last.
- **A dragged folder over another folder's servers goes below that folder**, as in the web client.
- **No ring onto a server's own folder** (the web client shows one, and the drop does nothing).
- **Leaving the rail cancels**, rather than the web client's 80 px margin either side: the drop has to land on the rail for gpui to hand it over, so the line goes as soon as the pointer leaves it.
- **Home and "+" (and the space below it) mean first and last**, matching the web client's "above the first entry" and "below the last".

### Verification Performed
- `cargo test --lib rail::`: 37 passed (12 new): `target_for` at 0.1, 0.5 and 0.9 over top-level servers (and the quarters' edges), a closed and an open folder's icon (below the open one's is its first server; onto its own folder is nothing), servers in an open folder, and for a dragged folder (top-level only; over its own servers, nothing); `apply_drop` above and below, onto a server (a new folder in its place, target first, "New Folder", no colour, open), into a folder (at its end; from another folder, which goes when emptied), beside a server in a folder (joins it there; reordered within its own), beside a top-level item (out of its folder; a folder's last server takes its place), a folder before and after a server and beside another folder's server (moves whole, folders untouched); thirteen no-op drops change nothing, including on the legacy order (no `serverSidebar` written); every target `target_for` shows keeps every server drawn exactly once.
- `cargo test`: 1047 passed, 0 failed (1035 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/ui/chat.rs` 17 (unchanged; hand-formatted, and its `--check` output matches the staged version's), `src/rail.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Drag a server between two others (top or bottom quarter of a neighbour): a line shows where; drop: it moves there. The dragged icon fades and its icon follows the pointer.
2. Drag a server onto the middle of another: a ring shows; drop: a "New Folder" appears in the target's place, holding the target then the dragged server.
3. Drag a server onto a closed folder's middle: it goes to the end of the folder. Onto an open folder's icon: the same; just below the icon: it goes first in the folder.
4. With a folder open, drag one of its servers above or below another in it: it reorders within the folder. Drag it to the top or bottom quarter of a top-level server: it leaves the folder (the folder goes if it was the last).
5. Drag a folder (open and closed) above or below servers and other folders: it moves whole; over a server's middle there's no ring.
6. Drag over Home: the line shows above the first item; over "+" or the space below it: below the last.
7. Drop where the item already is (or onto itself): nothing changes, and nothing is written (no "Couldn't save" toast, the web client doesn't change).
8. Press Esc mid-drag: the drag ends, nothing moves, and the open channel doesn't jump to the present. Drag off the rail and let go: nothing moves.
9. The web client shows the same order and folders after each drop.
10. Clicking servers and folders still opens them and toggles folders (including a click with a pixel of movement); right-click menus still open.
11. With enough servers that the rail scrolls, drags near its ends still drop where shown (no auto-scroll).
12. Light and dark themes: the line, ring and faded item are visible.
