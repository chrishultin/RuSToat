# Plan: Roles editor, part 2: permissions (RuSToat milestone 25)

## Summary
Milestone 25 of the [roadmap](roadmap.md) adds **permissions** to milestone 24's Roles page ([`roles-editor.md`](roles-editor.md)):
- a permission matrix for each role (allow, neutral or deny per permission);
- the same matrix for **Everyone**, the server's default permissions (on or off per permission);
- options the user may not choose, greyed out, exactly as Stoat would refuse them.

The matrix is a reusable piece, because milestone 27 (channel permission overrides) reuses it.

## Approach
**What Stoat allows.** Read from `/tmp/stoatchat-0.16.2` (tag `v0.16.2`) on 2026-11-09. Paths are under `crates/`.
- **A role's permissions:** `PUT /servers/{id}/permissions/{role}` with `DataSetServerRolePermission { permissions: Override { allow, deny } }` (`delta/src/routes/servers/permissions_set.rs`).
  - It needs **Manage Permissions** (not Manage Role). An unknown role is `NotFound`.
  - **The role must rank below the user's best role** (`rank <= member_rank` gives `NotElevated`). The owner is exempt: Stoat's permission calculation returns early for the owner without loading their membership, so `get_member_rank()` is `None`, which becomes `i64::MIN`. That's the same rule as milestone 24's `acting_rank`. A bot's (managed) role may have its permissions edited.
  - **The grant rule** (`revolt-permissions` `PermissionValue::throw_permission_override(current, next)`): the user must hold every bit newly allowed (`!current.allow & next.allow`) and every bit no longer denied (`current.deny & !next.deny`). Otherwise the answer is `CannotGiveMissingPermissions`. **For a permission the user doesn't hold, the role's setting may only get stricter** (allow → neutral → deny), compared with what Stoat has stored.
  - The answer is the whole `Server`. `ServerRoleUpdate { data: { permissions } }` follows.
- **Everyone (default permissions):** `PUT /servers/{id}/permissions/default` with `DataPermissionsValue { permissions: u64 }` (`permissions_set_default.rs`).
  - It needs Manage Permissions, with no rank rule.
  - **Its grant rule is stricter:** `throw_permission_override(None, { allow: new, deny: 0 })`, so the user must hold **every** bit the new value allows, not just the changed ones.
  - The answer is the `Server`. `ServerUpdate { data: { default_permissions } }` follows.
