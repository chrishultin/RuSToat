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

## Chunk 5: Close a DM

### Changes Made
- `src/store.rs`: `deactivate_dm(channel) -> bool`, the counterpart of `activate_dm`: sets a DM's `active` to false; `false` if it was already closed or isn't a DM. The DM stays in the store with its history.
- `src/ui/chat.rs`:
  - "Close conversation…" (red, `IconName::Close`) in `channel_menu` for DMs, after Leave group…'s place and before Copy channel ID.
  - `confirm_close_dm(channel)` (Assumption 8): "Close conversation with {name}?", "You can re-open it later, but it will disappear on both sides.", Cancel and Danger "Close", with milestone 19's waiting footer (`closing_dm` disables both buttons and Escape while Stoat answers).
  - `close_dm` sends `delete_channel` (`DELETE /channels/{dm}`, no query). On success, or `NotFound`, `dm_closed` applies it and the confirmation closes; otherwise a toast says why and the confirmation stays.
  - `dm_closed(channel)`: `deactivate_dm`, then `drop_stale_selection`, `remember_places` and `ensure_current_loaded`, so if it was open, Home's first conversation opens. Stoat's `ChannelUpdate { active: false }` then changes nothing.
- `tests/store_live.rs`: a DM closed here leaves Home and `shows_channel`, is still found by `profile::dm_with` (what "Message" uses), isn't closed twice, ignores non-DMs, survives the repeated `ChannelUpdate`, and `activate_dm` brings it back with its loaded history.

