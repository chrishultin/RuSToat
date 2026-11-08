# Implementation Record: Groups and DMs (RuSToat milestone 20)

_Plan: docs/plans/groups-and-dms.md_
_Started: 2026-11-07_

Pre-existing uncommitted changes, kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt `Diff in` counts (2026-11-07): `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15. Zero in every other file in scope.

Supervision: continuous, unattended overnight; manual checks batched at the end.

Commits: commit signing (Secretive, through Touch ID) wasn't available overnight, so each chunk is left staged, uncommitted, on top of milestone 19's staged chunks 2–6, and its tree (from `git write-tree`), with its patch against the one before, is recorded in `/tmp/rustoat-chunks/ledger.txt`. They're to be committed one by one in the morning, each from its recorded tree.

## Chunk 1: Follow group membership live

### Changes Made
- `src/api/events.rs`: `ServerEvent::ChannelGroupJoin { channel, user }` and `ServerEvent::ChannelGroupLeave { channel, user }` (Stoat's `id` is the group), parsed in one arm as the typing events are, and forwarded. One parse test for both.
- `src/store.rs`:
  - `add_recipient(channel, user) -> bool`: appends to a group's `recipients` if they aren't in it. `false` for a repeat, a DM, a server channel or an unknown channel.
  - `remove_recipient(channel, user) -> bool`: takes them out of a group's `recipients`; `false` if they weren't in it.
  - `apply_message` (the live path: the `Message` event, and the user's own send response) sets a group's `owner` to the `to` of a `channel_ownership_changed` system message (Assumption 2). History pages (`set_history`, `append_page`, `prepend_page`) don't.
  - `is_group_owner(channel) -> bool`: the channel is a group and its owner is the user.
- `src/ui/chat.rs`:
  - `ChannelGroupJoin`: `add_recipient`; if they were new, `fetch_user` when the store doesn't know them (Stoat sends existing members no user object), then `refresh_suggestions` and `forget_member_lists`.
  - `ChannelGroupLeave` of someone else: `remove_recipient`, then `refresh_suggestions` and `forget_member_lists`. Of the user (removed, or left on another device): `channel_deleted` (milestone 19's `remove_channel`, `forget_channel`, `drop_stale_selection`, `remember_places`, `ensure_current_loaded`).
  - The `Message` handler forgets the member lists after a `channel_ownership_changed` message. `SystemMessage` added to the hand-formatted models import.
- `tests/store_groups.rs` (new, from `ready.json`): adding twice adds once and the member list counts them; only groups take recipients; removing takes them out of the member list; a removed group leaves Home; a live ownership message hands the group on (twice, the later winning) and `is_group_owner` follows; one in a history page changes nothing; `is_group_owner` is false for a server channel of a server the user owns, a DM and an unknown channel.

### Divergences
- **The handlers also call `refresh_suggestions`,** as `ServerMemberJoin` and `ServerMemberLeave` do, since a group's `@` suggestions come from its recipients.
- **No owner crown changes,** because RuSToat's member list doesn't draw one yet. The owner change is in the store (and so in group permissions, where the owner has everything); the menu items that show it come in chunks 3 and 4.

### Verification Performed
- `cargo test --lib group_membership`: 1 passed. `cargo test --test store_groups`: 7 passed.
- `cargo test`: 960 passed, 0 failed (952 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/api/events.rs` 3, `src/ui/chat.rs` 17 and `src/store.rs` 15, unchanged (hand-formatted). `tests/store_groups.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. From the web client, add a friend to a group RuSToat is showing, with the member list open: they appear (also when RuSToat didn't know them before). Remove them: they disappear.
2. Have the web client's owner remove you: the group disappears from Home, and if it was open, Home's first conversation opens.
3. Leave a group from the web client on the same account: it disappears from RuSToat too.
4. Have the owner leave from the web client: the "gave ownership to" message shows, and the new owner is the group's owner (visible once chunks 3 and 4 add the owner-only menu items).

## Chunk 2: Create a group

### Changes Made
- `src/api/client.rs`: `create_group(name, users) -> Channel` (`POST /channels/create` with exactly `name` and `users`, from a private struct). `GlobalLimits.group_size: Option<u32>`.
- `src/groups.rs` (new, pure):
  - `NEW_GROUP_MAX` (49), `DEFAULT_GROUP_SIZE` (100), and `new_group_limit(group_size)`: 49, or fewer on an instance whose groups are smaller.
  - `pickable_friends(store, exclude)`: `friend_lists(store).all` without `exclude` (for chunk 3's members).
  - `filter_friends(rows, query)`: by display name or username, ignoring case; a blank query keeps all.
  - `toggle_pick(picked, user, limit)`: picks keep their order and stop at the limit. `limit_text(limit)`: "You can add at most 49 people." ("This group is full." at 0).
  - `group_error_text(err, name, fallback)` (Assumption 11's `NotFriends`, `GroupTooLarge { max }`, `NotOwner`, `NotInGroup` with the name, else `profile_edit::error_text`), and `create_group_error_text(err)` ("Couldn't create the group. Try again.").
- `src/friends.rs`: `FriendRow.username`, for the picker's search.
- `src/ui/group_dialogs.rs` (new):
  - `FriendPicker`: `new(store, exclude, picked, limit, window, cx)`, `picked() -> &[String]`, `set_disabled(bool, cx)`; emits `PicksChanged`. A search field ("Search for users…") over a 260 px `uniform_list` of friends with avatars; each row is a `Checkbox` (which doesn't take focus on click), disabled once the limit is reached unless picked. Under it, "{n} selected", or the limit text once full. Friends are read once, when it opens.
  - `CreateGroupDialog` ("Create a new group", 420 px): "Group name" with the 0/32 counter (milestone 19's `name_field`), "Select members to add" and the picker, Close and Create. Enter in the name creates. Errors show under the name and clear when the name or the picks change.
- `src/ui/server_dialogs.rs`: `name_field` and `field_label` are `pub(crate)`.
- `src/ui/chat.rs`:
  - `open_create_group(preselected)`, focusing the name; `group_created(channel)`: `add_channel` and `open_channel`.
  - Home's title row: a ghost xsmall "+" (`IconName::Plus`, tooltip "Create a group") at the right of "Direct Messages", once Ready. The title row sits above the scrolling list, so it doesn't move.
  - `group_size` from the root config, with a `group_size()` accessor the dialog reads.
  - `UserMenuItem::CreateGroupWith`: closes the card and opens the dialog with them picked. Label "Create group with…", `IconName::Users`.
- `src/ui/friends.rs`: an outline "New group" (`IconName::Users`) before "Add friend".
- `src/profile.rs`: `UserMenuItem::CreateGroupWith`, after Message, for friends (never the user).
- Tests: `tests/api_groups.rs` (new: the exact body and a parsed `Group`, a group of one sending `"users": []`, `NotFriends` and `GroupTooLarge` staying `Api` errors); `tests/links.rs` (`group_size` read from `root.json`, `None` without it); `tests/profile.rs` (the friend's full menu now has Create group with; it's for friends only, and shows once someone becomes a friend); `tests/friends.rs` (`pickable_friends`: friends only, offline too, by name, minus the excluded); `src/groups.rs` unit tests (filtering, picks and the limit, `new_group_limit`, `limit_text`, the error texts).

### Divergences
- **`group_error_text` takes a `fallback`** as well as the name (`group_error_text(err, name, fallback)`), since each action's "Couldn't … Try again." differs. `MissingPermission` isn't in it: it means different things when adding (chunk 3's "You can't add people to this group.") and removing, so callers match it first.
- **The picker's limit is `min(49, group_size - 1)`** rather than a fixed 49, so a smaller instance's `group_size` is respected. On Stoat's defaults it's 49.
- **The search placeholder** is "Search for users…" with RuSToat's ellipsis character; the field label is "Group name" (RuSToat's sentence case, as "Server name"), not the web client's "Group Name".
- **"Create group with…" sits after Message** in a friend's menu (it's RuSToat's own item, so there's no web-client order to follow). The existing full-menu test gained it.
- **Rows show the display name only,** as the web client's picker does; the search matches usernames too.

### Verification Performed
- `cargo test --lib groups::`: 6 passed. `cargo test --test api_groups`: 4 passed. `--test profile`, `--test links` and `--test friends`: pass, with one new test each.
- `cargo test`: 973 passed, 0 failed (960 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/api/client.rs` 3 and `src/ui/chat.rs` 17, unchanged (hand-formatted). `src/groups.rs`, `src/ui/group_dialogs.rs`, `tests/api_groups.rs`, `src/friends.rs`, `src/ui/friends.rs`, `src/profile.rs`, `src/ui/server_dialogs.rs`, `tests/profile.rs`, `tests/links.rs` and `tests/friends.rs` 0. `src/lib.rs` and `src/ui/mod.rs` got one `pub mod` line each and weren't formatted.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. From each entry point (the Friends page's "New group", Home's "+" beside "Direct Messages", "Create group with…" in a friend's menu, from a row and from their card's "…"), create a group with two friends: it opens, appears in Home, and the web client shows it for them.
2. "Create group with…" pre-selects that friend (and shows "1 selected").
3. Search narrows the picker by display name and by username; picks stay picked while hidden; clicking rows doesn't take the focus from the search field.
4. Create with nobody picked: a group of one opens.
5. Create is disabled with a blank name or one over 32 characters; Enter in the name creates.
6. Light and dark themes: the picker's rows, checkboxes and hover are readable.

