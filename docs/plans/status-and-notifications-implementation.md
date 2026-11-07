# Implementation Record: Status and notifications (milestone 14)

_Plan: docs/plans/status-and-notifications.md_
_Started: 2026-11-06_

## Setup

- Milestone 12 (`milestone-12-search-slowmode-gifs-9cce`) was fast-forwarded into `main` and pushed (`722cfaf..08f49a4`). This work is on `milestone-14-status-and-notifications`, branched from there, with the plan committed first (`03dfdb1`).
- Supervision: continuous. The user asked to batch every chunk's manual check for the end, so each chunk is committed with its manual check pending.
- Files that were already dirty and are kept out of every commit: `.gitignore` (a duplicated Riptide `.humanlayer/tasks/` entry) and `.shadow/` (untracked).
- Baseline: `cargo test` 722 passed, and clippy was clean.
- Baseline rustfmt drift (`Diff in` lines from `rustfmt --edition 2024 --check`): `src/ui/chat.rs` 17, `src/store.rs` 15, `src/api/client.rs` 3, `src/api/events.rs` 3, and 0 in `src/notify.rs`, `src/prefs.rs`, `src/profile.rs`, `src/assets.rs`, `src/ui/settings.rs`, `src/ui/app_prefs.rs`, `tests/api_users.rs`, `tests/store_unreads.rs`, `tests/shortcuts.rs`, `tests/store_live.rs`.

## Chunk 1: Presence and custom status

### Changes Made
- `src/api/client.rs`:
  - `edit_user(&DataEditUser)` sends `PATCH /users/@me`. Nulls are stripped, and so is an empty `remove` array: `DataEditUser.remove` is `serde(default)` and would otherwise always go as `"remove": []`.
  - `set_presence(Presence)`.
  - `set_status_text(Option<&str>)`: trims the text, and `None` or blank text sends `{"remove":["StatusText"]}`.
  - Private `edit_status` builds a status-only `DataEditUser`, because the type has no `Default`.
- `src/status.rs` (new, registered in `src/lib.rs`):
  - `STATUS_TEXT_MAX = 128`.
  - `PRESENCES`, the menu order.
  - `presence_label`: Busy is "Do Not Disturb".
  - `presence_hint`: Focus is "Only mentions notify you", and Busy is "You won't get any notifications".
  - `presence_dot`: Invisible draws the offline dot.
  - `StatusCheck` and `check_status_text`: trims, and counts characters rather than bytes.
  - Unit tests.
- `src/store.rs`:
  - `my_status() -> (Presence, Option<String>)`: Online if unset, and the text is trimmed (`None` if blank).
  - `set_my_status(Option<UserStatus>)` applies Stoat's answer at once.
