# Implementation Record: Channel permission overrides (milestone 27)

_Plan: [`channel-permissions.md`](channel-permissions.md)_
_Started: 2026-11-11_

Run in continuous mode. Manual checks are batched at the end, as the user asked. The plan and its roadmap link (uncommitted before chunk 1) were committed with chunk 1, as before.

Rustfmt drift at the start (`Diff in` hunks): `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3. `src/permission_matrix.rs`, `src/ui/permission_matrix.rs`, `src/channel_settings.rs`, `src/ui/channel_settings.rs`, `src/ui/server_settings.rs`, `src/permissions.rs`, `src/roles.rs`, `tests/permission_matrix.rs`, `tests/channel_settings.rs`, `tests/api_channels.rs`, `tests/store_channels.rs` and `tests/permissions.rs` were clean.

## Chunk 1: The channel list of permissions, and a read-only Permissions page

### Changes Made
- `src/permission_matrix.rs`:
  - `PermissionEntry.group_description`;
  - `CHANNEL_DESCRIPTIONS` (23) and `GROUP_DESCRIPTIONS` (12), filled into `PERMISSIONS` at compile time by a `const fn with_contexts`, so the 35 entries read as before;
  - `MatrixContext { Server, Channel, Group }` and `PermissionEntry::description(context)`;
  - `ShownEntry { entry, description, heading }` and `entries(context)`. A group's heading moves to its first shown entry, and a group with none shown has no heading.
- `src/ui/permission_matrix.rs`: `PermissionMatrix::context(MatrixContext)` (default `Server`). It renders `entries(context)` with each one's description.
- `src/channel_permissions.rs` (new, no GPUI):
  - `can_manage_channel_permissions` and `held`;
  - `OverrideTarget { Everyone, Role }`, and `OverrideRow` with `summary()` ("Allows N, denies M");
  - `override_rows` (with and without an override, in rank order, through `role_rows`);
  - `stored_override` and `group_permissions` (an unset value is `DEFAULT_PERMISSION_DIRECT_MESSAGE`);
  - `may_edit_override` (Manage Permissions, plus the rank rule through `acting_rank`) and `may_edit_group_permissions`;
  - `override_lock_notes` and `group_lock_note`.
- `src/channel_settings.rs`: `ChannelAccess.permissions` (Manage Permissions in the channel or group), and `any()` covers it.
- `src/ui/channel_permissions.rs` (new):
  - `ChannelPermissionsPage` with `PageMode::{List, Override, Group}`. The list is Everyone, "Role overrides" and "Roles without overrides" (with the empty states), with colour dots and chevrons;
  - it goes back to the list when an open role is deleted;
  - `OverrideEditor` (read-only matrix in the channel context, with an explanation of Stoat's order and the lock notes) and `GroupPermissionsEditor` (read-only, Values mode, group context);
  - `editor_bar` with "‹ Permissions", and `BackToList`.
- `src/ui/channel_settings.rs`: `permissions_page` (icon `ShieldCheck`) in `PAGES: [ChannelPageFn; 2]`, and the view holds the page entity.
- `src/ui/chat.rs`: "Channel settings…" / "Group settings…" and the info dialog's button now show when `channel_settings::access(..).any()`.
- `src/ui/server_settings.rs`: `colour_dot`, `lock_note` and `section_heading` are now `pub(crate)`.
- `src/assets.rs`: the icons test also checks `ShieldCheck`, `ChevronRight` and `ChevronLeft`.
- `src/lib.rs`, `src/ui/mod.rs`: `pub mod channel_permissions;` in each (by hand).
- Tests:
  - `tests/permission_matrix.rs`: 4 new tests (25 in all). The server context is every entry as before; the channel's 23 and the group's 12, in order, with their headings; contexts' own descriptions. The old check that `channel_description` is empty now checks every description is non-empty;
  - `tests/channel_permissions.rs` (new, 9 tests): who has Manage Permissions (owner, server-wide, override-only, plain, DM); groups (only the owner by default); the list split and order, with all-neutral counting as none; stored overrides; group defaults; the rank rule (owner, a moderator below, own role, above, a deleted role, a group); a member whose only role is the lowest edits no role's override; lock notes; Manage Permissions alone opens only the Permissions page;
  - `tests/channel_settings.rs`: the access tests now say which pages (a group's owner has both, its members only the Overview).

### Divergences
- **A few descriptions read better than the web client's,** for one channel: "Edit and delete this channel", "Edit this channel's role and default permissions", "Read past messages sent in this channel", "Send messages in this channel", "Bypass this channel's slowmode", "Connect to this voice channel". For groups: "Edit the group's name, description and icon", "Change what members may do in the group", "Send messages in the group", "Mention everyone in the group". The others are the web client's.
- **The editors explain Stoat's order of application.** Stoat applies the server's defaults, then Everyone's channel override, then roles' server-wide permissions, then role overrides in the channel. Roles' own permissions apply over Everyone's channel override, which isn't obvious, so each editor says so in a line under its heading.
- **`held(store, channel)`** is a small helper the plan didn't list, used by chunks 2–4.
- **The group page's heading is "Members"**, and it has no back bar (there's no list to go back to).

Look-ahead: chunk 2 makes `OverrideEditor` editable (state, follow, save). Chunk 4 does the same for `GroupPermissionsEditor`. No amendments.

### Verification Performed
- `cargo test --test permission_matrix --test channel_permissions --test channel_settings`: 25, 9 and 22 passed.
- Full suite: `cargo test` gave 1,351 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` is still 17 (one new hunk was hand-formatted). `src/permission_matrix.rs` (formatted whole; it was clean), the new files, and every other touched file are clean.