### Divergences
- **`delete_channel`, not `leave_channel(id, false)`:** the plan allowed either; a DM takes no `leave_silently`, so the request is exactly the web client's.
- **"Close conversation…" is red, with `IconName::Close`,** like Leave group… (the web client's item for a DM is its destructive delete-channel button).
- **No toast on success,** as for Delete channel: the DM leaving Home shows it worked, and Assumption 8 lists none. Refusals use `profile_edit::error_text` with "Couldn't close the conversation. Try again." (Assumption 11 has no text for this).
- **`dm_closed` always runs `remember_places` and `ensure_current_loaded`,** as `channel_deleted` does, rather than only when the selection changed (as the `ChannelUpdate` handler does). A DM that was open only as Home's default, not an explicit selection, then still has its successor loaded. The `ChannelUpdate` handler itself is unchanged.
- **The test sits in `tests/store_live.rs`** beside the existing "closed DM reopened here" test, and also checks the history is kept and `dm_with` still finds the closed DM.

### Verification Performed
- `cargo test --test store_live dm_closed`: 1 passed.
- `cargo test`: 993 passed, 0 failed (992 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/ui/chat.rs` 17 and `src/store.rs` 15, unchanged (hand-formatted). `tests/store_live.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Right-click a DM in Home: "Close conversation…" (red) asks "Close conversation with {name}?". Close: it leaves Home here and in the other person's web client. Groups and Saved Notes don't offer it.
2. Close the DM that's open: Home's first conversation opens.
3. Message them from their card (and from the Friends page's Message): the DM comes back with its history.
4. Cancel and Escape keep the DM; while Stoat answers, both buttons are disabled and Escape does nothing.

## Chunk 6: Mutuals on profile cards

### Changes Made
- `src/api/client.rs`: `fetch_mutual(user) -> MutualResponse` (`GET /users/{id}/mutual`).
- `src/profile.rs`:
  - `shows_mutuals(store, user)`: not on the user's own card (Stoat refuses it), a bot's (as the web client) or an unknown user's.
  - `Mutuals { friends: Vec<MutualFriend>, places: Vec<MutualPlace> }` and `mutuals(store, &MutualResponse)` (Assumption 10): friends (ID, display name, avatar), then servers, then groups (ID, name, icon, `MutualKind`), each looked up in the store; unknown IDs, DMs and anything that isn't a group are left out; nothing is capped.
  - `overflow_text(n)`: "+{n}".
- `src/ui/profile_card.rs`:
  - `CardContent.mutuals`, and `render_profile_card` takes an `OnMutuals`. After Roles, "Mutual friends" and "Mutual servers and groups" (in the card's upper-case section headings) show up to 8 round 24 px icons each, then "+N"; each icon has its name as a tooltip, and an empty row isn't drawn. The whole row is clickable, with a hover background that reaches a little past the icons without moving them.
  - `MutualList` (`Friends`, `Places`, with their titles), `MutualPick` (`Friend`, `Server`, `Group`) and `render_mutuals_list(list, mutuals, on_pick, cx)`: every entry with a 32 px icon and its name, scrolling past 400 px. `MUTUALS_DIALOG_WIDTH` 360 px.
  - Servers without an icon show their initial (`avatar`'s placeholder); groups without one show the group glyph, as in Home's list.
- `src/ui/chat.rs`:
  - `mutuals: HashMap<String, Option<MutualResponse>>`, beside `profiles`. `open_card` fetches them (`fetch_mutuals`) alongside the profile when `shows_mutuals`; the last answer shows meanwhile. `MissingPermission` stores `None`, hiding both rows; other failures (a `RateLimited` included) are logged and keep what was there.
  - `card_mutuals(user)`: the cached answer through `profile::mutuals`, so a server left since drops out at once.
  - `open_mutuals(user, list)`: closes the card (focus back to the message box) and opens `open_mutuals_dialog`, titled with the row's heading, drawn from the store each time and closing itself if its list empties.
  - `mutual_picked`: the list closes; a friend's card opens where the mouse is; a server (if still known) is selected with `select_place`; a group opens with `open_channel`.
- Tests: `tests/api_users.rs` (`GET /users/u1/mutual` parses all three lists; `MissingPermission` stays an `Api` error); `tests/profile.rs` (friends then servers then groups, with names, avatars and kinds; DMs, server channels and unknown users, servers and groups left out; nothing capped; `overflow_text`; `shows_mutuals` for others, not yourself, bots or strangers).

### Divergences
- **Mutuals are fetched every time a card opens,** as profiles are, rather than once per session; the cached answer shows while the new one comes, so the rows don't flicker. Each card open is two requests on the shared `/users` bucket; a refusal for rate limits keeps the rows from before.
- **Clicking a row closes the card** before the list opens, and **choosing a friend closes the list** before their card opens: the card floats in the chat view, under a dialog's focus trap and Escape, so the two aren't shown together.
- **The rows sit after Roles, before Member since.**
- **The `Mutuals` tests are in `tests/profile.rs`,** where `profile.rs`'s tests already live, not unit tests in `src/profile.rs`; the API test also checks `MissingPermission`.
- **`shows_mutuals` also skips users the store doesn't know** (their card can't open anyway).
- **Lists keep Stoat's order,** not sorted by name.
- **Each icon on the card has its name as a tooltip,** and the list dialog has the dialog's own title and ✕, with no Close button of its own.
- **Without an icon,** a server shows its first initial (as other avatars) rather than the rail's two-letter initials, which don't fit 24 px.

### Verification Performed
- `cargo test --test profile mutual`: 4 passed (and `the_overflow_counts_the_rest`). `cargo test --test api_users mutual`: 2 passed.
- `cargo test`: 1000 passed, 0 failed (993 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts: `src/api/client.rs` 3 and `src/ui/chat.rs` 17, unchanged (hand-formatted). `src/profile.rs`, `src/ui/profile_card.rs`, `tests/profile.rs` and `tests/api_users.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Open the card of a friend who shares a server and a group with you: once it has loaded, "MUTUAL FRIENDS" and "MUTUAL SERVERS AND GROUPS" show after Roles (or where Roles would be), with their icons; hovering an icon names it. With more than 8 in a row, 8 show and then "+N".
2. Hovering a row highlights it. Click it: the card closes and "Mutual friends" (or "Mutual servers and groups") lists every one, scrolling if long.
3. In the list, click a friend: the list closes and their card opens where the mouse was. Click a server: it opens. Click a group: it opens in Home.
4. Your own card, a bot's, and a blocked user's show neither row; someone with nothing in common shows neither.
5. Open a card near the bottom of the window: when the rows arrive, the card grows and stays inside the window.
6. Leave a shared server, then open the card again: it's gone from the row at once.

## Summary

_Completed: 2026-11-08_

### Chunks Completed
1. Group members and owners follow `ChannelGroupJoin`, `ChannelGroupLeave` and ownership messages live; a group you're removed from leaves Home.
2. Create a group from the Friends page's "New group", Home's "+" or a friend's "Create group with…", with a searchable friend picker, and open it.
3. Add friends to a group from its header, remove members as its owner, and invite people to groups.
4. Leave a group, silently if you like, choosing the next owner if it's yours; make another member owner.
5. Close a DM from its menu; messaging the person reopens it.
6. Mutual friends, and mutual servers and groups, on profile cards, with full lists.

### Divergences
- Chunk 1:
  - The handlers also call `refresh_suggestions`, as server member joins and leaves do.
  - No owner crown changes, since RuSToat's member list doesn't draw one; the owner change shows in the owner-only menu items.
- Chunk 2:
  - `group_error_text` takes a `fallback` as well as the name; callers match `MissingPermission` first.
  - The picker's limit is `min(49, group_size - 1)`, so a smaller instance is respected.
  - The search placeholder is "Search for users…"; the field label is "Group name" (sentence case).
  - "Create group with…" sits after Message in a friend's menu.
  - Picker rows show the display name only; the search matches usernames too.
- Chunk 3:
  - The adding loop is `AddMembersDialog::add`, not `ChatView::add_members`, so the dialog keeps the error and the remaining picks.
  - Added people are applied locally at once, like removals.
  - The add picker's limit is the group's room (`group_size` minus its members), not 49; a full group shows "This group is full."
  - Remove from group is only offered for people in the group; `NotInGroup` counts as done.
  - Removing has its own Missing Permission text, "Only the group's owner can remove members."
  - Remove from group is a plain (not red) item, in its own section after Mention.
  - Create invite… in the channel menu keeps `UserPlus`; only the header's button is a `Link` on groups.
- Chunk 4:
  - One leave confirmation (`LeavePrompt` with a `LeaveTarget`) serves servers and groups.
  - The handover is only sent when the owner picks someone other than Stoat's choice.
  - The dialog follows the group live (a departed pick falls back; everyone leaving turns it into "Delete group").
  - No silent checkbox for the only member.
  - A new `ChatStore::set_group_owner` applies the PATCH response and ownership messages.
  - Make owner is a primary button with no success toast; its icon is `Crown`, newly bundled.
  - Handing over has its own Missing Permission text.
  - A member the store doesn't know is listed in the New owner picker by ID.
- Chunk 5:
  - `delete_channel`, not `leave_channel(id, false)`, as the web client sends.
  - "Close conversation…" is red, with `IconName::Close`.
  - No toast on success; refusals say "Couldn't close the conversation. Try again."
  - `dm_closed` always runs `remember_places` and `ensure_current_loaded`.
  - The test sits in `tests/store_live.rs` and also checks history and `dm_with`.
- Chunk 6:
  - Mutuals are fetched every time a card opens (as profiles are), the last answer showing meanwhile; rate-limit refusals keep it.
  - Clicking a row closes the card before the list opens; choosing a friend closes the list before their card opens.
  - The rows sit after Roles, before Member since.
  - The `Mutuals` tests are in `tests/profile.rs`; the API test also checks `MissingPermission`.
  - `shows_mutuals` also skips unknown users.
  - Lists keep Stoat's order.
  - Card icons have their names as tooltips; the list dialog has no Close button of its own.
  - A server without an icon shows one initial, not the rail's two.

### Definition of Done
- [ ] `ChannelGroupJoin` and `ChannelGroupLeave` update members live; being removed drops the group; owners update from transfers and from `channel_ownership_changed`. (Manual pass pending. The parse, adding and removing recipients, a removed group leaving Home, live-only ownership messages and `set_group_owner` are test-verified.)
- [ ] Groups can be created from the Friends page, the Home "+", and a friend's menu, with a searchable friend picker capped at 49, and open when created. (Manual pass pending. The request body, a group of one, filtering, picks and the limit, the error texts and "Create group with…" for friends only are test-verified.)
- [ ] "Add friends to group" adds people one by one with named errors; "Remove from group…" is owner-only and confirmed. (Manual pass pending. The requests, `add_limit`, the named error texts and the menu item's visibility are test-verified.)
- [ ] Groups get invites from their menu and header; DMs don't. (Manual pass pending. `can_invite` and `can_add_to_group` for groups, DMs and Saved Notes are test-verified.)
- [ ] "Leave group…" has the silent checkbox; owners pick the next owner (default Stoat's choice); the last member is told leaving deletes it. (Manual pass pending. Both `leave_silently` values, `default_next_owner` and the leaving error texts are test-verified.)
- [ ] "Make group owner…" transfers ownership after confirmation. (Manual pass pending. The PATCH body, the menu item's visibility before and after a handover, and `set_group_owner` are test-verified.)
- [ ] "Close conversation…" closes a DM; messaging the person reopens it. (Manual pass pending. `deactivate_dm`, `activate_dm` bringing it back with its history, and `dm_with` finding the closed DM are test-verified.)
- [ ] Profile cards show mutual friends and mutual servers and groups (up to 8, then "+N"), with full lists. (Manual pass pending. The request, `mutuals`, `overflow_text` and `shows_mutuals` are test-verified.)
- [x] `cargo test` (1000 passed) and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/groups-and-dms-implementation.md`. (Manual pass pending. The steps are in each chunk's section and consolidated below.)

### Remaining Manual Steps
One sitting, in this order. You need three accounts on `api.stoat.chat`, all friends with each other: **A** (you, in RuSToat), **B** (in the web client, in a browser) and **C** (signed in to RuSToat as a second account). A, B and C should share a server (create one and invite them if needed). Sign in to the web client as A too, in a private window, for step 8.

**Create groups (chunk 2)**
1. As A, on the Friends page click "New group": "Create a new group" opens with "Group name" (0/32) focused, "Select members to add", and the picker. Search narrows it by display name and by username; picks stay picked while hidden; clicking rows doesn't take the focus from the search field. Create is disabled with a blank name or one over 32 characters.
2. Name it "M20 One", pick B and C, and press Enter in the name: it opens, appears in Home, and B's web client shows it.
3. Click the "+" at the right of "Direct Messages" (tooltip "Create a group"); it doesn't move when Home's list scrolls. Create "Solo" with nobody picked: a group of one opens.
4. Right-click B on the Friends page: "Create group with…" opens the dialog with B picked ("1 selected"). Close it. Open B's card, its "…", and "Create group with…" again: B is picked. Create "M20 Two" with just B.

**Follow membership live (chunk 1)**
5. As B in the web client, create "B's Group" with A and C (in that order): it appears in A's Home.
6. Open B's Group in RuSToat with the member list showing. As B, add a friend of B's whom A doesn't know: they appear in the member list. As B, remove them: they disappear.
7. As B, remove A from B's Group while it's open in RuSToat: it disappears from Home and Home's first conversation opens. As B, add A back: it reappears.
8. As A in the private web client window, leave B's Group: it disappears from RuSToat too. As B, add A back again.
9. As B, leave B's Group (B owns it): the "gave ownership to" message names the first other member in the group's order (C, since A was added back last). Switch RuSToat to C: right-click A in B's Group's member list, and "Remove from group…" and "Make group owner…" are offered. As C, make A the owner (chunk 4's confirmation), and switch back to A.

**Add, remove and invite (chunk 3)**
10. Open M20 Two: the header shows "Add friends to group" (person with a plus) and Create invite (a link). The picker leaves out B. Add C and another friend: both appear in the member list at once, and the group shows the "added" messages.
11. Make one add fail (for example, have a friend unfriend A while the dialog is open): the error names them, anyone added before stays added and leaves the list, the rest stay picked.
12. In M20 Two, right-click C in the member list and on one of C's messages: "Remove from group…" asks "Remove C?" with "Are you sure you want to remove C from M20 Two?". While Stoat answers, both buttons are disabled and Escape does nothing. Remove: C disappears, and B's web client shows the removal. It isn't offered on A, in a group A doesn't own, or for C in another channel.
13. Create an invite for M20 Two from its header, and again from its right-click menu: the title is "Invite people to M20 Two" with no "To #…" line. As C (switch accounts in RuSToat), join with the link: M20 Two appears for C.
14. A DM and Saved Notes offer neither header button nor Create invite….

**Leave and hand over (chunk 4)**
15. As A, right-click B in M20 Two's member list (and on one of B's messages): "Make group owner…" asks "Make B the owner?" with "You'll stay in the group, but only B will be able to remove members or hand ownership on." Make owner: the "gave ownership" message shows, and A is no longer offered "Remove from group…" or "Make group owner…". Neither shows for a plain member or on yourself.
16. As A (now a plain member), right-click M20 Two in Home: "Leave group…" (red) asks "Leave M20 Two?" with "You won't be able to rejoin unless you are re-invited." and "Don't notify others that you've left". Tick it and Leave: it leaves Home, the toast says "Left M20 Two.", and B's web client shows no "left" message.
17. Leave B's Group (A owns it now, with C): "New owner" shows C. Untick the box and Leave: C becomes owner, and the web client shows the "left" message.
18. Open "Leave group…" on M20 One (A owns it, with B and C): "New owner" shows B (the first other member). Pick C and leave: C becomes owner (the "gave ownership" message names C).
19. Open "Leave group…" on Solo: "You're the only member, so leaving deletes the group.", no checkbox, and the button says "Delete group". Delete it: it's gone.

**Close a DM (chunk 5)**
20. Right-click A's DM with B in Home: "Close conversation…" (red) asks "Close conversation with B?" with "You can re-open it later, but it will disappear on both sides." Cancel and Escape keep it. Groups and Saved Notes don't offer it.
21. Open the DM and close it: it leaves Home here and in B's web client, and Home's first conversation opens. While Stoat answers, both buttons are disabled and Escape does nothing.
22. Open B's card and Message: the DM comes back with its history. Close it again and use the Friends page's Message: it comes back again.

**Mutuals (chunk 6)**
23. Create a group with B and C again, and make sure A and B share a server. Open B's card: once it has loaded, "MUTUAL FRIENDS" (C) and "MUTUAL SERVERS AND GROUPS" (the server, then the group) show after Roles; hovering an icon names it. With more than 8 in a row, 8 show and then "+N".
24. Hover a row: it highlights. Click it: the card closes and the list opens, titled "Mutual friends" or "Mutual servers and groups", scrolling if long. Click C: the list closes and C's card opens where the mouse was. In the other list, click the server: it opens; click the group: it opens in Home.
25. A's own card, a bot's card and a blocked user's card show neither row.
26. Open a card near the bottom of the window: when the rows arrive, the card grows and stays inside it. Leave a shared server and reopen the card: the server is gone from the row.

**Two accounts and themes (plan Manual Tests 2 and 3)**
27. With A and C both signed in, switch between them: New group, the Home "+", Create group with…, Add friends to group, Remove, Make owner, Leave, Close conversation, Create invite and the mutuals lists each act for the account on screen.
28. In light and dark themes: the friend picker's rows, checkboxes and hover, Create a new group, Add friends to group, the Remove, Make owner, Leave (with its New owner dropdown and checkbox) and Close conversation dialogs, the red menu items, the card's mutual rows and their hover, and the mutuals list are readable.
