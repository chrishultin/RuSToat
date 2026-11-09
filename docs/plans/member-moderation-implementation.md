# Implementation Record: Member moderation (milestone 22)

_Plan: docs/plans/member-moderation.md_
_Started: 2026-11-08_

**Mode:** continuous. The user asked for all manual checks to be batched at the end, so none was done per chunk.

**Pre-existing dirty files, kept out of every commit:** `.gitignore` (a duplicated Riptide `.humanlayer/tasks/` entry) and `.shadow/` (untracked). The plan file was untracked at the start and goes in with chunk 1.

## Chunk 1: Kick and ban

### Changes Made
- `src/permissions.rs`:
  - `member_rank(store, server, user)`: Stoat's `get_ranking`, the best role's rank, or `i64::MAX`.
  - `acting_rank(store, server)`: the user's rank, or `i64::MIN` for the owner.
  - `Moderation` (nine booleans) and `moderation(store, server, target)`, with every field worked out now.
  - `is_timed_out` made `pub`.
- `src/api/client.rs`: `kick_member` (`DELETE /servers/{s}/members/{u}`) and `ban_user` (`PUT /servers/{s}/bans/{u}`, nulls stripped). Imports `DataBanCreate` and `ServerBan`.
- `src/moderation.rs` (new): `BAN_REASON_MAX`, `BAN_HISTORY`, `moderation_error_text(err, fallback)`, with unit tests. Registered in `src/lib.rs` by hand.
- `src/profile.rs`:
  - `UserMenuItem::{Kick, Ban}` and `UserMenuItem::is_destructive`.
  - `user_menu` gets a moderation group after group membership, offered when the channel has a server and `moderation` allows it.
- `src/ui/moderation_dialogs.rs` (new; registered in `src/ui/mod.rs` by hand):
  - `confirm_member_action`: a confirmation that sends one request, stays busy until Stoat answers, then closes and toasts a refusal.
  - `run_member_action`: the same without asking, for Shift.
  - `open_ban_dialog` / `BanDialog`: a reason textarea counted to 1024, a "Delete message history" radio group, and a danger Ban button. A refusal toasts and keeps the dialog open.
- `src/ui/chat.rs`:
  - Labels: "Kick…" (`UserX`) and "Ban…" (`Ban`).
  - Destructive items are drawn in the danger colour, as "Leave server…" is.
  - `on_user_menu` arms; `ChatView::kick` ("Kick {name}?", Shift skips) and `ChatView::open_ban`.
- Tests:
  - `tests/permissions.rs`: `moderation_store(me)` fixture and 8 tests covering rank, rank order, the owner, self, owner targets, each permission, Timeout Members targets, and unknown memberships.
  - `tests/api_members.rs`: kick path; ban body with reason and history; a bare ban sends `{}`; `NotElevated` gives the rank toast.
  - `tests/profile.rs`: `store_owned_by(owner)` fixture. The two exact menu tests now include `Kick, Ban`, because the user owns the fixture server. New tests: presence rules, and `is_destructive`.

### Divergences
- **The plan's `confirm_then` is the MFA "Confirm it's you" dialog**, not a plain confirmation. Added `confirm_member_action` / `run_member_action` instead. The plan was amended in three places (its "What exists" notes, chunk 1's files, and chunk 2's End timeout).
- **Kick closes its confirmation when Stoat refuses, and toasts.** The group "Remove member" confirmation stays open instead, but retrying a refused kick doesn't help.
- **Look-ahead for later chunks:**
  - `Role` in revolt-models 0.16.2 has `owner: Option<String>` (managed by a bot), so chunk 3 can check managed roles.
  - gpui-component's `PopupMenu` has `submenu`, already used for server notifications, so chunk 4's "Move to ›" can be a real submenu.

