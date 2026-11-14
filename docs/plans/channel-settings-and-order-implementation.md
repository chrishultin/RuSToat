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

## Chunk 2: Slowmode and the mature flag

### Changes Made
- `src/channel_settings.rs`:
  - `ChannelForm.slowmode: u64` (0 for unset and for groups), read by `of` and sent by `changes` only for a server channel whose slowmode changed (0 included);
  - `SLOWMODE_CHOICES` (the web client's 11), `slowmode_label` (a choice's words, else `format_duration_short`) and `slowmode_choices(current)`, which adds a stored value that isn't a choice in its place;
  - `mature_edit(bool)` (`nsfw` alone), `MaturePrompt` and `mature_prompt(kind, mature)` (the web client's words, "group" for a group), `mature_state` (the row's line) and `mature_error_text`.
- `src/ui/channel_settings.rs`, `ChannelOverviewPage`:
  - `slowmode` joins the form's state, `reset` and per-field `follow`;
  - a "Slowmode" row (server channels only) with a dropdown button. It offers the choices plus the stored value, and the chosen one if it's an older stored value;
  - a "Mature" row below Save, under a border, with the current state and "Mark as mature…" / "Unmark as mature…";
  - `confirm_mature` opens an alert dialog ("Keep as is", and the confirm button that waits while sending), and `set_mature` sends `mature_edit`, applies the answer through `channel_edited` (which gates or ungates the open channel), and closes the dialog. A refusal toasts `mature_error_text` and leaves the dialog open;
  - the page's search keywords gain slowmode, mature and nsfw.
- Tests:
  - `tests/channel_settings.rs`: 8 new tests (22 in all). Slowmode's starting values; a changed slowmode sent including off; a group never sends one; the choices and labels; a stored value off the list offered in its place; `mature_edit`; the prompts and state lines; the mature refusal texts. The fixture's `ch-b` gained `slowmode: 45`;
  - `tests/api_channels.rs`: 2 new wiremock tests (the mature body is exactly `{"nsfw":true}`, and slowmode off is `{"slowmode":0}`);
  - `tests/store_mature.rs`: a saved mature flag shows at once, both ways.

### Divergences
- **The slowmode risk didn't apply:** `send_block_left` already treats a channel without slowmode as unblocked, so the composer's countdown stops once the store says it's off.
- **No `tests/store_slowmode.rs` change:** chunk 1's `a_saved_channel_edit_then_its_event_are_one_state` and `a_saved_edit_clears_what_the_answer_lacks` already cover slowmode in the store.
- The mature button's labels end with "…", since they open a confirmation.

Look-ahead: chunks 3–5 don't touch Channel Settings. No amendments.

### Verification Performed
- `cargo test --test channel_settings --test api_channels --test store_mature`: 22, 5 and 3 passed.
- Full suite: `cargo test` gave 1,310 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: the new files (`src/channel_settings.rs`, `src/ui/channel_settings.rs`, `tests/channel_settings.rs`, `tests/api_channels.rs`) are clean, and `tests/store_mature.rs` is still clean.

### Manual Validation
Deferred to the end (batched): set slowmode to 10 seconds and back; mark and unmark a channel mature, and check the age gate.

## Chunk 3: The top section, `"default"`, and "Move to ›"

### Changes Made
- `src/channel_order.rs` (new, no GPUI):
  - `DEFAULT_CATEGORY` (`"default"`) and `DEFAULT_TITLE` (`"Default"`);
  - `Layout { top, categories }` and `layout(&Server)`. The top section is `"default"`'s channels, then every channel no category lists, in server order. The other categories keep every ID they list, hidden and stale ones included;
  - `Layout::into_categories(server)`: the list to send. `"default"` is written first (keeping its stored title) when it already exists or when the top section's order differs from the server's own order of the uncategorized channels. Each channel is listed once;
  - `move_to_end(server, channel, section)`: `None` when nothing changes or the channel or category is unknown. It compares layouts, so a no-op never sends;
  - `listed_once` (moved here from `src/server_edit.rs`, which now imports it).