- **Types:** `Override` and `DataPermissionsValue` are in `revolt-permissions` (their serde comes on through `revolt-models`' `serde` feature). `DataSetServerRolePermission` is in `revolt-models`. A role's stored value is `OverrideField { a, d }` (`i64`).
- **The permission list** is `revolt_permissions::ChannelPermission`: 35 single-bit permissions plus `GrantAllSafe` and `GrantAll`. Each has a doc comment but no name list. Its optional `try-from-primitive` feature (`num_enum` 0.6.1, already in `Cargo.lock`) lets a test try every bit.
- **Two permissions the web client hides are enforced:** `ViewAuditLogs` (1 << 40, on `GET /servers/{id}/audit_logs`) and `UseExternalEmojis` (1 << 41, when sending; migration 54 added it to every server's defaults).

**What the web client does** (`/tmp/for-web-0.16.3`, tag `stoat-for-web-v0.16.3`, `packages/client/components/app/interface/settings/`):
- **`channel/permissions/ChannelPermissionsEditor.tsx`** is one editor for every kind:
  - `server_default` uses checkboxes (allow only);
  - `server_role` uses a tri-state switch per permission (allow, neutral, deny), as do channel overrides and groups;
  - its list of 33 entries has headings: Admin, Members, Channels, Messaging, Voice, Mentions. Each entry has a title and per-context descriptions;
  - **a row is disabled whenever the user lacks that permission** (`context.permission & bit`). That's stricter than Stoat, which lets anyone make a setting stricter;
  - one "Save" (or "Save permissions") with Reset, in a sticky panel at the bottom.
- **`server/roles/ServerRoleEditor.tsx`** puts that editor under the role's fields, and its single Save sends the role edit and then the permissions.
- **`ServerSettings.tsx`:**
  - the Roles page shows for **Manage Role or Manage Permissions**;
  - `roles/default` opens the default-permissions editor (the overview's "Everyone" row, "Permissions available without any role").

**What exists in RuSToat** (as of 2026-11-09, after milestone 24, `5f0c5b8`):
- **`src/roles.rs`:**
  - `role_rows`, `RoleForm { name, hoist, colour, icon }` with `changes`;
  - `may_edit`, `may_delete`, `may_reorder` and `draggable`. All of them test `access(store, server).roles`, which is **Manage Role** today;
  - `role_update(&Role)`, and the error texts.
- **`src/server_settings.rs`:** `Access { overview, roles, invites, bans, ownership }` and `access(store, server)`, where `roles` is Manage Role and needs a known membership or ownership.
- **`src/permissions.rs`:**
  - `server_permissions(store, user, server)` gives the user's bits (the owner has every one; timeouts are applied);
  - `has(bits, permission)`;
  - `acting_rank` (`i64::MIN` for the owner);
  - `RoleLock`.
- **`src/ui/server_settings.rs` (3,150 lines):**
  - `roles_page`;
  - `RolesPage` with `RolesMode::{List, Editing(Entity<RoleEditor>)}`, drag to reorder, and `CreateRoleDialog`;
  - `RoleEditor`: "‹ Roles", then heading, lock note, name, colour, icon, hoist, and **Reset and Save under the fields**, then Copy role ID and Delete. Its save is upload-then-`PATCH`, through `save` and `save_failed`, and it has `filled` for live-follow.
- **`src/ui/chat.rs`:**
  - `ChatView::role_saved(server, &Role)` applies a whole role;
  - `server_edited(&Server)` applies the name, description, images, system messages and owner, **but not `default_permissions`**;
  - `server_permissions_changed`.
- **`src/store.rs`:** `apply_server_update(id, PartialServer, clear)` applies `default_permissions` through `apply_options`. Live `ServerUpdate` and `ServerRoleUpdate` already change permissions (`tests/store_roles.rs`).
- **Controls:** gpui-component's `ButtonGroup` (`button/button_group.rs`) takes `Button` children with `.selected()` and `.disabled()`, and `on_click(&Vec<usize>)` gives the clicked index. `Checkbox` is in use in RuSToat.

**The chosen shape** (user's choices on 2026-11-09):
- **Exactly Stoat's grant rule, not the web client's.**
  - For a role, a permission the user doesn't hold offers only settings no looser than the stored one. Allow, Neutral and Deny are each enabled or not, so a moderator can always revoke.
  - For Everyone, a permission the user doesn't hold can be turned off but not on. If one is still on, Save explains which and is disabled.
- **One Save and Reset for the whole role editor**, moved to a bar at the top beside "‹ Roles", so they stay reachable with the long matrix below.
  - Save sends, in order: the icon upload, then the role `PATCH`, then the permissions `PUT`, skipping any step with nothing to do.
  - Everyone's editor has the same bar.
- **Every permission in `ChannelPermission`** (all 35), including View Audit Logs and Use External Emojis. A test fails if `revolt-permissions` gains one the list lacks.
- **The Roles page shows for Manage Role or Manage Permissions.**
  - Role fields, Create, Delete and dragging still need Manage Role.
  - The matrices need Manage Permissions.
  - Each part is read-only without its permission.
- **Logic and view are split, so milestone 27 can reuse both:**
  - `src/permission_matrix.rs` (new, no views) holds the list, settings, the grant rules and the error texts;
  - `src/ui/permission_matrix.rs` (new) renders a matrix in **Values** mode (checkboxes) or **Overrides** mode (Deny / Neutral / Allow).
  - Rejected: growing `src/ui/server_settings.rs`, which is already 3,150 lines and would then have to be shared with channel settings.
- **The tri-state control is a `ButtonGroup`** of three small buttons per row, with icons `Close`, `Minus` and `Check` and tooltips "Deny", "Neutral" and "Allow".
- **Saving applies at once:** a role's new permissions through `role_saved` (with the role taken from the answer's `roles`), and Everyone's through a new `ChatView::default_permissions_saved`, followed by `server_permissions_changed`. The event that follows changes nothing.

## Assumptions
1. **The list, its order and its words** follow the web client's (server context), with the two missing permissions added, and the titles' grammar made consistent:
   - **Admin:** Manage Channels ("Create, edit and delete channels"), Manage Server ("Edit the server's information and settings"), Manage Permissions ("Edit any permissions on the server"), Manage Roles ("Create and edit server roles"), Manage Customisation ("Create server emoji"), **View Audit Logs ("See the server's audit log")**.
   - **Members:** Kick Members, Ban Members, Timeout Members, Assign Roles, Change Nickname, Manage Nicknames, Change Avatar, Remove Avatars (the web client's descriptions).
   - **Channels:** View Channels, Read Message History, Send Messages, Manage Messages, Manage Webhooks, Invite Others (the web client's server descriptions).
   - **Messaging:** Send Embeds, Upload Files, Masquerade, React, **Use External Emojis ("Use custom emoji from other servers")**, Bypass Slowmode.
   - **Voice:** Connect, Speak, Video, Mute Members, Deafen Members, Move Members, Listen.
   - **Mentions:** Mention Everyone, Mention Roles.

   Each entry also gets a `channel` description slot, left `None` until milestone 27 fills it.
2. **"Everyone" is a row at the bottom of the role list**: the Users icon, "Everyone", and "Permissions available without any role". It isn't draggable and takes no drops. It opens `RolesMode::Everyone(Entity<DefaultPermissionsEditor>)`, with "‹ Roles" back.
3. **A matrix row** shows the title, the description in muted text, and the control on the right, under its group's heading. Disabled options have the tooltip "You don't have this permission, so you can only make it stricter." In Values mode the tooltip is "You don't have this permission." A read-only matrix (no Manage Permissions, or the role ranks too high) disables everything, and the lock note says why.
4. **Live changes:** the matrix follows the stored value until it's touched, then keeps the edit, like the other fields (`filled`). Whether an option is allowed is worked out from the **stored** value and the user's **current** permissions at every render, as Stoat compares with what it has stored.
5. **Everyone with a permission the user lacks still on:** the row's checkbox is enabled (to turn it off), and under the bar the editor says "You don't have {titles}, so Everyone can't keep {them}. Turn {them} off to save." Save is disabled until that's fixed. This mirrors Stoat's rule rather than sending a request that's bound to fail.
6. **Save order in the role editor:** upload, then `PATCH`, then `PUT`.
   - If the `PATCH` fails, nothing more is sent.
   - If the `PUT` fails after the `PATCH` worked, the role fields show as saved (live-follow) and the permissions keep their edit, with a toast.
   - Reset puts back every field and the matrix.
   - Milestone 24's Save and Reset under the fields move to the top bar.
7. **Refusals:**
   - `CannotGiveMissingPermissions`: "You can't grant permissions you don't have.";
   - `NotElevated`: "That role ranks at or above your highest role.";
   - `MissingPermission`: "You don't have permission to manage permissions.";
   - `NotFound` on a role: "That role was deleted." (the page then goes back to the list through milestone 24's follow);
   - anything else: `error_text` with "Couldn't save the permissions."
8. **No warning when an edit would take a permission away from the user themselves** (removing View Channels from Everyone, say). If they lose access, milestone 23's window rules close the page or window. A warning is a possible follow-up.

## Plan

### Chunk 1: Permission list and read-only matrices
**Goal:** anyone with Manage Role or Manage Permissions can see what each role, and Everyone, allows and denies.
**Files likely touched:**
- `src/permission_matrix.rs` (new, no views):
  - `PermissionEntry { permission: ChannelPermission, title, server_description, channel_description: Option<&str>, heading: Option<&str> }` and `PERMISSIONS: [PermissionEntry; 35]` in display order (Assumption 1);
  - `Setting { Deny, Neutral, Allow }` with `setting_of(&Override, bit)` and `with_setting(Override, bit, Setting) -> Override`;
  - `override_of(OverrideField) -> Override`.
- `src/roles.rs`:
  - `can_manage_roles(store, server)` and `can_manage_permissions(store, server)` (each a permission plus a known membership, or ownership, as `access` checks);
  - `may_edit`, `may_reorder`, `draggable` and Create use `can_manage_roles` instead of `access().roles`;
  - `may_edit_permissions(store, server, role)`: Manage Permissions and the rank rule (owner exempt; a bot's role is fine).
- `src/server_settings.rs`: `Access.roles` becomes Manage Role or Manage Permissions (doc updated).
- `src/ui/permission_matrix.rs` (new):
  - `PermissionMatrix` (a `RenderOnce`) with `Mode::{Values(u64), Overrides(Override)}`, `enabled: Fn(bit, Setting) -> bool`, and `on_change(bit, Setting)`;
  - group headings, rows (title, description, control), `Checkbox` for Values and a `ButtonGroup` (Close / Minus / Check, with tooltips) for Overrides.
- `src/ui/mod.rs`, `src/lib.rs`: the modules, by hand.
- `src/ui/server_settings.rs`:
  - the "Everyone" row at the bottom of the list (Assumption 2) and `RolesMode::Everyone`;
  - `DefaultPermissionsEditor` (read-only in this chunk);
  - `RoleEditor` gains a read-only "Permissions" section (the matrix in Overrides mode);
  - "Create role…" shows only with Manage Role.
- `src/assets.rs`: check `Minus` and `Check` load (both bundled), and add `Users` to `server_settings_icons_load`.
- `Cargo.toml`: under `[dev-dependencies]`, `revolt-permissions = { version = "=0.16.2", default-features = false, features = ["try-from-primitive"] }`, for the coverage test.
- `tests/permission_matrix.rs` (new); `tests/roles.rs`; `tests/server_settings.rs`.

**Depends on:** None (milestone 24 is done)
**Risks:**
- **The coverage test's dev feature.** With resolver 2 (edition 2024), dev-dependency features apply only to test builds. Check `cargo build` doesn't pull in `num_enum` for the app (`cargo tree -e features -i num_enum`). If unification causes trouble, use a hand-written `ALL_BITS` constant instead and say so.
- **`ButtonGroup` with disabled children.** Check that a disabled child doesn't fire `on_click`, and that `selected` draws clearly in both themes. If not, use three separate small `Button`s in an `h_flex`.
- **Gating by two permissions** changes milestone 24's behaviour for someone with only Manage Permissions: they now see the page. `may_edit` and friends must not use `Access.roles` any more. Test both one-permission cases.
- **35 rows × 3 buttons** in the editor on every render. That should be fine; check the editor scrolls smoothly in a debug build.

**How to verify:**
- `cargo test --test permission_matrix`:
  - every single bit `ChannelPermission::try_from(1 << n)` accepts (n in 0..52) is in `PERMISSIONS` exactly once, and nothing else is;
  - headings start each group;
  - `setting_of` and `with_setting` round-trip for each setting, and clear the other side.
- `cargo test --test roles`:
  - `can_manage_roles` and `can_manage_permissions` for the owner, a Manage Role-only member and a Manage Permissions-only member;
  - `may_edit_permissions` for the owner, below, own role, above, and a bot's role;
  - `may_edit` is false for a Manage Permissions-only member.
- `cargo test --test server_settings`: `Access.roles` is true for Manage Permissions only.
- Then the full suite.

**Manual check:**
- As the owner: Everyone and a role show their current permissions, matching the web client.
- An account with only Manage Permissions sees the Roles page, without "Create role…" or drag handles.

**Shippable when:** Server Settings shows every role's and Everyone's permissions, grouped and described, to anyone with Manage Role or Manage Permissions.

### Chunk 2: Edit Everyone's permissions
**Goal:** turn the server's default permissions on and off, within what the user may grant.
**Files likely touched:**
- `src/permission_matrix.rs`:
  - `value_enabled(held, stored, edited, bit) -> bool` (turning on needs the bit; turning off is always allowed);
  - `missing_from_default(held, value) -> Vec<&PermissionEntry>` (Assumption 5);
  - `default_change(stored: u64, edited: u64) -> Option<u64>`;
  - `permissions_error_text` (Assumption 7).
- `src/api/client.rs`: `set_default_permissions(server, u64) -> Server` (`PUT /servers/{id}/permissions/default`, `{"permissions": n}`).
- `src/ui/chat.rs`: `ChatView::default_permissions_saved(server, bits, cx)` runs `apply_server_update` with `PartialServer { default_permissions }` and then `server_permissions_changed`.
- `src/ui/server_settings.rs`: `DefaultPermissionsEditor` becomes editable:
  - a top bar ("‹ Roles", Reset, Save);
  - `filled` and live-follow;
  - the missing-permissions note;
  - Save, refusal toasts, and "Saved.".
- `tests/permission_matrix.rs`; `tests/api_roles.rs`; `tests/store_roles.rs` (or `tests/roles.rs`).

**Depends on:** Chunk 1
**Risks:**
- **Taking your own access away.** Removing View Channels or Send Messages from Everyone can hide channels from the user. `server_permissions_changed` handles that already, but check the settings window stays usable.
- **A default the user can't fully hold** (their role denies a default permission): Save stays disabled until it's off. Make sure the note says so plainly.

**How to verify:**
- Unit tests:
  - `value_enabled` for a held bit, an unheld bit that's on (can turn off), and an unheld bit that's off (can't turn on);
  - `missing_from_default` lists only unheld bits that are on;
  - `default_change` gives `None` when unchanged;
  - error texts.
- wiremock: `PUT /servers/srv/permissions/default` sends exactly `{"permissions": <n>}` with the token and parses the `Server`; `CannotGiveMissingPermissions` reads as its text.
- Store: `default_permissions_saved`, then the matching `ServerUpdate`, gives one value, and `can_view` follows it.
- Then the full suite.

**Manual check:**
- As the owner, turn off Everyone's Send Messages: a plain member can't send at once, and the web client agrees.
- As a moderator lacking Manage Server, its checkbox can't be turned on.

**Shippable when:** the server's default permissions can be changed from RuSToat, only in ways Stoat accepts.

### Chunk 3: Edit a role's permissions
**Goal:** set each permission of a role to allow, neutral or deny, saved together with the role's other fields.
**Files likely touched:**
- `src/permission_matrix.rs`:
  - `setting_enabled(held, stored: &Override, bit, Setting) -> bool`: held gives every setting; unheld gives only settings no looser than the stored one (Deny < Neutral < Allow);
  - `override_change(stored, edited) -> Option<Override>`;
  - `may_grant(held, stored, edited) -> bool`, Stoat's whole check, as a guard on Save.
- `src/roles.rs`: `RoleForm` gains `permissions: Override`. `RoleForm::of` fills it, and `has_changes` counts it. The `PATCH` diff (`changes`) leaves it out, and `permissions_change(&Role) -> Option<Override>` gives it separately.
- `src/api/client.rs`: `set_role_permissions(server, role, &Override) -> Server` (`PUT /servers/{id}/permissions/{role}`, `{"permissions":{"allow":…,"deny":…}}`).
- `src/ui/server_settings.rs`, `RoleEditor`:
  - the matrix becomes editable when `may_edit_permissions`;
  - the matrix takes part in `filled`, `follow` and `reset`;
  - Reset and Save move into the top bar, beside "‹ Roles" (Assumption 6);
  - `save` runs upload → `PATCH` → `PUT`, applying each answer (`role_saved`; for the `PUT`, the role from `saved.roles`);
  - the lock note covers both cases (fields need Manage Role; permissions need Manage Permissions; rank locks both).
- `tests/permission_matrix.rs`, `tests/roles.rs`, `tests/api_roles.rs`.

**Depends on:** Chunks 1 and 2 (the top bar pattern)
**Risks:**
- **Two requests per Save.** A failure in between leaves a half-saved role. Assumption 6 handles it, and the toast must say which part failed ("Saved the role, but not its permissions: …").
- **Comparing with the stored value.** If a live event changes the stored override while the user edits, the allowed options change under them. `may_grant` re-checks before sending, against the role as it is then.
- **Moving Save to the top** changes milestone 24's layout. Check the bar reads well, and that Enter in the name field still saves.

**How to verify:**
- Unit tests:
  - `setting_enabled` for held, and unheld from stored Allow (all), Neutral (Neutral, Deny) and Deny (Deny only);
  - `may_grant` mirrors `throw_permission_override` on a table of cases, including multi-step edits that end within the rule;
  - `override_change` gives `None` when unchanged;
  - `RoleForm::changes` never carries permissions, while `has_changes` counts them.
- wiremock:
  - `PUT /servers/srv/permissions/r1` sends exactly `{"permissions":{"allow":…,"deny":…}}`;
  - a combined save sends `PATCH` then `PUT` (two mocks, `expect(1)` each);
  - `NotElevated` and `CannotGiveMissingPermissions` read as their texts.
- Store: a role's permissions saved through `role_saved`, then its `ServerRoleUpdate`, are consistent, and `can_pin` follows them.
- Then the full suite.

**Manual check:**
- As the owner, deny Send Messages on a role a member holds: they can't send at once, and the web client agrees.
- As a mid-rank moderator lacking Manage Server: on a lower role that allows it, only Neutral and Deny are enabled; on one that denies it, only Deny is.
- Rename a role and change a permission in one Save: both land.

**Shippable when:** a role's permissions can be edited within Stoat's grant and rank rules, saved with its other fields in one Save.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–24):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start (after milestone 24):
  - `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3, `tests/api_upload.rs` 10;
  - `src/roles.rs`, `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/permissions.rs`, `src/assets.rs`, `tests/roles.rs`, `tests/api_roles.rs` and `tests/server_settings.rs` are clean.
- Check any other file's count with `git show HEAD:<file> | rustfmt --edition 2024 --check | grep -c '^Diff in'` before formatting it whole.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/permission_matrix.rs` (new):
  - the list covers every `ChannelPermission` bit exactly once (`try-from-primitive`), and its headings;
  - `setting_of` and `with_setting`;
  - `value_enabled`, `missing_from_default` and `default_change`;
  - `setting_enabled`, `may_grant` (a table mirroring `throw_permission_override`) and `override_change`;
  - `permissions_error_text`.
- `tests/roles.rs`:
  - `can_manage_roles`, `can_manage_permissions` and `may_edit_permissions`;
  - `may_edit`, `may_reorder` and `draggable` need Manage Role;
  - `RoleForm` with permissions.
- `tests/server_settings.rs`: `Access.roles` for Manage Permissions only.

### Integration Tests
wiremock tests against the exact routes, headers and bodies:
- `PUT /servers/{id}/permissions/default` (`{"permissions": n}`, answering the `Server`);
- `PUT /servers/{id}/permissions/{role}` (`{"permissions":{"allow","deny"}}`, answering the `Server`);
- a combined role save: `PATCH /servers/{id}/roles/{role}`, then `PUT …/permissions/{role}`.

Failure cases that become plain toasts: `CannotGiveMissingPermissions`, `NotElevated`, `MissingPermission`, `NotFound`.

Store (`tests/store_roles.rs` or `tests/roles.rs`):
- default permissions saved, then `ServerUpdate`, are one value, and `can_view` follows it;
- a role's permissions saved, then `ServerRoleUpdate`, are consistent, and `can_pin` follows them.

### Manual Tests
Run with two accounts and the web client, on a test server owned by the first account, plus a mid-rank moderator role with Manage Permissions but not Manage Server. Batch them at the end if the user asks:
1. **Read:** Everyone and each role show the same permissions as the web client, including View Audit Logs and Use External Emojis.
2. **Everyone:** turn Send Messages off and on, and a plain member follows at once.
3. **Role:** allow, neutral and deny a few permissions on a role, and members holding it follow at once (member list, pinning, channel visibility).
4. **Combined save:** rename a role and change its permissions in one Save.
5. **Greying (moderator):** unheld permissions offer only stricter settings; roles at or above their rank are read-only; Everyone can't turn on an unheld permission.
6. **Access:** with only Manage Permissions, the Roles page shows, with role fields read-only and matrices editable. With only Manage Role, the reverse.
7. **Live changes:** a permission changed in the web client while its editor is open is followed if untouched, and kept if edited.
8. **Themes:** headings, descriptions, the three-way control (selected and disabled) and tooltips are readable in both.

## Definition of Done
- [ ] Every `ChannelPermission` (35) is listed once, grouped and described, and a test fails if `revolt-permissions` gains one the list lacks.
- [ ] The Roles page shows for Manage Role or Manage Permissions. Role fields, Create, Delete and dragging need Manage Role; matrices need Manage Permissions.
- [ ] "Everyone" opens the default permissions as checkboxes. Save sends one `PUT …/permissions/default`, only when every allowed bit is held, and the change shows at once.
- [ ] A role's permissions are Deny / Neutral / Allow per permission. Unheld permissions offer only settings no looser than the stored one, and roles at or above the user's rank are read-only (the owner is exempt).
- [ ] The role editor's single Save (in a top bar) sends the upload, the `PATCH` and the `PUT` as needed, applies each at once, and reports a partial failure plainly.
- [ ] Refusals read plainly: `CannotGiveMissingPermissions`, `NotElevated`, `MissingPermission`, `NotFound`.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan.
