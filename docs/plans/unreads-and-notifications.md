# Plan: Unreads and notifications (RuSToat)

## Summary
Show which channels, servers and DMs have unread messages and mentions, mark them read as the user reads them, and notify about new messages the way Stoat's web client does. Notifications appear as in-app toasts, a Dock badge, and macOS banners. Without this the client can't be used day to day: nothing tells the user where new activity is.

*Ordering: milestone 2 ("Rich messages and reactions", `docs/plans/rich-messages-and-reactions.md`) is paused after its chunk 3. This plan runs next at the user's request; milestone 2 resumes at its chunk 4 (images) afterwards.*

## Approach
**State comes from Stoat; the rules come from Stoat's own web client.** Unread state is the server's: `Ready.channel_unreads` (requested with `ready=channel_unreads`) gives one `ChannelUnread` per channel:
- `_id`: `{ channel, user }`
- `last_id`: the last message the user read
- `mentions`: IDs of unread messages that mention the user

It's kept live by our own `Message` events and by `ChannelAck { id, user, message_id }`, which arrives when another of the user's sessions reads a channel.

Reading is reported with `PUT /channels/{id}/ack/{message}` and `PUT /servers/{id}/ack` (both return 204). `GET /sync/unreads` returns the same list as `Ready`, and is used only as a fallback.

**Notification rules mirror `stoatchat/for-web`** (read 2026-11-04):
- **Stored settings** (`components/state/stores/NotificationOptions.ts`, synced as the user setting **`notifications`**; request it with `ready=user_settings[notifications]`):
  - shape: `{ server: {id: level}, channel: {id: level}, server_mutes: {id: {until?: ms}}, channel_mutes: {id: {until?: ms}} }`
  - levels are `"all" | "mention" | "none"`
  - the legacy level `"muted"` migrates to a mute
- **Level resolution:** the channel's own level, else the server's level (default **`"mention"`**), else the channel-type default (`DirectMessage`/`Group`/`SavedMessages` = **`"all"`**).
- **Muted:** a channel is muted if it, or its server, has a mute with no `until` or an `until` in the future.
- **Notify decision** (`components/client/NotificationsWorker.tsx`), skipping in this order:
  1. the channel is being viewed and the window is focused
  2. own messages
  3. blocked authors
  4. muted channels or servers
  5. level `"none"`
  6. level `"mention"` and the user isn't mentioned
  7. own presence `Busy`, or `Focus` and not mentioned
  8. `SavedMessages`
- **Titles:**
  - DM: `@user`
  - group: `@user - Group name` (just the group name for system messages)
  - server: `@user (#channel, Server)`
- **Body:** the content, or the system-message text.

**Delivery, all three chosen by the user:**
- **In-app toasts** use gpui-component's `window.push_notification(Notification::new().title(…).message(…).on_click(…))`. They work with `cargo run`.
- **The Dock badge** uses AppKit directly (`NSApplication::sharedApplication().dockTile().setBadgeLabel(…)`) via `objc2-app-kit`, which is already in the dependency tree. It also works with `cargo run`.
- **macOS banners** use GPUI's `cx.show_system_notification(SystemNotification { tag, title, body, actions })` and `cx.on_system_notification_response(…)`. GPUI deliberately disables these when the process has no bundle identifier, because `UNUserNotificationCenter` would abort an unbundled process ("bundleProxyForCurrentProcess is nil"). So banners need RuSToat to run from a `.app` bundle. A small script builds and ad-hoc signs one. Running the executable inside the bundle directly keeps terminal logs.
- **Routing:** when the window is active, show a toast (unless that channel is on screen). When it's inactive, show a banner if bundled, otherwise a toast.

**Rejected:** `notify-rust`/`mac-notification-sys`, which are in the tree via a dependency. They use the deprecated `NSUserNotificationCenter`, whereas GPUI already wraps the supported API. Also rejected: computing unreads purely client-side, because it would disagree with the user's other clients.

