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
