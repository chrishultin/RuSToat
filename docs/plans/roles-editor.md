# Plan: Roles editor, part 1 (RuSToat milestone 24)

## Summary
Milestone 24 of the [roadmap](roadmap.md) adds a **Roles page** to the Server Settings window built in milestone 23 ([`server-settings.md`](server-settings.md)). On it, someone with **Manage Role** can:
- list the server's roles, highest first;
- create, rename and delete roles;
- set a role's colour, hoisting and icon;
- reorder roles by dragging them.

Milestone 25 (the permission matrix) adds permissions to the same role editor, so the editor is built with room for it.

## Approach
**What Stoat allows.** Read from `/tmp/stoatchat-0.16.2` on 2026-11-09. Paths are under `crates/`.

**Every role route needs Manage Role** (`MissingPermission` otherwise):
- **Create:** `POST /servers/{id}/roles` with `DataCreateRole { name }` (`delta/src/routes/servers/roles_create.rs`).
  - `name` is 1–32 characters. `rank` is ignored: a new role goes to the bottom (`rank = roles.len()`, `core/database/src/models/servers/model.rs` `create_inner`).
  - The answer is `NewRoleResponse { id, role }`.
  - Past `server_roles` roles (200 in `Revolt.toml`; `GET /` doesn't send it), the refusal is `TooManyRoles { max }`.
  - `ServerRoleUpdate` (the whole role) goes to every member.
- **Edit:** `PATCH /servers/{id}/roles/{role}` with `DataEditRole { name, colour, hoist, icon, remove: [Colour, Icon] }` (`roles_edit.rs`).
  - `rank` and `permissions` are ignored here.
  - `colour` is 1–128 characters of CSS (the `RE_COLOUR` validator).
  - `icon` is an Autumn file ID uploaded with the tag `icons`. An unknown or used ID is `NotFound`.
  - **The role must rank below the user's best role** (`role.rank <= member_rank` gives `NotElevated`). The owner counts as ranked above everyone (`unwrap_or(i64::MIN)`).
  - The answer is the `Role`; `ServerRoleUpdate` follows.
- **Delete:** `DELETE /servers/{id}/roles/{role}` (`roles_delete.rs`).
  - It has the same rank rule (`NotElevated`).
  - A managed role (one a bot owns, `role.owner`) is `InvalidOperation`. An unknown one is `NotFound`.
  - `ServerRoleDelete` follows.
- **Reorder:** `PATCH /servers/{id}/roles/ranks` with `DataEditRoleRanks { ranks: [role ids] }` (`roles_edit_positions.rs`).
  - The list is **every role, highest first**. Missing or extra IDs are `InvalidOperation`.
  - **A non-owner may not change the position of any role at or above their own best rank** (`NotElevated`). In effect, they reorder only the roles below them, among the positions below them.
  - Ranks become the indices (`set_role_ordering`). The answer is the `Server`; `ServerRoleRanksUpdate { id, ranks }` follows.
- **Size limit:** role icons share the `icons` limit (2.5 MB; milestone 23's `RootConfig::icon_size_limit`).

**What the web client does** (`/tmp/for-web-0.16.3/packages/client/components/app/interface/settings/server/roles/`):
- **`ServerRoleOverview.tsx`:**
  - "Server Roles", a drag-handled list (each row has a colour dot, or a dashed circle with no colour, the name and the icon), and a "+" button that opens a "create role" modal and then the new role;
  - an "Everyone" row ("Permissions available without any role"), which is milestone 25's;
  - every drop sends the whole order (`setRoleOrdering`).
- **`ServerRoleEditor.tsx`:**
  - "Role Name" (counter, 1–32);
  - "Role Colour", a colour picker with swatches and a light/dark preview of the name;
  - "Role Icon", cropped square, `icons` limit;
  - "Hoist Role", a checkbox: "Display users with this role separately in the member list";
  - the permissions editor (milestone 25);
  - "Copy role ID" and "Delete Role".
  - Save sends only what changed: a cleared colour or icon goes in `remove`, and the icon is uploaded first.

**What exists in RuSToat** (as of 2026-11-09, after milestone 23):
- **Server Settings:**
  - `src/ui/server_settings.rs` has `ServerSettingsView`, with `PAGES: [ServerPageFn; 4]` (Overview, Invites, Bans, Ownership). Each `ServerPageFn` returns `Option<SettingPage>`;
  - when the set of visible pages changes, the keyed page reset sends the sidebar back to its first page;
  - the window closes itself when its server goes or no page is left;
  - `Load<T>` and the `load_once` pattern; `OverviewPage` (`filled` / `follow` live-follow, Reset, Save, `Saving::{Uploading, Sending}`, `save_failed`);
  - `confirm_member_action_then` (a confirmation whose answer goes to a callback).
- **Logic:** `src/server_settings.rs` holds `Access { overview, invites, bans, ownership }` and `access(store, server)`. That decides who sees "Server settings…" (`Access::any`).
- **Images:** `src/ui/profile_editor.rs` has `ImageSlot`, `prompt_for_image`, `upload_image(client, server, tag, path)`, `Preview::of(image, tag, …)`, `avatar_preview` and `render_image_row`. `ProfileImage` is in `src/profile_edit.rs`, and `ImageLimits.server_icon` is the `icons` limit.
- **Live role events are already handled.**
  - `ServerRoleUpdate`, `ServerRoleDelete` and `ServerRoleRanksUpdate` are parsed in `src/api/events.rs`.
  - `src/store.rs` has `apply_role_update`, `apply_role_delete` (taken off members and channel overrides too) and `apply_role_ranks` (rank = index).
  - `ChatView::server_permissions_changed` runs after each.
  - The roadmap's "needs `ServerRoleRanksUpdate` events" is therefore already met, and this milestone only sends edits.
- **Roles are already drawn elsewhere:**
  - the member list hoists roles (`store.rs` ~2744);
  - names take the best role's colour through `store::solid_colour` (CSS to `#rrggbb`; gradients give `None`);
  - `permissions::assignable_roles` sorts roles by rank and marks `RoleLock::{Managed, Rank}`;
  - `permissions::acting_rank` gives the owner `i64::MIN`.
- **Colours:**
  - `rail::PALETTE` (12 named hex colours) and `rail::parse_colour`;
  - gpui-component's `ColorPicker` / `ColorPickerState` (`gpui_kit::component::color_picker`): `featured_colors`, a hex field, `ColorPickerEvent::Change(Option<Hsla>)`, `set_value` / `clear_value`, and `Hsla::to_hex()`.
- **Drag and drop:** the rail's (`src/ui/chat.rs` ~3940–4080, ~13560–13660):
  - `on_drag` with a `DragPreview`, and `on_drag_move` per row to set a target;
  - `with_drop_mark(item, DropMark::{Above, Below}, cx)` draws the line;
  - `rail::apply_drop` is the pure reordering, with unit tests.

**The chosen shape:**
- **A Roles page with in-page navigation** (user's choice).
  - `roles_page` shows a `RolesPage` entity. It is either **the list** or **one role's editor**, with a "‹ Roles" link back.
  - The sidebar can't hold a page per role, since the pages are a fixed list of functions.
  - Rejected: a dialog per role. It would be cramped once milestone 25's permission matrix joins the editor.
- **Plain logic in a new `src/roles.rs`, with no GPUI**, unit-testable, as `server_settings.rs` is for milestone 23:
  - the ordered role rows and what the user may do with each;
  - the role form and its diff to a `DataEditRole`;
  - colour choices;
  - the reorder and Stoat's rank rule;
  - error texts.
- **Access gains `roles`** (Manage Role), so "Server settings…" also shows for someone who may only manage roles.
- **Saving applies at once:** create, edit, delete and reorder update the store right away, through the existing `apply_role_update`, `apply_role_delete` and `apply_role_ranks` paths and `server_permissions_changed`. The event that follows changes nothing.
- **Reordering is drag and drop** (user's choice), modelled on the rail:
  - a pure `roles::reorder(order, dragged, target) -> Option<Vec<String>>` and `roles::may_reorder(…)`, mirroring Stoat's check;
  - drop lines through the rail's `with_drop_mark`;
  - one `PATCH …/roles/ranks` per drop.
- **Colour is gpui-component's `ColorPicker`** (user's choice), with `rail::PALETTE` as its featured colours, a hex field, and "No colour".
  - A colour set elsewhere that `parse_colour` can't draw (a gradient, a CSS name) is shown as "Custom colour (set elsewhere)" and kept unless it's changed. RuSToat never writes anything but `#rrggbb`.

## Assumptions
1. **Who may see the Roles page:** anyone with Manage Role (the owner too), with a known membership, as Overview requires. It goes after Overview in the sidebar: Overview, Roles, Invites, Bans, Ownership, as in the web client's order.
2. **The list** shows every role, highest first, as `rank` sorts them (ties by ID, as `assignable_roles` does). Each row has:
   - a colour dot (or a dashed circle), the name, and the icon if it has one;
   - "Hoisted" as a muted note;
   - a lock with a tooltip on roles the user can't edit: "Ranks at or above your highest role", or "Managed by a bot" (which can be edited but not deleted).

   Clicking a row opens its editor; locked rows open read-only. "Everyone" (default permissions) is left to milestone 25.
3. **Create role…** opens a small dialog: "Role name", 1–32 characters, with "Create". On success the new role is added to the store at once and its editor opens, as the web client does. `TooManyRoles` toasts "This server has {max} roles, the most Stoat allows."
4. **The editor** holds the name, colour, icon, hoist, Save and Reset, "Copy role ID", and a red "Delete role…".
   - Fields follow live changes until touched, then keep the edit (the Overview's `filled` pattern).
   - Save sends one `PATCH` with only what changed, after uploading a new icon.
   - If the role is deleted elsewhere while open, the page goes back to the list with a toast "That role was deleted."
   - If it moves to or above the user's rank (a reorder elsewhere, a role edit), the editor turns read-only.
5. **Delete role…** confirms with `confirm_member_action_then`: "Delete {name}?", "Members with it lose it, and so does every channel's override for it." It's offered unless the role is managed or locked by rank. On success the role leaves the store and the page returns to the list. `NotFound` counts as done.
6. **Colour:**
   - the picker's value is written as `#rrggbb` (lower case);
   - "No colour" sends `remove: ["Colour"]`;
   - an unchanged custom CSS colour is left out of the edit.
   - A small preview shows the name in the chosen colour on the theme's background, rather than the web client's two light and dark previews.
7. **Icon:** works as the server icon does in milestone 23:
   - `icons` tag and the `icons` limit;
   - Change… / Remove, and no crop editor;
   - a square preview with rounded corners, centre-cropped.

   Role icons aren't drawn anywhere else in RuSToat yet, so only the list and the editor show them.
8. **Drag to reorder:**
   - Only rows the user may move are draggable: below their best role, or every role for the owner. Managed roles can be moved like any other, as Stoat allows.
   - Drop lines appear only where the result passes `may_reorder`.
   - A drop sends the full order and applies it at once. A refusal toasts and puts the order back (the store keeps Stoat's last order until the answer).
   - While the editor is open, the list isn't shown, so there's no drag there.
9. **Reorders made while saving:** with one in flight, further drags are disabled ("Saving the order…"), so two orders can't race.

## Plan

### Chunk 1: Roles page: list and create
**Goal:** someone with Manage Role sees the server's roles, highest first, and can create one.
**Files likely touched:**
- `src/roles.rs` (new, no GPUI):
  - `ROLE_NAME_MAX` (32), `ROLES_MAX` (200);
  - `RoleRow { id, name, colour: Option<String /* #rrggbb */>, icon: Option<String /* file ID */>, hoist, rank, lock: Option<RoleLock> }`;
  - `role_rows(store, server)`, sorted as `assignable_roles` sorts, reusing `RoleLock` from `permissions.rs`;
  - `check_role_name(text)` (trimmed, 1–32);
  - `create_error_text` (`TooManyRoles { max }`, `MissingPermission`, `FailedValidation`);
  - `role_partial(&Role) -> PartialRole`, for applying a whole role to the store.
- `src/server_settings.rs`: `Access` gains `roles` (Manage Role), counted by `any()`.
- `src/api/client.rs`: `create_role(server, name) -> NewRoleResponse`.
- `src/ui/chat.rs`: `ChatView::role_saved(server, &Role, cx)` (`apply_role_update` with the whole role, then `server_permissions_changed`).
- `src/ui/server_settings.rs`:
  - `roles_page` in `PAGES` after Overview;
  - `RolesPage` in list mode (rows as above) with "Create role…";
  - `CreateRoleDialog` (one field with a counter, and Create).
- `src/lib.rs`: the module, by hand.
- `tests/roles.rs` (new); `tests/server_settings.rs` (access); `tests/api_servers.rs` or a new `tests/api_roles.rs`; `src/assets.rs` (icons for lock and plus, if not bundled).

**Depends on:** None (milestone 23 is done)
**Risks:**
- **`Access` gaining a field** changes milestone 23's exact `Access { … }` test values. Update them; `the_owner_sees_every_page` gains `roles: true`.
- **Page order shifts.** Inserting Roles second changes the page indices. The keyed reset already handles changing page sets, but check that the sidebar doesn't jump when Roles appears for an owner who had it all along.
- **A role created here and its `ServerRoleUpdate`.** Applying the whole role then the event must give one role, not two. `apply_role_update` merges by ID, so check it with a store test.

**How to verify:**
- `cargo test --test roles`:
  - `role_rows` order and locks: the owner, a mid-rank moderator, and a managed role;
  - `check_role_name`: blank, 32 and 33 characters.
- `cargo test --test server_settings`: `access` gives `roles` for Manage Role only.
- wiremock: `POST /servers/srv/roles` sends `{"name":"Mods"}` and parses `NewRoleResponse`; `TooManyRoles { max: 200 }` reads as its text.
- Store: creating, then the matching `ServerRoleUpdate`, gives one role with the same fields.
- Then the full suite.

**Manual check:**
- As the owner, Roles lists every role in the web client's order.
- Create "Testers": it appears at the bottom and its editor opens, and the web client shows it.
- An account with only Manage Role sees "Server settings…" and the Roles page, with the roles above it locked.

**Shippable when:** moderators with Manage Role can see the roles in order and create new ones from Server Settings.

### Chunk 2: Role editor: name, hoist, delete
**Goal:** open a role, rename it, hoist it or not, copy its ID, or delete it.
**Files likely touched:**
- `src/roles.rs`:
  - `RoleForm { name, hoist }` with `of(&Role)` and `changes(&Role) -> Option<DataEditRole>` (only what changed; `None` when nothing did or the name is invalid);
  - `empty_role_edit()` (`DataEditRole` has no `Default`);
  - `may_edit(store, server, role) -> bool` / `may_delete` (rank and managed rules);
  - `edit_error_text` / `delete_error_text` (`NotElevated` "That role ranks at or above your highest role.", `InvalidOperation` "Roles that belong to a bot can't be deleted.", `MissingPermission`).
- `src/api/client.rs`: `edit_role(server, role, &DataEditRole) -> Role` (through `edit_body`) and `delete_role(server, role)`.
- `src/ui/chat.rs`: `ChatView::role_deleted(server, role, cx)` (`apply_role_delete`, then `server_permissions_changed`).
- `src/ui/server_settings.rs`:
  - `RolesPage` gains `Editing(Entity<RoleEditor>)` mode with "‹ Roles";
  - `RoleEditor`: name field with counter, a Hoist switch with the web client's description, Reset, Save, "Copy role ID", and "Delete role…";
  - live-follow; going back to the list when the role is deleted elsewhere; read-only when locked.
- `tests/roles.rs`, `tests/api_roles.rs`.

**Depends on:** Chunk 1
**Risks:**
- **In-page navigation inside a `SettingItem::render`.** The page is one item rendering the `RolesPage` entity, so switching modes is just state. Check that scroll position and focus behave when switching (the field gets focus on open).
- **The page set changing while editing.** If Manage Role is taken away, the Roles page disappears through milestone 23's keyed reset. Check that the editor entity is dropped with it and doesn't send a stale Save.
- **`DataEditRole` serialisation.** Its `rank` and `permissions` must not be sent as `null`; `edit_body` strips nulls. Test the exact body.

**How to verify:**
- Unit tests:
  - `RoleForm::changes`: unchanged gives `None`; a rename sends only `name` (trimmed); toggling hoist sends only `hoist`;
  - `may_edit` / `may_delete` for the owner, a moderator below and above the role, and a managed role;
  - error texts.
- wiremock: `PATCH /servers/srv/roles/r1` sends `{"name":"Mods"}`; `DELETE /servers/srv/roles/r1` sends once; `NotElevated` reads as its text.
- Then the full suite.

**Manual check:**
- Rename a role and hoist it: the member list groups it at once, and the web client agrees.
- Delete it: it leaves members and the list.
- As a lower moderator, a higher role opens read-only with no Delete.

**Shippable when:** a role's name and hoisting can be changed, and a role deleted, from its editor, within Stoat's rank rules.

### Chunk 3: Role colour
**Goal:** pick a role's colour from the palette or a hex value, or remove it.
**Files likely touched:**
- `src/roles.rs`:
  - `ColourChoice { None, Hex(String), Custom(String) }` from a role's CSS colour: `#rrggbb` if `rail::parse_colour` reads it, else `Custom`;
  - `RoleForm` gains `colour: ColourChoice`. `changes` sends `colour: "#rrggbb"`, `remove: [Colour]` for None, and nothing for an unchanged choice (Custom included);
  - `hex_of(Hsla)`, normalised to lower-case `#rrggbb`.
- `src/ui/server_settings.rs`:
  - `RoleEditor` gains a `ColorPickerState` (featured colours from `rail::PALETTE`) and "No colour";
  - a "Custom colour (set elsewhere)" note with its swatch when it can't be drawn;
  - a preview of the name in the colour.
- `tests/roles.rs`.

**Depends on:** Chunk 2
**Risks:**
- **Round-tripping through `Hsla`.** A hex value turned into `Hsla` and back may not give the same hex. Only `ColorPickerEvent::Change` should set the form's colour, and the comparison should be on normalised hex strings, so opening a role never reads as a change.
- **The picker inside the settings page.** Its popover anchors to the button, so check it isn't clipped by the page's scroll area. If it is, use `ColorSelect` or `anchor(Anchor::TopLeft)`.
- **Gradients from the web client** must survive a Save of the name alone. A test covers it.

**How to verify:**
- Unit tests:
  - `#E5484D` is `Hex("#e5484d")`; `linear-gradient(…)` and `red` are `Custom`;
  - changing to another hex sends it; None sends `remove: [Colour]`;
  - an unchanged Custom colour sends nothing;
  - `hex_of` of a palette colour round-trips.
- Then the full suite.

**Manual check:**
- Pick a palette colour: the name colour changes in the member list and messages at once, and in the web client.
- Type a hex value.
- Remove the colour.
- A gradient set in the web client shows as Custom and survives a rename.

**Shippable when:** role colours can be set to any solid colour or removed, without disturbing colours RuSToat can't draw.

### Chunk 4: Role icon
**Goal:** set or remove a role's icon.
**Files likely touched:**
- `src/roles.rs`: `RoleForm` gains `icon: ProfileImage`, with `new_file()`. `changes` sends a new uploaded ID as `icon`, a removed one as `remove: [Icon]`, and leaves out a picked file until it's uploaded (milestone 23's `image_change` rule).
- `src/server_settings.rs`: make `image_change` `pub(crate)` (or move it to `profile_edit.rs`) so both forms share it.
- `src/ui/server_settings.rs`:
  - `RoleEditor` gains an `ImageSlot` and an image row (square preview, `icons` tag, `ImageLimits.server_icon`);
  - Save uploads first (`Saving::Uploading`, then `Sending`), as `OverviewPage::save` does;
  - list rows draw the icon.
- `tests/roles.rs`; `tests/api_upload.rs` or `tests/api_roles.rs` (upload then edit).

**Depends on:** Chunk 2
**Risks:**
- **A square preview** isn't in `profile_editor.rs` (avatars are round, banners are strips). Add a small `square_preview`, or a shape parameter to `avatar_preview`, without changing the existing callers' look.
- **A failed upload** must keep the form dirty and send nothing, as in milestone 23.

**How to verify:**
- Unit tests: a new icon sends `icon` with no remove; a removed one sends `remove: [Icon]`; a picked one waits for its upload.
- wiremock: upload to `/icons`, then `PATCH /servers/srv/roles/r1` `{"icon":"f-icon"}`.
- Then the full suite.

**Manual check:**
- Set a PNG icon: it shows in the list, the editor, and the web client.
- Remove it.
- A 3 MB file is refused before upload.

**Shippable when:** role icons can be set and removed from RuSToat.

### Chunk 5: Drag roles to reorder
**Goal:** drag a role up or down the list to change its rank.
**Files likely touched:**
- `src/roles.rs`:
  - `RoleDrop { Before(String), After(String) }`;
  - `reorder(order: &[String], dragged, target) -> Option<Vec<String>>` (`None` when nothing changes);
  - `may_reorder(store, server, old, new) -> bool`: Stoat's rule (the owner may do anything; otherwise no role at or above the user's best rank changes position) plus "every role exactly once";
  - `draggable(store, server, role)`;
  - `reorder_error_text`.
- `src/api/client.rs`: `edit_role_ranks(server, &[String])` (`PATCH /servers/{id}/roles/ranks`, answering the `Server`).
- `src/ui/chat.rs`:
  - `ChatView::roles_reordered(server, &[String], cx)` (`apply_role_ranks`, then `server_permissions_changed`);
  - make `with_drop_mark`, `DropMark` and `DragPreview` usable from `server_settings.rs` if they aren't already (`pub(crate)`).
- `src/ui/server_settings.rs`, `RolesPage`'s list:
  - rows are `on_drag` sources (a `RoleDrag { id }` with a `DragPreview` of the row) when `draggable`;
  - `on_drag_move` targets above or below each row's midpoint, kept only if `may_reorder` passes;
  - drop lines through `with_drop_mark`;
  - on drop, apply at once and send. A refusal toasts and restores Stoat's order.
  - "Saving the order…" disables dragging meanwhile.
- `tests/roles.rs`, `tests/api_roles.rs`.

**Depends on:** Chunk 1 (and the list as chunks 2–4 leave it)
**Risks:**
- **Dragging inside gpui-component's settings scroll area.**
  - The rail drags inside its own scroll view; here the page scrolls.
  - Check that `on_drag_move` fires for rows scrolled into view during the drag, and that nothing breaks without auto-scroll.
  - If long lists need it, add edge auto-scroll as the rail does (or note it as a follow-up).
- **The drop must not race a `ServerRoleRanksUpdate`** from someone else: apply ours, and let the later event win. A test covers the order of application.
- **Clicks vs drags.** A drag starts after a few pixels and cancels the click (as on the rail), so clicking a row still opens its editor.
- **`chat.rs` drift is 17 hunks.** Hand-format, and keep the count.

**How to verify:**
- Unit tests:
  - `reorder` moves a role before or after another, and gives `None` for a no-op;
  - `may_reorder`:
    - the owner may move anything;
    - a moderator may reorder roles below them;
    - nobody may move their own role, a higher one, or another role into a position at or above their own;
    - a list missing a role is refused.
  - `draggable` matches.
- wiremock: `PATCH /servers/srv/roles/ranks` sends `{"ranks":["r-a","r-c","r-b"]}`; `NotElevated` reads as its text.
- Store: applying our order and then the same `ServerRoleRanksUpdate` gives one consistent order.
- Then the full suite.

**Manual check:**
- As the owner, drag a role from the bottom to the top. The member list's hoisted sections and name colours follow at once, and the web client shows the new order.
- As a mid-rank moderator, roles above you can't be dragged, and no drop line appears at or above your role.
- Drag a role off the list: nothing changes.

**Shippable when:** roles can be reordered by dragging, within Stoat's rank rules, and the change shows at once everywhere.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–23):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start (after milestone 23):
  - `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3, `tests/api_upload.rs` 10;
  - `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/ui/profile_editor.rs`, `src/permissions.rs` and `tests/server_settings.rs` are clean.
- Check any other file's count with `git show HEAD:<file> | rustfmt --edition 2024 --check | grep -c '^Diff in'` before formatting it whole.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/roles.rs` (new):
  - `role_rows` order and locks;
  - `check_role_name`;
  - `RoleForm::changes` for name, hoist, colour (hex, none, custom kept) and icon (new, removed, waiting);
  - `ColourChoice` parsing and `hex_of` round-trips;
  - `may_edit` / `may_delete`;
  - `reorder` and `may_reorder` (owner, moderator below, own role, higher role, positions above, missing role);
  - `draggable`;
  - error texts.
- `tests/server_settings.rs`: `Access.roles` for Manage Role, the owner, and others.

### Integration Tests
wiremock tests against the exact routes, headers and bodies:
- `POST /servers/{id}/roles` (`{"name":…}`, `NewRoleResponse`);
- `PATCH /servers/{id}/roles/{role}` (only changed fields; `remove` for colour and icon);
- `DELETE /servers/{id}/roles/{role}`;
- `PATCH /servers/{id}/roles/ranks` (`{"ranks":[…]}`, answering the `Server`);
- the upload to `/icons` followed by the edit.

Failure cases: `MissingPermission`, `NotElevated`, `InvalidOperation` (managed role, bad order), `TooManyRoles`, `NotFound` (already deleted counts as done) become plain toasts.

Store (`tests/store_roles.rs`):
- a created role, then its `ServerRoleUpdate`, is one role;
- an edit applied locally, then its event, is consistent;
- a local reorder, then the same `ServerRoleRanksUpdate`, is consistent.

### Manual Tests
Run with two accounts and the web client, on a test server owned by the first account. Batch them at the end if the user asks, as in recent milestones:
1. **List:** roles show highest first, with colours, icons and "Hoisted"; locked roles say why.
2. **Create** "Testers": it appears at the bottom and its editor opens.
3. **Edit:**
   - rename, hoist, set a palette colour, type a hex colour, set an icon;
   - check the member list, message names and the web client;
   - remove the colour and the icon.
4. **Custom colours:** a gradient set in the web client shows as Custom and survives a rename.
5. **Delete** a role: members lose it.
6. **Reorder:** drag to reorder as the owner. As a mid-rank moderator (Manage Role), higher roles are locked and can't be dragged above you.
7. **Live changes:** a role edited or deleted in the web client while its editor is open is followed, or the page returns to the list.
8. **Permissions:** an account with only Manage Role sees "Server settings…" with just Roles; without it, Roles is gone.
9. **Themes:** in both, the colour picker, previews, drop lines and locks are readable.

## Definition of Done
- [ ] The Roles page lists every role highest first, with colour, icon, hoist note and lock reason. It shows for Manage Role or the owner, and "Server settings…" is offered with only Manage Role.
- [ ] Create role adds a role (name 1–32) at the bottom and opens it. `TooManyRoles` reads plainly.
- [ ] The role editor saves name, hoist, colour (palette or hex `#rrggbb`, or none) and icon (`icons`, within its limit, set or removed). It sends only what changed, keeps custom CSS colours unless changed, and follows live edits.
- [ ] Delete role is offered for unmanaged roles below the user's rank (any for the owner), confirms, and removes the role everywhere at once.
- [ ] Dragging reorders roles with one `PATCH …/roles/ranks` per drop, only where Stoat's rank rule allows. The change shows at once, and is put back on refusal.
- [ ] Roles the user can't edit (rank or managed) open read-only, and say why.
- [ ] Stoat's refusals become plain toasts: `MissingPermission`, `NotElevated`, `InvalidOperation`, `TooManyRoles`, `NotFound`.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan.
