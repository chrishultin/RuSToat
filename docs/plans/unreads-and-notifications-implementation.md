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

## Chunk 3: Notification rules and in-app toasts

_Same worktree and branch as chunk 2 (`feature/mark-as-read`), stacked on `8df2cfe`._

### Changes Made
- `src/notify.rs` (new, pure):
  - `Level` is `All`, `Mention` or `None`.
  - `NotificationSettings::parse(json)` reads the `notifications` user setting the way the web client's `clean()` does:
    - `server` and `channel` levels; unknown levels are ignored
    - the legacy level `"muted"` becomes an open-ended mute
    - `server_mutes` and `channel_mutes` as `{until?: ms}`; non-objects are ignored
    - garbage parses as defaults
  - `level(channel, server)`: the channel's level, else the server's level (default `Mention`), else `All` (DMs, groups, Saved Notes).
  - `is_muted(channel, server, now)`: a server or channel mute with no `until`, or an `until` in the future.
  - `should_notify(store, message, Viewing { channel, window_active }, now) -> Option<Notice { tag, channel, title, body }>`. It skips, in the web client's order:
    1. the channel is being viewed in the active window
    2. own messages
    3. blocked authors
    4. `SuppressNotifications` (mask 2)
    5. muted
    6. level `None`, or `Mention` when not mentioned
    7. presence `Busy`, or `Focus` when not mentioned
    8. Saved Notes
  - Titles: `@author` for DMs; `@author - Group` for groups (just the group name for system messages); `@author (#channel, Server)` in servers. The body is `markdown::plain_text`, or the system-message text, cut to 200 characters with "…".
  - 3 unit tests: truncation, settings parsing, garbage settings.
- `src/api/events.rs`:
  - `"user_settings[notifications]"` is added to `READY_FIELDS`. `Ready.notifications: Option<NotificationSettings>` is parsed from the same `[ts, json]` map as `ordering`.
  - New `ServerEvent::UserSettingsUpdate { notifications: Option<…> }`, decoded from `{ id, update }` and forwarded. It's `None` when the update doesn't touch `notifications`.
  - New unit test `settings_updates_carry_notification_settings`.
