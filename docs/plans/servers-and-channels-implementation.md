# Implementation Record: Servers and channels (RuSToat milestone 19)

_Plan: docs/plans/servers-and-channels.md_
_Started: 2026-11-07_

Pre-existing uncommitted changes, kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt `Diff in` counts (2026-11-07): `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15. Zero in `src/permissions.rs`, `src/main.rs`, `src/ui/actions.rs`, `src/ui/invites.rs`, `src/invites.rs`, `src/ui/profile_card.rs`, `tests/store_servers.rs`, `tests/permissions.rs` and `tests/store_ready.rs`.

Supervision: continuous, unattended overnight; manual checks batched at the end.

## Chunk 1: Create a server

### Changes Made
- `src/api/client.rs`: `create_server(name) -> CreateServerLegacyResponse` (`POST /servers/create`), with a private body holding only `name`. `CreateServerLegacyResponse` added to the hand-formatted import list.
- `src/server_edit.rs` (new, pure):
  - `NAME_MAX` (32) and `AUP_URL`
  - `valid_name(text)`: trimmed, 1–32 characters by `chars().count()`
  - `create_server_error_text(&ApiError)`: `TooManyServers` and `CantCreateServers` in their own words, else `profile_edit::error_text` with "Couldn't create the server. Try again."
  - 2 unit tests.
- `src/ui/server_dialogs.rs` (new): `CreateServerDialog` and `CREATE_SERVER_DIALOG_WIDTH` (420).
  - A "Server name" field with a "{n}/32" counter (red past 32), the error line, Close and Create (`loading` while creating, disabled until the name is valid). Enter creates.
  - On the official instance (`web_app` is `https://stoat.chat`), "By creating this server, you agree to the Acceptable Use Policy.", with the policy's name a link (`InteractiveText`) to `https://stoat.chat/aup`.
  - Success hands the response to `ChatView::server_created` and closes the dialog; no toast.
- `src/ui/chat.rs`:
  - `server_added(server, channels, cx)`: `add_server(server, channels, [])`, then `fetch_own_member` if the membership isn't known. `invite_joined`'s server branch now uses it.
  - `open_create_server(window, cx)` (title "Create a server", the field focused) and `server_created(response, cx)`, which adds the server and opens its first channel in sidebar order (General), or the server if there's none.
  - `fetch_own_member` skips a server whose fetch is already on its way (`own_member_fetches`), so the response and its `ServerCreate` ask once. This applies to invite joins too.
  - The rail's "+" is now `rail-add` (tooltip "Create or join a server"). A left click opens a menu beside the rail: "Create a server…" (`Plus`) and "Join a server…" (`LogIn`).
    - `rail_button`'s round face is factored out as `rail_icon(id, content, tooltip, selected, cx) -> Stateful<Div>`; `rail_button` is unchanged for callers.
    - `RailMenuButton` wraps that face so it can be a `DropdownMenu` trigger (it implements `Selectable`, `Styled`, `InteractiveElement` and `IntoElement`), and keeps the hover look while the menu is open. The menu is anchored `Anchor::LeftCenter`, which opens it to the right of the button.
- `src/ui/actions.rs`: the `CreateServer` action. `src/main.rs`: "Create Server…" before "Join Server…" in the Stoat menu, routed through `root.shown_chat()` and doing nothing without one.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- `tests/api_servers.rs` (new): create sends exactly `{"name":"Lounge"}` with the token and parses a server with one "General" channel and no categories; a 400 `TooManyServers { max: 100 }` stays an `Api` error.
- `tests/store_servers.rs`: a create response (no `categories` key) then its `ServerCreate`, and the reverse. Both give one rail entry, at the end, with General.

### Divergences
- **The "+" isn't a ghost `Button`.** gpui-component's `DropdownMenu` needs a `Selectable` trigger, which a `div` isn't, so (rather than the plan's fallback of a `Button` styled like the rail) a small `RailMenuButton` wrapper makes the rail's own face selectable. The "+" looks and behaves exactly like the other rail buttons, including the hover morph from circle to rounded square.
- **`fetch_own_member` now dedupes in-flight fetches.** The plan's Risks wanted one fetch for the response and the event. Before, both could start one, because the membership isn't known until the first returns.
- **The dialog's title is "Create a server"** (the web client says "Create server"), to match the menu item and "Join a server".
- **The test fixtures carry `approximate_member_count`,** which `revolt-models`' `Server` requires. Stoat sends it in the real response.

