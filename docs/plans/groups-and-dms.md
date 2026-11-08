# Plan: Groups and DMs (RuSToat milestone 20)

## Summary
Milestone 20 of the [roadmap](roadmap.md) makes groups and DMs something you can manage, not just read:
- **Follow group membership live:** people added, removed or leaving, you being removed, and owners changing. Today RuSToat drops these events.
- **Create a group** from a picker of your friends: a "New group" button on the Friends page, a "+" beside "Direct Messages", and "Create group with…" in a friend's menu.
- **Add friends to a group** from its header, and **remove members** (owner only) from the member list.
- **Leave a group,** optionally silently. An owner picks who takes over. **Transfer ownership** from the member list.
- **Close a DM.**
- **Create group invites,** which milestone 18 deferred.
- **Mutual friends, and mutual servers and groups,** on profile cards.

The references are Stoat's server (`stoatchat/stoatchat` v0.16.2: `crates/delta/src/routes/channels/{group_create,group_add_member,group_remove_member,channel_delete,channel_edit,invite_create}.rs`, `crates/delta/src/routes/users/{find_mutual,open_dm}.rs`, `crates/core/database/src/models/channels/model.rs` (`create_group`, `add_user_to_group`, `remove_user_from_group`, `create_dm`), `crates/core/database/src/events/client.rs`, `crates/core/database/src/tasks/last_message_id.rs`, `crates/delta/src/util/ratelimits.rs`) and the web client (`stoatchat/for-web` `stoat-for-web-v0.16.3`: `components/modal/modals/{CreateGroup,AddMembersToGroup,RemoveMember,DeleteChannel}.tsx`, `components/app/menus/{UserContextMenu,ChannelContextMenu}.tsx`, `src/interface/navigation/channels/HomeSidebar.tsx`, `src/interface/channels/ChannelHeader.tsx`, `components/ui/components/features/profiles/ProfileMutuals.tsx`). Both were read on 2026-11-07.

## Approach
**What the API does:**
- **Group permissions:** the owner has everything. Members have `ViewChannel + ReadMessageHistory` plus `group.permissions`, which defaults to `DEFAULT_PERMISSION_DIRECT_MESSAGE`. That includes `ManageChannel` and `InviteOthers`, so **every member can add people and create invites by default.** Removing members and transferring ownership are checked by hand as **owner only.**
- **Create:** `POST /channels/create` with `DataCreateGroup { name (1–32), description?, icon?, users: [id] (at most 49), nsfw? }` returns the `Channel::Group`.
  - Errors, in order: `IsBot`, `FailedValidation`, `NotFriends` (403) for anyone in `users` who isn't your friend, then `GroupTooLarge { max }` (403; `group_size` is 100, but the 49-user validator caps a new group at 50).
  - `ChannelCreate` goes to every member, you included. No system message.
- **Add a member:** `PUT /channels/{id}/recipients/{user}`, no body, 204. Needs `InviteOthers`; the person must be **your** friend.
  - Errors: `MissingPermission`, `InvalidOperation` (not a group), `NotFound`, `NotFriends`, `AlreadyInGroup` (409), `GroupTooLarge { max }`.
  - Events: `ChannelGroupJoin { id, user }` to the group, a `user_added { id, by }` system message, then `ChannelCreate` to the added person. **Existing members get no user object** for the newcomer.
- **Remove a member:** `DELETE /channels/{id}/recipients/{user}`, 204. Owner only, else `MissingPermission { ManageChannel }`. `CannotRemoveYourself` (400), `NotInGroup` (404).
  - Events: `ChannelGroupLeave { id, user }` to the group (the removed person gets it too, then is unsubscribed), and a `user_remove { id, by }` system message. **No `ChannelDelete` for the removed person.**
- **Leave:** `DELETE /channels/{id}?leave_silently=true|false`, 204.
  - **An owner leaving** with others present makes the **first other member in `recipients` order** the owner. It writes that directly: **no `ChannelUpdate`**. A `channel_ownership_changed { from, to }` system message is sent, even when silent.
  - The **last member leaving deletes the group** (`ChannelDelete`, no `ChannelGroupLeave`).
  - Otherwise `ChannelGroupLeave { id, user }` (the leaver gets it too) and, unless silent, a `user_left` system message.