- `src/store.rs`:
  - `server_channels` builds the top section and the categories from `layout` (`"default"` never gets a heading);
  - `category_of` returns `None` for `"default"`, so "Create channel…" from a top-section channel doesn't try to move the new one into it.
- `src/ui/chat.rs`:
  - `categories_saving: Option<Task<()>>`;
  - `save_categories(server, edited, window, cx)` applies the order at once (`categories_changed`), sends `edit_categories`, applies the answer, or on failure restores the old categories (only if the store still shows what was sent) and toasts `categories_error_text`. While one is saving, another is ignored;
  - `move_channel_to(server, channel, section, window, cx)` (through `move_to_end`);
  - "Move to ›" in `channel_menu`, after the Create items, with Manage Channels across the server and at least one category: "No category", then each category, the current one ticked (and doing nothing).
- `src/lib.rs`: `pub mod channel_order;` (by hand).
- Tests:
  - `tests/channel_order.rs` (new, 11 tests): layouts without categories, with categories, with `"default"` anywhere in the list, and with hidden or stale IDs; moving into a category and between categories; moving out with and without writing `"default"`; an existing `"default"` kept first with its title; no-ops; unknown channel or category; hidden channels kept in place; each channel listed once;
  - `tests/store_channels.rs`: 2 tests. `"default"` is the headless top section with its own channels first, and `category_of` ignores it; a `"default"` of only gone channels shows nothing;
  - `tests/api_servers.rs`: a move out of a category sends `"default"` first, exactly.

### Divergences
- **`save_categories` takes no `(server, Task)` pair:** one task field is enough, since only one save runs at a time across servers.
- **The rollback compares the whole stored list** with what was sent, rather than a separate counter, as the plan's Assumption 12 says.
- **A channel already where "Move to" points** is ticked and does nothing, rather than being disabled, so the tick stays readable.

Look-ahead: chunks 4–5 reuse `layout`, `into_categories`, `save_categories` and `categories_saving`. Chunk 5's `move_channel` can be built on `Layout::take` plus an insert at a position. No amendments.