- `src/markdown.rs`: `pub fn plain_text()` (mentions as `@Name`, custom emoji as `:name:`).
- `src/store.rs`:
  - holds `NotificationSettings`: from `Ready`, with `set_notification_settings` and `notification_settings()`
  - `my_presence()` (from our user's `status.presence` in `Ready`)
  - `is_muted(channel)`, at the current time
  - **Moved from chunk 1:** `ChannelEntry.unread` and the server roll-up skip muted channels (no dot), while mention counts still show.
- `src/ui/chat.rs`:
  - The events loop runs via `cx.spawn_in(window, …)`, and `on_connection_event` gets the window.
  - On each live `Message`, `should_notify` runs before `apply_message`, with `Viewing` set to the open channel and `window.is_window_active()`.
  - `show_toast()` calls `window.push_notification(Notification::new().id1::<ChatView>(tag).title(…).message(…).on_click(open))`. Toasts are deduplicated per channel tag, and clicking one runs `open_channel()`, which switches to the server (or Home) and selects the channel.
  - `UserSettingsUpdate` with `notifications` replaces the settings live.
  - gpui-component's toast layer is mounted automatically in every `gpui_kit::open_window` root (`WindowStateLayers`), so no extra layer code was needed.
- `tests/notify_rules.rs` (new), 12 tests, one per rule:
  - a DM notifies by default, with the web title
  - viewing skips only while the window is active
  - own and blocked authors
  - suppressed messages
  - servers default to mentions only, with the server title
  - level resolution: channel over server, DM turned down
  - channel, server, expired and legacy mutes
  - Busy and Focus
  - Saved Notes
  - group titles and system messages
  - plain, truncated bodies
  - muted channels hide their dot but keep mentions

### Divergences
- **Minor: presence comes only from `Ready`.** Changing status mid-session takes effect after a reconnect, because `UserUpdate` events aren't handled.
- **Minor:** only the `notifications` key of `UserSettingsUpdate` is used. A server-order change in the web app still needs a reconnect.
- **Implemented here, moved from chunk 1:** muted-indicator behaviour (Assumption 3).
- My first version of `muted_channels_hide_their_dot_but_keep_mentions` forgot that the fixture has no read positions, so `#general` was unread and kept Alpha's dot lit. The test was fixed to read `#general` first; the code was right.

### Verification Performed
- `cargo test --test notify_rules`: 12 passed.
- `cargo test`: 141 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt`: applied.
- Live: the app connects with `…&ready=user_settings[ordering]&ready=user_settings[notifications]`, and `Ready` parses as before (73 unreads, 0 skipped), with no warnings.

### Manual Validation
The user tested toasts in the running worktree build and confirmed "Works".

## Chunk 4: Dock badge

_Same worktree and branch (`feature/mark-as-read`), stacked on `5f075fb`._

### Changes Made
- `Cargo.toml`: macOS-only target dependencies at the versions GPUI already uses, so nothing new is built:
  - `objc2 = "0.6"`
  - `objc2-foundation` 0.3 with `NSString`
  - `objc2-app-kit` 0.3 with `NSApplication`, `NSDockTile` and `NSResponder`
- `src/dock.rs` (new):
  - `badge_label(count)`: nothing at 0, the number for 1 to 99, "99+" past 99.
  - `set_badge(count)`: `NSApplication::sharedApplication(mtm).dockTile().setBadgeLabel(…)`. It needs a `MainThreadMarker` and logs and skips if it isn't on the main thread. All the AppKit calls used are safe in objc2-app-kit 0.3, so there's no `unsafe`. It's a no-op off macOS.
  - Unit test for the labels.
- `src/store.rs`: `attention_count()` (Assumption 5) is the sum over channels of:
  - 0 if muted
  - otherwise the unread mention count, except that an unread DM or group whose level is `All` counts `max(mentions, 1)`, so it's counted once even with mentions
- `src/ui/chat.rs`: `ChatView.dock_badge` remembers the last count shown. `render` recomputes `attention_count()` and calls `dock::set_badge` only when it changes. Every unread or ack change already re-renders.
- `src/ui/app_root.rs`: `log_out` clears the badge.
- `tests/notify_rules.rs`: +2 tests:
  - `attention_count_is_mentions_plus_unread_conversations`: the base of 3 unread conversations, a server mention adds 1, a plain server message adds 0, a DM with 2 mentions counts 2, and reading it drops it
  - `…_skips_muted_and_mention_only_conversations`

### Divergences
- None in approach. Main-thread safety is by construction: badge updates happen in `render`, which GPUI runs on the main thread. `MainThreadMarker::new()` guards it anyway.
- **Bug found in live testing, fixed: the badge stuck at 1.** The user marked everything read, and the badge still showed 1.
  - **Diagnosis:** I added a debug breakdown (`ChatStore::attention_breakdown()`, logged as `dock badge: N` plus `+n <channel> [id, read state, mentions]` on each change). It showed a single contributor: `+1 DirectMessage <contact> [<closed-DM-id>, behind, 1 mentions]`.
  - **Cause:** a **closed DM** (`active: false`). It isn't listed in Home, so it can't be opened and read, yet it kept an unread mention in Stoat's read state. `attention_for` skipped closed DMs' unread dot but still counted their mentions.
  - **Fixes:**
    - Closed DMs contribute 0.
    - In `apply_ready`, mentions at or before a channel's `last_id` are dropped as already read, a defensive fix for a second possible cause.
    - A debug line names any closed DM whose mentions are being ignored.
  - **Regression tests:** `a_closed_dm_with_an_old_mention_doesnt_pin_the_badge` (it fails without the fix, because the closed DM adds 1) and `mentions_at_or_before_the_read_position_are_already_read`.

### Verification Performed
- `cargo test`: 146 passed, 0 failed, 5 ignored. That's 144 before the stuck-badge fix, plus its 2 regression tests. `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt`: applied.
- Live: relaunched the worktree build (`cargo run`, not bundled). The first run showed the stuck badge described above. On the relaunch with the fix, Ready logged `closed DM <closed-DM-id> has 1 unread mention(s); not counted`, and no `dock badge:` line followed, so the count stayed at 0.

### Manual Validation
- The user saw the Dock badge appear. It first showed the wrong count, because of the stuck-at-1 bug above.
- After the fix, with everything read, the user confirmed the badge is cleared ("lgtm").
