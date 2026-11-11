# Plan: Channel permission overrides (RuSToat milestone 27)

## Summary
Milestone 27 of the [roadmap](roadmap.md) adds a **Permissions** page to milestone 26's Channel Settings window ([`channel-settings-and-order.md`](channel-settings-and-order.md)). It reuses milestone 25's matrix ([`role-permissions.md`](role-permissions.md)):
- **A server channel's overrides:** Everyone's override and each role's, each one Allow, Neutral or Deny per permission.
- **A group's member permissions:** on or off per permission.

As on the Roles page, the options greyed out are exactly the ones Stoat would refuse.

## Approach
**What Stoat allows.** Read from `/tmp/stoatchat-0.16.2` (tag `v0.16.2`) on 2026-11-11. Paths are under `crates/`.
- **A role's override in a channel:** `PUT /channels/{id}/permissions/{role}` with `DataSetRolePermissions { permissions: Override { allow, deny } }` (`delta/src/routes/channels/permissions_set.rs`).
  - It needs **Manage Permissions in that channel** (channel permissions, so overrides count). The channel must be a server channel (`InvalidOperation` otherwise), and an unknown role is `NotFound`.
  - **The role must rank below the user's best role** (`NotElevated`), and the owner is exempt. This is the same rule as milestone 25, so `acting_rank` applies.
  - **The grant check compares the edit with the role's server-wide permissions, not with its current override in the channel.** It calls `throw_permission_override(role.permissions, next)`. The user must hold every bit the edit allows that the role's server value doesn't, and every bit the role's server value denies that the edit doesn't.
    - So a moderator may allow, in a channel, a permission they lack, if the role already has it server-wide.
    - If the role is denied server-wide a permission the moderator lacks, every channel override for that role must keep denying it.
    - `held` is the user's permissions **in that channel**.
  - There's no removing an override: all-neutral (`{0, 0}`) is the same as none. The answer is the whole `Channel`, and `ChannelUpdate { data: { role_permissions } }` (the whole map) follows.
- **Everyone's override in a channel:** `PUT /channels/{id}/permissions/default` with `DataDefaultChannelPermissions::Field { permissions: Override }` (`permissions_set_default.rs`). The body is `{"permissions":{"allow":a,"deny":d}}`; the type is `#[serde(untagged)]`.
  - It needs Manage Permissions in the channel, with no rank rule.
  - The grant check is against the channel's **stored** default override: `throw_permission_override(default_permissions, next)`. When none is stored, the user must hold every bit the edit allows, which is the same as a stored `{0, 0}`.
  - The answer is the `Channel`, followed by `ChannelUpdate { data: { default_permissions } }`.
- **A group's member permissions:** the same route with `DataDefaultChannelPermissions::Value { permissions: u64 }`, a body of `{"permissions": n}`.
  - It needs Manage Permissions in the group, which is the owner's (`GrantAllSafe`). Members have it only if the group's permissions include it, and `DEFAULT_PERMISSION_DIRECT_MESSAGE` doesn't.
  - **There's no grant check.**
  - Members always have View Channel and Read Message History (`DEFAULT_PERMISSION_VIEW_ONLY | permissions`), and an unset value means `DEFAULT_PERMISSION_DIRECT_MESSAGE`.
- **Use External Emojis** is only checked in server channels (`core/database/src/models/messages/model.rs`), so it means nothing in a group.

**What the web client does** (`/tmp/for-web-0.16.3`, tag `stoat-for-web-v0.16.3`, `packages/client/components/app/interface/settings/channel/permissions/`):
- **`ChannelPermissionsOverview.tsx`** is a server channel's Permissions page:
  - "Everyone" ("Permissions available when no role overrides apply");
  - "Role Overrides": roles with any bit set, each "Grants N permissions and denies M permissions";
  - "Roles with No Overrides" ("No permissions set yet");
  - empty states "No role overrides have been configured." and "All roles have overrides.";
  - roles in rank order, each with its colour dot. Each opens an editor.