### Verification Performed
- `cargo test --test channel_order --test store_channels --test api_servers`: 11, 16 and 19 passed.
- Full suite: `cargo test` gave 1,324 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` is still 17 (two new hunks were hand-formatted) and `src/store.rs` 14. `src/server_edit.rs`, `src/channel_order.rs`, `tests/channel_order.rs`, `tests/store_channels.rs` and `tests/api_servers.rs` are clean.

### Manual Validation
Deferred to the end (batched): a server reordered in the web client shows no "DEFAULT" header and the same top section; "Move to ›" into a category and back.

## Chunk 4: Drag to reorder categories

### Changes Made
- `src/channel_order.rs`:
  - `CategoryDrop { Before(id), After(id) }`;
  - `category_target(shown, dragged, hovered, fraction)`: the top half of a section is before it and the bottom half after it. Over the top section it's before the first category. `None` over itself or where it already is;
  - `move_category(server, dragged, &target)`: reorders the full list (hidden categories keep their places relative to the anchors), keeps `"default"` first, and doesn't add it. `None` when nothing changes or either category is unknown.
- `src/ui/chat.rs`:
  - `ChannelDrag::Category(id)` (the drag value, with `category()`) and `ChannelDragState { dragged, target }`, held in `ChatView.channel_drag`;
  - `channel_dragging`, `start_channel_drag`, `end_channel_drag`, `set_channel_target`, `channel_list_drag_moved` (off the list, no target), `section_drag_over` (hit-tests the whole section, header and channels), `drop_on_channel_list` (through `move_category` and chunk 3's `save_categories`), `section_mark` and `may_drag_channels` (a server, Manage Channels across it, nothing saving);
  - a category header is draggable (`on_drag`, before its `context_menu`) when `may_drag_channels`, with a pill preview (`list_drag_face`: the `Folder` icon and the title);
  - each section is `relative`, takes `on_drag_move`, dims to `DRAGGED_OPACITY` while it's the one dragged, and shows the line;
  - the list takes `on_drag_move`, `on_drop` and `on_mouse_up_out` for `ChannelDrag`;
  - Esc ends a channel-list drag too (`end_rail_drag(cx) | end_channel_drag(cx)` in the keystroke hook);
  - `with_drop_line(item, mark, gap, inset, cx)`, the line part of `with_drop_mark`, which now calls it with 8 px and 12 px. The sidebar uses 2 px (its `gap_0p5`) and 4 px.
- `tests/channel_order.rs`: 6 new tests (17 in all). Targets before and after, over the top section, and no-op targets (itself, its neighbours' near halves, already first, nothing shown, unknown); moves after and before; no-op and unknown moves; `"default"` kept first and not added; a hidden category keeps its place.

### Divergences
- **No `First` variant:** over the top section the target is `Before(first shown category)`, which is the same thing and draws the line on that section.
- **`with_drop_line` is a new function** that `with_drop_mark` delegates to for lines, rather than an offset parameter on `with_drop_mark`, so the rail's and roles' calls stay as they were.
- **The drag preview is a pill** (`list_drag_face`), not a copy of the header, which would be too faint at the preview's opacity.

Look-ahead: chunk 5 adds `ChannelDrag::Channel`, a channel target type alongside `CategoryDrop` in `ChannelDragState`, and row-level hit-testing. No amendments.

### Verification Performed
- `cargo test --test channel_order`: 17 passed.
- Full suite: `cargo test` gave 1,330 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` is still 17 (one new hunk was hand-formatted). `src/channel_order.rs` and `tests/channel_order.rs` are clean.

### Manual Validation
Deferred to the end (batched): drag categories above and below others; Esc cancels mid-drag; a click still folds; a member without Manage Channels can't drag.

## Chunk 5: Drag channels within and between categories

