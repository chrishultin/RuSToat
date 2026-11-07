# Plan: Status and notifications (RuSToat milestone 14)

## Summary
Milestone 14 of the [roadmap](roadmap.md) adds three things Stoat's web client (`stoatchat/for-web`, read at `56bc94cd`, 2026-11-05) has:
- **Status:** set your presence (Online, Idle, Focus, Do Not Disturb, Invisible) and a custom status, from your avatar.
- **Notification settings:** set each server's and channel's level (All, Mentions, None) and mute them, for a while or until you unmute, from their right-click menus. These write Stoat's synced `notifications` setting, so other clients follow.
- **Sounds:** a sound when a message notifies, with an on/off switch.

## Approach
**What exists already.**
- **Notifications are read but never written.**
  - `src/notify.rs` parses the synced setting into `NotificationSettings`: four private maps for levels and mutes. It also has `level()` and `is_muted()`.
  - `Ready` (`user_settings[notifications]`) and `UserSettingsUpdate` events fill `ChatStore.notifications` (`src/api/events.rs` `notifications_from_settings`, which drops the setting's timestamp).
  - Mutes already hide unread dots and Dock badges (`store.rs` ~2998, ~3151, `attention_for` ~1489). Nothing shows that something is muted, and there's no serializer or write call.
- **Presence is read but never set.**
  - `ChatStore::my_presence()` already feeds `notify::should_notify`: Busy notifies nothing, and Focus only mentions.
  - The user panel at the bottom of the channel list (`chat.rs` `render_user_panel` ~4880) draws your avatar with no click handler. Its status line shows connection state (`status_line` ~4602).
  - `StoatClient` has no `PATCH /users/@me`.
- **Menus.**
  - Server icons use `rail_button` (`chat.rs` ~7983), which takes a flat `Vec<(&str, MenuAction)>`. Items today: "Mark as read" and "Copy server ID".
  - Channel, DM and group rows (`render_channel_row` ~4831) only have "Copy channel ID".
  - gpui-component 0.9's `PopupMenu` has submenus (`PopupMenu::submenu`) and checked items (`PopupMenuItem::checked`). It has no radio items, so a radio group is checked items.
- **Settings window** (milestone 13): `SettingsView::app_pages()` in `src/ui/settings.rs` lists the sections. `app_page` has the switch pattern to copy (`SettingField::switch` over `AppPrefs`). App preferences live in `Prefs` (`src/prefs.rs`, `prefs.json`).
- **No audio.** Banners are silent: gpui's `show_system_notification` sets no sound. There's no audio crate, but `objc2-app-kit` is already a macOS dependency.

**Status follows for-web.**
- `PATCH /users/@me` sends only what changed:
  - `{ "status": { "presence": "Busy" } }` for a presence
  - `{ "status": { "text": "…" } }` for a custom status
  - `{ "remove": ["StatusText"] }` to clear it
- The backend merges `status`, so setting a presence keeps the text.
- Text is at most 128 characters (backend validation).
- The live `UserUpdate` echo updates the store. RuSToat already applies `UserUpdate`, including `clear`.
- There's no automatic Idle: for-web has none.

**Notification settings are written whole, with a timestamp.** for-web's `Sync.ts` sends `POST /sync/settings/set?timestamp=<ms>` with body `{"notifications": "<JSON string of the whole object>"}`. The value is a JSON-encoded string.
- The object is `{ server, channel, server_mutes, channel_mutes }`:
  - levels are `"all"`, `"mention"` and `"none"`
  - mutes are `{ "until": ms }`, or `{}` for until unmuted
- RuSToat keeps the parsed object as raw JSON beside its typed view, and edits that. Fields it doesn't know about survive a write.
- Like for-web's `clean()`, a write moves a legacy `"muted"` level into the mutes map and drops expired mutes.
- **Changes apply locally at once**, then are sent.
- **Timestamps decide between copies.** The store remembers the timestamp of the copy it holds. A `UserSettingsUpdate` with an older timestamp is ignored, so the echo of an earlier write can't undo a later one.
- **A failed write** puts back the last copy the server confirmed, and a toast says it couldn't be saved.
- **Rejected alternative: a per-key merge** (send only the changed server or channel). Stoat's settings store has no merge: a key's value is replaced whole. for-web sends the whole object too.

**Menus follow for-web.**
- **Server menu:**
  - "Mark as read"
  - "Mute Server", or "Unmute Server" with a "Muted until …" line
  - a "Notifications" submenu: All Messages, Mentions Only, None. The effective level is checked.
  - "Copy server ID"
- **Channel menu** (the same row function draws channels, DMs and groups):
  - "Mute Channel" or "Unmute Channel"
  - a "Notifications" submenu led by "Server Default (Mentions Only)", or "Default (All Messages)" in DMs and groups, which clears the override
  - "Copy channel ID"
- **Mute submenu** (both menus): "For 15 minutes", "For 1 hour", "For 3 hours", "For 8 hours", "For 24 hours" and "Until I turn it back on".
- **What a mute looks like:**
  - Muted channels, DMs and groups draw dimmed: muted text, and the icon or avatar at 0.3 opacity.
  - A muted server's icon isn't dimmed, but its tooltip says "Muted" or "Muted until …".
  - A timer redraws when the next mute runs out, as `schedule_slowmode_tick` does for slowmode.

**Sound is a built-in macOS sound.**
- Stoat's own sounds can't be bundled: a maintainer said the brand assets aren't licensed for other clients, and the fallback files carry no license.
- `NSSound::soundNamed("Glass")` plays a system sound with nothing to bundle. It needs the `NSSound` feature of `objc2-app-kit`.
- It plays only for **message** notices that are shown (banner or toast), after every existing rule in `should_notify` has passed. Friend requests don't play it.
- One app-wide switch, on by default, sits in a new "Notifications" settings section, with a "Play" preview.
- Rejected alternatives:
  - **Bundling a CC0 file** needs an audio decoding crate or a file in the bundle.
  - **Attaching a sound to banners** would mean patching gpui, and toasts would stay silent.

## Assumptions
1. **The status menu opens from your avatar in the user panel** (bottom of the channel list). RuSToat has no avatar of your own in the server rail, which is where for-web puts it. The menu has:
   - Online, Idle, Focus and Do Not Disturb, each with its dot
   - Invisible, with a hollow dot
   - "Set status…", or "Edit status…" with the text and "Clear status" when one is set

   The check marks the current presence. The labels and tooltips follow for-web: Busy is "Do Not Disturb" ("You won't get any notifications"), and Focus is "Only mentions notify you".
2. **The custom status editor** is a dialog titled "Set your status". It has one text field (up to 128 characters, with a counter) and Cancel/Save buttons. Saving it empty clears the status, which fixes a for-web bug where it doesn't.
3. **The user panel shows your presence:**
   - a coloured dot on your avatar
   - while connected, the status line shows your custom status, or else the presence label ("Online", "Do Not Disturb", …)
   - while not connected, the connection state ("Connecting…", "Offline · retrying…") wins, as today
4. **Notification menus** are as in Approach (user decision). Channels, DMs and groups share one menu. A channel whose server is muted still offers "Mute Channel"; the server's mute is what silences it.
5. **Default levels** don't change (already in `NotificationSettings::level`): servers default to Mentions, and DMs, groups and Saved Notes to All.
6. **Writes are optimistic.** The menu takes effect at once; a failure reverts it and shows "Couldn't save notification settings." There's no retry queue: the next change, or the next `Ready`, brings the server's copy back in line.
7. **Sounds** (user decision):
   - the macOS "Glass" sound
   - for message notices only (banner or in-app toast), never for friend requests
   - one switch in Settings → Notifications ("Notification sounds", "Play a sound when a message notifies you"), on by default, kept in `prefs.json`, not synced
   - a "Play" button to preview it
   - on other platforms, nothing plays
8. **Unchanged:** Busy and Focus keep their current effect on notifications, and nothing sets Idle automatically.

## Plan

### Chunk 1: Presence and custom status
**Goal:** Clicking your avatar sets your presence or custom status, and the user panel shows them.
**Files likely touched:**
- `src/api/client.rs`: `edit_user(&DataEditUser) -> Result<User, ApiError>`, `PATCH /users/@me` (`revolt_models::v0::DataEditUser`, nulls stripped as in `send_message`). Helpers:
  - `set_presence(Presence)`
  - `set_status_text(Option<&str>)`: `None` or blank sends `remove: ["StatusText"]`
- `src/profile.rs` (or a new `src/status.rs`):
  - `STATUS_TEXT_MAX = 128`
  - `check_status_text(&str) -> StatusCheck` (`Clear`, `TooLong(n)`, `Ok(trimmed)`)
  - `presence_label(Presence)` and its tooltip text
- `src/store.rs`: `my_status() -> (Presence, Option<String>)` beside `my_presence()`. `apply_user_update` already handles `StatusText`/`StatusPresence` clears.
- `src/ui/chat.rs`:
  - `render_user_panel`: the avatar gets a presence dot, a pointer cursor, and a `Popover` (or `DropdownMenu`) with the status menu.
  - The status line shows the custom status or presence label while connected.
  - Picking a presence sends at once; on failure, a toast ("Couldn't change your status.").
  - "Set status…" opens a dialog through `window.open_dialog` (as at ~3116) with an `InputState`, a character count and Save/Cancel.
  - "Clear status" sends the remove.
  - No optimistic update: the `UserUpdate` echo, or the returned `User`, updates the store. Apply the returned `User`'s status at once, so the UI doesn't wait on the socket.
- `src/assets.rs`: any icon the menu needs that isn't bundled (check `IconName` first).

**Depends on:** None.
**Risks:**
- **A `Popover` on the avatar** inside the user panel must not steal clicks from the "Log out" button beside it. If `Popover` sizing is awkward, use `DropdownMenu`, the button-anchored `PopupMenu` used elsewhere in `chat.rs`.
- **Multiple accounts.** Status is per account: each `ChatView` has its own client, so this is natural. Check the switcher shows the new status after a change.
- **`DataEditUser` serialisation.** Some fields serialise as `null` when unset (as `DataMessageSend` did); `PATCH` with `"avatar": null` could clear the avatar. Strip nulls (the existing `without_nulls`) and assert the exact body in tests.

**How to verify:** `cargo test`:
- `tests/api_users.rs` (wiremock, exact `body_json`):
  - presence Busy sends `{"status":{"presence":"Busy"}}`
  - text sends `{"status":{"text":"brb"}}`
  - clearing sends `{"remove":["StatusText"]}`
  - the returned `User` parses
  - a 400 `FailedValidation` is an `Api` error
- unit tests for `check_status_text`: blank, whitespace, 128 and 129 characters (counting characters, not bytes), and trimming
- a store test: a `UserUpdate` frame with `clear: ["StatusText"]` for `me` clears `my_status()` text

**Manual check:**
- Click your avatar: the menu shows the five presences with the current one checked.
- Pick Do Not Disturb: the dot and status line change, and the web client shows Do Not Disturb.
- Set "brb": it shows under your name, here and in the web client.
- Clear it, then set Invisible: other accounts see you offline.
- Set a status in the web client: RuSToat updates live.

**Shippable when:** presence and custom status can be set and cleared from the avatar, the panel shows them, and the tests pass.

### Chunk 2: Write notification settings; server menu
**Goal:** A server's right-click menu mutes it (for a while or until unmuted) and sets its level, synced to Stoat.
**Files likely touched:**
- `src/notify.rs`:
  - `NotificationSettings` keeps `raw: serde_json::Map<String, Value>` (what was parsed) beside the typed maps.
  - Mutators that change both:
    - `set_server_level(id, Level)`
    - `set_channel_level(id, Option<Level>)`: `None` removes the override
    - `mute_server(id, until: Option<i64>)` and `mute_channel(…)`
    - `unmute_server(id)` and `unmute_channel(id)`
  - Readers:
    - `server_level(id) -> Level`: effective, default Mention
    - `channel_override(id) -> Option<Level>`
    - `server_mute(id) -> Option<Option<i64>>`: `Some(None)` is until unmuted
    - `channel_mute(id)`
    - `next_mute_expiry(now_ms) -> Option<i64>`
  - `to_json(now_ms) -> String`:
    - writes `raw` with the four keys replaced
    - moves legacy `"muted"` levels into mutes
    - leaves out expired mutes
    - keeps unknown keys
  - `MuteFor` (15 min, 1 h, 3 h, 8 h, 24 h, Forever), with `until(now_ms)` and `label()`.
- `src/api/events.rs`: keep the setting's timestamp. `Ready.notifications` and `ServerEvent::UserSettingsUpdate` carry `(i64, NotificationSettings)`.
- `src/api/client.rs`: `set_settings(&[(&str, String)], timestamp_ms)` sends `POST /sync/settings/set?timestamp=…` with `{key: value}`. Values are already JSON strings.
- `src/store.rs`:
  - `notifications_updated: i64`
  - `set_notification_settings` takes the timestamp, and ignores a copy older than the one held
  - `server_mute_until(server) -> Option<Option<i64>>`, for the tooltip
- `src/ui/chat.rs`:
  - `update_notifications(cx, |settings| …)`:
    1. copies the current settings (to revert to)
    2. applies the change, and sets `notifications_updated = now`
    3. resyncs the panes and the channel list
    4. sends `to_json` with that timestamp
    5. on failure, restores the copy (only if nothing newer has landed) and shows the toast
  - Overlapping writes: tag each with a sequence number, so only the latest failure reverts.
  - `rail_button`'s menu becomes a builder closure (`Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>`), so it can have submenus. Home passes none.
  - The server menu: Mark as read; Mute Server (submenu) or Unmute Server ("Muted until …" as a disabled label); Notifications (submenu, checked items); separator; Copy server ID.
  - The server icon's tooltip adds "Muted" or "Muted until {time}".

**Depends on:** None.
**Risks:**
- **Echo ordering.** Stoat sends `UserSettingsUpdate` to every session, including the writer's. Without the timestamp check, two quick changes can flicker back. The timestamp test below must cover an older echo arriving after a newer local change.
- **Clock skew.** The backend clamps future timestamps to its own now. If this Mac's clock runs ahead, the server stores an earlier timestamp than RuSToat holds, and an echo carrying it would look older and be ignored. That's harmless, since the content is the same. But a real later change from another client, stamped with the server's clamped time, could also be ignored. Mitigation: take `notifications_updated` from the echo whose content matches the last write. If skew still bites, compare content rather than time for echoes. Note what happens in the implementation record.
- **`rail_button` is shared** (Home, servers, maybe the account switcher). Changing its signature touches every caller; keep the change mechanical.
- **Submenus and checked items** in gpui-component 0.9 haven't been used in this app yet. Check keyboard and mouse handling early, and fall back to flat items with "✓" labels if submenus misbehave.

**How to verify:** `cargo test`:
- `src/notify.rs` unit tests:
  - each mutator, then `level`/`is_muted`
  - `to_json` round-trips through `parse`
  - unknown top-level keys and unknown per-entry values survive
  - legacy `"muted"` is written as `{}` in the mutes map
  - expired mutes are dropped
  - `MuteFor::until`, and `next_mute_expiry`
- `tests/api_settings.rs` (new, wiremock):
  - `POST /sync/settings/set`, with query `timestamp=…`, header `x-session-token` and body `{"notifications":"<string>"}`, where the value is a string (assert it parses back as the object)
  - a 401 is `Unauthorized`
- `src/api/events.rs` unit test: `UserSettingsUpdate` keeps the timestamp.
- `tests/store_unreads.rs` (or a new `tests/store_notifications.rs`):
  - an older settings copy is ignored and a newer one applies
  - muting a server hides its unread dot (as today) and gives `server_mute_until`

**Manual check:**
- Right-click a server and pick Mute Server → For 15 minutes. Its unread dot goes, the tooltip says "Muted until …", and the web client shows it muted.
- Unmute it. Set Notifications → All Messages: the check moves, and a message there notifies.
- Change a level in the web client: RuSToat's menu shows it.
- Mute two servers quickly: neither flickers back.

**Shippable when:** servers can be muted, unmuted and given a level from their menu, synced both ways, and the tests pass.

### Chunk 3: Channel, DM and group menus; muted look
**Goal:** Channels, DMs and groups can be muted and given a level from their right-click menus, and muted ones look muted until the mute runs out.
**Files likely touched:**
- `src/store.rs`:
  - `ChannelEntry` gains `muted: bool` (the channel's own mute, or its server's)
  - `channel_mute_until(channel)`, for the menu line
  - `level_source(channel)`, the inherited level, for "Server Default (Mentions Only)"
- `src/ui/chat.rs`:
  - `render_channel_row`'s menu:
    - "Mute Channel" (the same duration submenu), or "Unmute Channel" with "Muted until …"
    - "Notifications": "Server Default (…)", or "Default (…)" in DMs and groups, then All Messages, Mentions Only and None
    - separator, then Copy channel ID
  - Muted rows draw muted: `colors.muted` text, and the icon or avatar at `opacity(0.3)`.
  - `schedule_mute_tick`: a one-shot timer to `next_mute_expiry` that resyncs the list and re-arms. It's armed after `Ready`, after settings changes and after updates.
- `tests/store_*`: entry `muted` cases.

**Depends on:** Chunk 2.
**Risks:**
- **`ChannelEntry` equality drives list diffing.** Adding `muted` must change the entry when a mute starts or ends, or rows won't redraw. Check the list is rebuilt on settings changes, not only on unread changes.
- **The timer and long mutes.** `Timer` with a 24-hour duration is fine; re-arm on every change, and drop it when there are no timed mutes.
- **Saved Notes** has no server and defaults to All. Muting it is allowed (for-web allows it), but check that the "Default (All Messages)" label is right.

**How to verify:** `cargo test`:
- store tests:
  - a muted channel's entry is `muted` and not `unread`
  - a channel in a muted server is `muted`
  - a mute that has expired isn't muted (fixed clock)
  - `level_source` gives the server's level, or All for a DM
  - setting a channel's level to `None` returns it to the server default
- `notify` tests: a channel override wins over the server; `None` falls back.

**Manual check:**
- Right-click a channel and mute it for 15 minutes: it dims at once and undims by itself when the mute ends, with no restart.
- Set Notifications → None, then Server Default: the subtitle shows the server's level.
- Mute a DM: it dims, and its messages stop notifying.
- Do it from the web client: RuSToat follows live.

**Shippable when:** channels, DMs and groups can be muted and levelled from their menus, muted rows dim and undim on time, and the tests pass.

### Chunk 4: Notification sounds
**Goal:** A message that notifies plays a sound, unless the user turns sounds off in Settings.
**Files likely touched:**
- `Cargo.toml`: add `"NSSound"` to `objc2-app-kit`'s features.
- `src/sound.rs` (new):
  - `MESSAGE_SOUND = "Glass"`
  - `play_message_sound()`: `NSSound::soundNamed` then `play`. It's a no-op off macOS, or when the sound is missing (logged once).
  - Pure: `should_play(sounds_on, is_message_notice) -> bool`.
- `src/prefs.rs`: `Prefs.notification_sounds: bool`, serde-defaulted to `true`, with the `Default` entry and file round-trip tests (a missing key reads as `true`).
- `src/ui/settings.rs`: `notifications_page` in `app_pages()` (after Appearance). A "Sounds" group holds:
  - `SettingField::switch` for "Notification sounds" ("Play a sound when a message notifies you")
  - a "Play" button that previews it
- `src/ui/chat.rs`: in the message-notice path (`chat.rs` ~792–801, after `should_notify` returns a notice, for both banner and toast delivery), play the sound if `AppPrefs::get(cx).notification_sounds`. Not in `show_friend_banner` or friend-request toasts.
- `src/assets.rs`: a `Bell` or `Volume2` icon for the settings sidebar, if needed.

**Depends on:** None (independent of chunks 1–3).
**Risks:**
- **Several messages at once** (a reconnect backlog, or a busy channel): each notice would play. Throttle to one sound per 1 s, through a `last_played: Option<Instant>` on the view, or a static in `sound.rs`.
- **`NSSound` must be used on the main thread.** GPUI's view callbacks are on the main thread; don't call it from `spawn_io`.
- **Banners and "Do Not Disturb" in macOS Focus.** The sound plays even if macOS Focus would hide the banner. That's acceptable, since RuSToat's own Busy presence already suppresses notices. Note it.

**How to verify:** `cargo test`:
- `src/prefs.rs` tests: the default is on; a missing key is on; `false` round-trips.
- `sound.rs` unit tests: `should_play`, and the throttle (fixed `Instant`s).
- `tests/shortcuts.rs`, or a settings test if one exists: the Notifications page is listed.
- Build check: `cargo build` on macOS links `NSSound`.

**Manual check:**
- With the window in the background, a DM plays Glass with its banner. With RuSToat in front, a message in another channel plays it with the toast.
- Settings → Notifications → turn sounds off: messages notify silently. "Play" previews the sound.
- Busy: no notice and no sound.
- A burst of 10 messages plays once or twice, not 10 times.

**Shippable when:** message notices play the sound when it's switched on, the switch persists across relaunches, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`. Never run it on a `mod.rs` or `lib.rs`, since it formats their child modules too.
- A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines (from `rustfmt --edition 2024 --check <file>`) hasn't grown. Record each touched file's starting count before chunk 1.

### Unit Tests
- **`notify.rs`:** the mutators, `to_json` round-trip with unknown keys kept, legacy `"muted"` migration, expired mutes dropped, `MuteFor::until`, `next_mute_expiry`.
- **Status:** `check_status_text` (blank, whitespace, 128 and 129 characters, multi-byte characters).
- **`events.rs`:** `UserSettingsUpdate` keeps its timestamp.
- **`prefs.rs`:** `notification_sounds` default and round-trip.
- **`sound.rs`:** `should_play`, and the throttle.

### Integration Tests
- **`tests/api_users.rs`:** `PATCH /users/@me` bodies for presence, text and clearing; validation errors.
- **`tests/api_settings.rs`** (new): `POST /sync/settings/set` query, header and string-valued body; a 401.
- **Store tests:** settings timestamps (an older copy is ignored); `ChannelEntry.muted` for channel, server and expired mutes; `level_source`; `my_status` after `UserUpdate` with `clear`.

### Manual Tests
You need an `api.stoat.chat` account, the web client open on the same account, and a second account to send messages.
1. Run every chunk's manual check.
2. With two accounts in RuSToat: status, mutes and levels set on one never show on the other.
3. Change a mute in the web client while RuSToat is open, and the reverse: each follows within a second or two, with no flicker.
4. Relaunch: presence, status, mutes and the sound switch are all as left.
5. Light and dark themes: the presence dots, the dimmed muted rows, the menu check marks and the status dialog are readable.

## Definition of Done
- [ ] Clicking your avatar opens a menu to set Online, Idle, Focus, Do Not Disturb or Invisible, and to set, edit or clear a custom status of up to 128 characters. The requests are `PATCH /users/@me` with only the changed fields, and the user panel shows the presence dot and status.
- [ ] Servers' right-click menus mute (15 min, 1 h, 3 h, 8 h, 24 h, or until unmuted), unmute, and set All, Mentions Only or None. Channels', DMs' and groups' menus do the same, plus Server Default or Default.
- [ ] Changes write the whole `notifications` setting with `POST /sync/settings/set?timestamp=…`, keep unknown fields, apply at once, revert with a toast on failure, and follow other clients live without older echoes undoing newer changes.
- [ ] Muted channels, DMs and groups draw dimmed, muted servers say so in their tooltip, and both return to normal when a timed mute ends, without a relaunch.
- [ ] A message notice (banner or toast) plays the macOS "Glass" sound, at most once a second. Friend requests don't. A Settings → Notifications switch, on by default and kept in `prefs.json`, turns it off, and "Play" previews it.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/status-and-notifications-implementation.md`.
