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
