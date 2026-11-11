# Plan: Channel settings and order (RuSToat milestone 26)

## Summary
Milestone 26 of the [roadmap](roadmap.md) adds two things:
- **Channel Settings**, a window of its own (like Server Settings), whose Overview page edits a channel's name, description, icon and slowmode, and marks it mature or not. Groups get the same page, without slowmode.
- **Channel order:** drag categories, and drag channels within and between categories, in the sidebar. A "Move to ›" submenu on a channel does the same without dragging.

Milestone 27 (channel permission overrides) adds a Permissions page to the same window, and milestone 28 a Webhooks page, as in the web client.

## Approach
**What Stoat allows.** Read from `/tmp/stoatchat-0.16.2` (tag `v0.16.2`) on 2026-11-10. Paths are under `crates/`.
- **Editing a channel:** `PATCH /channels/{id}` with `DataEditChannel` (`delta/src/routes/channels/channel_edit.rs`, `core/models/src/v0/channels.rs:172`).
  - It needs **Manage Channel on that channel** (channel permissions, so overrides count).
  - Fields: `name` (1–32), `description` (0–1024), `icon` (an Autumn file ID from the **`icons`** tag, as the web client uploads it), `nsfw`, `slowmode` (seconds, 0–21600), `remove: [Description | Icon | Slowmode | …]`, plus `owner`, `voice` and `archived`, which this milestone doesn't send.
  - It works for **server channels** (`TextChannel`, which includes voice channels in 0.16) and **groups**. Slowmode only applies to `TextChannel`; for a group it's ignored. A DM is `InvalidOperation`.
  - The answer is the whole `Channel`. `ChannelUpdate { data, clear }` follows. For a group, a new name, description or icon also posts a system message.
  - **In a group, Manage Channel** is the owner's, or a member's when the group's `permissions` allow it. `DEFAULT_PERMISSION_DIRECT_MESSAGE` (used when `permissions` is unset) includes it, so by default every member may edit a group.
- **Ordering:** only through the server's `categories` (`PATCH /servers/{id}` with `categories`, `delta/src/routes/servers/server_edit.rs`).
  - It needs **Manage Channel across the server** (`calculate_server_permissions`, no overrides).
  - The list is sent whole. A channel listed twice is `InvalidOperation`; unknown channel IDs are dropped.
  - `DataEditServer` has **no `channels` field**, so the order of channels outside every category (`server.channels`) can't be changed.

**What the web client does** (`/tmp/for-web-0.16.3`, tag `stoat-for-web-v0.16.3`, `packages/client/`):
- **`components/app/interface/settings/ChannelSettings.tsx`:** pages Overview, Permissions (Manage Permissions) and Webhooks, for server channels and groups.
- **`…/settings/channel/Overview.tsx`:**
  - icon, "Channel Name" (1–32, with a counter), "Channel Description" (up to 1024, "This channel is about..."), and, for text channels only, "Channel Slowmode";
  - slowmode is a select: Slowmode off, 5s, 10s, 30s, 1m, 5m, 10m, 30m, 1h, 2h, 6h. It sends `slowmode: n`, including 0;
  - a blank description goes into `remove`, and so does a removed icon;
  - Reset and Save;
  - **"Mark as Mature"** is a separate button below the form, with a confirmation (`modal/modals/ChannelToggleMature.tsx`):
    - title "Mark this channel as mature?" or "Unmark this channel as mature?";
    - text "Users will be asked to confirm their age before joining this channel." or "Users will no longer be asked to confirm their age before joining this channel. Please ensure the content is appropriate for all ages.";
    - buttons "Keep as is" and "Mark as mature" / "Unmark as mature". It sends `{ nsfw }` alone.
- **`src/interface/navigation/channels/ServerSidebar.tsx` and `ordering.ts`:**
  - **The `"default"` category.** Channels in a category with ID `"default"`, followed by every uncategorised channel, form the top section, which has no header and is always first.
    - Every drag in the web client writes this category (title "Default"): a channel drag sends every category, with `"default"` holding the top section's order; a category drag sends `["default", …the rest]`.
    - So any server reordered in the web client has a `"default"` category, which **RuSToat shows today as a "DEFAULT" header**.
  - Categories are dragged by their header, and channels by their row, within and between categories, with Manage Channel across the server. Dragging is off while a category is folded.
  - `applyChannelOrdering` keeps channels the user can't see in place around the visible ones.

