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
