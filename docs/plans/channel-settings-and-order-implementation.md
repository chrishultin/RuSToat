# Implementation Record: Channel settings and order (milestone 26)

_Plan: [`channel-settings-and-order.md`](channel-settings-and-order.md)_
_Started: 2026-11-10_

Run in continuous mode. Manual checks are batched at the end, as the user asked. The plan and its roadmap link (uncommitted before chunk 1) were committed with chunk 1, as milestones 23–25 did.

Rustfmt drift at the start (`Diff in` hunks): `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3. `src/server_edit.rs`, `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/ui/server_dialogs.rs`, `src/permissions.rs`, `src/profile_edit.rs`, `src/ui/profile_editor.rs`, `src/assets.rs`, `tests/store_channels.rs`, `tests/api_servers.rs`, `tests/permissions.rs`, `tests/server_settings.rs` and `tests/store_mature.rs` were clean.

## Chunk 1: Channel Settings window: name, description and icon

### Changes Made
- `src/channel_settings.rs` (new, no GPUI):
  - `ChannelAccess { overview }` with `any()`, and `access(store, channel)`;
  - `ChannelKind { Channel, Group }` with `of(&Channel)` and `noun()`;
  - `window_title(Option<&Channel>)`: "Channel Settings — #{name}" or "Group Settings — {name}";
  - `ChannelForm { name, description, icon }` with `of`, `is_valid`, `new_file`, `has_changes` and `changes` (only what changed, trimmed; a blank description or a removed icon goes into `remove`);
  - `empty_channel_edit()`, `save_error_text` (Assumption 7), `DESCRIPTION_MAX`, `ICON_TAG` (`"icons"`), and `NAME_MAX` re-exported from `server_edit`.
- `src/permissions.rs`: `can_edit_channel` (a server channel or group, with Manage Channels in `channel_permissions`). `ChatStore::can_edit_channel` wraps it.
- `src/api/client.rs`: `edit_channel(channel, &DataEditChannel) -> Channel`, a `PATCH /channels/{id}` through `edit_body`. The type path is written out in full, so the drifted import block isn't touched.
- `src/store.rs`: `apply_channel_edit(&Channel) -> bool`. It takes only the name, description, icon, nsfw and (text channels) slowmode from the answer, and clears a missing description, icon or slowmode. A group keeps its owner, members and permissions.
- `src/ui/chat.rs`:
  - `open_channel_settings(channel, cx)` and `channel_edited(&Channel, cx)`, which applies the edit, calls `sync_pane`, and reloads the open channel (so a newly mature one is gated at once);
  - "Channel settings…" / "Group settings…" (`Settings` icon) in `channel_menu` after the Create items, when `can_edit_channel`;
  - the Channel info dialog passes a "Channel settings…" button when the user may edit the channel.
- `src/ui/server_dialogs.rs`: `render_info`'s `on_settings` is now `Option<(&'static str, OnSettings)>`, so the button's label is the caller's. Its ID is now `info-settings`.
- `src/ui/channel_settings.rs` (new):
  - `ChannelSettingsWindow` (a `Global`, one window, 680×520) with `open`, `open_new` and `close`;
  - `ChannelSettingsView` with `PAGES: [ChannelPageFn; 1]`. It closes when the channel goes, access is lost, or the chat is released, and it follows the window title;
  - `ChannelOverviewPage`: icon row (`avatar_preview`, round, like the sidebar's channel icons), name and description with counts, Reset and Save, per-field live-follow through `filled`, and upload-then-`PATCH` saving through `channel_edited`.
- `src/lib.rs`, `src/ui/mod.rs`: `pub mod channel_settings;` in each (by hand).
- Tests:
  - `tests/channel_settings.rs` (new, 14 tests): access for the owner, a server-wide manager, an override-only manager, a plain member, groups (default, and one whose permissions leave it out) and a DM; the title and nouns; the form's starting values; unchanged forms; trimmed names; validity limits; descriptions and icons (removed, replaced, picked); the edit never carries owner, nsfw, archived, voice or slowmode; the refusal texts;
  - `tests/api_channels.rs` (new, 3 wiremock tests): the exact `PATCH` bodies for a text channel and a group (no nulls, no empty `remove`), and a refusal's text;
  - `tests/store_channels.rs`: 4 tests. A saved edit then its `ChannelUpdate` are one state; a missing description or slowmode is cleared; a group keeps its owner, members and permissions; an unknown channel or a DM does nothing;
  - `tests/permissions.rs`: `can_edit_channel` for groups, and the same answers as `can_manage_channel` for server channels.

### Divergences
- **The icon preview is round** (`avatar_preview`), not square as Assumption 5 said, because the sidebar draws channel icons round, as Server Overview draws a server's.
- **The window is 680×520**, not 640×520, so the sidebar and the form fit side by side.
- **Nothing was needed for sign-out:** the window closes itself when its chat is released, as Server Settings does, so no `close` call was added elsewhere.
- **`channel_edited` doesn't call `forget_member_lists` or `drop_stale_selection`** as the live handler does: the form never changes permissions, so neither can change.

Look-ahead: chunk 2 adds slowmode to `ChannelForm` (and `changes`), and the mature button. Nothing else is affected. The plan needs no amendments.

### Verification Performed
- `cargo test --test channel_settings --test api_channels --test store_channels --test permissions`: 14, 3, 14 and 50 passed.
- Full suite: `cargo test` gave 1,299 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: the new files are clean. `src/ui/chat.rs` is still 17 (two new hunks were hand-formatted), `src/store.rs` 14 and `src/api/client.rs` 3. `src/permissions.rs`, `src/ui/server_dialogs.rs`, `tests/store_channels.rs` and `tests/permissions.rs` are still clean.

### Manual Validation
Deferred to the end (batched, as the user asked): rename a channel and a group, set and clear a description and icon, and check that a plain member has no "Channel settings…".
