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