**What exists in RuSToat** (as of 2026-11-10, after milestone 25, `874291c`):
- **`src/store.rs`:**
  - `ChannelSection { title, id, channels }` (56); `fold_sections` (66);
  - `server_channels(server)` (3660): uncategorised channels first (in `server.channels` order), then each category. It has no `"default"` case;
  - `category_of` (3711), `has_category` (3722), `set_categories` (3058);
  - `apply_channel_update(id, PartialChannel, clear)` (2085) already applies every field and `remove` for text channels and groups;
  - `apply_server_edit(&Server)` (3029) is the model for applying a `PATCH` answer at once;
  - `is_mature` (2729) and `slowmode` (3552).
- **`src/server_edit.rs`:** `with_channel_in`, `with_category_added`, `with_category_renamed` and `without_category`, the private `listed_once`, and `categories_error_text`. Its tests are in the same file.
- **`src/api/client.rs`:**
  - `edit_categories(server, &[Category]) -> Vec<Category>` (1160);
  - `edit_server(server, &DataEditServer) -> Server` (1190), using `edit_body` (1665) to drop nulls and an empty `remove`;
  - `transfer_group` (1071) is the only `PATCH /channels/{id}` so far;
  - `upload_file` and `ui::profile_editor::upload_image(client, file_server, tag, path)`.
- **`src/permissions.rs`:** `can_manage_channel` (188) is `in_server` plus Manage Channel on the channel, and is used for Delete channel. `can_manage_channels` (199) is server-wide.
- **`src/ui/server_settings.rs`:** `ServerSettingsWindow` (a `Global`, one window at a time, `open`/`open_new`/`close`), `ServerSettingsView` with `PAGES`, and `OverviewPage`. That page has image slots, name and description fields, live-follow through `filled`, `save` (upload, then `PATCH`, then `server_edited`), `save_failed`, and a dropdown-button menu for system messages.
- **`src/server_settings.rs`:** `OverviewForm` (`of`, `is_valid`, `new_files`, `has_changes`, `changes`), `image_change`, `ServerImage::tag`/`limit`, `empty_server_edit` and `save_error_text`.
- **`src/ui/chat.rs`:**
  - `channel_menu` (4177) and `category_menu` (4570);
  - `render_channel_list` (8965) and `render_channel_row` (9254), with no drag handlers yet;
  - `categories_changed` (7301), `move_into_category` (7224), `open_channel_info` (7448) and `info_content` (7459, text channels only);
  - the shared drag pieces `DropMark`, `with_drop_mark` (13636, which assumes a `gap_2` column), `DragPreview` (13692) and `DRAGGED_OPACITY`;
  - the Esc-cancels-drag hook (612).
- **Drag patterns:** the role list (`RolesPage` in `src/ui/server_settings.rs`, with logic in `src/roles.rs`: `RoleDrop`, `reorder`, `may_reorder`) and the server rail (`src/rail.rs`: `DropTarget`, `target_for`, `apply_drop`).
  - Both hit-test by hand in `on_drag_move` (top half before, bottom half after).
  - Both apply the change at once and roll back with a toast if Stoat refuses.
  - Roles block new drags while one is saving.
- **`src/ui/server_dialogs.rs`:** `render_info(content, on_link, on_settings, cx)` (789) shows "Server settings…" when given `on_settings`.