### Changes Made
- `src/channel_order.rs`:
  - `ChannelDrop { Before(id), After(id), EndOf(Option<section>), StartOf(category) }`;
  - `ChannelHover { Channel(id), Heading { category, above } }` and `channel_target(hovered, fraction)` (the geometry only): over a row, the top half is before it and the bottom half after it; over a heading, the top half is the end of the section above (the top section when there's none) and the bottom half the start of that category;
  - `move_channel(server, channel, &target)`. A drop beside a channel is placed by that channel's ID in the full layout, so hidden channels don't move. `None` when nothing changes, or the channel, its neighbour or the category is unknown. `"default"` is written only when needed (chunk 3's rule);
  - `move_to_end` is now `move_channel(…, EndOf(…))`, and `Layout::find_mut` finds a channel's section and position.
- `src/ui/chat.rs`:
  - `ChannelDrag::Channel(id)` (with `channel()`), and `ListDrop { Category, Channel }` as the drag state's target;
  - `channel_drag_over(hovered, event, cx)` shows a target only if `move_channel` would change something; `section_drag_over` now only handles category drags;
  - `drop_on_channel_list` matches the drag with its target (`move_category` or `move_channel`);
  - `row_mark` (Before → above, After → below) and `heading_mark` (EndOf the section above → above the heading, StartOf → below it);
  - `render_channel_row` makes a server channel's own row draggable when `may_drag_channels`, with a pill preview of its icon and name. In the list, each row sits in a `relative` wrapper that takes `on_drag_move`, shows the line, and dims while dragged; the wrapper covers a voice channel's participants. Each heading is wrapped likewise, and knows the section shown above it.
- `tests/channel_order.rs`: 8 new tests (25 in all). The geometry; moves within the top section (writing `"default"`) and within a category; between categories and into the top section; into an empty category; no-ops (each neighbour's near side, the start of its own category, the end of its own section, itself); unknown neighbours or categories; hidden channels kept in place; every drop over every channel lists each once and keeps a stale ID.

### Divergences
- **Folded categories aren't special-cased:** a folded category's heading takes drops (its start, or the end of the section above), and any rows it still shows (the open channel, or one with mentions) take them too. Disallowing those rows would only hide valid targets.
- **`channel_target` is geometry only.** Whether a drop changes anything is `move_channel`'s answer, which the view checks before showing a line, so no-ops show nothing.
- **Hidden-channel no-ops:** dropping a channel just past a hidden one moves it in the full list, so it's sent even though the visible order looks the same. It's harmless, and rare.
- **Scrolling while dragging** isn't verified yet (manual check). If the list doesn't scroll mid-drag, "Move to ›" is the fallback.

### Verification Performed
- `cargo test --test channel_order`: 25 passed.
- Full suite: `cargo test` gave 1,338 passed, 0 failed. `cargo clippy --all-targets -- -D warnings` is clean.
- Rustfmt: `src/ui/chat.rs` is still 17 (one new hunk was hand-formatted). `src/channel_order.rs` and `tests/channel_order.rs` are clean.

### Manual Validation
Deferred to the end (batched): reorder within sections, between categories, into an empty category and back to the top; scrolling mid-drag; a refused drop rolls back.

## Summary

_Completed: 2026-11-11_

### Chunks Completed
1. Channel Settings window: name, description and icon (`76fe078`)
2. Slowmode and the mature flag (`a871c71`)
3. The top section, `"default"`, and "Move to ›" (`df1fd25`)
4. Drag to reorder categories (`30cce4e`)
5. Drag channels within and between categories (this commit)

### Divergences
- Chunk 1: the icon preview is round, like the sidebar's. The window is 680×520. No sign-out hook was needed. `channel_edited` skips what can't change.
- Chunk 2: the slowmode countdown risk didn't apply. Store slowmode coverage came from chunk 1. The mature buttons end with "…".
- Chunk 3: one task field for saving orders. The rollback compares the whole list. "Move to"'s current section is ticked and does nothing.
- Chunk 4: no `First` variant. `with_drop_line` is new beside `with_drop_mark`. The preview is a pill.
- Chunk 5: folded categories aren't special-cased. `channel_target` is geometry only. Hidden-channel no-ops may send. Scrolling mid-drag is unverified.
- Process: commits are signed through Secretive (Touch ID), and the chunk 2 commit waited for the user to approve it.

### Definition of Done
- [x] "Channel settings…" (or "Group settings…") opens one Channel Settings window, from the channel's menu or its info dialog, for anyone with Manage Channel on that channel or group, and it closes when that's lost or the channel goes.
- [x] Name, description and icon save in one `PATCH /channels/{id}` (after the icon's upload), with only what changed, and show at once.
- [x] Slowmode offers the web client's choices plus any other stored value, and is sent only for server channels.
- [x] The mature button asks first, sends `{ nsfw }` alone, and the age gate follows at once.
- [x] A `"default"` category is drawn as the top, headerless section, and is written only when an order needs it.
- [x] Categories and channels can be dragged into any order and section, and "Move to ›" moves a channel to the end of a section. Each saves at once and rolls back with a toast if refused; hidden channels keep their places.
- [x] Refusals read plainly: `MissingPermission`, `FailedValidation`, `NotFound`, and the categories' own texts.
- [x] `cargo test` (1,338) and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [x] The roadmap links this plan (committed with chunk 1).

### Remaining Manual Steps
The plan's Manual Tests 1–11, batched at the end as the user asked (two accounts and the web client): overview; access; live changes; slowmode; mature; `"default"`; Move to; drag categories; drag channels (with a hidden channel); refusal rollback; themes. Also check that the channel list scrolls while dragging.