### Verification Performed
- `cargo test --lib server_edit`: 2 passed. `cargo test --test api_servers --test store_servers`: 2 and 8 passed. The first run of `creating_a_server_sends_only_its_name` failed on a fixture missing `approximate_member_count`, which was fixed in the fixture.
- `cargo test`: 906 passed, 0 failed (900 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts:
  - `src/api/client.rs` 3 and `src/ui/chat.rs` 17: unchanged, after hand-formatting `create_server` and moving the new `use` into order.
  - `src/main.rs` 0 and `src/ui/actions.rs` 0.
  - New files (`src/server_edit.rs`, `src/ui/server_dialogs.rs`, `tests/api_servers.rs`) 0, and `tests/store_servers.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Click the rail's "+": a menu with "Create a server…" and "Join a server…" opens to the right of the rail, not clipped by the window edge (try with the window short enough that the "+" is near the bottom).
2. Create a server: it appears at the end of the rail, General opens, and you can send a message.
3. Stoat → Create Server… opens the same dialog. With the login view on screen, it does nothing.
4. "+" → Join a server… still opens Join a server, and joining still works.
5. The Create button is disabled for an empty or blank name and past 32 characters; the counter shows the count and turns red past 32.
6. On stoat.chat, the Acceptable Use Policy line shows and its link opens `https://stoat.chat/aup` in the browser. On another instance, it doesn't show.
7. Light and dark themes: the dialog, the counter and the link are readable.

## Chunk 2: Delete a channel, and follow deletions made elsewhere

### Changes Made
- `src/api/events.rs`: `ServerEvent::ChannelDelete { id }`, parsed and forwarded, with a parse test.
- `src/api/client.rs`: `delete_channel(channel)` (`DELETE /channels/{id}`, no body, returns `()`).
- `src/permissions.rs`: `can_manage_channel(store, user, channel)`: a server channel, and Manage Channels in `channel_permissions`. `src/store.rs`: the `ChatStore::can_manage_channel(channel)` wrapper.
- `src/store.rs`:
  - `remove_channel(id) -> bool`, for any kind of channel: takes it out of `channels`, its server's `channels` and every category's `channels` (the server is read from the removed channel), and drops its history, read state and pending messages. `false` if it wasn't known, so the second of the response and the event does nothing.
  - The per-channel part of `remove_server` is now the private `drop_channel_state(id)`, shared by both.
- `src/server_edit.rs`: `delete_channel_error_text(&ApiError)`: `MissingPermission` is "You can't delete this channel.", else `profile_edit::error_text` with "Couldn't delete the channel. Try again." One unit test.
- `src/ui/chat.rs`:
  - `forget_channel(channel, cx)`: what `leave_server` cleared per channel (pane, loading flags and errors, jump, banner, `pending_ack`). `leave_server` now calls it for each removed channel.
  - `channel_deleted(channel, cx)`: `store.remove_channel`, then (if it was known) `forget_channel`, `drop_stale_selection`, `remember_places`, `ensure_current_loaded`. The `ChannelDelete` handler and the user's own delete both use it.
  - "Delete channel…" at the end of `channel_menu`, before Copy channel ID's separator, for server channels where `can_manage_channel`. It's drawn like "Delete message" (danger colour, `Trash` icon).
  - `confirm_delete_channel(channel, window, cx)`: "Delete {name}?", "Once it's deleted, there's no going back.", Cancel and a Danger "Delete". Shift doesn't skip it.
  - `delete_channel(channel, window, cx)`: sets `deleting_channel`, so the dialog's buttons are disabled (Delete shows a spinner) and Escape doesn't close it. On 204 or `NotFound` it runs `channel_deleted` and closes the dialog; otherwise the dialog is enabled again and a toast says why.
- `tests/store_channels.rs` (new, from `ready.json`): deleting a categorised channel takes it out of the server's list, its category, the sidebar and its history, and the category stays; a second removal returns false; other servers, channels and categories are untouched; DMs and groups can be removed too.
- `tests/api_servers.rs`: `DELETE /channels/c1` with the token and an empty body; a 403 `MissingPermission` stays an `Api` error.
- `tests/permissions.rs`: `can_manage_channel` is false with the default permissions, true for the owner (even where a channel takes it away), true for a role granting it, false when a channel override denies it to that role, and false in DMs, groups and Saved Notes. The shared test server gains a lowest-ranked `builder` role granting Manage Channels.

### Divergences
- **The menu item's icon is `Trash`, not `IconName::Delete`,** and it's drawn in the danger colour, as "Delete message" is. That's the app's existing destructive menu item.
- **The confirmation has its own footer** (Cancel and Delete), like the external-link prompt, rather than `.confirm()`'s default buttons. The default buttons can't be disabled or show a spinner, which Assumption 13 needs. Escape is also off while the request runs, so the dialog can't be closed under it.
- **A failed delete leaves the confirmation open,** enabled again, beside the toast, as in the web client (which shows the error in the dialog). Cancel closes it.
- **The title uses the bare channel name** ("Delete general?"), as the web client does.
- **The store's per-channel helper is `drop_channel_state`,** so it isn't confused with `ChatView::forget_channel`.

### Verification Performed
- `cargo test --test store_channels`: 4 passed. `--test api_servers`: 4 passed. `--test permissions`: 33 passed. `cargo test --lib server_edit` and the `channel_delete_decodes` events test pass.
- `cargo test`: 916 passed, 0 failed (906 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts:
  - `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17 and `src/store.rs` 15: unchanged, after hand-formatting `delete_channel` and the dialog's `busy` check.
  - `src/permissions.rs`, `src/server_edit.rs`, `tests/api_servers.rs` and `tests/permissions.rs` 0. New file `tests/store_channels.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. As a server's owner, right-click a channel: "Delete channel…" is last before Copy channel ID, in red. Choose it: "Delete {name}?" asks. Cancel keeps the channel.
2. Delete it: Delete shows a spinner while the request runs, then the dialog closes and the channel disappears from the sidebar. If it was open, the sidebar's first channel opens.
3. Delete the last channel of a category: the category disappears (folded or not) until chunk 3, which shows empty categories. No empty header is left behind.
4. Delete a channel from the web client: it disappears from RuSToat within a moment, and if it was open there, another channel opens with no pane or spinner left behind.
5. As a member without Manage Channels, the channel menu has no Delete. DMs and groups never show it.
6. With a channel open in a second account in RuSToat, delete it from the first: it disappears in both.
7. Light and dark themes: the red menu item and the dialog are readable.

## Chunk 3: Create a channel, and show empty categories

### Changes Made
- `src/api/client.rs`:
  - `create_channel(server, name, voice) -> Channel` (`POST /servers/{id}/channels`), with a private body of exactly `{"type":"Text"|"Voice","name":…}` (`DataCreateServerChannel` would send `"description": null`).
  - `edit_categories(server, categories) -> Vec<Category>` (`PATCH /servers/{id}`), sending exactly `{"categories":[…]}` and returning the categories from Stoat's answer (none if it has no `categories` key). `Category` added to the hand-formatted import list.
- `src/server_edit.rs`:
  - `with_channel_in(categories, channel, category) -> Option<Vec<Category>>`: the channel taken out of every category (every copy) and appended to `category`; `None` if that category is gone.
  - `create_channel_error_text` (`TooManyChannels`, `MissingPermission`, else "Couldn't create the channel. Try again.") and `categories_error_text` (`MissingPermission`, `InvalidOperation`, else "Couldn't change the categories. Try again."), worded as in Assumption 14.
  - 6 unit tests.
- `src/permissions.rs`: `can_manage_channels(store, user, server)`: the membership is known, and Manage Channels in `server_permissions`. `src/store.rs`: the `ChatStore::can_manage_channels(server)` wrapper.
- `src/store.rs`:
  - `server_channels` shows empty categories (Assumption 1): a category is left out only when it has no visible channels and lists at least one channel that's still in `server.channels`. So a new category, or one listing only deleted channels, shows with no rows; one of channels the user can't see stays hidden.
  - `set_categories(server, categories)` (Assumption 6; unknown servers ignored) and `category_of(server, channel) -> Option<String>`.
- `src/ui/server_dialogs.rs`:
  - `CreateChannelDialog` and `CREATE_CHANNEL_DIALOG_WIDTH` (420): "in {CATEGORY}" (uppercase, as the sidebar heading) when there's a category, a "Channel name" field with the "{n}/32" counter and the error line, a "Channel type" `RadioGroup` (Text Channel, the default, and Voice Channel), Close and Create (`loading` while creating, disabled until the name is valid). Enter creates. Success hands the channel to `ChatView::channel_created` and closes the dialog; a refusal shows in the dialog.
  - The name field (label, input, counter, error) is now the private `name_field(label, input, busy, error, cx)`, with `field_label(label, cx)`, shared with `CreateServerDialog`, whose look is unchanged.
- `src/ui/chat.rs`:
  - `open_create_channel(server, category: Option<String>, window, cx)`: looks up the category's title (dropping a category that's gone) and opens "Create channel", the field focused.
  - `channel_created(channel, category: Option<(id, title)>, window, cx)`: `add_channel`, then `move_into_category` if there's a category, then `open_channel`.
  - `move_into_category(server, channel, name, (id, title), window, cx)`: `with_channel_in` over the store's categories; skipped if the category is gone; otherwise `edit_categories`, whose answer goes into the store with `set_categories`. On failure the channel stays uncategorized and a toast says "Created #{name}, but couldn't move it into {title}."
  - "Create channel…" (`Plus` icon), through the free `create_channel_item(view, server, category)`, each where `can_manage_channels` (checked when the menu opens):
    - in the category heading's menu, which is now the lazy `category_menu(server, category, cx)`: "Create channel…" (into that category), a separator, then Copy category ID
    - in `channel_menu`, for server channels, after Create invite… and before Delete channel…, into the channel's category (`category_of`) or uncategorized
    - in `channel_list_menu(server, cx)`, a new right-click menu on `channel-list` (uncategorized). It's attached only when the server is shown and the user may manage its channels, since an empty context menu would still take the focus. Rows and headers have their own menus, which handle a right-click first and stop it, so the list's opens only on the space around and below them.
  - "Nothing here yet." only when the server has no sections at all.
- Tests:
  - `tests/api_servers.rs`: a voice channel sends exactly `{"type":"Voice","name":"Lobby"}` with the token and parses a `TextChannel` with `voice: {}`; a text channel sends `"type":"Text"`; a 400 `TooManyChannels { max: 200 }` stays an `Api` error; the PATCH sends exactly `{"categories":[{"id":"k1","title":"Chat","channels":["c1"]}]}` and returns its categories; an answer without `categories` gives none.
  - `tests/store_ready.rs`: a category with no channels and one whose only channel was deleted show as empty sections; categories of only channels the user can't see stay hidden (also when they list a deleted channel besides). The fixture's "Archive" (which lists only a channel no longer on the server) now shows, empty, in the sidebar and fold tests.
  - `tests/store_channels.rs`: `ChannelCreate` then the create response, and the reverse, give one row (last uncategorized) and one ID in the server's list; `set_categories` moves rows and `category_of` follows; an unknown server's categories are ignored. The deletion test now expects the empty "Archive".
  - `tests/permissions.rs`: `can_manage_channels` is false with Stoat's defaults, true for the owner, true for a role granting it (a channel override doesn't matter), false for an unknown server, and false for everyone, the owner too, before their membership is known.