## Assumptions
1. **Missing entries count as unread.** A channel with a `last_message_id` but no `ChannelUnread` entry is unread, which matches the web client's `(unread?.last_id ?? "0") < lastMessageId`. If that proves noisy against the web app in chunk 1's manual check, treat missing entries as read and record the change.
2. **Mentioned** means any of:
   - `message.mentions` contains the user
   - `message.role_mentions` intersects the user's roles in that server (from `Ready` members)
   - flag bit 2 (`MentionsEveryone`)
   - flag bit 3 (`MentionsOnline`), only while the user is online

   Flag `SuppressNotifications` (bit 1) suppresses notifications but not unread state. *(Corrected in chunk 1:)* `MessageFlags` values are **bit positions**, not masks. The backend's `MessageFlagsValue::has_value` tests `flags & (1 << flag)`, so the masks are 2 (suppress), 4 (@everyone) and 8 (@online).
3. **Muted channels and servers** show no unread dot and never notify, but still show mention counts. This is an assumption, because the web client's sidebar rule for muted channels wasn't checked.
4. **Mark as read automatically** when the channel is open, the window is active, and the list is at the newest message (`MessageScrollerState::is_following_tail()`). The ack for the newest message is debounced by 1 s and only sent if it's newer than `last_id`. Manual: right-click a server in the rail and choose "Mark as read".
5. **Dock badge** = unread mentions plus unread DMs/groups that are at level `"all"`. It's cleared at 0, and recomputed whenever unread state changes.
6. **Notification text** is plain: mentions as `@Name`, custom emoji as `:name:`, Markdown left as typed, truncated to 200 characters.
7. **One notification per channel** (`tag` = channel ID): a newer message replaces the older banner. Reading a channel dismisses its banner (`cx.dismiss_system_notification(tag)`). Clicking a banner or toast focuses the window and opens that channel.
8. **The bundle** is `target/debug/RuSToat.app`, with `CFBundleIdentifier` `chat.stoat.rustoat`, the executable copied in, and `codesign --force --sign -` (ad-hoc). Expect the first banner to trigger macOS's notification permission prompt, and a Keychain "Always Allow" prompt, because the bundle has a different code identity from `cargo run`.
9. **Live settings:** a `UserSettingsUpdate` event that includes `notifications` updates the rules immediately. Other settings are ignored.

## Open Questions
- *(Deferred)* Should notification settings (levels and mutes) be editable from RuSToat? This plan only reads and honours them.
- *(Deferred)* Should notifications play a sound? Banners use the system default; toasts are silent.

## Plan

### Chunk 1: Unread indicators
**Goal:** The sidebar shows where new activity is: bold channel names and dots for unread channels, red mention counts, and the same summarised on each server in the rail and on Home. All of it comes from Stoat's own unread state and stays live.
**Files likely touched:**
- `src/api/events.rs`:
  - add `"channel_unreads"` to `READY_FIELDS`
  - `Ready.unreads: Vec<ChannelUnread>` (parsed leniently)
  - `ServerEvent::ChannelAck { id, user, message_id }`
- `src/store.rs`:
  - an `unreads` map
  - `is_unread(channel)` and `mention_count(channel)`
  - per-server and Home summaries (`ServerEntry.unread` / `.mentions`, `ChannelEntry.unread` / `.mentions`)
  - `is_mentioned(&Message)` (Assumption 2)
  - `apply_message` updates mentions for messages that mention the user
  - `apply_ack(channel, message_id)` sets `last_id` and drops mentions at or before it
- `src/ui/chat.rs`: channel rows (bold plus dot), mention badges, rail indicators
- tests: a `channel_unreads` fixture plus a new `tests/store_unreads.rs`
**Depends on:** None
**Risks:**
- **Missing entries:** Assumption 1 may mark many old channels unread. Compare with the web app and switch if needed.
- **Role mentions** need the user's member roles. `Ready.members` is only the user's own memberships, which is enough.
- **DM entries:** `ChannelUnread` entries for DMs may only exist after the first ack.
**How to verify:** `cargo test --test store_unreads` covers:
- Ready entries giving unread or read
- a missing entry
- a live message making a channel unread
- a mention via an ID, a role, the `@everyone` flag, or `@online` (only while online)
- an ack clearing up to an ID
- server and Home summaries
- *(moved to chunk 3, which parses the notification settings)* muted channels (no dot, mention count kept)
**Manual check:** The dots and counts match the web app's sidebar for the user's account. A message sent from another client lights its channel up live, and reading it in the web app clears it here (via `ChannelAck`).
**Shippable when:** Unread and mention indicators match the web app and update live, and the tests pass.

