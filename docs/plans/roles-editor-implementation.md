# Implementation Record: Roles editor, part 1 (milestone 24)

_Plan: [`roles-editor.md`](roles-editor.md)_
_Started: 2026-11-09_

Run in continuous mode. Manual checks are batched at the end, as in recent milestones. The plan and its roadmap link (uncommitted before chunk 1) were committed with chunk 1, as milestone 23 did.

Rustfmt drift at the start (`Diff in` hunks): `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3, `tests/api_upload.rs` 10. `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/ui/profile_editor.rs`, `src/permissions.rs`, `src/profile_edit.rs`, `src/rail.rs`, `src/assets.rs`, `tests/server_settings.rs`, `tests/api_servers.rs` and `tests/store_roles.rs` were clean.

## Chunk 1: Roles page: list and create

### Changes Made
- `src/roles.rs` (new, no GPUI): `ROLE_NAME_MAX` (32), `ROLES_MAX` (200), `RoleRow { id, name, colour, icon, hoist, rank, lock }` with `lock_text()`, `lock_text(RoleLock)`, `role_rows(store, server)` (sorted by rank then ID, as `assignable_roles` sorts), `check_role_name`, `create_error_text` (`TooManyRoles`, `MissingPermission`, `FailedValidation`, else `error_text`), and `role_update(&Role) -> (PartialRole, Vec<FieldsRole>)`.
- `src/lib.rs`: `pub mod roles;` (by hand).
- `src/server_settings.rs`: `Access.roles` (Manage Role), counted by `any()`.
- `src/api/client.rs`: `create_role(server, name) -> NewRoleResponse`, sending only `{"name"}` (as `create_server` does, since `DataCreateRole` would add `"rank": null`). `NewRoleResponse` was added to the model import, with the import block hand-wrapped as rustfmt wants it.
- `src/ui/chat.rs`: `ChatView::role_saved(server, &Role, cx)`, which applies the whole role through `apply_role_update` (with `role_update`'s clears) and then `server_permissions_changed`.
- `src/ui/server_settings.rs`:
  - `roles_page`, second in `PAGES` (now 5: Overview, Roles, Invites, Bans, Ownership), with the `Shield` icon;
  - `RolesPage`, which observes the chat so it follows live role events: a header ("Roles higher in the list outrank those below them.") with "Create role…", then a row per role with a colour dot (`colour_dot`: filled, or a dashed circle), the name, the icon (`icons/{id}` from the file server, 18 px), "Hoisted", and a lock icon whose tooltip gives the reason;
  - `CreateRoleDialog`: the `name_field` from `server_dialogs` ("Role name", counter, error), Cancel and Create (Enter works too). On success it calls `role_saved` and closes; a refusal shows `create_error_text` under the field. It opens through the existing `open_focused_dialog`.
- `src/assets.rs`: `Lock` and `Shield` added to `ExtraIcons`, and `server_settings_icons_load` checks `Shield`, `Lock` and `Plus`.
- `tests/roles.rs` (new, 10 tests): row order and locks for the owner and a mid-rank moderator; a bot's role above the user is rank-locked; colours, icons and hoisting on rows; ties broken by ID; an unknown server; names (blank, 32, 33, trimmed); create error texts; `role_update` clears what the role lacks; a created role and then its `ServerRoleUpdate` give one role.
- `tests/server_settings.rs`: the fixture gains `u-roler` with a Manage Role role. `the_owner_sees_every_page` gains `roles: true`, the Manage Server test asserts `!roles`, and there's a new `manage_role_shows_only_roles`.
- `tests/api_roles.rs` (new, 3 tests): `POST /servers/srv/roles` sends exactly `{"name":"Mods"}` with the token and parses `NewRoleResponse`; `TooManyRoles { max: 200 }` and `MissingPermission` read as their texts.

### Divergences
- **`role_partial(&Role) -> PartialRole` became `role_update(&Role) -> (PartialRole, Vec<FieldsRole>)`.** A whole role applied as a partial can't take away a colour or icon that the saved role no longer has, so the clears come with it. This matters for chunks 3 and 4, where Stoat's answer to removing a colour or icon has neither.
- **Lock priority:** `role_rows` reports `Rank` before `Managed`, unlike `assignable_roles`, which reports `Managed` first. A bot's role above the user can't be edited at all, so rank is the stronger reason.
- **Opening the editor after Create** waits for chunk 2, since the editor doesn't exist yet. For now Create adds the role to the store, so it appears at the bottom of the list at once.
- The store test (created role, then its event) is in `tests/roles.rs`, not `tests/store_roles.rs`, because it exercises `role_update`.
- The create dialog reuses `server_dialogs::name_field`, whose counter uses `server_edit::NAME_MAX`. That is 32, the same as `ROLE_NAME_MAX`.

Look-ahead: none of the remaining chunks depends on the `role_partial` name, and chunk 2 calls `role_saved` as planned. The plan needs no amendments.

### Verification Performed
- `cargo test --test roles`: 10 passed.
- `cargo test --test api_roles --test server_settings`: 3 and 32 passed.
- Full suite: `cargo test` gave 1,195 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt drift: `src/ui/chat.rs` 17 and `src/api/client.rs` 3 are unchanged. `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/assets.rs` and `tests/server_settings.rs` are still clean, and the new `src/roles.rs`, `tests/roles.rs` and `tests/api_roles.rs` are clean.

### Manual Validation
Deferred to the end-of-milestone manual pass (plan's Manual Tests 1, 2 and 8). One thing to watch there: the sidebar shouldn't jump for an owner, who has Roles from the first render.

## Chunk 2: Role editor: name, hoist, delete

### Changes Made
- `src/roles.rs`:
  - `RoleForm { name, hoist }` with `of`, `is_valid` and `changes(&Role) -> Option<DataEditRole>`. `changes` sends only what changed and trims the name; it gives `None` when nothing changed or the name is invalid;
  - `empty_role_edit()`;
  - `may_edit(store, server, role)`: `Access.roles`, and the role ranking below `acting_rank`, so the owner may edit any role and a bot's role can be edited. `may_delete` is the same, plus not a bot's role;
  - `edit_error_text` (`NotElevated`, `MissingPermission`, `NotFound`, `FailedValidation`) and `delete_error_text` (`NotElevated`, `InvalidOperation`, `MissingPermission`).
- `src/api/client.rs`: `edit_role(server, role, &DataEditRole) -> Role`, whose body goes through `edit_body` so nulls and an empty `remove` are dropped, and `delete_role(server, role)`. `DataEditRole` and `Role` were added to the model import, hand-wrapped.
- `src/ui/chat.rs`: `ChatView::role_deleted(server, role, cx)` runs `apply_role_delete` and then `server_permissions_changed`.
- `src/ui/server_settings.rs`:
  - `RolesPage` gains `RolesMode::{List, Editing(Entity<RoleEditor>)}`, `open`, `back` and `follow`. It now observes with the window, so `follow` can toast. Rows are clickable (with a hover background) and open the editor.
  - `follow` goes back to the list with an info toast, "That role was deleted.", when the edited role leaves the store. It goes back silently when the user loses Manage Role, which drops the editor and any unsaved edit.
  - `CreateRoleDialog` emits `RoleCreated(id)` after `role_saved`, and the page opens that role's editor.
  - `RoleEditor` has:
    - "‹ Roles" (emits `BackToRoles`);
    - a heading with the role's colour dot and name;
    - a lock note when the role can't be changed (rank) or can't be deleted (a bot's);
    - "Role name" (the `field` helper with a counter; Enter saves);
    - a "Hoist role" checkbox with the web client's description;
    - Reset and Save, shown only when the role is editable;
    - "Copy role ID";
    - "Delete role…", shown only when deletable.
  - The editor's fields follow live changes until they're touched (`filled`, as `OverviewPage` does). Its rights are worked out on every render, so a role that moves to or above the user's rank turns read-only at once. The name field takes focus on first draw when the role is editable.
  - Save sends one `PATCH`, applies Stoat's answer through `role_saved`, resets the form and toasts "Saved."; a refusal toasts `edit_error_text` and keeps the edit.
  - Delete confirms through `confirm_member_action_then` ("Delete {name}?", "Members with it lose it, and so does every channel's override for it."). Success or `NotFound` emits `BackToRoles` first, so the page doesn't take it for a delete made elsewhere, then `role_deleted` and a "Deleted {name}." toast.
- `tests/roles.rs`: 9 new tests:
  - `RoleForm::changes`: unchanged, spaces only, a rename (trimmed), hoist alone, invalid names;
  - `may_edit` and `may_delete` for the owner, a moderator (above, own role, a bot's role below, below), a plain member, and unknown roles or servers;
  - a moderator losing edit rights when a role moves above them;
  - edit and delete error texts;
  - an edit applied locally, then its partial `ServerRoleUpdate`, stays consistent.
- `tests/api_roles.rs`: 6 new tests:
  - `PATCH /servers/srv/roles/r1` sends exactly `{"name":"Mods"}`, and exactly `{"hoist":false}` (no `rank`, no `remove`);
  - `NotElevated` reads as its text;
  - `DELETE` is sent once with no body;
  - `InvalidOperation` reads as "Roles that belong to a bot can't be deleted.";
  - an already-deleted role is `NotFound`.

### Divergences
- **Hoist is a checkbox**, gpui-component's `Checkbox` as used elsewhere in RuSToat, rather than a switch. The web client uses a checkbox too.
- `may_edit` also needs `Access.roles` (Manage Role and a known membership), so it answers fully on its own. Stoat checks the same.
- `edit_error_text` also covers `NotFound` (an icon ID Stoat doesn't know, from chunk 4) and `FailedValidation`.
- Losing Manage Role while editing returns the page to its list (as well as hiding the page through milestone 23's keyed reset). This answers the plan's "stale Save" risk. A Save already in flight still reaches Stoat, which decides; its answer goes nowhere if the editor is gone.
- `delete_error_text` is imported into `ui/server_settings.rs` as `delete_role_error_text`, because the server's own `delete_error_text` is already imported there.

Look-ahead: chunks 3 and 4 extend `RoleForm` and `RoleEditor` as planned, and chunk 4's upload uses the shared `Saving` enum, which already has `Uploading`. No plan amendments.

### Verification Performed
- `cargo test --test roles --test api_roles`: 20 and 9 passed.
- Full suite: `cargo test` gave 1,211 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt drift: `src/ui/chat.rs` 17 and `src/api/client.rs` 3 are unchanged. `src/ui/server_settings.rs`, `src/roles.rs`, `tests/roles.rs` and `tests/api_roles.rs` are clean.

### Manual Validation
Deferred to the end-of-milestone pass (plan's Manual Tests 3, 5 and 7, plus chunk 2's read-only check as a lower moderator). Also check there that the name field takes focus when the editor opens, and that the scroll position is sensible when switching between the list and the editor.

## Chunk 3: Role colour

### Changes Made
- `src/roles.rs`:
  - `ColourChoice { None, Hex(String), Custom(String) }` with `of(Option<&str>)`. `#rgb` and `#rrggbb` that `rail::parse_colour` reads become `Hex`, normalised to lower-case `#rrggbb`; anything else is `Custom`. `solid()` gives the colour to draw: a hex, or custom CSS that `store::solid_colour` can still draw, such as "red".
  - `hex_of(Hsla)` goes through `Hsla::to_rgb()` and rounds each channel; it ignores alpha.
  - `RoleForm` gains `colour: ColourChoice`. `changes` sends `colour: "#rrggbb"` for a new hex and `remove: ["Colour"]` for None, and never sends custom CSS. Choices are compared after normalising, so an unchanged colour sends nothing.
  - The module doc now says "no views (only GPUI's colour types)".
