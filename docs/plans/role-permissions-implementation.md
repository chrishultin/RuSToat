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

## Chunk 2: Edit Everyone's permissions

### Changes Made
- `src/permission_matrix.rs`: the module doc now states Stoat's grant rule. New functions:
  - `value_enabled(held, bit, Setting)`: off is always allowed, on only if held;
  - `missing_from_default(held, value) -> Vec<&'static PermissionEntry>`: unheld permissions that are on, in list order;
  - `missing_note(&[&PermissionEntry]) -> Option<String>`: Assumption 5's text, with "it" or "them" and "A, B and C";
  - `default_change(stored, edited) -> Option<u64>`;
  - `follow_value(filled, edited, now) -> u64`: per-bit live-follow;
  - `permissions_error_text`: Assumption 7's texts.
- `src/api/client.rs`: `set_default_permissions(server, u64) -> Server`, a `PUT /servers/{id}/permissions/default` with `revolt_permissions::DataPermissionsValue` (written out in full, so the import block, which has rustfmt drift, isn't touched).
- `src/ui/chat.rs`: `ChatView::default_permissions_saved(server, bits, cx)` applies `PartialServer { default_permissions }` through `apply_server_update`, then `server_permissions_changed`.
- `src/ui/server_settings.rs`:
  - `editor_bar(actions, cx)`, shared by both editors: "‹ Roles" on the left and, if the user may save, Reset and Save on the right;
  - `DefaultPermissionsEditor` is now editable: `filled` and `edited` (`u64`), `saving`, `rights()` (Manage Permissions, plus the bits held via `server_permissions`), `can_save`, `reset`, `follow` (through `follow_value`), `set`, and `save`. Save sends one `PUT`, applies the answer's `default_permissions` through `default_permissions_saved`, and toasts "Saved."; a refusal toasts `permissions_error_text`;
  - the matrix's `enabled` is `value_enabled(held, …)`, and it's read-only while saving or without Manage Permissions;
  - the missing-permissions note shows under the bar, in the danger colour with `TriangleAlert`, whenever the edited value has an unheld permission on. Save stays disabled until it's fixed.
- `tests/permission_matrix.rs`: 6 new tests (`value_enabled`, `missing_from_default`, `missing_note` for one, two and three permissions, `default_change`, `follow_value`, and the refusal texts).
- `tests/api_roles.rs`: 2 new wiremock tests. The `PUT` sends exactly `{"permissions": n}` with the token and parses the `Server`; `CannotGiveMissingPermissions` reads as its text.
- `tests/store_roles.rs`: `saved_default_permissions_then_their_server_update_are_one_value`, in which `can_view` follows.

### Divergences
- **`value_enabled` takes `(held, bit, Setting)`**, not the plan's `(held, stored, edited, bit)`. Stoat checks the whole new default value against what the user holds, so the stored value doesn't change what may be chosen, and the setting being chosen (on or off) is what matters.
- **Live-follow is per permission** (`follow_value`). Permissions the user changed keep their edit, and the rest follow, rather than the whole matrix freezing at the first touch. That's closer to how each field of the role editor follows on its own.
- The top bar scrolls with the page: the settings page is a scrolling list, and GPUI has no sticky positioning. It sits at the top, above the matrix, as planned.

Look-ahead: chunk 3 reuses `editor_bar` and `permissions_error_text`. A `follow_override` like `follow_value` will give the role's matrix the same per-permission live-follow. No plan amendments are needed.

### Verification Performed
- `cargo test --test permission_matrix --test api_roles --test store_roles`: 14, 16 and 13 passed.
- Full suite: `cargo test` gave 1,261 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt drift: `src/ui/chat.rs` 17 and `src/api/client.rs` 3 are unchanged. The other touched files are clean.

### Manual Validation
Deferred to the end (batched): turn off Everyone's Send Messages as the owner, and check a moderator lacking Manage Server can't turn it on.

## Chunk 3: Edit a role's permissions

### Changes Made
- `src/permission_matrix.rs`:
  - `setting_enabled(held, &stored, bit, Setting)`: every setting for a held permission; for an unheld one, only settings no looser than the stored one (Deny < Neutral < Allow);
  - `may_grant(held, &stored, &edited)`: Stoat's `throw_permission_override` check, word for word;
  - `override_change(&stored, &edited) -> Option<Override>`;
  - `follow_override(filled, edited, now)`: per-permission live-follow, like `follow_value`.
- `src/roles.rs`: `RoleForm` gains `permissions: Override`. `of` fills it through `override_of`, `has_changes` counts it, and `permissions_change(&Role) -> Option<Override>` gives it separately. The doc on `changes` (the `PATCH`) says permissions are never in it.
- `src/api/client.rs`: `set_role_permissions(server, role, &Override) -> Server`, a `PUT /servers/{id}/permissions/{role}` with `revolt_models::v0::DataSetServerRolePermission` (written out in full, so the drifted import block isn't touched).
- `src/ui/server_settings.rs`, `RoleEditor`:
  - it holds `permissions: Override`, and `rights()` returns `RoleRights { fields, permissions, delete, held }`;
  - `can_save` is true when the name and icon are fine and either fields the user may change (Manage Role) or permissions they may change (Manage Permissions) differ;
  - `reset` is now `fill(true)`. `fill(false)` refills the fields and keeps the matrix's edit, after the `PATCH` works. `follow` also follows the matrix through `follow_override`;
  - `save` runs upload, then `PATCH` (if the fields changed and the user has Manage Role), then `PUT` (if the permissions changed and the user has Manage Permissions). Before the `PUT` it re-checks `may_grant` against the role as stored then. The `PUT`'s answer is applied through `role_saved` with the role taken from the answer's `roles`. A full success resets the form and toasts "Saved.". A failed `PATCH` stops there (`save_failed`). A failed or locally refused `PUT` goes through `permissions_failed`, which prefixes "Saved the role, but not its permissions: " when the `PATCH` went first, and keeps the matrix's edit;
  - the matrix is editable with `may_edit_permissions` (read-only while saving), and its `enabled` is `setting_enabled(held, stored, …)` against the stored override at each render;
  - Reset and Save moved into `editor_bar` at the top, shown with Manage Role or Manage Permissions. The old row under the fields is gone. Enter in the name field still saves.
- `tests/permission_matrix.rs`: 7 new tests (21 in all). `setting_enabled` for held and unheld permissions from each stored setting. `may_grant` is checked against `revolt-permissions`' own `throw_permission_override` for every case over three permissions (8 held sets × 27 × 27 overrides). Each enabled single-step setting is one Stoat grants. There are also multi-step edits, `override_change` and `follow_override`.
- `tests/roles.rs`: the `RoleForm` literals gain `permissions`, and the unchanged-form test also checks `permissions_change` and `has_changes`. There are 2 new tests: the form starts with the role's permissions, and permissions go in their own request, never the `PATCH`, while `has_changes` counts them.
- `tests/api_roles.rs`: 3 new wiremock tests. The `PUT` sends exactly `{"permissions":{"allow","deny"}}` with the token and parses the `Server`. A rename plus new permissions are a `PATCH` then a `PUT`, with two mocks at `expect(1)` each. `NotElevated`, `CannotGiveMissingPermissions` and `NotFound` read as their texts.
- `tests/store_roles.rs`: `saved_role_permissions_then_their_event_are_consistent_and_pinning_follows`.

### Divergences
- **The store test is in `tests/store_roles.rs`**, not `tests/roles.rs`, because `can_pin` needs channels, which only that fixture has.
- **The `may_grant` test is checked against Stoat's own code** (`PermissionValue::throw_permission_override`, run with `futures::executor::block_on`) rather than a hand-written table.
- **A locally refused `PUT`** (the stored override changed meanwhile, so the edit can no longer be granted) toasts "You can't grant permissions you don't have." without sending. With the `PATCH` already done, it gets the "Saved the role, but not its permissions: " prefix.
- **The combined-save wiremock test drives the client** in the editor's order (`changes` then `edit_role`, then `permissions_change` then `set_role_permissions`) rather than the GPUI editor, which has no headless harness.

### Verification Performed
- `cargo test --test permission_matrix --test api_roles --test store_roles --test roles`: 21, 19, 14 and 49 passed.
- Full suite: `cargo test` gave 1,274 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt drift: `src/ui/chat.rs` 17 and `src/api/client.rs` 3 are unchanged. Every other touched file is clean.

### Manual Validation
Deferred to the end (batched): deny Send Messages on a role as the owner; the moderator greying; a rename and a permission change in one Save.

## Summary

_Completed: 2026-11-09_

### Chunks Completed
1. Permission list and read-only matrices (`7dab5a1`)
2. Edit Everyone's permissions (`1135c76`)
3. Edit a role's permissions (this commit)

### Divergences
- Chunk 1: the dev-feature risk didn't apply (`num_enum` was already in the app). `role_lock_notes` was added early. A disabled option's tooltip keeps its name ("Allow: …"). The chosen Deny and Allow are coloured. The `ButtonGroup` has no group `on_click`.
- Chunk 2: `value_enabled` takes `(held, bit, Setting)`. Live-follow is per permission. The top bar scrolls with the page (no sticky positioning in GPUI).
- Chunk 3: the store test lives in `tests/store_roles.rs`. `may_grant` is tested against Stoat's own function. A locally refused `PUT` toasts without sending. The combined-save test drives the client.

### Definition of Done
- [x] Every `ChannelPermission` (35) is listed once, grouped and described, and `every_permission_is_listed_exactly_once` fails if `revolt-permissions` gains one.
- [x] The Roles page shows for Manage Role or Manage Permissions. Role fields, Create, Delete and dragging need Manage Role; matrices need Manage Permissions.
- [x] "Everyone" opens the default permissions as checkboxes. Save sends one `PUT …/permissions/default`, only when every allowed bit is held, and the change shows at once.
- [x] A role's permissions are Deny / Neutral / Allow per permission. Unheld permissions offer only settings no looser than the stored one, and roles at or above the user's rank are read-only (the owner is exempt).
- [x] The role editor's single Save (in a top bar) sends the upload, the `PATCH` and the `PUT` as needed, applies each at once, and reports a partial failure plainly.
- [x] Refusals read plainly: `CannotGiveMissingPermissions`, `NotElevated`, `MissingPermission`, `NotFound`.
- [x] `cargo test` (1,274) and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [x] The roadmap links this plan (committed with chunk 1).

### Remaining Manual Steps
The plan's Manual Tests 1–8 (two accounts and the web client), batched at the end as the user asked: read; Everyone; role; combined save; greying (moderator); access; live changes; themes.