### Chunk 2: Mark as read
**Goal:** Reading a channel in RuSToat marks it read on Stoat, so the user's other clients agree. Servers can be marked read from the rail.
**Files likely touched:**
- `src/api/client.rs`: `ack_message(channel, message)` and `ack_server(server)`
- `src/store.rs`: `apply_ack` (already present), plus `apply_server_ack(server)` for every channel in it
- `src/ui/chat.rs`:
  - an ack check on render, on scroll and on window activation (`window.is_window_active()` / `observe_window_activation`) with a 1 s debounce and an in-flight guard
  - a right-click "Mark as read" on rail servers, using gpui-component's context menu
- `tests/api_unreads.rs`: `wiremock` tests
**Depends on:** Chunk 1
**Risks:**
- **Ack spam:** without the debounce and the "newer than `last_id`" check, every live message would trigger a request.
- **Background acks:** acking while the window is in the background would hide messages the user never saw. Require the window to be active.
- **Context menus** in the rail inside a scroll area may need a gpui-component example to get right.
**How to verify:**
- `cargo test --test api_unreads`: the exact `PUT` paths with the session header, 204 handling, and a server ack.
- Store tests: a server ack clears all of its channels, and an ack older than `last_id` is ignored.
**Manual check:** Open an unread channel in RuSToat. The web app's unread dot for it clears within about 2 s. "Mark as read" on a server clears every channel in it in both clients.
**Shippable when:** Reading in RuSToat syncs to Stoat without spamming acks, and the tests pass.

### Chunk 3: Notification rules and in-app toasts
**Goal:** Decide exactly as the web client does whether a new message should notify, and show it as an in-app toast. Clicking the toast opens the channel. This is the first visible notification, and it works with `cargo run`.
**Files likely touched:**
- `src/notify.rs` (new, pure):
  - `NotificationSettings`, parsed from the `notifications` user setting (legacy `"muted"`, `until` expiry)
  - `level_for(channel)`, `is_muted(channel, now)`
  - `should_notify(&ChatStore, &Message, Viewing { channel, window_active }, now) -> Option<Notice { tag, channel, title, body }>`, implementing the skip rules and title formats from the Approach
- `src/api/events.rs`:
  - add `user_settings[notifications]` to `READY_FIELDS` (alongside `ordering`)
  - parse it in `Ready`
  - `ServerEvent::UserSettingsUpdate { update }`
- `src/store.rs`: holds the settings and the user's presence (from `Ready.users`). *(From chunk 1)* `is_unread` and the summaries skip muted channels and servers (no dot, mention count kept; Assumption 3), now that mutes are known.
- `src/ui/chat.rs`: calls `should_notify` on each live `Message`. When it returns a `Notice`, it calls `window.push_notification(Notification::new().title(…).message(…).on_click(open channel))`, deduplicated per channel tag.
- `tests/notify_rules.rs`
**Depends on:** Chunk 1 (`is_mentioned`, unreads), Chunk 2 (viewing/active state)
**Risks:**
- **Toast layer:** gpui-component draws toasts in its `Root` notification layer. Check that `gpui_kit::open_window`'s `gpui_base::Root` renders that layer once `gpui_component::init` has run. If not, render the layer in `AppRoot`.
- **Settings shape:** the `notifications` value is a JSON string inside the `[timestamp, json]` tuple, the same pattern as `ordering`. Unknown keys must be ignored.
- **Getting the rules wrong is user-visible**, either as noise or as missed messages, so every skip rule gets a test.
**How to verify:** `cargo test --test notify_rules` has a case for each rule:
- viewing and focused skips; viewing but unfocused notifies
- own messages, blocked authors and muted (channel, server, or expired mute) skip
- `"none"` skips; `"mention"` with and without a mention
- the server default `"mention"`; the DM default `"all"`
- Busy skips; Focus with and without a mention
- Saved Notes skips
- `SuppressNotifications` skips
- each title format and the body truncation
**Manual check:** In another channel, receive a DM, a server mention and a plain server message from another account. Toasts appear for the DM and the mention only, under default settings. Muting a channel in the web app stops them live. Clicking a toast opens that channel.
**Shippable when:** Toasts follow the web client's rules (tested), settings update live, and clicking a toast navigates.

