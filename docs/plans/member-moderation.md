# Plan: Member moderation (RuSToat milestone 22)

## Summary
Milestone 22 of the [roadmap](roadmap.md) lets a moderator act on server members from the member menu, and lets anyone report content:
- **Remove people:** kick, and ban (with a reason, optionally deleting their recent messages).
- **Time out** a member, or end their timeout.
- **Edit a member:** their nickname, removing their server avatar, and their roles.
- **Voice moderation** (added at planning, user's choice): server mute, server deafen, move to another voice channel, and disconnect from a call.
- **Report** a message, a user or a server to Stoat's moderation team (`POST /safety/report`).

## Approach
**What Stoat allows** (`stoatchat/stoatchat` v0.16.2, read from `/tmp/stoatchat-0.16.2` on 2026-11-08: `crates/delta/src/routes/servers/{member_remove,ban_create,member_edit}.rs`, `routes/safety/report_content.rs`):
- **Kick:** `DELETE /servers/{server}/members/{user}`.
  - Needs Kick Members.
  - Refused for yourself (`CannotRemoveYourself`) and for the owner (`InvalidOperation`).
  - The target's rank must be below yours (`NotElevated`).
- **Ban:** `PUT /servers/{server}/bans/{user}` with `DataBanCreate { reason (≤1024), delete_message_seconds (0–604800) }`.
  - Needs Ban Members, with the same self, owner and rank rules as kick.
  - Works on non-members too, so a message author who already left can be banned.
- **Member edit:** `PATCH /servers/{server}/members/{user}` (`DataMemberEdit`; RuSToat already has `StoatClient::edit_member`). For someone else:
  - **Nickname** needs Manage Nicknames.
  - **Avatar can only be removed** (`remove: ["Avatar"]`, Remove Avatars). Setting one is `InvalidOperation`. Pronouns are refused.
  - **Roles** needs Assign Roles. Only roles ranked below yours can be *added*, and managed (bot) roles can't be added at all.
  - **Timeout:** `timeout: <timestamp>`, or `remove: ["Timeout"]` to end it. Needs Timeout Members. Refused on yourself, and on anyone who has Timeout Members themselves (`IsElevated`).
  - **Voice:**
    - `can_publish` (server mute) needs Mute Members.
    - `can_receive` (server deafen) needs Deafen Members.
    - `voice_channel` (move) and `remove: ["VoiceChannel"]` (disconnect) need Move Members. A move also needs Connect in the destination, and fails if the target isn't in a call (`NotConnected`).
  - **Every edit of someone else** needs their rank below yours (`NotElevated`).
- **Rank:** a member's rank is their best (lowest-numbered) role's `rank`, or `i64::MAX` with no roles. The owner passes every rank check. RuSToat already holds servers' roles, ranks and members, live (milestone 7).
- **Report:** `POST /safety/report` with `{ content: ReportedContent, additional_context (≤1000) }`.
  - `ReportedContent` is `Message { id, report_reason }`, `Server { id, report_reason }` or `User { id, report_reason, message_id? }`.
  - Messages and servers use `ContentReportReason`; users use `UserReportReason` (both in `revolt-models` 0.16.2).
  - You can't report yourself, your own message, or a server you own (`CannotReportYourself`).
- **What the client hears afterwards:** a kick or ban sends `ServerMemberLeave`, and edits send `ServerMemberUpdate`. Voice changes come as milestone 33's voice events. RuSToat already handles all of these, so the store needs no new events.

**What the web client does** (`stoatchat/for-web` v0.16.3: `app/menus/{UserContextMenu,MessageContextMenu,ServerContextMenu}.tsx`, `modal/modals/{KickMember,BanMember,BanNonMember,TimeoutMember,RemoveTimeout,ServerIdentity,UserProfileRoles,ReportContent}.tsx`):
- **User menu order:**
  1. Edit identity, Edit roles;
  2. Timeout / Remove timeout, Kick, Ban (marked destructive);
  3. Report user, with Block.
- **Dialogs:**
  - **Kick** confirms.
  - **Ban** asks for a reason and "Delete message history": none, 1 hour, 6 hours, 1 day, 3 days, 7 days.
  - **Timeout** offers 1 minute, 5 minutes, 10 minutes, 1 hour, 1 day, 1 week.
  - **Report:**
    - a reason picker, plus "Give us some detail";
    - "No reason specified" needs detail;
    - menus offer "Report message" and "Report server".

**What exists in RuSToat** (line numbers as of 2026-11-08):
- **Pieces to extend:**
  - `src/profile.rs` `user_menu(store, user, channel) -> Vec<UserMenuItem>` builds the person menu as plain data (tested in `tests/profile.rs`). `src/ui/chat.rs` draws it (`People::menu` ~10903, `user_menu_label` ~10946) and acts on it (`on_user_menu` ~5629). The menu knows the channel it was opened from, and so the server.
  - `src/permissions.rs` works out permission bits with `revolt-permissions` (`server_permissions`, `channel_permissions`, `has`), and has a private `is_timed_out(member)` (~414). Tested in `tests/permissions.rs`.
  - `src/ui/identity_dialog.rs` `IdentityEditor`: the user's *own* nickname, avatar and pronouns on a server (milestone 15), saved through `profile_edit::IdentityForm` and `edit_member`.
- **Building blocks to reuse:**
  - ~~`src/ui/confirm_dialog.rs` `confirm_then`~~ (amended in chunk 1: that's the MFA "Confirm it's you" dialog). Chunk 1 added `moderation_dialogs::confirm_member_action` (asks, then runs one request, busy until Stoat answers, toasting a refusal) and `run_member_action` (the same without asking, for Shift, as for Block).
  - `src/ui/server_dialogs.rs` and `src/ui/group_dialogs.rs` are the dialog patterns. `AddMembersDialog` has a checkable list.
  - The message menu (~11271, "Copy message link") and `server_menu` (~4778) take new items the same way.
- **Context for the voice items:** milestone 33's `ChatStore::voice_participants` / `voice_rows` and the voice participant rows (`render_voice_participant`), which aren't interactive yet.

**Decisions** (user, 2026-11-08):
1. **Voice moderation is in this milestone** (one chunk).
2. **The web client's choices otherwise:** its timeout lengths and ban-history options; a reason only on bans; Report in the user, message and server menus with Stoat's reason lists.

**Rejected alternatives.**
- **Offering every item and letting Stoat refuse.** The client has roles, ranks and permissions, so it can hide what Stoat would refuse, as the web client does. Stoat's errors still get a toast for races, such as roles changing while the menu is open.
- **A separate "Ban user" for non-members**, as in the web client. One "Ban…" item works for both, because the endpoint takes either. RuSToat's member cache is lazy anyway, so "not a member" isn't reliably known.
- **A setter for other members' avatars.** Stoat refuses it, so the only option is Remove.

## Assumptions
1. **Who sees what:**
   - Moderation items appear only in a server context: the menu was opened from a server channel, or from that server's member list.
   - They need the permission, a target other than yourself and other than the owner, and a target rank below yours (the owner passes).
   - Timeout is also hidden if the target has Timeout Members.
   - The rules are one pure function, `permissions::moderation(store, server, target) -> Moderation`, a struct of booleans (`kick`, `ban`, `timeout`, `nickname`, `remove_avatar`, `roles`, `mute`, `deafen`, `move_members`), unit-tested with fixture roles.
   - An unknown target membership (not cached) counts as rank `i64::MAX` with no roles. Ban is still offered (it works on non-members). The member-only items are offered and left to Stoat's check, as the member may just not be cached.
2. **Menu order**, following the web client and RuSToat's existing groups:
   1. people (Profile, Message, …);
   2. **server** (Edit identity…, Edit roles…);
   3. **voice** (Server mute / unmute, Server deafen / undeafen, Move to ›, Disconnect from call), only while the target is in one of this server's calls;
   4. group membership;
   5. **moderation** (Time out… / End timeout…, Kick…, Ban…);
   6. friend requests;
   7. standing (Remove friend, Block / Unblock, **Report user…**);
   8. copy.

   Destructive items use the menu's danger styling if it has one.
3. **Kick** asks "Kick {name} from {server}?" (Shift skips, as for Block).
4. **Ban dialog:**
   - a reason field (optional, ≤1024, counted like other fields);
   - "Delete message history": None, 1 hour, 6 hours, 1 day, 3 days, 7 days (`delete_message_seconds` 0, 3600, 21600, 86400, 259200, 604800);
   - a danger "Ban" button.
5. **Timeout dialog:** 1 minute, 5 minutes, 10 minutes, 1 hour, 1 day, 1 week (default 1 minute, as the web client).
   - The timestamp is now plus the duration, from the Mac's clock.
   - "End timeout…" confirms.
   - The profile card shows "Timed out until {time}" for a member in timeout, so moderators see why the item reads "End timeout".
6. **Edit identity for someone else** reuses `IdentityEditor` with a target user:
   - nickname (Manage Nicknames), plus "Remove avatar" only if they have a server avatar and you have Remove Avatars;
   - no upload, no pronouns;
   - the title is "Change {name}'s identity on {server}".
   - If generalising the dialog turns out tangled, a small separate dialog in the same module is fine (record it).
7. **Edit roles dialog:**
   - lists the server's roles highest first, each with a checkbox, its colour dot and its name;
   - a role you may not add (ranked at or above yours, or managed) is disabled with a tooltip, unless it's already on the member, in which case you can still remove it (Stoat allows that once you outrank the member);
   - Save sends the whole `roles` list.
   - If the role model has no "managed" field in 0.16.2, only rank is checked (note it in the record).
8. **Report dialog:**
   - a reason dropdown (Stoat's lists, with the web client's English labels) and a detail field (≤1000);
   - "No reason specified" needs detail;
   - success toasts "Report sent. Thank you.";
   - offered in the user menu (not for yourself), the message menu (not on your own messages or system messages) and the server menu (not on servers you own);
   - a user reported from a message's author menu doesn't attach the message (`message_id` stays `None`); that can come later.
9. **Voice moderation:**
   - **Server mute/deafen** toggles `can_publish` / `can_receive` and reads their current value from the cached member (both default `true`).
   - **Move to ›** is a submenu of this server's other voice channels where *you* have Connect.
   - **Disconnect** sends `remove: ["VoiceChannel"]`.
   - **Voice participant rows** (milestone 33) open the person's menu on right-click, with the channel's server as context, like member list rows.
   - A server-muted participant already shows mic-off (Stoat sends `is_publishing: false`), so no new icon.
10. **Errors** toast through one `moderation_error_text(err)` in `src/moderation.rs`:
    - `NotElevated` / `IsElevated` → "They rank too high for you to do that.";
    - `MissingPermission` → "You don't have permission to do that.";
    - `CannotReportYourself` → "You can't report yourself.";
    - `NotConnected` → "They're no longer in a call.";
    - rate limits and the rest via `profile_edit::error_text`.
11. **New code layout:**
    - `src/moderation.rs` (no GPUI): durations, the ban history options, report reasons and labels, the timestamp helper, error texts.
    - `src/ui/moderation_dialogs.rs`: the ban, timeout, roles and report dialogs.
    - API calls in `src/api/client.rs`.

## Open Questions
None blocking. Deferred: attaching the message as context when reporting its author (`message_id`), and an audit-log reason header (`X-Audit-Log-Reason`) for kicks and edits, which the web client doesn't send either.

## Plan

### Chunk 1: Kick and ban
**Goal:** A moderator can kick or ban someone from the member menu, and the menu offers it only when Stoat would allow it.
**Files likely touched:**
- `src/api/client.rs`: `kick_member(server, user)`, `ban_user(server, user, &DataBanCreate) -> ServerBan`.
- `src/permissions.rs`:
  - `member_rank(store, server, user) -> i64` (Stoat's `get_ranking`).
  - `moderation(store, server, target) -> Moderation` (Assumption 1). Every field is worked out now, so later chunks only wire the UI.
  - `is_timed_out` made `pub`.
- `src/moderation.rs` (new): `BAN_HISTORY` options, `moderation_error_text`; `pub mod moderation;` in `src/lib.rs` (hand-edited, never formatted).
- `src/profile.rs`: `UserMenuItem::{Kick, Ban}`, and the moderation group in `user_menu` (Assumption 2).
- `src/ui/chat.rs`: labels and icons (`UserX`, `Ban`), `on_user_menu` arms. Kick through `confirm_member_action` (Shift: `run_member_action`), and Ban opens the dialog.
- `src/ui/moderation_dialogs.rs` (new): `BanDialog`. Plus `pub mod moderation_dialogs;` in `src/ui/mod.rs` (by hand).
- Tests: `tests/api_members.rs` (kick and ban requests and bodies, wiremock), `tests/permissions.rs` (rank and `moderation` cases), `tests/profile.rs` (menu items present or absent).

**Depends on:** None.
**Risks:**
- **The owner's rank:** the owner may have no roles (rank `i64::MAX`), yet Stoat lets them act on anyone. `moderation` must special-case the owner as Stoat's `are_we_server_owner` does. Test it.
- **The target's membership may not be cached** (a message author on a big server). See Assumption 1. Check how `user_menu` gets the server when opened from a message row.
- **Ban's history deletion is server-side and silent:** messages vanish through `BulkMessageDelete` / `MessageDelete` events, which RuSToat already applies.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with:
- `tests/permissions.rs`:
  - a moderator with Kick and Ban above the target gets both;
  - an equal or lower rank gets neither;
  - the owner gets both on anyone;
  - nobody gets them on themselves or on the owner.
- `tests/api_members.rs`: `DELETE /servers/s/members/u`; `PUT /servers/s/bans/u` with `{"reason":"spam","delete_message_seconds":86400}`, omitting what's unset.
- `tests/profile.rs`: Kick/Ban appear only in a server context with the permission.

**Manual check:** with the user's account owning a test server and the second account as a member, kick the second account (it disappears from the member list; its RuSToat leaves the server). Have it rejoin, then ban it with "1 hour" of history (its recent messages vanish).
**Shippable when:** kick and ban work from the menu, and are hidden when Stoat would refuse them.

### Chunk 2: Timeouts
**Goal:** A moderator can time a member out for a chosen time, or end it early, and the card shows the timeout.
**Files likely touched:**
- `src/moderation.rs`: `TIMEOUTS` (Assumption 5), `timeout_until(now, seconds) -> Timestamp` (unit-tested).
- `src/profile.rs`: `UserMenuItem::{TimeOut, EndTimeout}`, chosen by `is_timed_out`.
- `src/ui/moderation_dialogs.rs`: `TimeoutDialog` (duration choice, then `edit_member` with `timeout`).
- `src/ui/chat.rs`: menu arms. End timeout is `confirm_member_action`, then `edit_member` with `remove: [Timeout]`.
- `src/ui/profile_card.rs` (and `src/profile.rs`'s card content): "Timed out until {time}".
- Tests: `tests/api_members.rs` (timeout body as an RFC 3339 timestamp; removal), `tests/profile.rs` (item swap; card line).

**Depends on:** Chunk 1 (`moderation`, the dialog module).
**Risks:**
- **`Timestamp` building:** `iso8601_timestamp::Timestamp` from a `SystemTime` or a parsed string. Check the crate's API.
- **A timeout expiring while the menu is cached:** the menu is built when it opens, so it's fine. The card's line should hide once it's in the past (rebuilt on open).

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with `timeout_until` tests (e.g. 60 s after a fixed instant serialises to the expected string), the request bodies, and the menu swap for a member whose `timeout` is in the future or the past.
**Manual check:** time out the second account for 1 minute. It can't send messages (its message box says so, from milestone 12's permissions). The card shows the timeout. End it early, and it can send again.
**Shippable when:** timeouts can be given and ended, and are visible.

### Chunk 3: Edit a member's identity and roles
**Goal:** A moderator can change a member's nickname, remove their server avatar, and change their roles.
**Files likely touched:**
- `src/ui/identity_dialog.rs`: a target user; for others, nickname plus "Remove avatar" only (Assumption 6).
- `src/profile_edit.rs`: `IdentityForm` builds the edit for someone else (no pronouns, avatar removal only).
- `src/ui/moderation_dialogs.rs`: `RolesDialog` (Assumption 7).
- `src/permissions.rs`: `assignable_roles(store, server) -> Vec<(role_id, bool)>`, or a rank check helper for the dialog.
- `src/profile.rs`: `UserMenuItem::{EditIdentity, EditRoles}` (server group).
- `src/ui/chat.rs`: labels (`UserPen`, `IdCard` or similar) and arms.
- Tests: `tests/profile.rs` (items), `tests/permissions.rs` (assignable roles: below rank yes, at/above no, owner all), `src/profile_edit.rs` unit tests (someone else's form never sends pronouns or an avatar ID), `tests/api_members.rs` (roles body).

**Depends on:** Chunk 1.
**Risks:**
- **`IdentityEditor` is built around the user's own profile** (uploads, previews, pronouns). Generalising may touch more than it seems. The fallback is a small dialog (Assumption 6).
- **Managed roles:** check whether `Role` in `revolt-models` 0.16.2 marks them. If not, Stoat's refusal is toasted.
- **Roles changing under the dialog** (a live `ServerRoleUpdate`): the dialog reads roles when it opens. Stoat's refusal covers a race.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with the tests above.
**Manual check:**
- Rename the second account's nickname: the member list, messages and typing names update live.
- Remove its server avatar.
- Give it a role, then take it away: the member list section and name colour change.
- A role above yours is disabled.

**Shippable when:** nicknames, server avatars and roles of others can be edited within Stoat's rules.

### Chunk 4: Voice moderation
**Goal:** A moderator can server-mute, server-deafen, move or disconnect someone who's in one of the server's calls.
**Files likely touched:**
- `src/profile.rs`: `UserMenuItem::{ServerMute(bool), ServerDeafen(bool), MoveTo, DisconnectVoice}` in the voice group, shown when `store` lists the target in one of the server's voice channels.
- `src/ui/chat.rs`:
  - arms that call `edit_member` with `can_publish`, `can_receive`, `voice_channel` or `remove: [VoiceChannel]`;
  - the "Move to ›" submenu (the server's other voice channels where `can_connect` holds);
  - voice participant rows get a right-click person menu.
- `src/store.rs`: a lookup of which of a server's voice channels a user is in (`voice_channel_of(server, user)`), if `voice_participants` alone is awkward.
- `src/assets.rs`: `MicOff` / `HeadphoneOff` are already in (milestone 33). Add others only if used.
- Tests: `tests/profile.rs` (voice items only while in a call; per permission), `tests/api_members.rs` (the four bodies), `tests/store_voice.rs` (the lookup, if added).

**Depends on:** Chunk 1 (`moderation`); milestone 33 (voice states and rows).
**Risks:**
- **Live effect of a server mute on the target's own RuSToat:**
  - Stoat updates LiveKit permissions (`sync_user_voice_permissions`), and the target's microphone publication may be force-unpublished.
  - Milestone 33's panel reads Speak at render, but `Controls` holds the publication.
  - Check that nothing panics or loops if it's taken away. Unmuting may need a rejoin. Record what happens; fixing it fully is milestone 34 material if it's large.
- **Moves exercise milestone 33's chunk 7 path:** the target's RuSToat should follow (held call, then join). This is its first real test. Bugs found there are fixed here if small.
- **The submenu API:** check how gpui-component's `PopupMenu` does submenus. If it has none, a "Move to…" dialog listing channels is fine.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with the tests above.
**Manual check:** with both accounts in a voice channel:
- server mute: the target's row shows mic-off and the web client can't hear them;
- unmute: they're heard again;
- server deafen and undeafen;
- move to the other channel: the target's RuSToat panel shows the new channel, with no toast;
- disconnect: after about 3 s the target toasts "You were removed from the call."

**Shippable when:** the four voice actions work from the menu and the participant rows.

### Chunk 5: Reports
**Goal:** Anyone can report a message, a user or a server to Stoat's moderation team.
**Files likely touched:**
- `src/api/client.rs`: `report(&ReportedContent, additional_context)`.
- `src/moderation.rs`: reason lists and labels for users and content, and the "needs detail" rule (`report_ready(reason, detail) -> bool`).
- `src/ui/moderation_dialogs.rs`: `ReportDialog` (Assumption 8).
- `src/profile.rs`: `UserMenuItem::Report` (standing group, not for yourself).
- `src/ui/chat.rs`: "Report message…" in the message menu (not your own, not system messages), "Report server…" in `server_menu` (not servers you own), and the user menu arm.
- `src/assets.rs`: an icon for Report (`Flag` or `TriangleAlert`, already loaded by the safety test).
- Tests: `tests/api_members.rs` or a new `tests/api_safety.rs` (each `ReportedContent` body: `{"content":{"type":"Message","id":…,"report_reason":"SpamAbuse"},"additional_context":"…"}`), `src/moderation.rs` unit tests (labels cover every reason; `NoneSpecified` needs detail).

**Depends on:** None (independent of chunks 1–4, but uses the dialog module from chunk 1).
**Risks:**
- **Reports go to Stoat's real moderation team.** Manual testing must not file real reports carelessly. Use the second account's own message, label the detail "RuSToat test, please ignore", and file at most one of each kind. Or test against wiremock only and mark the live check optional. Ask the user before filing live ones.
- **Reason enums' serde names:** check they serialise as `"SpamAbuse"` etc. by round-tripping in a test.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with the body tests above.
**Manual check:** (with the user's go-ahead) report the second account's message once, and see the success toast. Reporting your own message isn't offered.
**Shippable when:** the three report kinds can be filed from their menus.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- **Formatting:**
  - Don't run `cargo fmt` across the crate. Format only files you touch with `rustfmt --edition 2024 <file>`, and never on `mod.rs`, `lib.rs` or `main.rs`.
  - For files that already have drift, hand-format new code and check that the `Diff in` count from `rustfmt --edition 2024 --check <file>` hasn't grown.
  - Starting counts (2026-11-08):
    - with drift: `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 14;
    - clean: `src/profile.rs`, `src/permissions.rs`, `src/ui/identity_dialog.rs`, `src/profile_edit.rs`, `src/ui/server_dialogs.rs`, `src/ui/confirm_dialog.rs`, `src/ui/profile_card.rs`, `tests/profile.rs`, `tests/permissions.rs`, `tests/api_members.rs`;
    - new files must be clean.
- **Manual testing** needs two RuSToat accounts in one test server that the user's account owns, plus the web client for the voice checks.

### Unit Tests
- `src/moderation.rs`:
  - durations and ban history map to the right seconds;
  - `timeout_until`;
  - report reason labels cover every variant;
  - `report_ready`;
  - `moderation_error_text` for each error.
- `src/profile_edit.rs`: someone else's identity edit never carries pronouns or an avatar upload.

### Integration Tests
- `tests/permissions.rs`: `member_rank`, `moderation` (ranks, owner, self, permission bits, Timeout Members target), assignable roles.
- `tests/profile.rs`: each new menu item's presence rules and the group order.
- `tests/api_members.rs` (and `tests/api_safety.rs`): request paths and bodies for kick, ban, timeout and its end, nickname, avatar removal, roles, the four voice edits, and the three reports, plus an error response turning into the right toast text.

### Manual Tests
1. Every chunk's manual check.
2. With a role between the two accounts' ranks: the lower account sees no moderation items for the higher one, and Stoat's refusal toasts if forced (stale menu).
3. Both themes: the dialogs are readable, and destructive buttons stand out.

## Definition of Done
- [ ] Kick and Ban (reason, history deletion) work from the member menu, and appear only with the permission, on a lower-ranked non-owner other than yourself (owner: anyone).
- [ ] Time out (six lengths) and End timeout work. The card shows "Timed out until …".
- [ ] A moderator can set or clear another member's nickname, remove their server avatar, and change their roles. Roles at or above their rank (and managed roles, if marked) can't be added.
- [ ] Server mute, server deafen, Move to and Disconnect work on someone in one of the server's calls, from the member menu and from voice participant rows.
- [ ] Report user, Report message and Report server send `POST /safety/report` with a reason and detail, and are not offered on your own content.
- [ ] Stoat's refusals (`NotElevated`, `IsElevated`, `MissingPermission`, `CannotReportYourself`, `NotConnected`) show plain toasts.
- [ ] `cargo test && cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