### Verification Performed
- `cargo test`: 1102 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after removing one `clone_on_copy`.
- rustfmt drift is unchanged:
  - `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 14.
  - Clean files are still clean: `profile.rs`, `permissions.rs`, the new `moderation.rs` and `moderation_dialogs.rs`, and the three test files.

### Manual Validation
Deferred to the end at the user's request (kick, then ban with 1 hour of history).

## Chunk 2: Timeouts

### Changes Made
- `Cargo.toml` / `Cargo.lock`: `iso8601-timestamp = "0.2"` as a direct dependency, the crate revolt-models already uses for `Timestamp`.
- `src/moderation.rs`: `TIMEOUTS` (1 minute to 1 week), `timeout_until(now: SystemTime, seconds) -> Timestamp`, `timeout_edit(until)` and `end_timeout_edit()` (both via `profile_edit::empty_member_edit`), with unit tests:
  - the lengths;
  - 60 s after 2026-11-08T12:00:00Z serialises to `2026-11-08T12:01:00.000Z`, alone and in the edit;
  - ending a timeout removes `Timeout`.
- `src/profile.rs`:
  - `UserMenuItem::{TimeOut, EndTimeout}` lead the moderation group, chosen by `permissions::is_timed_out` on the cached member.
  - `ProfileCard::timed_out_until`, set only while the timeout runs and in a server context.
- `src/ui/moderation_dialogs.rs`:
  - `open_timeout_dialog` / `TimeoutDialog`: a "Duration" radio group with the first option chosen, and a Time out button.
  - `MemberActionRequest` gets `danger: bool`: Kick is destructive, End timeout isn't.
- `src/ui/chat.rs`:
  - Labels "Time out…" and "End timeout…" (`Clock`).
  - Arms `ChatView::open_timeout` and `ChatView::end_timeout` (confirms "End {name}'s timeout?", then `edit_member` with `remove: [Timeout]`).
- `src/ui/profile_card.rs`: a "Timed out until {format_until}" line, in the danger colour with a clock, above Status.
- Tests:
  - `tests/profile.rs`: fixture gains `u-timed` (until 2999) and `u-lapsed` (2001). The two exact menus now start the moderation group with `TimeOut`. New tests: the item swap, and the card line.
  - `tests/api_members.rs`: the timeout body as an RFC 3339 string; `{"remove":["Timeout"]}`; `IsElevated` gives the rank toast.

### Divergences
- **A new direct dependency** (`iso8601-timestamp`), already in the lock through revolt-models. revolt-models doesn't re-export `Timestamp`, so `timeout_until` can't name its return type without it.
- **The confirmation's button can be non-destructive** (`danger: false`, for End timeout).
- **The menu doesn't draw "Time out…" as destructive**: the web client marks only Kick and Ban.
- Later chunks are unaffected.

### Verification Performed
- `cargo test`: 1110 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift is unchanged (`client.rs` 3, `chat.rs` 17, `store.rs` 14). `profile_card.rs` and the other clean files are still clean.

### Manual Validation
Deferred to the end: time out for 1 minute, check the message box and card, then end it early.

## Chunk 3: Edit a member's identity and roles

### Changes Made
- `src/permissions.rs`:
  - `RoleLock { Rank, Managed }`.
  - `assignable_roles(store, server) -> Vec<(role_id, Option<RoleLock>)>`: highest first. A role is locked if it's a bot's (`role.owner`), or ranked at or above the user's `acting_rank`. The owner may add anything but a bot's role.
- `src/profile_edit.rs`: `IdentityForm::changes_for_other(loaded, edited)` sends the nickname (or its removal) and avatar removal only. Pronouns and setting an avatar are never sent. With a unit test.
- `src/profile.rs`: `UserMenuItem::{EditIdentity, EditRoles}` in a new server group after the people group.
  - Edit identity needs Manage Nicknames, or Remove Avatars on someone with a server avatar.
  - Edit roles needs Assign Roles.
  - Both also need the membership to be cached.
- `src/ui/identity_dialog.rs`: a new `MemberIdentityEditor` (title "Change {name}'s identity on {server}").
  - A nickname field, with their display name as placeholder; it's disabled without Manage Nicknames.
  - A "Remove their server avatar" checkbox beside their avatar, shown only if they have one and you have Remove Avatars.
  - Save is enabled only when something changed. Stoat's answer goes through `ChatView::identity_saved`, and a refusal toasts.
- `src/ui/moderation_dialogs.rs`: `open_roles_dialog` / `RolesDialog` ("Edit {name}'s roles").
  - The server's roles, highest first, each a checkbox with its colour dot and name, in a list that scrolls past 320 px.
  - A locked role is disabled with a tooltip, unless the member already has it, so it can still be taken away.
  - Save sends the whole `roles` list. Ticks start from the member's roles, so ones deleted from the server are kept as they are.
- `src/ui/chat.rs`:
  - Labels "Edit identity…" (`UserPen`) and "Edit roles…" (`IdCard`).
  - `ChatView::open_member_identity` and `ChatView::open_roles`.
  - `identity_saved`'s doc now covers other members' edits too (it already applied any member).
- Tests:
  - `tests/permissions.rs`: the fixture gains a managed `bot` role (rank 4). Two tests: locks for a mid-rank moderator; the owner, where only the bot's role is locked.
  - `tests/profile.rs`: the exact menus gain `EditIdentity, EditRoles`. A new test covers presence: their own menu, outside a server, an uncached membership, no permission.
  - `tests/api_members.rs`: nickname plus `remove: ["Avatar"]`; roles sent whole; an empty list clears them.

### Divergences
- **A separate `MemberIdentityEditor` rather than a generalised `IdentityEditor`** (Assumption 6's fallback). The own-identity editor is built around uploads, pronouns and the user's own permissions.
- **Edit identity and Edit roles need the target's membership to be cached.** Assumption 1 offers member-only items for uncached memberships and leaves the check to Stoat. But Edit roles sends the whole list, so opening it on an unknown membership would wipe their roles, and the nickname field would start blank. Kick, Ban and Time out still follow Assumption 1.
- **Managed roles are checked.** `Role.owner` exists in 0.16.2, which matches Stoat's `is_managed`.
- **`open_roles_dialog` takes `&ChatView` plus its weak handle** (as `CreateChannelDialog::new` does), to stay within clippy's argument limit.
- Later chunks are unaffected.

### Verification Performed
- `cargo test`: 1117 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after the argument-count fix above.
- rustfmt drift is unchanged (`client.rs` 3, `chat.rs` 17, `store.rs` 14). `identity_dialog.rs`, `profile_edit.rs` and the other clean files are still clean.

### Manual Validation
Deferred to the end:
- rename the second account (member list, messages and typing update);
- remove its server avatar;
- give it a role, then take it away;
- check that a role above yours is disabled.

## Chunk 4: Voice moderation

### Changes Made
- `src/store.rs`: `ChatStore::voice_channel_of(server, user)`, which of a server's voice channels someone is in a call in.
- `src/profile.rs`:
  - `UserMenuItem::{ServerMute(bool), ServerDeafen(bool), MoveTo, DisconnectVoice}` in a new voice group, after the server items and before group membership. The group appears only while the target is in one of the server's calls.
  - The bool is the action: `true` mutes or deafens, `false` undoes it. It's read from the cached member's `can_publish` / `can_receive`, which default to on.
  - Mute and deafen follow Mute Members and Deafen Members.
  - Move to (only if there's somewhere to go) and Disconnect follow Move Members.
  - `move_targets(store, server, call)`: the server's other voice channels, in its order, that the user may Connect to.
- `src/ui/chat.rs`:
  - Labels: "Server mute" / "Server unmute" (`MicOff` / `Mic`), "Server deafen" / "Server undeafen" (`HeadphoneOff` / `Headphones`), "Move to" (`Volume2`), "Disconnect from call" (`PhoneOff`).
  - `ChatView::edit_voice` sends the edit at once through `run_member_action`, and a refusal toasts.
  - `ChatView::move_member` sends `voice_channel`.
  - `People::menu` turns `MoveTo` into a real submenu (`submenu_with_icon`) of channel names from `move_menu_targets`.
  - Voice participant rows get `.context_menu(people.menu(..))`, with the call's channel as context, through a new `people` parameter to `render_voice_participant`.
- `src/assets.rs`: `people_icons_load` also checks the moderation icons (Clock, UserPen, Mic/MicOff, Headphones/HeadphoneOff, PhoneOff, Volume2).
- Tests:
  - `tests/profile.rs`: the fixture gains voice channels `ch-voice`, `ch-lounge` and `ch-locked` (Connect denied by default), with ADA, `u-blocked` (server muted and deafened) and `u-me` in `ch-voice`. Four tests:
    - the items and where their group sits;
    - the unmute and undeafen labels;
    - absence (not in a call, themselves, outside a server, no permission);
    - `move_targets` for the owner and for a plain member.
  - `tests/store_voice.rs`: `voice_channel_of`.
  - `tests/api_members.rs`: the four bodies (`can_publish`, `can_receive`, `voice_channel`, `remove: ["VoiceChannel"]`); `NotConnected` gives "They're no longer in a call."

### Divergences
- **No `UserMenuItem` carries the move's destination.** `MoveTo` only marks where the submenu goes, its arm in `on_user_menu` does nothing, and the submenu's items call `move_member` directly.
- **Mute, deafen and disconnect act at once, without asking**, as the web client does. The plan named no confirmation for them.
- **Live effect of a server mute (checked statically).** Nothing can panic: `Controls` only calls `mute()` / `unmute()` on its publication and has no unwraps. If LiveKit force-unpublishes the microphone, a later server unmute won't republish it, so the target probably needs to rejoin to be heard again. That fix is milestone 34 material, as the plan says; the manual pass should confirm it.
- Chunk 5 is unaffected.

### Verification Performed
- `cargo test`: 1124 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift is unchanged (`client.rs` 3, `chat.rs` 17, `store.rs` 14). `assets.rs`, `tests/store_voice.rs` and the other clean files are still clean.

### Manual Validation
Deferred to the end, with both accounts in a voice channel and the web client:
- server mute, then unmute;
- server deafen, then undeafen;
- move to the other channel;
- disconnect;
- check whether unmuting needs a rejoin.

## Chunk 5: Reports — done in milestone 23

**Implemented as chunk 7 of milestone 23** ([`server-settings.md`](server-settings.md); record in [`server-settings-implementation.md`](server-settings-implementation.md), "Chunk 7: Reports").

The user asked to commit and merge into main before chunk 5 was written, so it was deferred. What was learnt then:
- `ReportedContent`, `ContentReportReason` and `UserReportReason` are in `revolt_models::v0` (`safety_reports.rs`).
- Stoat's `DataReportContent` is defined only in the route (`crates/delta/src/routes/safety/report_content.rs`), so the client needs its own body type:
  - `content: ReportedContent`;
  - `additional_context: String` (default `""`, at most 1000).
- The web client (`packages/client/components/modal/modals/ReportContent.tsx`) has the reason lists, in its order, with "Other" (`NoneSpecified`) last and needing detail.
  - Content reasons: Illegal "Content breaks one or more laws", IllegalGoods "Drugs or illegal goods", IllegalExtortion "Extortion or blackmail", IllegalPornography "Revenge or underage pornography", IllegalHacking "Illegal hacking or cracking", ExtremeViolence "Extreme violence, gore or animal cruelty", PromotesHarm "Promotes harm", UnsolicitedSpam "Unsolicited advertising or spam", Raid "Raid or spam attack", SpamAbuse "Spam or similar platform abuse", ScamsFraud "Scams or fraud", Malware "Malware or phishing", Harassment "Harassment or cyberbullying".
  - User reasons: UnsolicitedSpam, SpamAbuse, InappropriateProfile "User's profile has inappropriate content", Impersonation "Impersonation", BanEvasion "Ban evasion", Underage "Not of minimum age to use the platform".
  - Titles: "Tell us what's wrong with this user/server/message"; field labels "Reason for report" and "Give us some detail".

## Summary

_Completed (chunks 1–4): 2026-11-08_

### Chunks Completed
1. Kick and ban: `e4a5866`.
2. Timeouts: `d2179ec`.
3. Edit a member's identity and roles: `7b37654`.
4. Voice moderation: `0f85b53`.
5. Reports: deferred at the user's request, then done as milestone 23's chunk 7.

### Divergences
- The plan's `confirm_then` is the MFA dialog. `moderation_dialogs::confirm_member_action` / `run_member_action` were added instead, and the plan was amended.
- Kick's confirmation closes and toasts when Stoat refuses.
- The confirmation's button can be non-destructive (End timeout).
- `iso8601-timestamp` is a direct dependency, to name `Timestamp`.
- `MemberIdentityEditor` is a separate dialog, rather than a generalised `IdentityEditor`.
- Edit identity and Edit roles need a cached membership, so a stale or unknown one can't wipe roles.
- Managed (bot) roles are locked, through `Role.owner`.
- `open_roles_dialog` takes `&ChatView` (clippy's argument limit).
- "Move to" is a real submenu; `MoveTo` carries no destination.
- Mute, deafen and disconnect act without asking.
- A server unmute may need the target to rejoin, because LiveKit drops their publication. This was found by reading the code, not tested live; it's milestone 34 material.

### Definition of Done
- [x] Kick and Ban (reason, history deletion) from the member menu, offered only with the permission, on a lower-ranked non-owner other than yourself (owner: anyone). Unit and wiremock tested; not yet checked by hand.
- [x] Time out (six lengths) and End timeout. The card shows "Timed out until …". Tested; not yet checked by hand.
- [x] Set or clear another member's nickname, remove their server avatar, change their roles. Roles at or above your rank, and managed roles, can't be added. Tested; not yet checked by hand.
- [x] Server mute, server deafen, Move to and Disconnect, from the member menu and the voice participant rows. Tested; not yet checked by hand.
- [ ] Report user, message and server: **not done** (chunk 5).
- [x] Stoat's refusals become plain toasts: `NotElevated`, `IsElevated`, `MissingPermission`, `NotConnected` (and `CannotReportYourself`'s text, ready for chunk 5).
- [x] `cargo test` (1124 passed) and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew.

### Remaining Manual Steps
Batched at the user's request, and **not yet done**. The work was merged into main before them.
1. Kick the second account. Have it rejoin, then ban it with "1 hour" of history.
2. Time it out for 1 minute: its message box refuses, and the card shows the timeout. End it early.
3. Rename it, remove its server avatar, and give and take away a role. Check that a role above yours is disabled.
4. With both accounts in a voice channel, plus the web client:
   - server mute and unmute (does unmuting need a rejoin?);
   - server deafen and undeafen;
   - move to the other channel;
   - disconnect (the target toasts "You were removed from the call.").
5. With a role between the two accounts' ranks: the lower account sees no moderation items for the higher one.
6. Both themes: the dialogs are readable, and destructive buttons stand out.
7. Chunk 5 (Reports) was implemented in milestone 23 (chunk 7), and is checked with that milestone's manual tests.
