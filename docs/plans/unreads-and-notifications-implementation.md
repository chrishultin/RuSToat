# Implementation Record: Unreads and notifications (RuSToat)

_Plan: docs/plans/unreads-and-notifications.md_
_Started: 2026-11-04_

_Pre-existing uncommitted files at start: `docs/plans/unreads-and-notifications.md` (this plan, new) and `docs/plans/rich-messages-and-reactions.md` (marked paused after its chunk 3). Both belong to this work and are committed with chunk 1._

## Chunk 1: Unread indicators

### Changes Made
- `src/api/events.rs`:
  - `"channel_unreads"` is added to `READY_FIELDS`.
  - `Ready.unreads: Vec<ChannelUnread>` (the `revolt-models` type) is parsed leniently.
  - New `ServerEvent::ChannelAck { channel, user, message_id }`, decoded from the wire's `{ id, user, message_id }` and forwarded.
  - The `Ready` log line includes the unread-entry count.
  - New unit test `channel_ack_decodes`, and the `events_url` test is updated.
- `src/store.rs`:
  - `ReadState { last_id, mentions: BTreeSet }` per channel, from `Ready.unreads` (only entries whose `user` is us).
  - `is_unread(channel)`: the channel's `last_message_id` is newer than `last_id`, or there's no position but there are messages (as in the web client). Saved Notes, closed DMs and channels without messages are never unread.
  - `mention_count(channel)`.
  - `is_mentioned(&Message)` is true for any of:
    - our ID in `mentions`
    - `role_mentions` intersecting our member roles in that server
    - `@everyone`
    - `@online` while we're online

    Our own messages never count.
  - `has_flag(flags, MessageFlags::X)` is `flags & (1 << flag) != 0`.
  - `apply_message`: our own message acks the channel locally; a message mentioning us adds to that channel's mentions. `last_message_id` was already bumped.
  - `apply_ack(channel, id)` only moves forward and drops mentions at or before `id`.
  - New fields `ChannelEntry.{unread, mentions}` and `ServerEntry.{unread, mentions}` (aggregated over the server's visible text channels), plus `home_summary()` for DMs and groups.
- `src/ui/chat.rs`:
  - A `ChannelAck` from our own user applies the ack, so reading in another client clears it here.
  - **Channel rows:** unread rows are semibold in full-strength sidebar text; rows with mentions get a red `mention_badge` pill ("99+" past 99) on the right.
  - **Rail:** a short left pill for unread servers (and Home), keeping the tall pill for the selected one; a red mention badge with a ring at the icon's bottom-right.
  - `rail_button` takes `unread` and `mentions`.
- Tests:
  - `tests/store_unreads.rs` (new), 8 tests:
    - positions from `Ready`, including another user's entry ignored, a missing entry, no messages, Saved Notes and a closed DM
    - a live message makes a channel unread
    - direct mention
    - role mention only with our role
    - @everyone and @online as bit positions (online vs offline), with suppress not counting as a mention
    - own messages mark read and never self-mention
    - acks only move forward
    - server and Home roll-ups
  - `tests/store_ready.rs`: `ServerEntry` expectations gain `unread`/`mentions`.
- `docs/plans/unreads-and-notifications.md`: Assumption 2 is corrected, and the muted-indicator test moves to chunk 3.

### Divergences
- **Correction: `MessageFlags` are bit positions.** The plan assumed masks. The backend's `MessageFlagsValue::has_value(bit)` uses `mask = 1 << bit`, so @everyone is mask 4 and @online is mask 8. That's implemented, tested and fixed in the plan. The check is a one-line shift, written here rather than depending on the AGPL `revolt-database`.
- **Minor, moved to chunk 3: muted channels' indicators.** Hiding dots for muted channels needs the notification settings, which chunk 3 parses. The plan is amended. Until then, muted channels show dots like any other.
- **Minor addition: sending a message acks its channel locally.** Everything before your own message is implicitly read. It's not in the plan, but without it the channel would light up with your own message.
- **Minor: home summary rule.** Home's indicator comes from `home_channels()`, which includes groups but not Saved Notes, and Saved Notes is never unread anyway.

### Verification Performed
- `cargo test`: 120 passed, 0 failed, 5 ignored:
  - 8 new `store_unreads` tests
  - 1 new event unit test
  - everything else unchanged
- `cargo clippy --all-targets -- -D warnings`: clean, after replacing `iter().any(== me)` with `contains(&me)`. `cargo fmt --check`: clean.
- Live: `events: Ready (5 servers, 75 channels, 29 users, 41 emoji, 73 unreads, 0 skipped)`. There are read positions for 73 of 75 channels, so Assumption 1 (missing entry counts as unread) affects at most 2 channels. The app is running with no warnings or errors.

### Manual Validation
The user compared the running app with the web app and confirmed "Matches": unread indicators and mention counts agree.

## Chunk 2: Mark as read

_Built in a git worktree (`.claude/worktrees/mark-as-read`, branch `feature/mark-as-read`, cut from `main` at `efda345`). The user had a second session ("Fix Peter's advanced formatting rendering") working in the main checkout at the same time, and chose to isolate this chunk rather than share the folder and git index. `.claude/worktrees/` is listed in `.git/info/exclude`, which isn't committed. The build cache was APFS-cloned (`cp -Rc`) from the main checkout, so no full rebuild was needed._

### Changes Made
- `src/api/client.rs`: `ack_message(channel, message)` is `PUT /channels/{channel}/ack/{message}`, and `ack_server(server)` is `PUT /servers/{server}/ack`. Both use `send_raw`, so a 204 with no body is success.
- `src/store.rs`:
  - `unacked_latest(channel)`: the channel's newest message ID if anything in it is unread or has an unread mention, otherwise `None`.
  - `apply_server_ack(server)` applies `apply_ack` to each of the server's channels at its newest message.
- `src/ui/chat.rs`:
  - **Auto ack:** `maybe_ack_current(window, cx)` runs only when:
    - `Ready` has arrived
    - `window.is_window_active()`
    - the open channel's pane isn't `is_scrolled_up()`. That covers following the tail, being scrolled to the end by hand, and content too short to scroll. The first version used `is_following_tail()`, which can stay false after the user scrolls back down manually, so it was changed before review.
    - `unacked_latest` returns something
  - **Debounce and dedupe:** it keeps one `pending_ack: (channel, message, Task)`. A different channel or message replaces it, which cancels the old task. A pending ack for the same channel and message is a no-op. After `ACK_DELAY` (1 s) the task clears the indicator locally, sends the request (logging failures), and clears `pending_ack`.
  - **When it runs:** after every render (deferred alongside `maybe_load_older`), and from `observe_window_activation`, so coming back to the window marks the visible channel read. `ChatView::new` now takes `window` for that subscription; `src/ui/app_root.rs` passes it.
  - **Mark as read:** right-click a server in the rail and choose "Mark as read", via gpui-component's `ContextMenuExt::context_menu` and `PopupMenuItem::new(…).on_click(…)`. It calls `mark_server_read`, which applies `apply_server_ack` locally, then `ack_server` in a detached task (failures logged). `rail_button` takes `Option<MarkRead>`; Home passes `None`.
- Tests:
  - `tests/api_unreads.rs` (new), 3 `wiremock` tests: the `ack_message` path and session header with 204; `ack_server` the same; 403 `MissingPermission` mapping to `ApiError::Api`.
  - `tests/store_unreads.rs`: +2 tests, `unacked_latest` (read, unread, no messages, after a live message) and `a_server_ack_clears_every_channel_in_it` (DMs untouched).

### Divergences
- **Process (user-directed): built in a worktree.** It merges back into `main` after review; see the header note.
- **Minor: Home has no "Mark as read".** The plan only specified servers.
- **Minor: the indicator clears when the debounce fires**, just before the request is sent, not after it succeeds. A failed request is logged and the channel stays read locally; it's acked again on the next new message.

### Verification Performed
- `cargo test --test api_unreads --test store_unreads`: 13 passed.
- `cargo test`: 125 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt`: applied.
- Live: launched the worktree build. The session restored and the app connected.

### Manual Validation
The user tested the worktree build against the web app and confirmed "Works": reading here clears unread state in the web app.