### Divergences
- **`edit_categories` returns `Vec<Category>`, not `Server`.** Only the categories are used (Assumption 6), and parsing the whole server would turn a successful edit into an error if any required field were missing (`revolt-models`' `Server` requires `approximate_member_count`, for instance). Stoat leaves out `categories` when there are none, which gives an empty list.
- **`channel_created` takes the channel and `(id, title)`, not a separate server, and a `window`.** The server is in the channel, and the window is needed for the failure toast.
- **The category's title for the dialog and the toast is read when the dialog opens.** A rename while it's open isn't reflected (chunk 4 adds renames; the move itself is by ID).
- **"Channel type" is labelled** above the radios; the web client's dialog has the two radios under the name with no label of their own.
- **Labels are in sentence case** ("Channel name"), as chunk 1's "Server name".
- **The background menu is only attached** when the user may manage the server's channels, as an empty context menu would still take the focus (gpui-component focuses it even when it draws nothing).

### Verification Performed
- `cargo test --lib server_edit`: 9 passed (6 new). `cargo test --test api_servers`: 9 passed (5 new). `--test store_ready`: 14 passed (3 new, 2 updated). `--test store_channels`: 8 passed (4 new, 1 updated). `--test permissions`: 35 passed (2 new).
- `cargo test`: 936 passed, 0 failed (916 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts:
  - `src/api/client.rs` 3, `src/ui/chat.rs` 17 and `src/store.rs` 15: unchanged, after hand-formatting the new imports (`Category` went into `store.rs`'s already-drifted import list).
  - `src/permissions.rs`, `src/server_edit.rs`, `src/ui/server_dialogs.rs`, `tests/api_servers.rs`, `tests/store_channels.rs`, `tests/store_ready.rs` and `tests/permissions.rs` 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. As a server's owner, right-click a category heading: "Create channel…", a separator, then Copy category ID. Create a text channel: the dialog says "in {CATEGORY}", the channel appears at the end of that category and opens.
2. Right-click a channel in another category: "Create channel…" is after Create invite… and before Delete channel…. Create a voice channel: it lands at the end of that category with the voice icon and opens.
3. Right-click the channel list's empty space below the last row: "Create channel…" only. The channel is uncategorized, at the bottom of the top group. Right-clicking a row or a heading opens only its own menu.
4. The web client shows the same placement for all three.
5. The Create button is disabled for an empty or blank name and past 32 characters; the counter shows it. Text Channel is chosen at first; the radios switch.
6. A category with no channels (make one in the web client, or delete its last channel) shows as a heading with no rows, folds and unfolds, and its menu still works.
7. As a member without Manage Channels, none of the three menus offers Create channel…, and right-clicking the empty space does nothing.
8. With a category deleted in the web client while the dialog is open, Create still makes the channel, uncategorized, with no toast.
9. Light and dark themes: the dialog, the "in {CATEGORY}" line and the radios are readable.

## Chunk 4: Create, rename and delete categories

### Changes Made
- `Cargo.toml`, `Cargo.lock`: the `ulid` crate (3.0.0). It adds only itself to the lock file; its `rand` (0.10.3) and `web-time` were already in the tree.
- `src/server_edit.rs`:
  - `new_category_id()`: `ulid::Ulid::generate()` as a 26-character string.
  - `with_category_added(categories, id, title) -> Vec<Category>`: appended, with no channels.
  - `with_category_renamed(categories, id, title)` and `without_category(categories, id)`, both `-> Option<Vec<Category>>`: `None` if the category is gone. Deleting the last one gives an empty list, which `edit_categories` sends as `{"categories":[]}`.
  - The private `listed_once`: every edit (and `with_channel_in`) keeps each channel only where it's first listed, so a list Stoat would refuse for a duplicate is never sent.
  - 6 unit tests.
- `src/ui/server_dialogs.rs`: `NameDialog` and `NAME_DIALOG_WIDTH` (420), for "Create a new category" ("Name", Create) and "Rename category" ("New name", prefilled with the title, Rename). It uses `name_field` (counter, error line), with Close and the action button (`loading` while saving, disabled until the name is valid). Enter saves. The categories are read from the store when you save (Assumption 5). Stoat's answer goes to `ChatView::categories_changed` and the dialog closes. A refusal shows in the dialog (`categories_error_text`). A category deleted while its Rename dialog is open shows "This category has been deleted." and nothing is sent. `CreateServerDialog` and `CreateChannelDialog` weren't folded onto it: they share only `name_field` and the buttons, which they already share.
- `src/ui/chat.rs`:
  - `categories_changed(server, categories, cx)`: `set_categories`, then `drop_stale_selection` (which forgets a deleted category's fold) and `remember_places`. Chunk 3's `move_into_category` uses it too.
  - `open_create_category(server, ..)` and `open_rename_category(server, category, ..)`, through the free `open_name_dialog(title, dialog, ..)`, which focuses the field.
  - "Create category…" (`FolderPlus`), through the free `create_category_item(view, server)`, wherever "Create channel…" is: the category heading's menu, the server-channel menu (right after Create channel…) and the channel list's background menu.
  - The category heading's menu, where `can_manage_channels`: Create channel…, Create category…, Rename category… (`Pencil`), Delete category… (in red with `Trash`, as Delete channel…), a separator, Copy category ID.
  - `confirm_delete_category(server, category, ..)`: "Delete {TITLE}?", "Its channels won't be deleted. They'll move out of the category.", Cancel and a Danger "Delete". It's built like `confirm_delete_channel`, using a new `deleting_category` field: the buttons are disabled with a spinner and Escape is off while the request runs.
  - `delete_category(..)`: `without_category` over the store's categories, then `edit_categories`. On success it calls `categories_changed` and closes the dialog. A refusal toasts and leaves the confirmation open and enabled again. If the category is already gone, nothing is sent and the dialog closes.
  - `category_title(server, category)`, now also used by `open_create_channel`.
- Tests:
  - `tests/api_servers.rs`: `PATCH /servers/s1` with no categories sends exactly `{"categories":[]}`.
  - `tests/store_channels.rs`: deleting `cat-chat` leaves its channels uncategorized, in the server's order, with none deleted, and the category gone. Setting no categories leaves every channel uncategorized.

### Divergences
- **`with_category_renamed` and `without_category` return `Option`.** The plan says "a missing ID changes nothing". Returning `None` lets the UI skip the request rather than send the list unchanged. Rename then says the category was deleted. Delete treats the category as already gone and closes the confirmation, as `NotFound` does for a channel.
- **The edits also remove duplicate channel IDs** that were already in the list (the first listing is kept), not only ones they would add. This includes `with_channel_in`. Stoat refuses any list with a channel twice.
- **`CreateServerDialog` and `CreateChannelDialog` stay as they are.** The plan allows this ("don't force it").
- **`ulid` is 3.0.0, the current release.** Its generator is `Ulid::generate()`.
- **A failed delete leaves the confirmation open** beside the toast, as for channels (chunk 2).

### Verification Performed
- `cargo test --lib server_edit`: 15 passed (6 new). `cargo test --test api_servers`: 10 passed (1 new). `--test store_channels`: 10 passed (2 new).
- `cargo test`: 945 passed, 0 failed (936 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts:
  - `src/ui/chat.rs` 17: unchanged, after hand-formatting the new code.
  - `src/server_edit.rs`, `src/ui/server_dialogs.rs`, `tests/api_servers.rs` and `tests/store_channels.rs`: 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. As a server's owner, right-click a category heading: Create channel…, Create category…, Rename category…, Delete category… (red), a separator, Copy category ID. The channel menu shows Create category… after Create channel…, and so does the list's empty space.
2. Create a category: "Create a new category", Create disabled until the name is valid. It appears at the bottom, empty. Create a channel in it from its menu: the channel lands in it.
3. Rename it: "Rename category" with the title filled in. The heading changes here at once, and in the web client.
4. Delete it: "Delete {TITLE}?" asks. Delete shows a spinner, then its channels move to the top group and none are deleted. The web client shows the same.
5. Delete the server's last category: everything is uncategorized, with no error.
6. Fold a category and rename it: it stays folded. Fold one and delete it, then create a category: the new one isn't folded.
7. Rename or delete a category from the web client while RuSToat's Rename dialog for it is open, then save: the error line says the category was deleted, or the new name is saved over the web client's.
8. As a member without Manage Channels, none of the menus offer the category items.
9. Light and dark themes: the dialogs and the red menu item are readable.
