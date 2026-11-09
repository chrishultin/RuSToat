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