- **`ChannelPermissionsEditor.tsx`** with `channel_default` and `channel_role` (tri-state) and `group` (checkboxes):
  - **it lists only permissions with a description for the context**: 22 for a server channel, 12 for a group (the server-only ones, like Kick, Ban, Manage Server and Manage Roles, are hidden);
  - each row is disabled whenever the user lacks that permission;
  - one Save with Reset.
- The page shows with Manage Permissions in the channel.

**What exists in RuSToat** (as of 2026-11-11, after milestone 26, `995aec2`):
- **`src/permission_matrix.rs`:**
  - `PermissionEntry { permission, title, server_description, channel_description: Option, heading }` and `PERMISSIONS: [_; 35]`. Every `channel_description` is `None`, left for this milestone;
  - `Setting`, `setting_of`, `with_setting`, `override_of`;
  - `setting_enabled(held, stored, bit, setting)` and `may_grant(held, stored, edited)`, which are Stoat's rule given whatever basis is passed as `stored`;
  - `value_enabled`, `override_change`, `default_change`, `follow_override`, `follow_value` and `permissions_error_text`.
- **`src/ui/permission_matrix.rs`:** `PermissionMatrix::new(id, Mode::{Values, Overrides})` with `.enabled`, `.on_change` and `.read_only`. It renders every entry with its `server_description`, and a heading wherever an entry has one.
- **`src/ui/server_settings.rs`:**
  - `RolesPage` (`RolesMode::{List, Editing, Everyone}`), `RoleEditor` and `DefaultPermissionsEditor`;
  - `editor_bar` (private, tied to `BackToRoles`), `lock_note` and `section_heading` (private);
  - `role_drag_face` and the colour dot.
- **`src/roles.rs`:** `role_rows(store, server)` (sorted by rank, with colour) and `may_edit_permissions`. **`src/permissions.rs`:** `channel_permissions(store, user, channel)` (overrides applied; `GrantAllSafe` for a server's or group's owner), `acting_rank`, `can_edit_channel` and `has`.
- **`src/channel_settings.rs`:** `ChannelAccess { overview }` and `access(store, channel)` (Manage Channel). **`src/ui/channel_settings.rs`:** `ChannelSettingsWindow`, and `ChannelSettingsView` with `PAGES: [ChannelPageFn; 1]`; it closes when `access(..).any()` is false.
- **`src/ui/chat.rs`:**
  - `channel_menu` offers "Channel settings…" when `can_edit_channel`, and so does the Channel info dialog;
  - `channel_edited` and `open_channel_settings`;
  - the `ChannelUpdate` handler (forget member lists, `sync_pane`, `drop_stale_selection`, reload).
- **`src/store.rs`:** `apply_channel_update` already applies `role_permissions`, `default_permissions` (and its clear) and a group's `permissions`. `apply_channel_edit` deliberately takes no permissions.