- **Transfer ownership:** `PATCH /channels/{id}` with `{"owner": id}` returns the group.
  - Errors: `MissingPermission` (`ManageChannel`), `InvalidOperation` (not a group), `NotOwner` (403), `NotInGroup` (404), `IsBot`.
  - Events: a `channel_ownership_changed` system message, then `ChannelUpdate { data: { owner } }`.
- **Close a DM:** `DELETE /channels/{dm}`, 204. Sets `active: false` and sends `ChannelUpdate { active: false }` **to both people.** `GET /users/{id}/dm` returns the closed DM unchanged; sending a message reactivates it, with no event. RuSToat's `message_user` already reactivates it locally (`activate_dm`).
- **Group invites:** `POST /channels/{group}/invites` is the same route and body as for servers, and returns `Invite::Group`. Needs `InviteOthers`. DMs are `InvalidOperation`.
- **Mutuals:** `GET /users/{id}/mutual` returns `{ users: [id], servers: [id], channels: [id] }`. `users` are friends of both of you; `channels` are groups and DMs containing both of you. Errors: `InvalidOperation` for yourself, `MissingPermission { ViewProfile }` (for example when blocked).
- **Rate limits** (10 s): `/channels/{id}/*` is 15 per channel; `/channels/create` is one bucket; `/users/*` is 20 across all user routes, shared with profiles and DMs.
- **Root config:** `features.limits.global.group_size` (100).

**What the web client does:**
- **Create a group:** "Create a new group", "Group Name" (1–32), "Select members to add" with "Search for users...", a multi-select list of friends sorted by display name. Close and Create. It doesn't open the group. Entry points: a "+" beside "Direct Messages" (tooltip "Create a new group"), and Home's "Create a group or server". Nothing on the Friends page.
- **Add members:** a header button "Add friends to group" opens the same picker, without people already in the group, and "Add" adds them one at a time.
- **Remove a member:** "Remove Member" in a group member's right-click menu, for the owner (or with `ManageChannel`), not for yourself or the owner. Dialog "Remove Member", "Are you sure you want to remove {name} from {group}?", Cancel and Remove.
- **Leave a group:** "Leave group" in the group's menu. "Leave {name}?", "You won't be able to rejoin unless you are re-invited.", Cancel and Leave. No silent option and no choice of the next owner.
- **Transfer ownership:** no UI.
- **Close a DM:** "Close chat" in the DM's menu. "Close conversation with {name}?", "You can re-open it later, but it will disappear on both sides.", Cancel and Close.
- **Mutuals:** on the full profile, a "Mutuals" card and a "Groups" card (mutual servers plus your groups containing them), each a grid of 20 px avatars that opens a list ("Mutual Friends", "Mutual Groups"). Hidden when empty, and for yourself and bots.

**What exists already** (line numbers as of 2026-11-07, before milestone 19):
- **Store** (`src/store.rs`):
  - `home_channels` (~3293) lists Saved Notes, then open DMs and groups by latest message. `shows_channel` (~3225) hides closed DMs.
  - `add_channel` (~1941) is idempotent. `apply_channel_update` (~1952) applies a group's `name`, `owner`, `description`, `icon` and `permissions`, and a DM's `active`. **Nothing changes `recipients`.**
  - Milestone 19 adds `remove_channel(id)` and `ChatView::forget_channel(id)`, used here for being removed from a group or leaving one.
  - `member_list` (~2589) builds a group's member list from `recipients`, **skipping users the store doesn't know.**
  - `add_user` (~2969) and `knows_user` (~2973). `ChatView::fetch_user` (`chat.rs` ~1243) fetches one user and refreshes member lists.
  - `activate_dm` (~3182).