### Chunk 4: Dock badge
**Goal:** Show the number of things needing attention on the Dock icon, so the user sees activity when RuSToat is in the background.
**Files likely touched:**
- `Cargo.toml`: `objc2`, `objc2-foundation` and `objc2-app-kit` (0.3, features `NSApplication` and `NSDockTile`) as macOS-only target dependencies, all already in the tree
- `src/dock.rs` (new): `set_badge(count: usize)`, which shows nothing at 0 and "99+" above 99
- `src/store.rs`: `attention_count()` (Assumption 5)
- `src/ui/chat.rs`: updates the badge whenever unread state changes, and clears it on logout
**Depends on:** Chunk 1
**Risks:**
- **Main thread:** AppKit calls need the main thread (`MainThreadMarker`). GPUI's UI callbacks run there, but assert it rather than assume.
- **Unbundled binaries:** should show a Dock tile with a badge. Verify with `cargo run` first, then with the bundle (chunk 5).
**How to verify:** Unit tests for `attention_count` (mentions plus `"all"`-level DM/group unreads; muted excluded) and for the badge label format (0, 1, 99, 100 giving "99+").
**Manual check:** With RuSToat in the background, a DM from another account adds a badge to the Dock icon. Reading it clears the badge.
**Shippable when:** The Dock badge tracks attention count with `cargo run`, and the tests pass.

### Chunk 5: macOS banners (app bundle)
**Goal:** Real macOS notifications when RuSToat is in the background, with clicks going to the right channel and stale banners dismissed when read.
**Files likely touched:**
- `scripts/bundle.sh` (new):
  - builds the app
  - assembles `target/debug/RuSToat.app/Contents/{Info.plist, MacOS/stoat-gpui}`
  - `CFBundleIdentifier` `chat.stoat.rustoat`, `CFBundleName` RuSToat, `LSMinimumSystemVersion`
  - `codesign --force --sign -`
  - prints the path to run
- `scripts/run-bundled.sh` (new): runs `…/RuSToat.app/Contents/MacOS/stoat-gpui` directly, so logs stay in the terminal
- `src/ui/chat.rs`:
  - routes notices to `cx.show_system_notification(SystemNotification { tag: channel id, title, body, actions: vec![] })` when the window is inactive and banners are available; toast otherwise
  - `cx.on_system_notification_response(…)` activates the window and opens the tagged channel
  - `cx.dismiss_system_notification(tag)` when that channel is acked
- `README.md`: running bundled, the notification permission prompt, and the Keychain prompt
**Depends on:** Chunks 2 and 3
**Risks:**
- **Detecting bundled mode:** GPUI logs `system notifications disabled: not running from an app bundle` and silently drops calls. RuSToat needs its own `NSBundle::mainBundle().bundleIdentifier()` check to know whether banners are available; without it, unbundled runs would lose notifications instead of falling back to toasts.
- **Permission:** the first banner triggers macOS's authorization prompt. If it's denied, banners silently do nothing; detect what's possible and log it.
- **Ad-hoc signing:** an ad-hoc-signed bundle in `target/` should be allowed to post notifications. If macOS refuses, document the limit and keep toasts and the badge.
- **Keychain:** the bundle's different code identity triggers a Keychain prompt on first run, as with any rebuild.
**How to verify:**
- `scripts/bundle.sh` succeeds, and `codesign -dv target/debug/RuSToat.app` shows an ad-hoc signature with identifier `chat.stoat.rustoat`.
- `scripts/run-bundled.sh` logs no `system notifications disabled` line.
- Unit test of the routing decision (active vs inactive, bundled vs not).
**Manual check:** Run bundled, allow notifications, and put RuSToat in the background. A DM from another account shows a macOS banner titled `@user`. Clicking it brings RuSToat forward on that DM. Reading it removes the banner from Notification Center.
**Shippable when:** Banners work from the bundle, clicks navigate, read channels' banners are dismissed, and unbundled runs fall back to toasts.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings` (optional live checks: `cargo test -- --ignored`).