**The chosen shape** (user's choices on 2026-11-11):
- **Exactly Stoat's grant rule, quirk included.**
  - For a role's override, each option is enabled per `setting_enabled(held_in_channel, role's server override, …)`, and Save re-checks `may_grant` against the same basis.
  - For Everyone's, the basis is the stored channel default (`{0, 0}` when none).
  - For a group, everything is enabled to whoever has Manage Permissions in it.
  - Rejected: the web client's rule (grey out anything unheld), which refuses changes Stoat allows.
- **Only the permissions that mean something in the context**, as in the web client:
  - **23 for a server channel:** the web client's 22, plus Use External Emojis, which Stoat checks per channel;
  - **12 for a group:** Manage Channels, Manage Permissions, Send Messages, Manage Messages, Manage Webhooks, Invite Others, Send Embeds, Upload Files, Masquerade, React, Mention Everyone and Mention Roles.
  - Bits stored for hidden permissions (set through the API) are kept as they are, because edits start from the stored value and only touch shown rows.
- **Groups are included:** their Permissions page is the on/off matrix of what members may do.
- **Where:** a Permissions page in the Channel Settings window, shown with Manage Permissions in the channel or group. For a server channel it's a list (Everyone, Role overrides, Roles without overrides) whose rows open an editor on the same page, as the Roles page does. For a group it's the matrix itself.
- **Logic and view are split**, as milestones 25 and 26 did:
  - `src/channel_permissions.rs` (new, no GPUI) holds who may edit what, the list rows, the grant basis and the error texts;
  - `src/ui/channel_permissions.rs` (new) holds the page and its editors;
  - `PermissionMatrix` gains a context (server, channel or group) choosing which rows show and which descriptions.

## Assumptions
1. **Descriptions** are the web client's, filled into `PermissionEntry.channel_description`, plus a new `group_description`:
   - **channel:** the 22 from the `TextChannel` and `Any` columns (Manage Channels "Edit and delete channel", Manage Permissions "Edit channel-specific role and default permissions", View Channel "Able to access this channel", and so on), and Use External Emojis "Use custom emoji from other servers";
   - **group:** the 12 from the `Group` and `Any` columns (Manage Channels "Edit group name and description", Manage Permissions "Whether other users can edit these settings", Invite Others "Add new members to the group", and so on).

   A `MatrixContext { Server, Channel, Group }` with `PermissionEntry::description(context) -> Option<&str>` chooses: the server context is every entry with its `server_description`, unchanged.
2. **Headings** in a context show before the first shown entry of their group. A group with no shown entry (Members, in a channel) has no heading.
3. **The list** (server channel):
   - "Everyone" first (the `Users` icon, "Permissions available when no role overrides apply");
   - then "Role overrides": roles whose channel override has any bit set, in rank order, each with its colour dot and "Allows N, denies M" (counting every bit, as the web client does);
   - then "Roles without overrides" ("No overrides yet").
   - The empty states are "No role has an override here." and "Every role has an override here."
   - Roles whose override the user can't edit (rank) still open, read-only, with a lock note.
4. **An editor** has a top bar with "‹ Permissions" on the left, and Reset and Save on the right when the user may save. Below it come a heading (the role's name, or "Everyone"), lock notes, and the matrix. It follows live changes per permission (`follow_override` and `follow_value`), as milestone 25's do.
5. **Lock notes:**
   - no Manage Permissions in the channel: "You don't have permission to change this channel's permissions.";
   - a role at or above the user's rank: "This role ranks at or above your highest role.";
   - a group whose permissions the user can't change: "Only the group's owner, or members allowed Manage Permissions, can change this."
6. **Disabled options' tooltips** are milestone 25's. For a role's channel override, an option enabled only because the role has it server-wide gets no special tooltip.
7. **Refusals** (`channel_permissions_error_text`), reusing milestone 25's texts where they fit:
   - `CannotGiveMissingPermissions`: "You can't grant permissions you don't have.";
   - `NotElevated`: "That role ranks at or above your highest role.";
   - `MissingPermission`: "You don't have permission to change this channel's permissions.";
   - `NotFound`: "That role or channel was deleted.";
   - `InvalidOperation`: "Stoat can't set that here.";
   - anything else: `error_text` with "Couldn't save the permissions."
8. **Saving applies at once:** the answer's permissions go through a new `ChatStore::apply_channel_permissions(&Channel)` and `ChatView::channel_permissions_saved(&Channel, cx)`.
   - The latter does what the `ChannelUpdate` handler does: forget member lists, `sync_pane`, `drop_stale_selection`, and reload the open channel. That logic is factored into one `ChatView::channel_changed(id, cx)` that both call.
   - The event that follows changes nothing.
9. **The window's access** becomes `ChannelAccess { overview, permissions }`.
   - "Channel settings…" (in the menu and the info dialog) shows when either is true, so someone with only Manage Permissions can open the window, and they see only the Permissions page.
   - The Overview page still needs Manage Channel.
10. **No warning when an edit would take a permission away from the user themselves** (denying View Channel to Everyone, say). If they lose access, the window closes, as milestone 23's and 26's do.
11. **"Clear" isn't a button:** setting every row to Neutral clears an override, and Reset puts back the stored one.
12. **An override Stoat would refuse as it stands says why** (added in chunk 3). Stoat checks the whole override, and a role's is compared with the role's server-wide permissions, so even the stored override may be refused for this user: say it allows Send Messages, which they lack in the channel and the role lacks server-wide.
    - Under the bar, in the danger colour, the editor names the blocking permissions: "You don't have {titles}, so this role's override can't set {them} looser than the role's server-wide permissions do. Make {them} stricter to save."
    - Save stays disabled until they're fixed, as milestone 25's Everyone editor does.
    - Blocking bits on permissions the channel doesn't show can't be fixed here. That's rare (only possible through the API) and accepted.

## Plan

### Chunk 1: The channel list of permissions, and a read-only Permissions page
**Goal:** anyone with Manage Permissions in a channel or group can see its overrides (or member permissions), with the permissions that apply there and the descriptions that fit.
**Files likely touched:**
- `src/permission_matrix.rs`:
  - `group_description` on `PermissionEntry`, and every `channel_description` and `group_description` filled (Assumption 1);
  - `MatrixContext`, `PermissionEntry::description(context)` and `entries(context) -> impl Iterator<Item = (&PermissionEntry, Option<&str>)>` (the heading to show before each entry, per Assumption 2).
- `src/ui/permission_matrix.rs`: `PermissionMatrix::context(MatrixContext)` (default `Server`), which renders only that context's rows and descriptions, with headings from `entries`.
- `src/channel_permissions.rs` (new, no GPUI):
  - `can_manage_channel_permissions(store, channel)` (Manage Permissions in `channel_permissions`, for a server channel or a group);
  - `OverrideTarget { Everyone, Role(String) }`;
  - `OverrideRow { target, name, colour, allows, denies }` and `override_rows(store, channel) -> (Vec<OverrideRow>, Vec<OverrideRow>)` (with and without overrides, in rank order);
  - `stored_override(channel, target) -> Override`;
  - `group_permissions(channel) -> u64` (unset is `DEFAULT_PERMISSION_DIRECT_MESSAGE`);
  - `may_edit_override(store, channel, target)` (Manage Permissions, plus the rank rule for a role, owner exempt; a deleted role is false);
  - `may_edit_group_permissions(store, channel)`;
  - `override_lock_notes(store, channel, target) -> Vec<&str>`.
- `src/channel_settings.rs`: `ChannelAccess.permissions`, `any()` and `access`.
- `src/ui/chat.rs`: "Channel settings…" and the info dialog's button use `channel_settings::access(..).any()` instead of `can_edit_channel`.
- `src/ui/channel_permissions.rs` (new):
  - `ChannelPermissionsPage` with `Mode::{List, Editing(Entity<OverrideEditor>)}` for a server channel, and a `GroupPermissionsEditor` for a group;
  - read-only editors (matrix with `.read_only(true)`) and the top bar's back button;
  - `permissions_page` in `ChannelSettingsView::PAGES` (icon `ShieldCheck` or `Lock`, whichever is bundled);
  - `overview_page` now checks `access.overview`.
- `src/ui/server_settings.rs`: make `lock_note` and `section_heading` `pub(crate)`, to reuse them.
- `src/lib.rs`, `src/ui/mod.rs`: the modules, by hand.
- `tests/permission_matrix.rs`, `tests/channel_permissions.rs` (new), `tests/channel_settings.rs`.

**Depends on:** None (milestones 25 and 26 are done)
**Risks:**
- **The Roles page must look exactly as it does now.** The server context must list all 35 rows with their headings; a test pins `entries(Server)` to `PERMISSIONS` in order.
- **Heading placement:** in the group context the Channels group starts with View Channel, which is hidden there. The heading must move to the group's first shown entry, not vanish.
- **Opening the window with only Manage Permissions** changes milestone 26's gate. The Overview page must hide, and the window must close on losing both.

**How to verify:**
- `cargo test --test permission_matrix`:
  - 23 channel and 12 group entries, exactly the expected permissions;
  - every shown entry has its description;
  - the server context is unchanged (35, same order, same headings);
  - headings in channel and group contexts land on the first shown entry, and Members has none in a channel.
- `cargo test --test channel_permissions`:
  - `can_manage_channel_permissions` for the owner, a server-wide manager, an override-only manager, a plain member, a group owner and a group member;
  - `override_rows` splits and orders roles, and counts bits;
  - `stored_override` with and without a stored value;
  - `group_permissions` unset and set;
  - `may_edit_override` for the owner, below, own role, above, a deleted role, and Everyone;
  - the lock notes.
- `cargo test --test channel_settings`: access with only Manage Permissions shows only `permissions`.
- Then the full suite.

**Manual check:** as the owner, a channel's Permissions page lists Everyone and the roles as the web client does, and each editor shows the same settings (read-only for now). A group's page shows its member permissions.

**Shippable when:** Channel Settings shows a channel's overrides and a group's member permissions, read-only, to anyone with Manage Permissions there.

### Chunk 2: Edit Everyone's override in a channel
**Goal:** allow, neutral or deny each permission for Everyone in one channel, within what the user may grant.
**Files likely touched:**
- `src/api/client.rs`: `set_channel_default_permissions(channel, &Override) -> Channel` (`PUT /channels/{id}/permissions/default` with `DataDefaultChannelPermissions::Field`; write the type paths in full).
- `src/store.rs`: `apply_channel_permissions(&Channel) -> bool` (a text channel's `role_permissions` and `default_permissions`, clearing the latter when absent; a group's `permissions`).
- `src/ui/chat.rs`: `channel_changed(id, cx)` factored out of the `ChannelUpdate` handler, and `channel_permissions_saved(&Channel, cx)`.
- `src/channel_permissions.rs`: `grant_basis(store, channel, target) -> Override` (Everyone: the stored default or `{0, 0}`) and `channel_permissions_error_text` (Assumption 7).
- `src/ui/channel_permissions.rs`: `OverrideEditor` is editable for Everyone:
  - `filled`, `edited`, `saving`, `rights()` (held in the channel, may edit), `can_save`, `reset`, `follow` (`follow_override`), `set` and `save`;
  - `enabled` is `setting_enabled(held, basis, …)`, and Save re-checks `may_grant` before sending.
- `tests/channel_permissions.rs`, `tests/api_channels.rs`, `tests/store_channels.rs`.

**Depends on:** Chunk 1
**Risks:**
- **Hiding the channel from oneself:** denying View Channel to Everyone, as a member who gets it only from Everyone, closes the window and drops the selection. That's intended (Assumption 10). Check that it doesn't panic or leave a dangling editor.
- **The untagged body:** the `Field` variant must serialise as `{"permissions":{"allow":..,"deny":..}}`, never `{"permissions": n}`. A wiremock test pins it.

**How to verify:**
- `cargo test --test channel_permissions`:
  - `grant_basis` for Everyone, stored and unset;
  - the enabled options for an unheld permission from each stored setting, and from unset;
  - `may_grant` with that basis, against `revolt-permissions`' own `throw_permission_override` for every case over three permissions, as milestone 25's test does, including `None` as the current value;
  - the refusal texts.
- `cargo test --test api_channels`: the exact `PUT` body, and the `Channel` parsed; `CannotGiveMissingPermissions` reads as its text.
- `cargo test --test store_channels`: a saved default override then its `ChannelUpdate` are one state, and `can_view` follows.
- Then the full suite.

**Manual check:** deny Send Messages to Everyone in one channel. A plain member can't send there at once, and the web client shows the same. Then clear it.

**Shippable when:** Everyone's override in a channel can be edited within Stoat's rule, and applies at once.

### Chunk 3: Edit a role's override in a channel
**Goal:** allow, neutral or deny each permission for a role in one channel, within the rank rule and Stoat's grant check against the role's server-wide permissions.
**Files likely touched:**
- `src/api/client.rs`: `set_channel_role_permissions(channel, role, &Override) -> Channel` (`PUT /channels/{id}/permissions/{role}` with `DataSetRolePermissions`).
- `src/channel_permissions.rs`: `grant_basis` for a role is **the role's server override** (`override_of(role.permissions)`).
- `src/ui/channel_permissions.rs`:
  - `OverrideEditor` is editable for a role (rank rule; read-only with the lock note otherwise);
  - it goes back to the list when the role is deleted;
  - the list's rows move between "Role overrides" and "Roles without overrides" as overrides are saved.
- `tests/channel_permissions.rs`, `tests/api_channels.rs`, `tests/store_channels.rs`.

**Depends on:** Chunk 2
**Risks:**
- **The quirk is surprising.** An option may be enabled for a permission the user lacks (when the role has it server-wide), and an option may be disabled even though the stored channel override is looser (when the role denies it server-wide). The tests must pin both directions against Stoat's own function, so nobody "fixes" it later.
- **The basis changes when the role's server permissions change** (in the Roles page, or elsewhere) while the editor is open. Work out `enabled` at every render from the role as stored now, and re-check `may_grant` at Save.

**How to verify:**
- `cargo test --test channel_permissions`:
  - `grant_basis` for a role is its server override;
  - the role-override cases checked against `throw_permission_override(role_server_override, next)` (an unheld permission the role allows server-wide may be allowed; one it denies server-wide must stay denied);
  - `may_edit_override` with the rank rule.
- `cargo test --test api_channels`: the `PUT` body; `NotElevated`, `CannotGiveMissingPermissions` and `NotFound` read as their texts.
- `cargo test --test store_channels`: a saved role override then its `ChannelUpdate` (the whole map) are one state, and `can_pin` follows.
- Then the full suite.

**Manual check:**
- Deny View Channel to a role in one channel, and a member with that role loses it at once.
- As a mid-rank moderator: roles at or above them are read-only, and unheld permissions offer what Stoat allows.

**Shippable when:** a role's override in a channel can be edited within Stoat's rank and grant rules, and applies at once.

### Chunk 4: Edit a group's member permissions
**Goal:** a group's owner (or a member allowed Manage Permissions) turns each of the group's 12 member permissions on or off.
**Files likely touched:**
- `src/api/client.rs`: `set_group_permissions(group, u64) -> Channel` (the same route with `DataDefaultChannelPermissions::Value`).
- `src/ui/channel_permissions.rs`: `GroupPermissionsEditor` is editable (Values mode, everything enabled with Manage Permissions), with Reset and Save in the top bar, live-follow (`follow_value`), and the save through `channel_permissions_saved`.
- `tests/channel_permissions.rs`, `tests/api_channels.rs`, `tests/store_groups.rs` (or `tests/store_channels.rs`).

**Depends on:** Chunk 2 (the save path)
**Risks:**
- **An unset value is `DEFAULT_PERMISSION_DIRECT_MESSAGE`,** not 0. Starting the matrix from 0 would show every permission off and then save that.
- **Hidden bits:** the group's hidden bits (View Channel and Read Message History are always given anyway; voice bits) are kept as stored, or as the default when unset.

**How to verify:**
- `cargo test --test api_channels`: the body is exactly `{"permissions": n}`, and the `Group` is parsed.
- `cargo test --test channel_permissions`: unset starts from the default; a change keeps hidden bits.
- Store: a saved value then its `ChannelUpdate` are one state, and `can_add_to_group` and `can_edit_channel` follow for a member.
- Then the full suite.

**Manual check:** as a group's owner, turn off Invite Others: a member loses "Add members" at once. Turn off Manage Channels: they lose "Group settings…".

**Shippable when:** a group's member permissions can be changed by its owner, and apply at once.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–26):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start (2026-11-11, `995aec2`):
  - `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3;
  - `src/permission_matrix.rs`, `src/ui/permission_matrix.rs`, `src/channel_settings.rs`, `src/ui/channel_settings.rs`, `src/ui/server_settings.rs`, `src/permissions.rs`, `src/roles.rs`, `tests/permission_matrix.rs`, `tests/channel_settings.rs`, `tests/api_channels.rs`, `tests/store_channels.rs` and `tests/permissions.rs` are clean.