- **Events** (`src/api/events.rs`): **`ChannelGroupJoin` and `ChannelGroupLeave` aren't parsed** (they fall to `Unknown`, ~653). The forwarded list is at ~1030–1056.
- **Friends page** (`src/ui/friends.rs` `render_friends_page` ~79): a header with "Add friend" (~101), tabs, and rows (`render_person` ~175) with buttons from `src/friends.rs` `row_actions` (~124). `friend_lists` (`src/friends.rs` ~90) gives every friend with name, avatar and presence.
- **The Home list title** "Direct Messages" is a plain `div` in `render_channel_list` (`chat.rs` ~5626).
- **Menus:**
  - `channel_menu` (`chat.rs` ~3392) serves every row, DMs and groups included.
  - The user menu is `profile::user_menu(store, user, channel)` (`src/profile.rs` ~233), handled in `on_user_menu` (`chat.rs` ~3857) through `UserMenuItem` (`profile.rs` ~156). `People` knows the channel it's shown in.
  - The member panel's rows (`render_member_row` ~6983) open that menu on right-click.
- **Profile card:** `open_card` (`chat.rs` ~3056) fetches `GET /users/{id}/profile` and caches it in `self.profiles`. `render_profile_card` (`src/ui/profile_card.rs` ~48) draws Status, Pronouns, Roles, Member since and About me.
- **Invites:** `can_invite` (`src/permissions.rs` ~139) requires `in_server` (server channels only). `open_create_invite` (`chat.rs` ~4244) returns early without a server, and its title is "Invite people to {server}". `CreateInviteDialog` (`src/ui/invites.rs` ~417) reads the channel name from `TextChannel` only. `client.create_invite` and `invite_code` already handle groups.
- **Client:** `open_dm` (`src/api/client.rs` ~622), `fetch_profile` (~612). No group, mutual or owner calls. `GlobalLimits` (~73) reads only `max_invite_duration_days`.

**Decisions** (user, 2026-11-07):
1. **Creating a group** has three entry points: a "New group" button next to "Add friend" on the Friends page, a "+" beside "Direct Messages", and "Create group with…" in a friend's menu (that friend pre-selected). Each opens a picker of your friends.
2. **Transferring ownership:** "Make group owner" in a member's menu in the group's member list (owner only, confirmed). **An owner leaving picks the next owner** in the leave dialog.
3. **Leaving a group** has the "Don't notify others that you've left" checkbox (Stoat supports `leave_silently` for groups).
4. **Mutuals** on the profile card: two rows of small avatars, "Mutual friends" and "Mutual servers and groups", up to 8 each then "+N". Clicking a row opens the full list.

**Rejected alternatives.**
- **Telling an owner who Stoat will pick, instead of letting them choose.** The user chose to let them pick (Decision 2).
- **Waiting for the `channel_ownership_changed` message to learn the new owner after a transfer.** The PATCH response has it, and the `ChannelUpdate` follows.
- **Removing a closed DM from the store.** It stays, hidden by `shows_channel`, so its history and `message_user` keep working.