## Chunk 3: Add and remove group members, and group invites

### Changes Made
- `src/api/client.rs`: `add_group_member(group, user)` (`PUT /channels/{group}/recipients/{user}`) and `remove_group_member(group, user)` (`DELETE`, same path), both without a body, returning `()`.
- `src/permissions.rs`: `can_invite` allows groups as well as server channels (not DMs or Saved Notes, whose defaults also hold Invite Others). New `can_add_to_group(store, user, channel)`: a group, with Invite Others. `src/store.rs`: `ChatStore::can_add_to_group(channel)`.
- `src/groups.rs`: `add_limit(group_size, members)` (how many more a group takes, 0 when full); `add_member_error_text(err, name)` ("Couldn't add {name}: {why}", with Missing Permission as "You can't add people to this group.", and "Couldn't add {name}. Try again." when there's no better reason); `remove_member_error_text(err, name)` (Missing Permission as "Only the group's owner can remove members.", else `group_error_text`).
- `src/ui/group_dialogs.rs`:
  - `FriendPicker` gains `search()`, `name_of(user)` and `exclude(users, limit, cx)` (drops people who've just joined from the list and the picks).
  - `AddMembersDialog` (`ADD_MEMBERS_DIALOG_WIDTH` 420 px, titled "Add friends to group"): the picker without the group's members, limited to `add_limit`; the error under it; Close and Add (disabled with nobody picked, spinning while adding). Add sends one `PUT` per person in pick order (Assumption 4): `AlreadyInGroup` is passed over, the first other refusal stops it and names the person, those added leave the list and the rest stay picked. With no refusal the dialog closes. Each success shows in the group at once through `ChatView::group_member_added`, and the requests carry on if the dialog is closed meanwhile.
- `src/ui/chat.rs`:
  - `group_member_added(channel, user, cx) -> bool` and `group_member_removed(channel, user, cx) -> bool`: chunk 1's `ChannelGroupJoin` and `ChannelGroupLeave` (someone else) handling moved into these, now shared with adding and removing.
  - The header: "Add friends to group" (ghost, `IconName::UserPlus`) in groups where `can_add_to_group`, opening `open_add_members(channel)` with the search focused. Create invite shows in groups too, with `IconName::Link` there so the two differ (server channels keep `UserPlus`).
  - `channel_menu`: Create invite… for any channel `can_invite` allows, so groups get it.
  - `open_create_invite` accepts groups: "Invite people to {group}".
  - `confirm_remove_member(channel, user)` (Assumption 5): "Remove {name}?", "Are you sure you want to remove {name} from {group}?", Cancel and Danger "Remove", with milestone 19's waiting footer (`removing_member` disables both buttons and Escape while Stoat answers). `remove_member` applies it locally on success and closes; `NotInGroup` counts as done; other refusals are a toast and the confirmation stays.
  - `UserMenuItem::RemoveFromGroup` closes the card and opens the confirmation. Label "Remove from group…", `IconName::UserMinus`.
- `src/ui/invites.rs`: `CreateInviteDialog.channel_name` is `None` for a group, which drops the "To #channel" line.
- `src/profile.rs`: `UserMenuItem::RemoveFromGroup`, in a section of its own after Profile…Mention, when the menu's channel is a group the user owns (`is_group_owner`), the person isn't the user, and they're in it.
- Tests: `tests/api_groups.rs` (`PUT` and `DELETE /channels/g1/recipients/u1` with empty bodies; `AlreadyInGroup` and `NotFriends` stay `Api` errors); `tests/permissions.rs` (`can_invite` and `can_add_to_group` in a group by default, not when its permissions drop Invite Others, always for its owner, not for a non-member; neither in a DM or Saved Notes; no adding in a server channel; replacing "invites to DMs and groups are not offered"); `tests/profile.rs` (the owner's full menu for a member has Remove from group in its own section; not on yourself, someone outside the group, or from another channel; not for a plain member); `src/groups.rs` unit tests (`add_limit`, the adding and removing error texts).

### Divergences
- **The adding loop is `AddMembersDialog::add`,** not a `ChatView::add_members`, so the dialog can show the error and keep the remaining picks. The store side is `ChatView::group_member_added`, called once per success.
- **Added people are applied locally at once,** like removals (Assumption 5), so they appear in the member list before their `ChannelGroupJoin` (which then changes nothing).
- **The add picker's limit is the group's room** (`group_size` minus its members, 100 by default), not chunk 2's 49 for new groups; a full group shows "This group is full."
- **Remove from group is only offered for people in the group,** so it doesn't show on a message author who has since left (Stoat would answer `NotInGroup`). A `NotInGroup` answer is treated as done, as `NotFound` is for deleting a channel.
- **Removing has its own Missing Permission text,** "Only the group's owner can remove members." (Assumption 11 only gives one for adding).
- **Remove from group is a plain menu item,** not red, as Remove friend and Block are. It sits in its own section after Mention (chunk 4's Make group owner can join it).
- **Create invite… in the channel menu keeps `IconName::UserPlus`;** only the header's button switches to `Link` on groups, where it sits beside Add friends.

### Verification Performed
- `cargo test --lib groups::`: 9 passed. `cargo test --test api_groups`: 8 passed. `--test permissions` and `--test profile`: pass, with two more tests each.
- `cargo test`: 984 passed, 0 failed (973 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/api/client.rs` 3, `src/ui/chat.rs` 17 and `src/store.rs` 15, unchanged (hand-formatted). `src/permissions.rs`, `src/profile.rs`, `src/groups.rs`, `src/ui/group_dialogs.rs`, `src/ui/invites.rs`, `tests/api_groups.rs`, `tests/permissions.rs` and `tests/profile.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. In a group, the header shows "Add friends to group" (person with a plus) and Create invite (a link). Add two friends: both appear in the member list at once, and the group shows the "added" messages. The picker leaves out people already in the group.
2. Make one add fail (for example, a friend who unfriends you while the dialog is open): the error names them, anyone added before stays added and leaves the list, the rest stay picked.
3. As owner, right-click a member (in the member list, and on one of their messages): "Remove from group…" asks "Remove {name}?", and Remove takes them out; the web client shows the removal. It isn't offered on yourself, in a group you don't own, or for them in another channel.
4. Create an invite for a group from its header and from its menu: the title is "Invite people to {group}" with no "To #…" line. Join it from another account.
5. DMs and Saved Notes offer neither button nor Create invite….

## Chunk 4: Leave a group and transfer ownership

### Changes Made
- `src/api/client.rs`: `transfer_group(group, owner) -> Channel` (`PATCH /channels/{group}` with exactly `{"owner": …}`, from a private struct) and `leave_channel(group, silently)` (`DELETE /channels/{group}?leave_silently=…`, no body). Milestone 19's `delete_channel` stays as it was for server channels.
- `src/store.rs`: `set_group_owner(channel, user) -> bool` (`false` for a repeat or anything but a group). Chunk 1's live `channel_ownership_changed` handling in `apply_message` now goes through it.
- `src/groups.rs`: `default_next_owner(recipients, owner)` (the first other member, `None` when alone); `transfer_error_text(err, name)` (Missing Permission as "Only the group's owner can do that.", else `group_error_text` with "Couldn't make {name} the owner. Try again."); `leave_group_error_text(err, handed_to)` ("Couldn't leave the group. Try again.", or "{name} is now the owner, but leaving failed…" when the group was handed on first).
- `src/profile.rs`: `UserMenuItem::MakeGroupOwner`, after Remove from group in the same section and with the same gating (the menu's channel is a group the user owns, the person isn't the user, and they're in it).
- `src/ui/chat.rs`:
  - The leave confirmation is shared by servers and groups: `LeavePrompt` holds a `LeaveTarget` (`Server` or `Group`) and the owner's `next_owner` pick; `confirm_leave_server` and the new `confirm_leave_group` both open `open_leave_prompt`, whose Leave goes through `leave_now` to `leave_server_now` or `leave_group_now`.
  - For a group (Assumption 6), each time it's drawn the dialog reads the group as it is now (`group_leave` → `GroupLeave { alone, candidates, next_owner }`). For its owner with others in it, a "New owner" row has an outline dropdown of the other members' display names in `recipients` order, starting at Stoat's choice. For the only member, the description is "You're the only member, so leaving deletes the group." and the button says "Delete group". Otherwise it's "You won't be able to rejoin unless you are re-invited.", the "Don't notify others that you've left" checkbox, Cancel and Danger "Leave", with milestone 19's waiting footer.
  - `leave_group_now`: if the user still owns the group and picked someone other than Stoat's choice, `transfer_group` is sent first, then `leave_channel`, in one request task. If the handover is refused, nothing else is sent and a toast says why. If the leave is refused after a handover, the new owner is applied locally and the toast says "{name} is now the owner, but leaving failed…". On success (or `NotFound`) the group goes through `channel_deleted`, the dialog closes and the toast says "Left {name}."
  - "Leave group…" (red, `IconName::LogOut`) in `channel_menu` for groups, after Delete channel's place and before Copy channel ID.
  - `confirm_make_owner(channel, user)` (Assumption 7): "Make {name} the owner?", "You'll stay in the group, but only {name} will be able to remove members or hand ownership on.", Cancel and primary "Make owner", waiting on `transferring_group` as Remove does on `removing_member`. `make_owner` applies the response's `owner` with `set_group_owner`, forgets the member lists and closes; a refusal is a toast and the confirmation stays.
  - `UserMenuItem::MakeGroupOwner` closes the card and opens the confirmation. Label "Make group owner…", `IconName::Crown`.
- `src/assets.rs`: `Crown` bundled, and in `people_icons_load`.
- Tests: `tests/api_groups.rs` (`DELETE /channels/g1` with `leave_silently=true` and `=false` and an empty body; `PATCH /channels/g1` with exactly `{"owner":"u2"}` parsing the group; `NotOwner` staying an `Api` error); `tests/profile.rs` (the owner's full menu for a member now has Make group owner after Remove from group; it's not on yourself, someone outside the group, from another channel, or for a plain member; once the group is handed on, neither item shows); `tests/store_groups.rs` (`set_group_owner` hands it on, a repeat changes nothing, only groups); `src/groups.rs` unit tests (`default_next_owner`, the handover and leaving error texts).

### Divergences
- **One confirmation for servers and groups.** Milestone 19's `LeavePrompt` gained a `LeaveTarget` and `next_owner` instead of a second prompt; the server flow is unchanged apart from its buttons' element IDs (`leave-cancel`, `leave-ok`).
- **The handover is only sent when it changes something.** If the owner leaves Stoat's choice picked (the default), RuSToat sends just the `DELETE` and Stoat hands the group on itself. If the user is no longer the owner by the time they press Leave, there's no handover either.
- **The dialog follows the group live.** If the picked person leaves while the dialog is open, the picker falls back to Stoat's choice; if everyone else leaves, it turns into "Delete group".
- **No silent checkbox for the only member,** since there's nobody to notify (Assumption 6 lists it for every member).
- **A new `ChatStore::set_group_owner`** applies the PATCH response's owner (and the owner after a handover whose leave failed); chunk 1's ownership messages use it too.
- **Make group owner is a primary (not Danger) button, with no toast on success,** like Remove; the "gave ownership" system message and the menu items changing show it worked. Its icon is `Crown`, newly bundled.
- **Handing over has its own Missing Permission text,** "Only the group's owner can do that." (the same words as `NotOwner`).
- **A member the store doesn't know** is listed in the New owner picker by user ID rather than left out, since Stoat may hand the group to them.

### Verification Performed
- `cargo test --lib groups::`: 12 passed. `cargo test --test api_groups`: 11 passed. `--test profile` and `--test store_groups`: pass, with one more test each.
- `cargo test`: 992 passed, 0 failed (984 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/api/client.rs` 3, `src/ui/chat.rs` 17 and `src/store.rs` 15, unchanged (hand-formatted). `src/groups.rs`, `src/profile.rs`, `src/assets.rs`, `tests/api_groups.rs`, `tests/profile.rs` and `tests/store_groups.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. As a member (not the owner), right-click a group in Home: "Leave group…" (red) asks "Leave {name}?" with the silent checkbox. Leave silently: the group leaves Home, the toast says "Left {name}.", and the web client shows no "left" message. Leave another without ticking it: the web client shows the "left" message.
2. As owner of a three-person group, open "Leave group…": "New owner" shows the first other member. Pick the second member and leave: they become owner in the web client (the "gave ownership" message names them).
3. As owner, leave with the default pick: Stoat's choice (the first other member) becomes owner.
4. As the only member, the dialog says "You're the only member, so leaving deletes the group.", has no checkbox, and its button says "Delete group"; the group is deleted.
5. As owner, right-click a member in the group's member list (and on one of their messages): "Make group owner…" asks "Make {name} the owner?"; Make owner hands it on, and afterwards neither "Remove from group…" nor "Make group owner…" is offered to you. Neither shows for a plain member or on yourself.
6. Light and dark themes: the New owner dropdown and the checkbox in the leave dialog are readable.