- Check any other file's count with `git show HEAD:<file> | rustfmt --edition 2024 --check | grep -c '^Diff in'` before formatting it whole.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/permission_matrix.rs`: the contexts' lists, descriptions and headings, and the server context unchanged.
- `tests/channel_permissions.rs` (new): access, list rows, stored values, grant bases, enabled options and `may_grant` checked against `revolt-permissions`' own function (Everyone's with `None`, roles' with the role's server value), the rank rule, lock notes, group defaults and the refusal texts.
- `tests/channel_settings.rs`: `ChannelAccess` with each permission alone.

### Integration Tests
wiremock tests (`tests/api_channels.rs`) against the exact routes, headers and bodies:
- `PUT /channels/{id}/permissions/default` with `{"permissions":{"allow","deny"}}` (a server channel) and with `{"permissions": n}` (a group);
- `PUT /channels/{id}/permissions/{role}`;
- each answering a `Channel`.

Failure cases that become plain toasts: `CannotGiveMissingPermissions`, `NotElevated`, `MissingPermission`, `NotFound`, `InvalidOperation`.

Store (`tests/store_channels.rs`, and groups'):
- a saved override or value, then its `ChannelUpdate`, are one state;
- `can_view`, `can_pin`, `can_add_to_group` and `can_edit_channel` follow at once.

### Manual Tests
Run with two accounts and the web client, on a test server owned by the first account (with a mid-rank moderator role that has Manage Permissions but not Ban Members), plus a group. Batch them at the end if the user asks:
1. **Read:** a channel's Everyone and role overrides, and a group's member permissions, match the web client. Only the channel's 23 (or the group's 12) permissions show.
2. **Everyone:** deny Send Messages in one channel; a plain member can't send there at once. Clear it.
3. **Role:** deny View Channel to a role in one channel; a member with it loses the channel at once. Allow Manage Messages to a role in one channel; they can pin there.
4. **The quirk (moderator):**
   - with a role allowed Ban Members server-wide, the moderator may allow Ban Members on it in a channel;
   - with a role denied something server-wide that the moderator lacks, only Deny is enabled for it;
   - roles at or above their rank are read-only.