- `src/ui/server_settings.rs`, `RoleEditor`:
  - gains `colour: ColourChoice` and a `ColorPickerState`. Only `ColorPickerEvent::Change` sets `colour` (through `hex_of`), and `set_colour` writes the picker with `set_value` / `clear_value`, which emit nothing. So opening a role or following a live change never counts as an edit. `reset` and `follow` handle the colour like the other fields.
  - `render_colour` draws "Role colour" with:
    - `ColorPicker` (small), whose featured colours are `rail::PALETTE`, plus "No colour" (disabled when there's none or while saving);
    - for a read-only role, just the colour dot;
    - a "Custom colour (set elsewhere): {css}" note with a dot when the colour is custom;
    - a preview of the name as typed, in the colour, on the theme's background with a border.
  - It sits between the name and Hoist.
- `tests/roles.rs`: 7 new tests:
  - `ColourChoice::of` for `#E5484D`, ` #fa0 `, a gradient, `red`, `#ff000080` and `var(--x)`, and `solid()`;
  - `hex_of` round-trips every palette colour and all 256 grey steps;
  - another hex is sent, and the same hex in another case isn't a change;
  - None sends `remove: [Colour]`;
  - a custom gradient survives a rename (only `name` is sent), and picking a hex replaces it;
  - a saved role without a colour clears it in the store and on its row.
  - The existing form tests now build forms with `..RoleForm::of(&role)`.

### Divergences
- `hex_of` rounds where gpui-component's `Colorize::to_hex` truncates, which is the round-trip risk the plan raised. The grey-step test covers it.
- The preview uses the name as typed (falling back to the saved name when the field is blank), not just the saved name.
- `ColourChoice` has `solid()`, so a custom CSS colour name still draws as its colour in the dot and the preview.
- No `ColorSelect` or anchor change yet: whether the picker's popover is clipped by the settings page can only be seen by running the app, so it's on the manual list.

Look-ahead: chunk 4 adds `icon` to `RoleForm` beside `colour`, and the existing tests' `..RoleForm::of(&role)` pattern absorbs it. No plan amendments.

### Verification Performed
- `cargo test --test roles`: 27 passed.
- Full suite: `cargo test` gave 1,218 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/server_settings.rs`, `src/roles.rs` and `tests/roles.rs` are clean.

### Manual Validation
Deferred to the end-of-milestone pass (plan's Manual Tests 3, 4 and 9). Also check there that the picker's popover isn't clipped inside the settings page; if it is, switch to `ColorSelect` or another anchor.

## Chunk 4: Role icon

### Changes Made
- `src/roles.rs`:
  - `RoleForm` gains `icon: ProfileImage` (`Current(file ID)` from the role, or `None`), with `new_file()` and `has_changes(&Role)`, which is true for a new file to upload or a change to send;
  - `changes` uses milestone 23's `image_change` rule: a new uploaded ID goes in `icon` with no remove, a removed icon is `remove: ["Icon"]`, and a picked file is left out until it's uploaded.
- `src/server_settings.rs`: `image_change` is `pub(crate)` so both forms share it.
- `src/ui/profile_editor.rs`: a new `square_preview(id, Preview, cx)`, 64 px with rounded corners and centre-cropped. With no icon it shows "No icon"; a file it can't draw shows the file's name. The avatar and banner previews are unchanged.
- `src/ui/server_settings.rs`, `RoleEditor`:
  - gains `icon: ImageSlot`, with `pick_icon` (checked against `ImageLimits.server_icon`, the `icons` limit) and `render_icon`;
  - `render_icon` uses `render_image_row`: "Role icon", the square preview, Change… and Remove, all disabled when the role is read-only or saving. It sits between the colour and Hoist.
  - `reset` and `follow` handle the icon as `OverviewPage` does, and an image problem enables Reset.
  - `can_save` also needs a valid name, no icon problem and `has_changes`.
  - `save` now uploads a new icon first (`Saving::Uploading`, with Save reading "Uploading…"), then works out the `PATCH` against the role as it is now and sends it (`Saving::Sending`). If the file server isn't known, it toasts and sends nothing.
  - `save_failed` toasts `edit_error_text` and keeps the form dirty after a failed upload or edit.
  - The list rows have drawn icons since chunk 1.
- `tests/roles.rs`: 4 new tests: the form starts with the role's icon; a new icon is sent by ID with no remove; a removed icon is in `remove` (and removing both colour and icon sends both); a picked icon waits for its upload, though a rename goes ahead without it.
- `tests/api_roles.rs`: 3 new tests:
  - an icon upload to `/icons` (multipart, with the token), then `PATCH /servers/srv/roles/r1` with exactly `{"icon":"f-icon"}`;
  - `{"remove":["Colour","Icon"]}`;
  - `{"colour":"#3e63dd"}`.

### Divergences
- `square_preview` is a new function, not a shape parameter on `avatar_preview`, so existing callers are untouched. It's 64 px, smaller than the 80 px avatar preview.
- The upload-then-edit test is in `tests/api_roles.rs`, not `tests/api_upload.rs`, which keeps `api_upload.rs`'s 10 drift hunks out of the picture. Two extra wiremock tests cover chunk 3's colour bodies.

Look-ahead: chunk 5 touches only the list and adds new functions, so nothing here affects it. No plan amendments.

### Verification Performed
- `cargo test --test roles --test api_roles`: 31 and 12 passed.
- Full suite: `cargo test` gave 1,225 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/server_settings.rs`, `src/ui/profile_editor.rs`, `src/server_settings.rs`, `src/roles.rs`, `tests/roles.rs` and `tests/api_roles.rs` are clean.

### Manual Validation
Deferred to the end-of-milestone pass (plan's Manual Test 3, and chunk 4's checks: set a PNG, remove it, and a 3 MB file refused before upload).

Note: chunk 4's commit was delayed. `git commit` hung twice waiting on the SSH signing prompt (`commit.gpgsign`, with a key held by a macOS app). Chunk 4 stayed staged and untouched while chunk 5 was built in the working tree, and was committed on its own (`15a437e`) once signing went through.

## Chunk 5: Drag roles to reorder

### Changes Made
- `src/roles.rs`:
  - `RoleDrop { Before(String), After(String) }`;
  - `role_order(store, server)` (the list's IDs, highest first);
  - `reorder(order, dragged, &RoleDrop) -> Option<Vec<String>>`, `None` for unknown roles, beside itself, or a drop that changes nothing;
  - `may_reorder(store, server, old, new)`, which mirrors `roles_edit_positions.rs`: Manage Role; every role exactly once (no missing, extra or repeated IDs); the owner may do anything; otherwise every role ranked at or above the user's `acting_rank` keeps its position. A member with no roles has `i64::MAX`, so they can move nothing, as Stoat's `None` top rank;
  - `draggable` (same as `may_edit`; a bot's role moves like any other);
  - `reorder_error_text` (`NotElevated`, `InvalidOperation`, `MissingPermission`).
- `src/api/client.rs`: `edit_role_ranks(server, &[String]) -> Server`, sending `{"ranks":[…]}` (signature hand-wrapped).
- `src/ui/chat.rs`: `ChatView::roles_reordered(server, ranks, cx)` runs `apply_role_ranks` and then `server_permissions_changed`. The rail's `with_drop_mark`, `DropMark`, `DragPreview` and `DRAGGED_OPACITY` were already `pub(crate)`, so nothing else changed there.
- `src/ui/server_settings.rs`, `RolesPage`:
  - gains `drag: Option<RoleDragState { dragged, target }>` and `reordering: Option<Task<()>>`;
  - rows the user may drag (and only while no order is being saved) are `on_drag` sources carrying `RoleDrag { id }`, with a `DragPreview` pill showing the colour dot and name. The dragged row fades to `DRAGGED_OPACITY`;
  - each row is wrapped in a `relative` div through `with_drop_mark` and takes `on_drag_move`. The row's top half targets `Before`, its bottom half `After`, and the target is kept only if `reorder` gives a new order that `may_reorder` accepts;
  - the list (`gap_2`, which the drop lines are centred in, with rows now `py_1`) clears the target when the pointer leaves it, handles `on_drop`, and forgets the drag on `on_mouse_up_out`;
  - a drop applies the new order to the store at once through `roles_reordered` and sends one `PATCH …/roles/ranks`. Success changes nothing more, since the `ServerRoleRanksUpdate` that follows agrees. A refusal re-applies the order from before the drop and toasts `reorder_error_text`;
  - while the order is being saved, the header reads "Saving the order…" and no row is draggable. Otherwise it reads "Roles higher in the list outrank those below them. Drag one to move it.";
  - clicking a row still opens its editor, because a drag starts only after a few pixels and cancels the click.
- `tests/roles.rs`: 14 new tests:
  - `role_order`;
  - `reorder` before and after, and its no-op and unknown cases;
  - `may_reorder`:
    - the owner may move anything;
    - a moderator may reorder the roles below them, including a bot's;
    - nobody may move their own role, a higher one, or a lower one into a place at or above theirs;
    - an order missing a role, repeating one, or with an extra one is refused;
    - without Manage Role nothing moves;
  - `draggable` matches;
  - reorder error texts;
  - store: our order then the same `ServerRoleRanksUpdate` is one consistent order, and a later one from someone else wins;
  - a refused order is put back.
- `tests/api_roles.rs`: 2 new tests: `PATCH /servers/srv/roles/ranks` sends exactly `{"ranks":["r-a","r-c","r-b"]}` and parses the `Server`; `NotElevated` reads as its text.

### Divergences
- `reorder` takes `&RoleDrop`, and a `role_order` helper was added for the list's order.
- `may_reorder` also rejects repeated IDs and needs Manage Role. Stoat's length-plus-contains check rejects repeats in effect too.
- Putting the order back after a refusal re-applies the order shown before the drop. If someone else's reorder arrived during the request, it's overwritten locally until the next `ServerRoleRanksUpdate`. That race is narrow, and Stoat's state is unaffected.
- **No edge auto-scroll** while dragging. Each row's `on_drag_move` uses window bounds, so rows scrolled into view still take drops, but long lists have to be dragged in stages or scrolled with the wheel. Left as a follow-up, as the plan allowed.
- The list's spacing changed (rows `py_1` with `gap_2`) so the rail's drop line sits centred between rows.
- The store tests are in `tests/roles.rs`, not `tests/store_roles.rs`, beside the reorder logic.

### Verification Performed
- `cargo test --test roles --test api_roles`: 42 and 14 passed.
- Full suite: `cargo test` gave 1,238 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt drift, final: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3 and `tests/api_upload.rs` 10, all unchanged. `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/ui/profile_editor.rs`, `src/permissions.rs`, `src/assets.rs` and `tests/server_settings.rs` are clean, and so are the new `src/roles.rs`, `tests/roles.rs` and `tests/api_roles.rs`.

### Manual Validation
Deferred to the end-of-milestone pass (plan's Manual Test 6, and chunk 5's checks: drag from bottom to top as the owner; as a mid-rank moderator, no drag handles or drop lines at or above their role; dragging off the list changes nothing).

## Summary

_Completed: 2026-11-09_

### Chunks Completed
1. Roles page: list and create (`12ebac9`)
2. Role editor: name, hoist, delete (`d17d7ba`)
3. Role colour (`35900dd`)
4. Role icon (`15a437e`)
5. Drag roles to reorder (this commit)

### Divergences
- `role_partial` became `role_update(&Role) -> (PartialRole, Vec<FieldsRole>)`, so that saving a role without a colour or icon clears it in the store (chunk 1).
- Rank locks take priority over Managed in the list (chunk 1).
- Opening the new role's editor after Create arrived with chunk 2.
- Hoist is a checkbox (chunk 2).
- `may_edit` and `may_reorder` also need Manage Role. Losing Manage Role while editing returns the page to its list (chunk 2).
- `hex_of` rounds where gpui-component's `to_hex` truncates. Custom CSS colour names still draw (chunk 3).
- `square_preview` is a new 64 px helper (chunk 4).
- The upload-then-edit and store tests are in `tests/api_roles.rs` and `tests/roles.rs` rather than `tests/api_upload.rs` and `tests/store_roles.rs` (chunks 1, 4 and 5).
- There's no edge auto-scroll while dragging. A refused reorder re-applies the order from before the drop (chunk 5).
- Chunk 4's commit was delayed by a signing prompt; see the note above.

### Definition of Done
- [x] The Roles page lists every role highest first, with colour, icon, hoist note and lock reason. It shows for Manage Role or the owner, and "Server settings…" is offered with only Manage Role. Covered by unit tests; to be confirmed manually.
- [x] Create role adds a role (name 1–32) at the bottom and opens it. `TooManyRoles` reads plainly.
- [x] The role editor saves name, hoist, colour (palette or hex `#rrggbb`, or none) and icon (`icons`, within its limit, set or removed). It sends only what changed, keeps custom CSS colours unless changed, and follows live edits.
- [x] Delete role is offered for unmanaged roles below the user's rank (any unmanaged role for the owner), confirms, and removes the role everywhere at once.
- [x] Dragging reorders roles with one `PATCH …/roles/ranks` per drop, only where Stoat's rank rule allows. The change shows at once, and is put back on refusal.
- [x] Roles the user can't edit (rank) open read-only and say why. A bot's role says it can be edited but not deleted.
- [x] Stoat's refusals become plain toasts: `MissingPermission`, `NotElevated`, `InvalidOperation`, `TooManyRoles`, and `NotFound` (already deleted counts as done; an edit's says Stoat couldn't find something).
- [x] `cargo test` (1,238 passed) and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and the new files are clean.
- [x] The roadmap links this plan.

### Remaining Manual Steps
The plan's Manual Tests 1–9 haven't been run. They need two accounts, the web client, and a test server owned by the first account. Points to watch, gathered from the chunks:
- the sidebar doesn't jump when the window opens for an owner;
- the name field takes focus when the editor opens, and switching between the list and the editor scrolls sensibly;
- the colour picker's popover isn't clipped inside the settings page (if it is, switch to `ColorSelect` or another anchor);
- a 3 MB icon is refused before upload;
- dragging feels right without edge auto-scroll;
- drop lines, locks, the picker and the previews are readable in both themes.