### Manual Validation
Deferred to the end (batched): a channel's list and editors match the web client, read-only; a group's page shows its member permissions.

## Chunk 2: Edit Everyone's override in a channel

### Changes Made
- `src/api/client.rs`: `set_channel_default_permissions(channel, &Override) -> Channel`, a `PUT /channels/{id}/permissions/default` with `DataDefaultChannelPermissions::Field` (type paths written out in full, so the drifted import block isn't touched).
- `src/store.rs`: `apply_channel_permissions(&Channel) -> bool`. For a text channel it takes the answer's `role_permissions` and `default_permissions` (clearing the latter when absent); for a group, its `permissions` when present. Nothing else is taken from the answer.
- `src/ui/chat.rs`:
  - `channel_changed(id, cx)`, factored out of the `ChannelUpdate` handler (forget member lists, `sync_pane`, `drop_stale_selection`, reload), which now calls it;
  - `channel_permissions_saved(&Channel, cx)` applies the answer and calls it.
- `src/channel_permissions.rs`: `grant_basis(store, channel, target)` (Everyone: the stored default, all neutral when unset; a role: `None` until chunk 3) and `channel_permissions_error_text` (Assumption 7).
- `src/ui/channel_permissions.rs`, `OverrideEditor`:
  - `filled`, `edited` and `saving`, and `rights()` (`OverrideRights { editable, held, basis }`);
  - `can_save` (editable, `may_grant` against the basis, changed, not saving), `reset`, per-permission `follow` (`follow_override`), `set` (`with_setting`) and `save`;
  - the bar's Reset and Save show when editable. The matrix is `setting_enabled(held, basis, …)`, read-only while saving or when not editable;
  - a save applies through `channel_permissions_saved`, resets and toasts "Saved."; a refusal toasts the plain text and keeps the edit;
  - `save` returns early for a role's override (no basis yet), which chunk 3 replaces.
- Tests:
  - `tests/channel_permissions.rs`: 4 new tests (13 in all). The basis for Everyone, stored or neutral, and none outside server channels; `may_grant` with the basis matches Stoat's `throw_permission_override` for every case over three permissions, with the stored default unset (`None`) and set; each enabled option is one Stoat takes; the refusal texts;
  - `tests/api_channels.rs`: 2 new wiremock tests. The `PUT` body is exactly `{"permissions":{"allow":2,"deny":4}}` and the answer parses; `CannotGiveMissingPermissions` reads as its text;
  - `tests/store_channels.rs`: 2 new tests. A saved default override, then its `ChannelUpdate`, are one state, and `can_view` follows at once; an answer without a default override clears it and takes nothing else.

### Divergences
- **`channel_edited` (milestone 26) still has its own lighter follow-up**, rather than calling `channel_changed`: an edit of name, icon or slowmode can't change visibility.
- **The editor is built for both targets now**, with saving held back for roles by an early return rather than a separate type. Chunk 3 only adds the role's basis, the client call and the save branch.

Look-ahead: chunk 3 as planned. Chunk 4 reuses `apply_channel_permissions` (its group branch is already in) and `channel_permissions_saved`. No amendments.

### Verification Performed
- `cargo test --test channel_permissions --test api_channels --test store_channels`: 13, 7 and 18 passed.
- Full suite: `cargo test` gave 1,359 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` 17, `src/store.rs` 14 and `src/api/client.rs` 3 are unchanged. The other touched files are clean.

### Manual Validation
Deferred to the end (batched): deny Send Messages to Everyone in one channel; a plain member can't send there at once; then clear it.

## Chunk 3: Edit a role's override in a channel

### Changes Made
- `src/api/client.rs`: `set_channel_role_permissions(channel, role, &Override) -> Channel`, a `PUT /channels/{id}/permissions/{role}` with `DataSetRolePermissions`.
- `src/channel_permissions.rs`:
  - `grant_basis` for a role is **the role's server-wide permissions** (`override_of(role.permissions)`); an unknown role is `None`;
  - `ungrantable(held, basis, edited)` lists the unheld permissions the edit sets looser than the basis. It's empty exactly when Stoat would take the edit;
  - `ungrantable_note(target, blocking)` puts that in words.
- `src/ui/channel_permissions.rs`, `OverrideEditor`:
  - a role's override saves through `set_channel_role_permissions` (the early return for roles is gone);
  - when the override as edited can't be granted, the blocking note shows under the lock notes, in the danger colour with `TriangleAlert`, and Save stays disabled;
  - the rank rule, going back to the list when the role is deleted, and the list's rows moving between sections were already in place from chunk 1 (they follow the store).
- `docs/plans/channel-permissions.md`: Assumption 12 (the blocking note).
- Tests:
  - `tests/channel_permissions.rs`: 6 new tests (19 in all). The fixture's `low` role is now allowed Manage Messages and denied Manage Webhooks server-wide, neither of which the moderator has. Tests: a role's basis is its server-wide permissions; an unheld permission the role has server-wide may be allowed (all three options enabled, and Stoat takes it); one the role is denied server-wide offers only Deny (Stoat refuses neutral); the stored override itself can be ungrantable, with the note naming Send Messages and Manage Webhooks, and fixed by making them stricter; `ungrantable` is empty exactly when Stoat's `throw_permission_override` passes, for every case over three permissions; a saved role override then its `ChannelUpdate` are one state, and `can_pin` follows at once;
  - `tests/api_channels.rs`: 2 new wiremock tests (the exact `PUT` body and parsed answer; `NotElevated`, `CannotGiveMissingPermissions` and `NotFound` read as their texts).

### Divergences
- **The blocking note (plan amended, Assumption 12).** This is a consequence of the quirk the plan didn't foresee. Stoat checks the whole override against the role's server-wide permissions, so a moderator may find the stored override itself refused (it allows something they lack in the channel that the role lacks server-wide). Each row's options were right, but Save would have stayed disabled without saying why. The editor now names the blocking permissions, as milestone 25's Everyone editor does. Blocking bits on permissions the channel doesn't show can't be fixed in the page; that's rare and accepted.
- **The store test is in `tests/channel_permissions.rs`**, not `tests/store_channels.rs`: `can_pin` needs a member with a role, which only that fixture has.

Look-ahead: chunk 4 (groups) has no grant check, so it isn't affected. No other amendments.

### Verification Performed
- `cargo test --test channel_permissions --test api_channels`: 19 and 9 passed.
- Full suite: `cargo test` gave 1,367 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/api/client.rs` is still 3. The other touched files are clean.

### Manual Validation
Deferred to the end (batched): deny View Channel to a role in one channel; the quirk as a moderator (an option enabled because the role has it server-wide, Deny only where it's denied server-wide, the blocking note); roles at or above them are read-only.

## Chunk 4: Edit a group's member permissions

### Changes Made
- `src/api/client.rs`: `set_group_permissions(group, u64) -> Channel`, the same `PUT …/permissions/default` with `DataDefaultChannelPermissions::Value` (`{"permissions": n}`).
- `src/permission_matrix.rs`: `with_value(value, bit, setting)`, a checkbox's change that touches only its bit.
- `src/ui/channel_permissions.rs`, `GroupPermissionsEditor`:
  - `filled`, `edited` and `saving`, starting from `group_permissions` (Stoat's default when unset);
  - `editable` (`may_edit_group_permissions`), `can_save`, `reset`, per-permission `follow` (`follow_value`), `set` (`with_value`) and `save` through `set_group_permissions` and `channel_permissions_saved` (chunk 2's store path already had the group branch);
  - its bar is the "Members" heading with Reset and Save on the right (no back button, since a group's page is this editor alone). Every option is enabled with Manage Permissions, as Stoat has no grant check here. The unused `BackToList` emitter is gone.
- Tests:
  - `tests/permission_matrix.rs`: `with_value`;
  - `tests/channel_permissions.rs`: 2 new tests (21 in all). Turning a group's permission off keeps every bit the page doesn't show (and the default has some); a saved group value, then its `ChannelUpdate`, are one state, and a member loses `can_add_to_group` and `can_edit_channel` at once;
  - `tests/api_channels.rs`: the body is exactly `{"permissions":1234}`, and the `Group` parses.

### Divergences
- **The store test is in `tests/channel_permissions.rs`**, whose fixture has groups with members, not `tests/store_groups.rs`.
- **The group editor has no back bar**, only its heading with Reset and Save, since there's no list to go back to.

### Verification Performed
- `cargo test --test channel_permissions --test api_channels --test permission_matrix`: 21, 10 and 26 passed.
- Full suite: `cargo test` gave 1,371 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` 17, `src/store.rs` 14 and `src/api/client.rs` 3 are unchanged. The other touched files are clean.

### Manual Validation
Deferred to the end (batched): as a group's owner, turn off Invite Others and Manage Channels; a member loses "Add members" and "Group settings…" at once.

## Summary

_Completed: 2026-11-11_

### Chunks Completed
1. The channel list of permissions, and a read-only Permissions page (`89369eb`)
2. Edit Everyone's override in a channel (`28cd1bb`)
3. Edit a role's override in a channel (`ce1afbf`)
4. Edit a group's member permissions (this commit)

### Divergences
- Chunk 1: a few descriptions reworded for one channel or group. The editors explain Stoat's order of application. A small `held` helper was added. The group page's heading is "Members".
- Chunk 2: `channel_edited` keeps its lighter follow-up. The editor was built for both targets, with role saves held back until chunk 3.
- Chunk 3: **the blocking note** (plan amended, Assumption 12). Stoat checks the whole override against the role's server-wide permissions, so even a stored override can be refused, and the editor now says which permissions block it. Also, the store test is in `tests/channel_permissions.rs`.
- Chunk 4: the store test is in `tests/channel_permissions.rs`; the group editor has no back bar.

### Definition of Done
- [x] Channel Settings has a Permissions page for anyone with Manage Permissions in the channel or group, and "Channel settings…" opens the window for them.
- [x] A server channel's page lists Everyone, then roles with overrides ("Allows N, denies M"), then roles without, in rank order.
- [x] The matrix shows the channel's 23 or the group's 12 permissions with fitting descriptions. The Roles page still shows all 35, unchanged (`the_server_context_is_every_permission_as_before`). Hidden bits are kept.
- [x] Everyone's override saves as `{"permissions":{"allow","deny"}}`; a role's override saves within the rank rule; both enable exactly what Stoat's `throw_permission_override` allows (against the stored default, or the role's server-wide value), checked against Stoat's own function.
- [x] A group's member permissions save as `{"permissions": n}`, starting from Stoat's default when unset.
- [x] Every save applies at once (visibility, pinning, group actions), and the event that follows changes nothing.
- [x] Refusals read plainly: `CannotGiveMissingPermissions`, `NotElevated`, `MissingPermission`, `NotFound`, `InvalidOperation`.
- [x] `cargo test` (1,371) and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [x] The roadmap links this plan (committed with chunk 1).

### Remaining Manual Steps
The plan's Manual Tests 1–8, batched at the end as the user asked (two accounts and the web client): read; Everyone; role; the quirk as a moderator (including the blocking note); group; access with only Manage Permissions; live changes; themes.