- `src/ui/chat.rs`:
  - State: `status_input`, its subscription (Enter saves; Change refreshes the counter), and `status_saving`.
  - `open_status_editor`, `save_status_text`, `change_status` (a single request path for presence and text: applies the returned user's status, forgets member lists, and shows a "Couldn't change your status." toast on failure) and `status_menu`.
  - Free functions `StatusChange`, `presence_colour`, `presence_dot`, `render_presence_item` (dot, name and hint) and `render_status_editor` (field, `n/128` counter, Cancel/Save; Save is disabled while too long).
  - `render_user_panel`: the avatar is a ghost `Button` with a presence dot, a "Set status" tooltip and a `dropdown_menu_with_anchor(Anchor::BottomLeft, …)`, so the menu opens upward. The dot is the offline one while not connected.
  - `status_line`: while connected, shows the custom status, else the presence label. Otherwise the connection state, as before.
- `src/assets.rs`: bundles `FaceSlightlySmilingPlus` for "Set status…". Adds a `status_icons_load` test.
- `tests/api_users.rs`: exact bodies for presence, text and clearing (`None` and blank), the returned `User` parsing, and a 400 `FailedValidation` being an `Api` error.
- `tests/store_live.rs`: `my_status` follows `UserUpdate` frames for `me` (including `clear: ["StatusText"]` keeping the presence, and other people's updates not touching it), and `set_my_status`.

### Divergences
- Minor: the menu's "Clear status" uses the bundled `Close` icon, and "Set status…" uses the newly bundled `FaceSlightlySmilingPlus` (the plan named no icons). Lucide's `x` and `smile-plus` aren't in gpui-kit's default set, and `smile-plus` isn't a name gpui-kit knows.
- Minor: `DataEditUser`'s `remove` always serialises, so `edit_user` drops it when empty. That's what keeps the exact bodies the plan asked for.
- Minor: the status menu uses `PopupMenuItem::element` (dot, name and hint) with checks on the right (`check_side(Side::Right)`), rather than tooltips. Menu items have no tooltips, so the hints are drawn under the name.
- The account switcher shows connection state, not presence, so it needed no change.

### Verification Performed
- `cargo test`: 735 passed, 0 failed (13 new). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `client.rs` 3, `store.rs` 15 and `chat.rs` 17 (all unchanged). `status.rs`, `assets.rs`, `tests/api_users.rs` and `tests/store_live.rs` are 0.

### Manual Validation
Pending: batched for the end at the user's request (the chunk 1 manual check in the plan).

## Chunk 2: Write notification settings; server menu

### Changes Made
- `src/notify.rs`:
  - `NotificationSettings` now keeps `raw: serde_json::Map` (the setting as parsed). The typed maps are a view rebuilt from it (`from_raw`) after each change (`edit`), so they can't drift apart.
  - `parse` moves legacy `"muted"` levels into the mutes maps as `{}`; an existing mute with an end wins over a legacy one.
  - Mutators: `set_server_level`, `set_channel_level(Option)`, `mute_server`, `mute_channel`, `unmute_server`, `unmute_channel`.
  - Readers: `server_level`, `channel_override`, `server_mute(id, now)`, `channel_mute(id, now)`, `next_mute_expiry(now)`.
  - `to_json(now)`: writes `raw` with ended mutes left out. Unknown keys and unknown per-entry values (levels like `"loud"`, extra fields in mute objects, non-object mutes) are kept.
  - `Level::as_str` and `Level::label` ("All Messages", "Mentions Only", "None").
  - `MuteFor` (with `ALL`, `until(now)` and `label()`), and `mute_label(until)` ("Muted" or "Muted until 14:05").
  - Unit tests.
- `src/time.rs`: `format_until` ("14:05", "tomorrow at 14:05", "9 Nov at 08:00") and `millis_until_label`, with a test.
- `src/api/events.rs`: `notifications_from_settings` keeps `updated_at`. `Ready.notifications` is `Option<(i64, NotificationSettings)>` and `UserSettingsUpdate.notifications` is `Option<(i64, Box<NotificationSettings>)>`. The event test checks the timestamp.
- `src/api/client.rs`: `set_settings(&[(&str, String)], timestamp_ms)` sends `POST /sync/settings/set?timestamp=…` with `{key: "<json string>"}`.
- `src/store.rs`:
  - `notifications_updated`.
  - `set_notification_settings(updated, settings) -> bool`: ignores an older copy. For clock skew, an older copy with the same content moves the held time back to it.
  - `restore_notification_settings` (forced, for reverting) and `server_mute_until(server)`.
- `src/ui/chat.rs`:
  - `update_notifications(change, window, cx)`:
    1. Saves the current settings and timestamp, so it can revert.
    2. Applies the change with timestamp `max(now, held + 1)`.
    3. Notifies, so the sidebar redraws from the store.
    4. Writes `to_json` with that timestamp.
    5. On failure, reverts, but only if this is the latest write (`notifications_write` sequence) and nothing newer has landed. Then a toast says "Couldn't save notification settings."
  - The `UserSettingsUpdate` handler ignores older copies.
  - `server_menu`: Mark as read; then Mute Server (a submenu of durations), or Unmute Server followed by a disabled "Muted until …" label; then a Notifications submenu (All Messages, Mentions Only, None, with the effective level checked); a separator; and Copy server ID.
  - Helpers shared with chunk 3: `MenuBuilder`, `SetMute`, `SetLevel`, `notifications_item`, `mute_items` and `level_items`.
  - `rail_button` takes `Option<MenuBuilder>` instead of a flat list. The server icon's tooltip is "Name · Muted until …" (or "· Muted") while muted.
- `src/ui/switcher.rs`: the account button's "Log Out" menu is built with a `MenuBuilder`. `MenuAction` is gone.
- `tests/api_settings.rs` (new): the query, header and string-valued body (parsed back, with unknown keys kept), and a 401 being `Unauthorized`.
- `tests/store_notifications.rs` (new):
  - Ready brings the timestamp.
  - A newer copy applies and an older one is ignored (including an older echo arriving after a newer change).
  - The clock-skew echo moves the time back, so a later change from another client applies.
  - Muting a server hides its unread dot and gives `server_mute_until`, both for a time and until unmuted. An ended mute doesn't count.
- `tests/notify_rules.rs`: fixtures updated for the `(timestamp, settings)` shape.

### Divergences
- Minor: `server_mute` and `channel_mute` take `now_ms` and return only mutes still running (the plan's signature had no time). That's what the menu and tooltip need.
- Minor: `UserSettingsUpdate` boxes the settings (`Box<NotificationSettings>`), because clippy's `large_enum_variant` fired on `ConnectionEvent` once the raw map was added. Other events already box their payloads.
- Minor: `update_notifications` just calls `cx.notify()`. The plan's "resync the panes" isn't needed: the rail and channel list are drawn from the store each frame, and no message pane depends on the settings. The Dock badge follows on the owner's next refresh, as before.
- Clock skew (a plan risk): handled as planned. An echo older than the held copy but with the same content (compared via `to_json`) moves `notifications_updated` back to the echo's time. Writes are stamped `max(now, held + 1)`, so a clock behind Stoat's still produces a newer copy locally. Whether skew bites in practice is for the manual check.
- `MenuAction` was removed, because nothing uses it any more.

### Verification Performed
- `cargo test`: 749 passed, 0 failed (14 new). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `client.rs` 3, `events.rs` 3, `store.rs` 15 and `chat.rs` 17 (all unchanged). `notify.rs`, `time.rs`, `switcher.rs`, `tests/notify_rules.rs`, `tests/api_settings.rs` and `tests/store_notifications.rs` are 0.

### Manual Validation
Pending: batched for the end (the chunk 2 manual check in the plan). It also covers the plan's risk that submenus and checked items haven't been used in this app before, for keyboard and mouse.

## Chunk 3: Channel, DM and group menus; muted look

### Changes Made
- `src/store.rs`:
  - `ChannelEntry.muted`: the channel's own mute, or its server's, right now. It's computed once in `entry_for`, which also hides the unread dot as before.
  - `channel_mute_until(channel)`: the channel's own running mute only. The server's mute isn't included, so a channel in a muted server still offers "Mute Channel".
  - `level_source(channel)`: the server's level, or All for DMs, groups and Saved Notes.
- `src/ui/chat.rs`:
  - `channel_menu(entry) -> MenuBuilder`, the same for channels, DMs, groups and Saved Notes:
    - "Mute Channel" (the duration submenu), or "Unmute Channel" followed by a "Muted until …" label
    - "Notifications": "Server Default (…)" for server channels or "Default (…)" elsewhere (this clears the override), then All Messages, Mentions Only and None, with the current override checked
    - a separator, then Copy channel ID
    - It reuses chunk 2's `mute_items`, `level_items` and `notifications_item`.
  - `render_channel_row`: a muted row's icon or avatar is wrapped at `MUTED_ICON_OPACITY` (0.3). Muted rows are never unread, so their text is already `muted_foreground` unless selected. The right-click menu is `channel_menu`.
  - `mute_tick` and `schedule_mute_tick`: a one-shot timer to 1 ms past `next_mute_expiry`. It notifies (undimming rows and updating server tooltips) and re-arms, and nothing is armed while no timed mute is running. It's armed after `Ready`, after an applied `UserSettingsUpdate`, after `update_notifications`, and after a revert.
- `tests/store_notifications.rs`:
  - a muted channel's entry is `muted` and not `unread`, and unmuting it clears that
  - a channel in a muted server is `muted`, but `channel_mute_until` is `None`
  - ended mutes (server and channel) aren't muted
  - `level_source` covers server channels, DMs, groups and Saved Notes, and follows the server's level
  - clearing a channel's level returns it to the server's

### Divergences
- None of substance. The plan's risk about `ChannelEntry` equality driving list diffing doesn't apply: `sections()` rebuilds the list from the store on every render, so `muted` shows on the next `cx.notify()`. The `notify` test that a channel override wins and `None` falls back is covered by chunk 2's `mutators_change_levels_and_mutes`.
- Saved Notes: its menu says "Default (All Messages)", as the plan expected.

### Verification Performed
- `cargo test`: 754 passed, 0 failed (5 new). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `store.rs` 15 and `chat.rs` 17 (unchanged). `tests/store_notifications.rs` is 0.

### Manual Validation
Pending: batched for the end (the chunk 3 manual check in the plan).
