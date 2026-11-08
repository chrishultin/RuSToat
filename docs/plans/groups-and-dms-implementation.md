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