## Assumptions
1. **`ChannelGroupJoin`** adds the user to `recipients` (once) and fetches the user if the store doesn't know them. **`ChannelGroupLeave`** removes them; if it's you, the group is removed (`remove_channel`, `forget_channel`, `drop_stale_selection`), as for leaving a server.
2. **Owner changes without a `ChannelUpdate`:** when a `channel_ownership_changed` system message arrives in a group, the store sets `owner` to its `to`. This is how RuSToat learns of an owner leaving.
3. **Creating a group:**
   - The name is required (1–32), as in the web client. Stoat's `DataCreateGroup` has no default name.
   - The picker lists friends only (Stoat requires it), sorted by display name, with a search box filtering by display name and username. It allows up to 49 people (Stoat's limit), and says so when reached.
   - Creating with nobody else selected is allowed: a group of one, which Stoat accepts.
   - After creating, the group is added to the store and **opened** (the web client doesn't open it; opening shows it worked).
4. **Adding members** sends one `PUT` per person in order, stopping at the first error. Those already added stay. The error names the person ("Couldn't add ada: …"). `AlreadyInGroup` is skipped silently. The button is in the group header ("Add friends to group", `IconName::UserPlus`), shown with `InviteOthers` on the group.
5. **Removing a member:** "Remove from group…" in a group member's menu, when you own the group and they aren't you. Confirmation: "Remove {name}?", "Are you sure you want to remove {name} from {group}?", Cancel and Danger "Remove". Removal is applied locally on success; `ChannelGroupLeave` repeats it harmlessly.
6. **Leaving:**
   - "Leave group…" in the group's menu, for every member.
   - "Leave {name}?", "You won't be able to rejoin unless you are re-invited.", the "Don't notify others that you've left" checkbox, Cancel and Danger "Leave".
   - **As owner with others present,** a "New owner" picker (members' display names, `DropdownMenu`) defaults to the first other member in `recipients` order (Stoat's choice). If you pick someone else, RuSToat sends the transfer `PATCH` first, then the `DELETE`. If the PATCH fails, nothing is left.
   - **As the only member,** the text says "You're the only member, so leaving deletes the group." and the button says "Delete group".
   - On success, the group is removed locally and the toast says "Left {name}."
7. **Make group owner:** in a group member's menu, when you own the group and they aren't you. "Make {name} the owner?", "You'll stay in the group, but only {name} will be able to remove members or hand ownership on.", Cancel and "Make owner". The response's `owner` is applied at once.
8. **Closing a DM:** "Close conversation…" in the DM's menu (the channel menu for DM rows). The web client's wording: "Close conversation with {name}?", "You can re-open it later, but it will disappear on both sides.", Cancel and "Close". On success, `active` is set false locally (the `ChannelUpdate` repeats it). If it was open, Home's first conversation opens.
9. **Group invites:** `can_invite` allows `Group` channels (not DMs or Saved Notes, which also have `InviteOthers` in their defaults). "Create invite…" appears in a group's menu, and the header button on a group. The dialog's title is "Invite people to {group}" with no "To #channel" line.
10. **Mutuals:**
    - Fetched with the profile when a card opens, cached per user for the session (like `profiles`), and not fetched for yourself or bots.
    - "Mutual friends": the `users`, as avatars.
    - "Mutual servers and groups": the `servers`, then the `channels` that are groups (DMs left out), as icons. Ones the store doesn't know are skipped.
    - Each row shows up to 8 (24 px), then "+N"; it's hidden when empty, and both are hidden on `MissingPermission`.
    - Clicking a row opens a list dialog ("Mutual friends" / "Mutual servers and groups", 360 px) of icon and name. Clicking a friend opens their card; clicking a server or group opens it and closes the dialog.
11. **Error texts:**
    - `NotFriends`: "You can only add your friends to a group."
    - `GroupTooLarge { max }`: "Groups can have at most {max} members."
    - `NotOwner`: "Only the group's owner can do that."
    - `NotInGroup`: "{name} isn't in this group any more."
    - `MissingPermission` (adding): "You can't add people to this group."
    - Anything else: `profile_edit::error_text(err, "Couldn't … Try again.")`.
12. **No keyboard shortcuts.**

## Plan

### Chunk 1: Follow group membership live
**Goal:** RuSToat keeps groups' members and owners right as people are added, removed or leave, and drops a group you're removed from.
**Files likely touched:**
- `src/api/events.rs`: `ChannelGroupJoin { id, user }` and `ChannelGroupLeave { id, user }`, parsed and forwarded, with parse tests.
- `src/store.rs`:
  - `add_recipient(channel, user) -> bool`, `remove_recipient(channel, user) -> bool`
  - when a message with `system: channel_ownership_changed { to }` is added to a group, set its owner (Assumption 2). Find where messages enter the store (`add_message` or the `Message` event path) and do it there.
  - `is_group_owner(channel) -> bool`
- `src/ui/chat.rs`: handlers. Join: `add_recipient`, `fetch_user` if unknown, `forget_member_lists`. Leave of someone else: `remove_recipient`, `forget_member_lists`. Leave of you: `remove_channel` and `forget_channel`, then `drop_stale_selection`, `remember_places`, `ensure_current_loaded`.

**Depends on:** milestone 19's `remove_channel` and `forget_channel`.
**Risks:**
- **System messages arriving out of order** with the owner's `ChannelUpdate`. Both carry the same owner, so either order ends the same.
- **History loading old ownership messages.** Only live messages (the `Message` event) should change the owner, not history pages, or an old transfer could overwrite a newer one. Apply it in the live path only.
- **A user fetch failing** leaves the recipient listed but not drawn (as today). That's acceptable.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/api/events.rs`: both events parse.
- `tests/store_groups.rs` (new): `add_recipient` twice adds once; `remove_recipient` removes; an ownership message sets the owner; `is_group_owner` follows it.

**Manual check:**
- From the web client, add a friend to a group RuSToat is showing with the member list open: they appear. Remove them: they disappear.
- Have the web client's owner remove you: the group disappears from Home.
- Have the owner leave from the web client: the member list's owner crown (or menu items in chunk 4) move to the new owner.

**Shippable when:** group membership and owners stay right live, and the tests pass.

### Chunk 2: Create a group
**Goal:** Create a group with chosen friends from the Friends page, the Home list or a friend's menu.
**Files likely touched:**
- `src/api/client.rs`: `create_group(name, users: &[String]) -> Channel` (`POST /channels/create`, a private struct with `name` and `users`). `GlobalLimits.group_size: Option<u32>`.
- `src/groups.rs` (new, pure): `filter_friends(rows, query)`, `group_error_text(&ApiError, name: Option<&str>)`, the picker's limit text.
- `src/ui/group_dialogs.rs` (new): `FriendPicker` (a search `InputState` and a scrolling list of friends with checkboxes, selection kept in order) and `CreateGroupDialog` ("Create a new group", "Group Name" with a counter, the picker, Close and Create).
- `src/ui/chat.rs`: `open_create_group(preselected: Option<String>)`; `group_created(channel)`: `add_channel`, open it; a "+" (`IconName::Plus`, ghost, xsmall, tooltip "Create a group") at the right of the "Direct Messages" title.
- `src/ui/friends.rs`: "New group" beside "Add friend".
- `src/profile.rs`: `UserMenuItem::CreateGroupWith`, shown for friends.

**Depends on:** None.
**Risks:**
- **Long friend lists.** Use `uniform_list` for the picker, as the Friends page does.
- **Checkbox rows inside a dialog** taking focus from the search box: clicks toggle selection without moving focus.
- **The `+` in the Home title** must not shift the title when the list scrolls.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/groups.rs`: `filter_friends` by display name and username, case-insensitively; empty query returns all; error texts for `NotFriends`, `GroupTooLarge { max: 50 }`.
- `tests/api_groups.rs` (new, wiremock): `POST /channels/create` sends exactly `{"name":"Trip","users":["u1","u2"]}` and parses a `Group`; a 403 `NotFriends` stays an `Api` error.
- `tests/api_login.rs` (or wherever `RootConfig` is parsed): `group_size` read, `None` without it.

**Manual check:**
- From each entry point, create a group with two friends: it opens, appears in Home, and the web client shows it for them.
- "Create group with…" pre-selects that friend.
- Search narrows the picker.

**Shippable when:** groups can be created from all three places, and the tests pass.

### Chunk 3: Add and remove group members, and group invites
**Goal:** Members add friends from the group header; owners remove members from the member list; groups get invites.
**Files likely touched:**
- `src/api/client.rs`: `add_group_member(channel, user)` (`PUT`), `remove_group_member(channel, user)` (`DELETE`), both `()`.
- `src/ui/group_dialogs.rs`: `AddMembersDialog` ("Add friends to group", the picker without current members, Close and Add).
- `src/ui/chat.rs`:
  - the header button "Add friends to group" for groups with `InviteOthers`
  - `add_members(channel, users)` (Assumption 4)
  - `confirm_remove_member(channel, user)` (Assumption 5)
  - `open_create_invite` and the header's Create invite accept groups (Assumption 9)
- `src/profile.rs`: `UserMenuItem::RemoveFromGroup`, shown when `channel` is a group you own and the user isn't you.
- `src/permissions.rs`, `src/store.rs`: `can_invite` allows groups; `can_add_to_group(channel)`.
- `src/ui/invites.rs`: the group title and no "To #channel" line.

**Depends on:** Chunk 1 (recipient updates), chunk 2 (`FriendPicker`).
**Risks:**
- **The header's invite and add buttons** both use `UserPlus`. Give "Add friends to group" `IconName::UserPlus` and Create invite `IconName::Link` (or similar) on groups, so they differ.
- **The user menu's channel.** "Remove from group" must only show from the group's own member list or messages, where `People` has the group as its channel.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `tests/api_groups.rs`: `PUT` and `DELETE /channels/g1/recipients/u1` with empty bodies; `AlreadyInGroup` and `NotFriends` stay `Api` errors.
- `tests/permissions.rs`: `can_invite` true in a group by default, false when the group's permissions drop `InviteOthers`, false in a DM and Saved Notes.
- `tests/profile.rs`: "Remove from group" shows for the owner on another member, not on yourself, not for a non-owner.

**Manual check:**
- Add two friends from the header: both appear in the member list and the group shows "added" messages.
- As owner, remove one: they disappear; the web client shows the removal.
- Create an invite for a group and join it from another account.

**Shippable when:** members can be added and removed and groups have invites, and the tests pass.

### Chunk 4: Leave a group and transfer ownership
**Goal:** Leave a group (silently if you like), choosing the next owner if you own it, and hand ownership over from the member list.
**Files likely touched:**
- `src/api/client.rs`: `leave_channel(id, silently)` (`DELETE /channels/{id}?leave_silently=…`), `transfer_group(id, owner) -> Channel` (`PATCH /channels/{id}` with exactly `{"owner": id}`). Milestone 19's `delete_channel` (no query) stays for server channels.
- `src/groups.rs`: `default_next_owner(recipients, owner) -> Option<String>`.
- `src/ui/chat.rs`: "Leave group…" in `channel_menu` for groups; `confirm_leave_group(channel)` (Assumption 6); `confirm_make_owner(channel, user)` (Assumption 7).
- `src/profile.rs`: `UserMenuItem::MakeGroupOwner`.

**Depends on:** Chunk 1.
**Risks:**
- **A `DropdownMenu` and a checkbox in one alert dialog.** If the alert dialog can't hold them cleanly, use `open_dialog` with a custom footer, as `CreateInviteDialog` does.
- **The two-step leave.** If the transfer succeeds and the leave fails, you're still in the group as a member; say so in the error ("{name} is now the owner, but leaving failed…").

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/groups.rs`: `default_next_owner` picks the first non-owner in order; `None` when alone.
- `tests/api_groups.rs`: `DELETE /channels/g1?leave_silently=true`; `PATCH /channels/g1` with exactly `{"owner":"u2"}` parses the group; `NotOwner` stays an `Api` error.

**Manual check:**
- As a member, leave silently: no "left" message in the web client.
- As owner of a three-person group, leave choosing the second member: they become owner.
- As the only member, the dialog says leaving deletes it.
- Make another member owner from the member list: "Remove from group" disappears for you.

**Shippable when:** leaving and transferring work with the owner's choice, and the tests pass.

### Chunk 5: Close a DM
**Goal:** Close a DM from its menu.
**Files likely touched:**
- `src/ui/chat.rs`: "Close conversation…" in `channel_menu` for DM rows; `confirm_close_dm(channel)` (Assumption 8) using `leave_channel(id, false)` (no query needed, but harmless) or `delete_channel`.
- `src/store.rs`: `deactivate_dm(channel)`.

**Depends on:** milestone 19's `delete_channel`.
**Risks:** reopening from a profile card must still work: `message_user` calls `activate_dm`. Test it.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`: `tests/store_live.rs`: `deactivate_dm` hides it from `home_channels`, and `activate_dm` brings it back.

**Manual check:** Close a DM: it leaves Home here and in the other person's web client. Message them from their card: it comes back with its history.

**Shippable when:** DMs can be closed and reopened, and the tests pass.

### Chunk 6: Mutuals on profile cards
**Goal:** Profile cards show mutual friends, and mutual servers and groups, with full lists one click away.
**Files likely touched:**
- `src/api/client.rs`: `fetch_mutual(user) -> MutualResponse`.
- `src/profile.rs` (pure): `Mutuals { friends: Vec<..>, places: Vec<..> }` from a `MutualResponse` and the store (Assumption 10), and `overflow_text(n)`.
- `src/ui/chat.rs`: fetch with the profile in `open_card`, cache in a `mutuals` map; handle clicks.
- `src/ui/profile_card.rs`: the two rows, after Roles (or wherever reads best), and the list dialog.

**Depends on:** None.
**Risks:**
- **Rate limits.** The `/users` bucket is 20 per 10 s, shared with profiles and DMs. Opening many cards quickly could hit it; the cache helps, and a `RateLimited` just leaves the rows out.
- **Card height.** The rows appear after the fetch; the card must grow without jumping off-screen. Check the popover repositions.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/profile.rs`: `Mutuals` keeps servers then groups, drops DMs and unknown IDs, and caps nothing (the UI caps); `overflow_text(3)` is "+3".
- `tests/api_users.rs`: `GET /users/u1/mutual` parses all three lists.

**Manual check:** Open a friend's card who shares servers and a group: both rows show. Click each: the list opens, and choosing a server opens it. Your own card shows neither.

**Shippable when:** mutuals show on cards with working lists, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`. Formatting rules as in milestone 19's plan: format only touched files with `rustfmt --edition 2024 <file>`, never `mod.rs`, `lib.rs` or `main.rs`; don't let a drifting file's `Diff in` count grow (check counts at the start of the milestone and record them in the implementation record).

### Unit Tests
- **`src/groups.rs`:** friend filtering, the default next owner, error texts.
- **`src/profile.rs`:** `Mutuals`, the overflow text, the new user-menu items' visibility.
- **`src/api/events.rs`:** `ChannelGroupJoin` and `ChannelGroupLeave`.

### Integration Tests
- **`tests/api_groups.rs`** (new): create, add, remove, leave (both query values), transfer; exact bodies and paths; `NotFriends`, `AlreadyInGroup`, `NotOwner` stay `Api` errors.
- **`tests/api_users.rs`:** mutuals.
- **`tests/store_groups.rs`** (new): recipients, ownership messages, being removed.
- **`tests/permissions.rs`:** `can_invite` for groups.
- **`tests/store_live.rs`:** closing and reopening a DM.

### Manual Tests
You need two friends' accounts (one can be in the web client), a group you own with both, and a group someone else owns.
1. Run every chunk's manual check.
2. With two accounts in RuSToat, every dialog acts for the account on screen.
3. Light and dark themes: the picker, the dialogs and the mutual rows are readable.

## Definition of Done
- [ ] `ChannelGroupJoin` and `ChannelGroupLeave` update members live; being removed drops the group; owners update from transfers and from `channel_ownership_changed`.
- [ ] Groups can be created from the Friends page, the Home "+", and a friend's menu, with a searchable friend picker capped at 49, and open when created.
- [ ] "Add friends to group" adds people one by one with named errors; "Remove from group…" is owner-only and confirmed.
- [ ] Groups get invites from their menu and header; DMs don't.
- [ ] "Leave group…" has the silent checkbox; owners pick the next owner (default Stoat's choice); the last member is told leaving deletes it.
- [ ] "Make group owner…" transfers ownership after confirmation.
- [ ] "Close conversation…" closes a DM; messaging the person reopens it.
- [ ] Profile cards show mutual friends and mutual servers and groups (up to 8, then "+N"), with full lists.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/groups-and-dms-implementation.md`.
