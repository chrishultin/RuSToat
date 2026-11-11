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