5. **Group:** as the owner, turn off Invite Others and Manage Channels for members; a member loses "Add members" and "Group settings…" at once.
6. **Access:** with only Manage Permissions in a channel, "Channel settings…" opens the window on Permissions, without Overview.
7. **Live changes:** a permission changed in the web client while its editor is open is followed if untouched, and kept if edited. Deleting the role goes back to the list.
8. **Themes:** the list, the editor bar and the matrix are readable in light and dark.

## Definition of Done
- [ ] Channel Settings has a Permissions page for anyone with Manage Permissions in the channel or group, and "Channel settings…" opens the window for them.
- [ ] A server channel's page lists Everyone, then roles with overrides ("Allows N, denies M"), then roles without, in rank order.
- [ ] The matrix shows the channel's 23 or the group's 12 permissions with fitting descriptions. The Roles page still shows all 35, unchanged. Hidden bits are kept.
- [ ] Everyone's override saves as `{"permissions":{"allow","deny"}}`; a role's override saves within the rank rule; both enable exactly what Stoat's `throw_permission_override` allows (against the stored default, or the role's server-wide value).
- [ ] A group's member permissions save as `{"permissions": n}`, starting from Stoat's default when unset.
- [ ] Every save applies at once (visibility, pinning, group actions), and the event that follows changes nothing.
- [ ] Refusals read plainly: `CannotGiveMissingPermissions`, `NotElevated`, `MissingPermission`, `NotFound`, `InvalidOperation`.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan.
