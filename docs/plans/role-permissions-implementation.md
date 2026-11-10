# Implementation Record: Roles editor, part 2: permissions (milestone 25)

_Plan: [`role-permissions.md`](role-permissions.md)_
_Started: 2026-11-09_

Run in continuous mode. Manual checks are batched at the end, as the user asked. The plan and its roadmap link (uncommitted before chunk 1) were committed with chunk 1, as milestones 23 and 24 did.

Rustfmt drift at the start (`Diff in` hunks): `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3, `tests/api_upload.rs` 10. `src/roles.rs`, `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/permissions.rs`, `src/assets.rs`, `tests/roles.rs`, `tests/api_roles.rs` and `tests/server_settings.rs` were clean.

## Chunk 1: Permission list and read-only matrices

### Changes Made
- `src/permission_matrix.rs` (new, no GPUI):
  - `PermissionEntry { permission, title, server_description, channel_description, heading }` with `bit()`;
  - `PERMISSIONS: [PermissionEntry; 35]` in the plan's order and words (Assumption 1), built with two `const fn` helpers (`entry`, `starts`);
  - `Setting { Deny, Neutral, Allow }` (ordered from strictest), with `ALL` and `label()`;
  - `setting_of(&Override, bit)` (a bit both allowed and denied reads as Deny, as Stoat applies allow then deny), `with_setting(Override, bit, Setting)` and `override_of(OverrideField)`.
- `src/ui/permission_matrix.rs` (new): `PermissionMatrix`, a `RenderOnce` with `Mode::{Values(u64), Overrides(Override)}`, `.enabled(Fn(bit, Setting) -> bool)`, `.on_change(Fn(bit, Setting, …))` and `.read_only(bool)`.
  - Group headings (upper case, muted), then a row per permission: title, muted description, and the control on the right.
  - Values: a `Checkbox`. Its next state (Allow or Neutral) decides whether it's enabled, with the tooltip "You don't have this permission." when it isn't.
  - Overrides: a `ButtonGroup` (outline, xsmall) of three `Button`s with the `Close`, `Minus` and `Check` icons and the tooltips "Deny", "Neutral" and "Allow". The chosen one is drawn `danger` for Deny and `success` for Allow. Each button has its own `on_click`; there's no group `on_click`, so a disabled button does nothing and the keyboard works. The handler checks `enabled` again before calling `on_change`.
  - A read-only matrix disables everything without the "you don't have" tooltips, since the page's lock note says why.
- `src/roles.rs`:
  - `can_manage_roles` and `can_manage_permissions`: the owner, or a known member whose roles give the permission;
  - `may_edit_permissions`: Manage Permissions and the rank rule (the owner is exempt, and a bot's role is fine);
  - `role_lock_notes(store, server, role) -> Vec<&str>`, one line per reason: rank (alone, since it locks everything), no Manage Roles, no Manage Permissions, or a bot's role (only shown with Manage Roles, since deleting needs it anyway);
  - `may_edit` (and so `may_delete` and `draggable`) and `may_reorder` use `can_manage_roles` instead of `access().roles`. The `access` import is gone.
- `src/server_settings.rs`: `Access.roles` is Manage Role **or** Manage Permissions (doc updated).
- `src/ui/server_settings.rs`:
  - `RolesMode::Everyone(Entity<DefaultPermissionsEditor>)` and `RolesPage::open_everyone`. `follow` goes back to the list from either editor when `Access.roles` is lost, and from a role's editor when the role is deleted;
  - `render_everyone`: a row below the role list (outside the drag-and-drop list, under a border) with the `Users` icon, "Everyone" and "Permissions available without any role";
  - "Create role…" shows only with `can_manage_roles`;
  - `RoleEditor`: the lock note is now `role_lock_notes` (several lines possible), and a new "Permissions" section (read-only for now) sits between the Save row and Copy role ID / Delete;
  - `DefaultPermissionsEditor` (read-only): "‹ Roles", an "Everyone" heading with an explanation, a lock note without Manage Permissions, and the matrix in Values mode. It observes the chat, so it follows live changes;
  - helpers `lock_note` and `section_heading`.
- `src/lib.rs`, `src/ui/mod.rs`: `pub mod permission_matrix;` in each (by hand).
- `src/assets.rs`: `server_settings_icons_load` also checks `Users`, `Close`, `Minus` and `Check`.
- `Cargo.toml`: dev-dependency `revolt-permissions` with `try-from-primitive`. `Cargo.lock` gains one line (`num_enum` under `revolt-permissions`).
- `tests/permission_matrix.rs` (new, 8 tests): every single bit `ChannelPermission::try_from` accepts (0..64) is listed exactly once (35), nothing else is, and each entry is one bit; View Audit Logs and Use External Emojis are listed; the headings and where they start; descriptions are set and titles unique; `setting_of` (including both bits set); `with_setting` round-trips and clears the other side; `Setting`'s order and labels; `override_of`.
- `tests/roles.rs`: the fixture is now `store_with(me, mods_permissions)`, with `store(me)` unchanged. There are 5 new tests: `can_manage_roles` and `can_manage_permissions` for the owner, Manage Role only, Manage Permissions only and a plain member; `may_edit_permissions` for the owner (all roles), a moderator (above, own role, below, a bot's role) and Manage Role only; Manage Permissions alone can't edit, delete, drag or reorder; and the lock notes.
- `tests/server_settings.rs`: the fixture gains `u-permer` (a Manage Permissions role), plus a new `manage_permissions_also_shows_roles` test.

### Divergences
- **The dev-feature risk didn't apply.** `num_enum` was already in the app's normal build, through `revolt-models`' `partials` feature. `cargo tree -e normal,features -i revolt-permissions` shows `try-from-primitive` isn't enabled outside test builds.
- **`role_lock_notes` was added in this chunk** (not in the plan's file list). Someone with only Manage Permissions now sees the page, so the role editor's old lock note ("ranks at or above…" whenever it's not editable) would have given the wrong reason. It covers the permissions case too, which chunk 3 relies on. It replaces the old `render_lock(editable, deletable)`.
- **A disabled tri-state option's tooltip** is "Allow: You don't have this permission, so you can only make it stricter." It keeps the option's name in front of the plan's text, since the icon alone doesn't name it.
- **The chosen option is coloured**: red for Deny and green for Allow (`danger` and `success`), so the matrix reads at a glance. The plan didn't specify colours.
- **The group `on_click` isn't used** (see Changes). That settles the `ButtonGroup` risk without falling back to separate buttons.

Look-ahead: chunk 3's "lock note covers both cases" is already done by `role_lock_notes`, so chunk 3 only makes the matrix editable. No other chunk is affected. The plan needs no amendments.

### Verification Performed
- `cargo test --test permission_matrix --test roles --test server_settings`: 8, 47 and 33 passed.
- `cargo tree -e normal,features -i revolt-permissions` shows no `try-from-primitive`; it appears only with dev dependencies (`-e all`).
- Full suite: `cargo test` gave 1,252 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: the new files are clean, and `src/roles.rs`, `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/assets.rs`, `tests/roles.rs` and `tests/server_settings.rs` are still clean. `lib.rs` and `mod.rs` were edited by hand.

### Manual Validation
Deferred to the end (batched, as the user asked): the matrices match the web client as the owner, and a Manage Permissions-only account sees the page without "Create role…" or drag handles.