**The chosen shape** (user's choices on 2026-11-10):
- **A Channel Settings window**, separate from Server Settings, built the same way:
  - one at a time, its own `Global`;
  - pages from `PAGES` functions; this milestone has Overview only, and milestones 27 and 28 add theirs.
  - It's opened from "Channel settings…" (or "Group settings…") in the channel's right-click menu, and from a "Channel settings…" button in the Channel info dialog. It closes when the channel goes or the user loses Manage Channel on it.
  - Rejected: a Channels page inside Server Settings (it would grow `src/ui/server_settings.rs`, already 3,638 lines, and doesn't fit groups), and a dialog (milestone 27 needs pages).
- **Groups are included:** same window and page, titled "Group Settings", without slowmode.
- **The mature flag is its own button with a confirmation**, sent at once as `{ nsfw }` alone, with the web client's words.
- **The `"default"` category, as in the web client:**
  - a category with that ID is drawn as the top, headerless section, with every uncategorised channel after its own;
  - reordering the top section writes it;
  - a category drag keeps it first.
- **Ordering is by drag in the sidebar**, for anyone with Manage Channel across the server.
  - Categories are dragged by their header, and channels by their row.
  - The change shows at once and rolls back with a toast if Stoat refuses.
  - **Plus a "Move to ›" submenu** on a channel's right-click menu: "No category", then each category, putting the channel at the end of the chosen one.
- **Logic and view are split**, as milestone 25 did:
  - `src/channel_settings.rs` (new, no GPUI): who may edit, the form, the request and the error texts;
  - `src/channel_order.rs` (new, no GPUI): the server's layout, drop targets and the categories a move produces;
  - `src/ui/channel_settings.rs` (new): the window and its Overview page;
  - the drag wiring stays in `src/ui/chat.rs`, beside the sidebar it belongs to.

## Assumptions
1. **Who may edit a channel:** a new `permissions::can_edit_channel(store, user, channel)` is Manage Channel in `channel_permissions`, for a `TextChannel` or a `Group`, and false otherwise. `can_manage_channel` stays as it is, for Delete channel.
2. **The form** is `ChannelForm { name, description, icon: ProfileImage, slowmode: u64 }`:
   - `of(&Channel)` reads a missing description as "" and a missing slowmode as 0;
   - `changes(&Channel) -> Option<DataEditChannel>` sends only what changed, trimmed, as `OverviewForm::changes` does: a blank description or a removed icon goes into `remove`;
   - slowmode is sent as `slowmode: n`, including 0 (as the web client does), and only for a `TextChannel`;
   - `nsfw` is never in the form, because the mature button sends it alone.
3. **Words:**
   - window title "Channel Settings — #{name}" or "Group Settings — {name}";
   - fields "Channel icon" / "Group icon", "Channel name" / "Group name" ("A channel needs a name." / "A group needs a name."), "Channel description" / "Group description" with the placeholder "This channel is about…" / "This group is about…";
   - "Slowmode" with the help text "How long members wait between messages. Members with Bypass Slowmode don't wait.";
   - "Mature" with the current state ("Members are asked to confirm their age before opening this channel." or "Members aren't asked to confirm their age.") and the button.
4. **Slowmode choices** are the web client's: Off, 5 seconds, 10 seconds, 30 seconds, 1 minute, 5 minutes, 10 minutes, 30 minutes, 1 hour, 2 hours, 6 hours. A stored value that isn't one of them (set elsewhere) is shown as an extra choice, in `time::format_duration_short`'s words, so opening and saving the page never changes it.
5. **The icon** uses the `icons` tag and the server-icon size limit (`ImageLimits::server_icon`, which is Autumn's `icons` limit). It's previewed square, like a server icon, with the channel's `#` or group placeholder.
6. **Save** works like Server Overview's:
   - it uploads the icon first, then sends one `PATCH` built against the channel as stored then;
   - it applies the answer at once (`ChatStore::apply_channel_edit(&Channel)`, built like `apply_server_edit`, through a new `ChatView::channel_edited`), then resets the form and toasts "Saved.";
   - live-follow is per field through `filled`;
   - a failure keeps the edit and toasts.
7. **Refusals (`save_error_text`):**
   - `MissingPermission`: "You don't have permission to change this channel.";
   - `FailedValidation`: "Channel names are 1–32 characters, and descriptions at most 1024.";
   - `NotFound`: "That channel was deleted.";
   - anything else: `error_text(err, "Couldn't save the channel's settings.")`.

   The mature toggle uses the same texts, with the fallback "Couldn't change whether the channel is mature."
8. **The mature confirmation** is an alert dialog (`window.open_alert_dialog`, as `confirm_delete_channel` does) with the web client's title, text and buttons, plus "group" for "channel" in a group. Its confirm button waits, disabled, for the answer. The answer is applied at once.
9. **The top section and `"default"`** (`src/channel_order.rs`, used by `ChatStore::server_channels`):
   - The top section is the `"default"` category's channels, then every channel no category lists, in `server.channels` order. It keeps `id: None`, so it has no header, can't be folded, and has no category menu.
   - It shows even when only `"default"` holds its channels.
   - `category_of` returns `None` for `"default"`, so "Create channel…" from a top-section channel doesn't try to move the new channel into it.
   - **`"default"` is written only when it already exists, or when a move leaves the top section in an order Stoat wouldn't show without it** (one that differs from `server.channels` order). So moving a channel out to the end of the top section on a server without `"default"` doesn't create one if the server's order already puts it there.
   - When written, it's first in the list and titled "Default", as in the web client.
10. **What a move keeps:**
    - Channels the user can't see keep their places. A channel drop is anchored on a visible channel ID in the full list, so hidden IDs around it don't move.
    - Stale IDs (deleted channels) are left as they are; Stoat drops them.
    - Every channel is listed once (`listed_once`).
11. **Drop targets:**
    - **Dragging a channel:**
      - over a channel row, the top half drops before it and the bottom half after it;
      - over a category header, the top half drops at the end of the section above, so the top section stays reachable even when it's empty, and the bottom half drops at the start of that category;
      - a folded category takes drops on its header only;
      - dropping where nothing changes does nothing.
    - **Dragging a category** (by its header): over any other category's section (header and channels), the top half drops before it and the bottom half after it. Over the top section, it goes first among the categories.
    - The line is the accent line from `with_drop_mark`, adjusted for the channel list's tighter gap.
12. **Saving an order:**
    - it's applied at once with `set_categories`, then `edit_categories` is sent;
    - on success the answer is applied (`categories_changed`);
    - on failure the old categories go back, unless the store's categories changed meanwhile (an event from elsewhere), with the toast `categories_error_text`;
    - while one is saving, new drags and moves wait, as roles do.
13. **Esc cancels a channel or category drag**, through the existing keystroke hook beside `end_rail_drag`.
14. **"Move to ›"** shows on a server channel's menu with Manage Channel across the server, after the Create items.
    - It lists "No category", then each category by title. The channel's current place is ticked and does nothing.
    - It moves the channel to the end of the chosen section, through the same save as a drag.
    - If there are no categories, the submenu isn't shown.
15. **No drag in Home** (DMs and groups have no order to save).

## Plan

### Chunk 1: Channel Settings window: name, description and icon
**Goal:** someone with Manage Channel can rename a channel or group, and change its description and icon, from a Channel Settings window.
**Files likely touched:**
- `src/channel_settings.rs` (new, no GPUI):
  - `ChannelAccess { overview }` and `access(store, channel)` (Assumption 1);
  - `ChannelForm` with `of`, `is_valid`, `new_file`, `has_changes` and `changes` (Assumption 2), plus `empty_channel_edit()` (`DataEditChannel` has no `Default`);
  - `save_error_text` (Assumption 7) and `window_title`;
  - reuse `image_change` from `src/server_settings.rs` (make it `pub(crate)` there if needed) and `server_edit::NAME_MAX`/`valid_name`.
- `src/permissions.rs`: `can_edit_channel`.
- `src/api/client.rs`: `edit_channel(channel, &DataEditChannel) -> Channel`, a `PATCH /channels/{id}` through `edit_body`. Write the type paths in full, so the drifted import block isn't touched.
- `src/store.rs`: `apply_channel_edit(&Channel) -> bool`, building a `PartialChannel` and `clear` from the answer and calling `apply_channel_update`.
- `src/ui/chat.rs`:
  - `ChatView::channel_edited(&Channel, cx)`, which applies the edit and then reruns what the `ChannelUpdate` handler does (`sync_pane`, reload if the mature state changed);
  - `ChatView::open_channel_settings(channel, cx)`;
  - "Channel settings…" / "Group settings…" (icon `Settings`) in `channel_menu`, shown when `can_edit_channel`;
  - the Channel info dialog passes `on_settings` for a channel the user may edit.
- `src/ui/server_dialogs.rs`: `render_info` takes the settings button's label ("Server settings…" or "Channel settings…").
- `src/ui/channel_settings.rs` (new):
  - `ChannelSettingsWindow` (a `Global`, one window, 640×520) with `open`, `open_new` and `close`;
  - `ChannelSettingsView` with `PAGES: [ChannelPageFn; 1] = [overview_page]`, closing on channel gone, no access, or the chat released;
  - `ChannelOverviewPage`: icon row, name, description, Reset and Save, live-follow, and save/upload as Server Overview's.
- `src/ui/mod.rs`, `src/lib.rs`: the modules, by hand.
- `src/ui/app_root.rs` or wherever `ServerSettingsWindow::close` is called on sign-out or quit: close this window there too.
- `tests/channel_settings.rs` (new), `tests/api_channels.rs` (new), `tests/store_channels.rs`, `tests/permissions.rs`.

**Depends on:** None (milestone 25 is done)
**Risks:**
- **The `PATCH` answer for a group** is the whole group. `apply_channel_edit` must not clear fields the form never touches (`owner`, `recipients`, `permissions`). It should only apply name, description, icon, nsfw and slowmode, and clear only `Description`, `Icon` and (for text channels) `Slowmode`.
- **Two settings windows:** Server Settings and Channel Settings are separate globals, so both can be open at once. Check that closing on sign-out covers both, and that each follows its own chat.
- **`render_info`'s signature change** touches its other callers; there should be one (servers).
- `src/ui/chat.rs` has 17 rustfmt hunks and `src/store.rs` has 14: hand-format, and check the counts don't grow.

**How to verify:**
- `cargo test --test channel_settings`:
  - `ChannelForm::of` for a text channel and a group;
  - `changes` for each field: a trimmed name, a blank description going to `remove`, an icon removed (`remove`) or replaced (the new ID); nothing changed is `None`; an invalid name is `None`;
  - `access` for the owner, a member with and without Manage Channel (server-wide, and only through a channel override), a group's owner, a group member with default permissions, a group member whose group denies it, and a DM;
  - `save_error_text`.
- `cargo test --test api_channels`: the `PATCH` sends exactly the set fields (no nulls, no empty `remove`) with the token and parses a `TextChannel` and a `Group`; `MissingPermission` reads as its text.
- `cargo test --test store_channels`: `apply_channel_edit` then its `ChannelUpdate` are one state, and a group keeps its owner and recipients.
- Then the full suite.

**Manual check:**
- As the owner: rename a channel, give it a description and an icon, and the sidebar, header and Channel info follow at once. Do the same for a group.
- A plain member has no "Channel settings…".

**Shippable when:** a channel's or group's name, description and icon can be edited from a Channel Settings window, opened from its menu or its info dialog.

### Chunk 2: Slowmode and the mature flag
**Goal:** set a channel's slowmode, and mark a channel or group mature or not, from the Overview page.
**Files likely touched:**
- `src/channel_settings.rs`:
  - `SLOWMODE_CHOICES: [(u64, &str); 11]` and `slowmode_choices(stored) -> Vec<(u64, String)>` (Assumption 4);
  - `ChannelForm.slowmode` in `of` and `changes` (`TextChannel` only);
  - `mature_edit(nsfw) -> DataEditChannel`, `mature_prompt(kind, mature) -> (title, text, confirm)` and `mature_error_text`.
- `src/ui/channel_settings.rs`:
  - a "Slowmode" row with a dropdown button (as the system-messages rows), followed live like the other fields;
  - a "Mature" row with the state and the button, which opens the confirmation (Assumption 8), sends `mature_edit`, and applies the answer through `channel_edited`.
- `tests/channel_settings.rs`, `tests/api_channels.rs`, `tests/store_mature.rs` and/or `tests/store_slowmode.rs`.

**Depends on:** Chunk 1
**Risks:**
- **Marking the open channel mature** gates it for a user who hasn't confirmed their age. `channel_edited` must do what the live handler does (`ensure_current_loaded` and so on), so the age gate shows at once rather than after the event.
- **The slowmode badge and cooldown** read `store.slowmode`, so they follow the store. Check the composer's countdown isn't left running when slowmode is turned off.

**How to verify:**
- `cargo test --test channel_settings`:
  - the choices list, and a stored value that isn't a choice (say 45 s) added as an extra one;
  - `changes` sends `slowmode` only when it changed (including to 0), never for a group;
  - `mature_edit` is `{ nsfw }` alone;
  - the prompt texts for a channel and a group, mark and unmark.
- `cargo test --test api_channels`: the mature `PATCH` body is exactly `{"nsfw":true}`.
- `cargo test --test store_mature`: a saved `nsfw: true` makes `is_mature` true at once.
- Then the full suite.

**Manual check:**
- Set slowmode to 10 seconds: the header badge shows it, and a plain member counts down after sending. Turn it off.
- Mark a channel mature: the age gate shows for an account that hasn't confirmed. Unmark it.

**Shippable when:** slowmode and the mature flag can be changed from Channel Settings, and the chat follows at once.

### Chunk 3: The top section, `"default"`, and "Move to ›"
**Goal:** servers reordered in the web client look the same in RuSToat, and a channel can be moved between categories from its menu.
**Files likely touched:**
- `src/channel_order.rs` (new, no GPUI):
  - `DEFAULT_CATEGORY`;
  - `Layout { top: Vec<String>, categories: Vec<Category> }` and `layout(&Server) -> Layout` (Assumption 9);
  - `Layout::into_categories(&Server) -> Vec<Category>`, which writes `"default"` only when needed (Assumption 9);
  - `move_to_end(&Server, channel, section: Option<&str>) -> Option<Vec<Category>>`, where `None` means the top section and the result is `None` when nothing changes.
- `src/store.rs`: `server_channels` builds the top section from `layout`, and categories other than `"default"`; `category_of` ignores `"default"`.
- `src/ui/chat.rs`:
  - `ChatView::save_categories(server, new, window, cx)`, the optimistic save with rollback (Assumption 12), and a `categories_saving: Option<(String, Task<()>)>`;
  - the "Move to ›" submenu in `channel_menu` (Assumption 14).
- `src/server_edit.rs`: `listed_once` becomes `pub(crate)` (or moves to `channel_order`), so both use it.
- `tests/channel_order.rs` (new), `tests/store_channels.rs`.

**Depends on:** None (it doesn't touch chunks 1–2's files beyond `chat.rs`'s menu)
**Risks:**
- **`server_channels` feeds keyboard navigation, places, unread counts and folding.** The top section keeps `id: None`, so folding and the category menu don't apply. Check that channel navigation (⌥↑/⌥↓) walks the new order.
- **Creating `"default"` where the user didn't expect it:** Revite would show a "Default" header for it. That's the web client's behaviour too, and Assumption 9 keeps it to moves that need it.
- **Rollback racing an event:** only roll back if the store still has what was sent.

**How to verify:**
- `cargo test --test channel_order`:
  - `layout` with no categories, with categories, with `"default"` (its channels first, then uncategorised ones in server order), and with stale and hidden IDs;
  - `move_to_end` into a category, out to the top section (with and without `"default"`, and creating it only when the order needs it), to where it already is (`None`), and with a hidden channel kept in place;
  - every result lists each channel once.
- `cargo test --test store_channels`:
  - a server with `"default"` shows its channels in the top section, without a "Default" heading;
  - `category_of` ignores `"default"`.
- Then the full suite.

**Manual check:**
- On a server reordered in the web client, the top section matches the web client and there's no "DEFAULT" header.
- Move a channel into a category and back with "Move to ›", and the web client follows.

**Shippable when:** the sidebar draws `"default"` as the web client does, and "Move to ›" moves a channel between sections, saved at once with rollback.

### Chunk 4: Drag to reorder categories
**Goal:** drag a category by its header to put it before or after another.
**Files likely touched:**
- `src/channel_order.rs`: `CategoryDrop { Before(String), After(String), First }` and `move_category(&Server, dragged, &CategoryDrop) -> Option<Vec<Category>>`, which keeps `"default"` first and unchanged.
- `src/ui/chat.rs`:
  - `ChannelDrag { Category(String), Channel(String) }` as the drag value, and `ChannelDragState { dragged, target }` on `ChatView`;
  - `start_`, `end_`, `set_` and `…_drag_moved` methods like the rail's;
  - the header gets `on_drag` (with a `DragPreview` pill of the title) when `can_manage_channels` and nothing is saving;
  - each section gets `on_drag_move` hit-testing (Assumption 11), and the list gets `on_drop` and `on_mouse_up_out`;
  - a drop mark for the channel list's spacing (a variant of `with_drop_mark`, or an offset parameter);
  - Esc ends the drag (Assumption 13).
- `tests/channel_order.rs`.

**Depends on:** Chunk 3
**Risks:**
- **A click versus a drag on a header:** a header click folds the category. Check that a drag doesn't also fold it, and that a plain click still does.
- **`with_drop_mark` assumes `gap_2`:** the channel list uses `gap_0p5` within sections and `pt_3` above headers. The line must sit in the right gap; add an offset parameter rather than copying the function.
- **Hit-testing a section** needs the section's bounds, not just the header's. Put `on_drag_move` on the section's `v_flex`.

**How to verify:**
- `cargo test --test channel_order`: `move_category` before and after each neighbour, `First`, onto itself (`None`), with `"default"` present (kept first and unchanged) and absent (not created), and every channel kept where it was.
- Then the full suite.

**Manual check:**
- Drag a category above and below others; the line shows where it lands, and the web client follows. Esc cancels mid-drag.
- A member without Manage Channels can't drag.

**Shippable when:** categories can be reordered by dragging their headers, saved at once with rollback.

### Chunk 5: Drag channels within and between categories
**Goal:** drag a channel to any place in any section.
**Files likely touched:**
- `src/channel_order.rs`: `ChannelDrop { Before(String), After(String), EndOf(Option<String>), StartOf(String) }` and `move_channel(&Server, dragged, &ChannelDrop) -> Option<Vec<Category>>` (Assumptions 9–11). `move_to_end` becomes `move_channel(…, EndOf(…))`.
- `src/ui/chat.rs`: channel rows get `on_drag` (a pill with the `#` and name) and `on_drag_move` (before or after). Headers take channel drops (end of the section above, or start of this one). A folded category takes them on its header only.
- `tests/channel_order.rs`.

**Depends on:** Chunk 4
**Risks:**
- **A voice channel's row includes its participants.** The drag should start from the channel's own line, and the hit-test should use the whole wrapper's bounds.
- **Long lists:** the list may not scroll by itself while dragging. Check that scrolling with the trackpad works mid-drag. If it doesn't, "Move to ›" is the fallback, and say so in the record.
- **Hidden channels:** a drop anchored on a visible neighbour must not move hidden ones (the tests cover it; check one by hand with a private channel).

**How to verify:**
- `cargo test --test channel_order`:
  - `move_channel` before and after a neighbour in the same section;
  - into another category (`Before`/`After`/`StartOf`);
  - out to the top section (`EndOf(None)`, and `Before` a top channel), with `"default"` written only when needed;
  - onto itself (`None`);
  - hidden and stale IDs kept;
  - each channel listed once.
- Then the full suite.

**Manual check:**
- Reorder channels in the top section and in a category; move one between categories, into an empty category, and back to the top. The web client shows the same order.
- A drop the server refuses rolls back with a toast.

**Shippable when:** channels can be dragged to any place in any section, saved at once with rollback.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–25):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start (2026-11-10, `874291c`):
  - `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3;
  - `src/server_edit.rs`, `src/server_settings.rs`, `src/ui/server_settings.rs`, `src/ui/server_dialogs.rs`, `src/permissions.rs`, `src/profile_edit.rs`, `src/ui/profile_editor.rs`, `src/assets.rs`, `tests/store_channels.rs`, `tests/api_servers.rs`, `tests/permissions.rs`, `tests/server_settings.rs` and `tests/store_mature.rs` are clean.
- Check any other file's count with `git show HEAD:<file> | rustfmt --edition 2024 --check | grep -c '^Diff in'` before formatting it whole.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/channel_settings.rs` (new): `access`, `ChannelForm` (`of`, `is_valid`, `changes` per field, groups without slowmode), the slowmode choices, `mature_edit`, the prompts and the error texts.
- `tests/channel_order.rs` (new): `layout`, `move_to_end`, `move_category` and `move_channel`, including `"default"` (kept first, written only when needed), hidden and stale IDs, and each channel listed once in every result.
- `tests/permissions.rs`: `can_edit_channel` for server channels (with and without overrides), groups (owner, default member, a group that denies it) and DMs.

### Integration Tests
wiremock tests (`tests/api_channels.rs`, new) against the exact route, headers and bodies:
- `PATCH /channels/{id}` with only the set fields, answering a `TextChannel` or a `Group`;
- the mature body `{"nsfw":…}`;
- slowmode `0`.

Failure cases that become plain toasts: `MissingPermission`, `FailedValidation`, `NotFound`. `PATCH /servers/{id}` with categories is already covered in `tests/api_servers.rs`; add one case for a reordered list with `"default"` first.

Store (`tests/store_channels.rs`, `tests/store_mature.rs`):
- a saved edit, then its `ChannelUpdate`, are one state;
- a group keeps its owner and recipients;
- `is_mature` and `slowmode` follow at once;
- the top section with `"default"`.

### Manual Tests
Run with two accounts and the web client, on a test server owned by the first account, plus a group. Batched at the end, as the user asked:
1. **Overview:** rename a channel, set and clear its description, set and remove its icon. The sidebar, header, Channel info and the web client follow. Do the same for a group (system messages appear in the group).
2. **Access:** a plain member has no "Channel settings…"; a member given Manage Channel through one channel's override has it for that channel only; a group member has "Group settings…" by default.
3. **Live changes:** rename the channel in the web client while its settings are open. An untouched name follows, and an edited one is kept. Deleting the channel closes the window.
4. **Slowmode:** set 10 seconds, and the second account counts down; a value of 45 s set with `curl` (`PATCH /channels/{id}` with `{"slowmode":45}`) shows as its own choice; turn it off.
5. **Mature:** mark and unmark a channel and a group, with the confirmation. The second account sees the age gate at once.
6. **`"default"`:** a server reordered in the web client looks the same in RuSToat, with no "DEFAULT" header.
7. **Move to ›:** move a channel into a category, then to "No category", and the web client follows.
8. **Drag categories:** reorder by dragging headers; Esc cancels; a folded category still drags.
9. **Drag channels:** within a section, between categories, into an empty category, and back to the top; the web client shows the same. With a private channel the moderator can't see, the order of the rest changes and the private one stays put (check as the owner).
10. **Refusal:** remove Manage Channels from the moderator in the web client while they're mid-drag. The drop rolls back with "You can't manage channels in this server."
11. **Themes:** the settings window, the drop line and the drag pill are readable in light and dark.

## Definition of Done
- [ ] "Channel settings…" (or "Group settings…") opens one Channel Settings window, from the channel's menu or its info dialog, for anyone with Manage Channel on that channel or group, and it closes when that's lost or the channel goes.
- [ ] Name, description and icon save in one `PATCH /channels/{id}` (after the icon's upload), with only what changed, and show at once.
- [ ] Slowmode offers the web client's choices plus any other stored value, and is sent only for server channels.
- [ ] The mature button asks first, sends `{ nsfw }` alone, and the age gate follows at once.
- [ ] A `"default"` category is drawn as the top, headerless section, and is written only when an order needs it.
- [ ] Categories and channels can be dragged into any order and section, and "Move to ›" moves a channel to the end of a section. Each saves at once and rolls back with a toast if refused; hidden channels keep their places.
- [ ] Refusals read plainly: `MissingPermission`, `FailedValidation`, `NotFound`, and the categories' own texts.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan.