### Unit Tests
- Unread state:
  - read or unread from `Ready`, including a missing entry
  - live messages
  - acks, including older-than-`last_id` acks being ignored
  - server acks
  - summaries per server and for Home
- `is_mentioned`: user ID, role, `@everyone` flag, `@online` flag while online and not while offline.
- Notification settings: parsing (levels, mutes with and without `until`, legacy `"muted"`, unknown keys) and level resolution order.
- `should_notify`: every skip rule, title formats and body truncation.
- Dock: `attention_count` and badge label formatting.
- Routing: toast vs banner by window activity and bundle availability.

### Integration Tests
- `wiremock`: `PUT /channels/{id}/ack/{msg}` and `PUT /servers/{id}/ack` paths, the session header, and 204 handling.
- Event decoding: `ChannelAck`, `UserSettingsUpdate` (with `notifications`), and `Ready` with `channel_unreads` and `user_settings[notifications]`.
- Store pipeline: a `Ready` fixture plus a live `Message` mentioning the user gives an unread mention and an attention count of 1; an ack gives 0.

### Manual Tests
1. Unread dots and mention counts match the web app on login.
2. A message from another account lights its channel up live. Reading it in the web app clears it here.
3. Reading a channel here clears it in the web app within about 2 s. "Mark as read" on a server works.
4. With default settings, a DM and a mention show toasts and a plain server message doesn't. Muting in the web app takes effect live.
5. Setting status to Busy suppresses notifications; Focus allows only mentions.
6. The Dock badge counts attention and clears when everything is read.
7. Bundled and in the background: a DM shows a macOS banner, clicking it opens the DM, and reading it dismisses the banner.
8. Unbundled (`cargo run`): no banners, but toasts and the Dock badge still work.

## Definition of Done
- [ ] **Sidebar:** shows unread (bold plus dot) and mention counts per channel, per server (rail) and for Home, from `Ready.channel_unreads`, updated live by `Message` and `ChannelAck`. It matches the web app for the user's account.
- [ ] **Auto read:** viewing a channel with the window active and the list at the newest message sends at most one `PUT /channels/{id}/ack/{msg}` per newer message (debounced 1 s). "Mark as read" on a server sends `PUT /servers/{id}/ack`.
- [ ] **Rules:** follow `stoatchat/for-web`:
  - level resolution: channel, then server (default `"mention"`), then type default (DMs/groups `"all"`)
  - mutes with expiry, including server mutes
  - the skip rules (viewing and focused, own, blocked, muted, level, presence, Saved Notes, `SuppressNotifications`)
  - every rule has a test
- [ ] **Live settings:** notification settings come from the `notifications` user setting and update live on `UserSettingsUpdate`.
- [ ] **Toasts:** notifying messages show an in-app toast with the web client's title format, and clicking it opens the channel.
- [ ] **Dock badge:** shows the attention count (mentions plus unread `"all"`-level DMs/groups), "99+" above 99, and nothing at 0.
- [ ] **Bundle:** `scripts/bundle.sh` produces an ad-hoc-signed `RuSToat.app` (`chat.stoat.rustoat`).
- [ ] **Banners:** when bundled and in the background, notifying messages show macOS banners tagged per channel. Clicking one opens that channel, and reading a channel dismisses its banner. Unbundled runs fall back to toasts.
- [ ] **Checks:** `cargo test && cargo clippy --all-targets -- -D warnings` passes.
