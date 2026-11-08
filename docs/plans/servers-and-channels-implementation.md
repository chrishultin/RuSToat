# Implementation Record: Servers and channels (RuSToat milestone 19)

_Plan: docs/plans/servers-and-channels.md_
_Started: 2026-11-07_

Pre-existing uncommitted changes, kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt `Diff in` counts (2026-11-07): `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15. Zero in `src/permissions.rs`, `src/main.rs`, `src/ui/actions.rs`, `src/ui/invites.rs`, `src/invites.rs`, `src/ui/profile_card.rs`, `tests/store_servers.rs`, `tests/permissions.rs` and `tests/store_ready.rs`.

Supervision: continuous, unattended overnight; manual checks batched at the end.

Commits: chunk 1 is commit `0ed2e49`. Commit signing (Secretive, through Touch ID) wasn't available overnight, so chunks 2–6 were left staged, uncommitted, and each chunk's tree (from `git write-tree`), with its patch against the one before, was recorded in `/tmp/rustoat-chunks/ledger.txt` (`m19-c2` to `m19-c6`). They're to be committed one by one in the morning, each from its recorded tree.

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

## Chunk 5: Leave a server

### Changes Made
- `src/api/client.rs`: `leave_server(server, silently)` (`DELETE /servers/{id}?leave_silently=true|false`, no body, returns `()`). Its doc says that for the owner the same request deletes the server, so it must never be called for a server the user owns.
- `src/store.rs`: `is_owner(server) -> bool` (false for an unknown server).
- `src/server_edit.rs`: `leave_error_text(&ApiError)`: `profile_edit::error_text` with "Couldn't leave the server. Try again." (rate limits say how long to wait). One unit test.
- `src/ui/chat.rs`:
  - `may_leave(server)`: the server is known, the user's membership is known, and they don't own it.
  - "Leave server…" at the end of `server_menu`, before Copy server ID's separator, where `may_leave` (checked when the menu opens). It's drawn like Delete channel… (danger colour), with the `LogOut` icon.
  - `confirm_leave_server(server, ..)`: "Leave {name}?", "You won't be able to rejoin unless you are re-invited.", a "Don't notify others that you've left" checkbox (unticked), Cancel and a Danger "Leave". Its state is a new `leave_prompt: Option<LeavePrompt>` (`server`, `name`, `silently`, `leaving`), read on each render, as `link_prompt` is. While the request runs the buttons and the checkbox are disabled, Leave shows a spinner, and Escape is off, as in `confirm_delete_channel`.
  - `leave_server_now(..)`: the second guard. It re-reads the store when Leave is chosen. If the server is gone, the dialog closes and nothing is sent. If the user now owns it, nothing is sent, the dialog closes and a toast says "You own {name}, so you can't leave it." Otherwise it sends `leave_server` with the checkbox's value. On 204 or `NotFound` it runs the existing `leave_server` (Home opens if the server was on screen), closes the dialog and toasts "Left {name}." The `ServerMemberLeave` that follows finds nothing left to remove. Other errors toast and leave the confirmation open and enabled again.
- Tests:
  - `tests/api_servers.rs`: `DELETE /servers/s1` with `leave_silently=true` and with `=false` (matched on the query), the token and an empty body, 204 → `Ok(())`; a 404 `NotFound` stays an error with that type.
  - `tests/store_servers.rs`: `is_owner` for the fixture's owned and joined servers, an unknown server, a created server and a joined one; and after `ServerUpdate`s handing one server to the user and another away.

### Divergences
- **The owner guard also toasts.** If the user became the owner while the confirmation was open, nothing is sent and a toast says "You own {name}, so you can't leave it." The plan only said to re-read the owner, not what to show.
- **A failed leave leaves the confirmation open** beside the toast, as Delete channel and Delete category do. The web client shows the error in the dialog.
- **The "Left {name}." toast is a success toast,** like "Joined {name}.".
- **The server's name in the title and toast is read when the confirmation opens.**

### Verification Performed
- `cargo test --lib server_edit`: 16 passed (1 new). `cargo test --test api_servers`: 13 passed (3 new). `--test store_servers`: 10 passed (2 new).
- `cargo test`: 951 passed, 0 failed (945 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts:
  - `src/api/client.rs` 3, `src/ui/chat.rs` 17 and `src/store.rs` 15: unchanged, after hand-formatting the new code.
  - `src/server_edit.rs`, `tests/api_servers.rs` and `tests/store_servers.rs`: 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Right-click a server you joined: "Leave server…" is last before Copy server ID, in red. Choose it: "Leave {name}?" asks, with the checkbox unticked. Cancel keeps the server.
2. Leave with the box unticked: Leave shows a spinner, then the server disappears from the rail, Home opens if it was on screen, and "Left {name}." shows. The web client shows "{you} left" in the server's system channel (if it has one).
3. Join again and leave with the box ticked: no message in the web client.
4. On a server you own, the menu has no Leave server….
5. Light and dark themes: the red menu item, the dialog and the checkbox are readable.

## Chunk 6: Server and channel info

### Changes Made
- `src/server_edit.rs`: `first_line(description) -> Option<&str>` (Assumption 15): the first line that isn't blank, trimmed, with its Markdown left as typed. One unit test.
- `src/ui/server_dialogs.rs`:
  - `INFO_DIALOG_WIDTH` (440), `InfoContent` (ID, name, icon URL, placeholder, banner URL, description) and `InfoPlaceholder` (a server's initials, or a channel's `#` or speaker glyph).
  - `render_info(content, on_link, cx)`: the banner (140 px, cover, fixed height so a slow image doesn't move anything) across the top when there's one, then the 48 px icon (round, with the placeholder while it loads, if it fails, or if there's none) beside the name, then the description as Markdown (selectable, with math and markup as in bios, links through `on_link`) or "No description." in muted text, then Close.
- `src/ui/chat.rs`:
  - `markdown_body(key, text) -> Option<CardBio>`: trimmed, `markdown::prepare`d, keyed by `render_key`. `render_card`'s bio now uses it too.
  - `open_server_info(server, ..)`, `open_channel_info(channel, ..)` and `info_content(&Info) -> Option<InfoContent>`, read from the store (`server-info-{id}` and `channel-info-{id}` render keys, so an edited description is parsed again).
  - The free `open_info_dialog(view, Info, ..)`: the dialog's builder reads `info_content` each time it's drawn, so an edit made elsewhere shows at once. Once the server or channel is gone (left, deleted), it closes itself, once, through `window.defer`. Its links go through a `People` (`link_handler`), so external links get the usual warning, invite links open the Join dialog and mentions open cards.
  - The server's name above the channel list (`channel-list-title`) opens Server info: pointer cursor, the sidebar's hover colours, tooltip "Server info". Home's "Direct Messages" isn't clickable.
  - The header, for server channels: the name (at most `HEADER_NAME_MAX_WIDTH`, 240 px, and shrinking before the search box does) opens Channel info, and so does the description's first line after it (muted, normal weight, small, truncated, tooltip "Click to show full description"), truncated to the space left and only as wide as its text, so the header's empty space isn't clickable. DMs, groups and Saved Notes keep the old header.

### Divergences
- **One `render_info` with an `InfoContent`,** not `render_server_info` and `render_channel_info`: the two differ only in the banner and what stands in for the icon.
- **The dialogs have no title bar, padding or corner ✕.** The name is beside the icon, and the banner reaches the dialog's edges, as the profile card's does. A ✕ over a banner image could be unreadable. Close, Escape and clicking outside all close it.
- **Channel info shows the bare name beside the `#` or speaker glyph,** not `#name`. The glyph already says what it is, and a voice channel isn't a `#`.
- **The header's name shrinks, rather than being `flex_none`:** it's capped at 240 px and truncates in a narrow window too, so the search box and buttons are never pushed out.
- **Hover:** the name dims to the muted colour, the description line brightens to the foreground colour, and the server title takes the sidebar rows' hover background.
- **Channel mentions in descriptions are links, not pills,** as in bios.
- **The self-closing has one gap:** gpui-component only closes the topmost dialog. If the server or channel goes while another dialog is on top of its info (the external-link warning, opened from the description), the warning closes instead, and the info stays open, empty, until it's dismissed.

### Verification Performed
- `cargo test --lib server_edit`: 17 passed (1 new: `None` for `None`, "", "\n\n" and blank lines; "Rules\nmore" gives "Rules"; "\n  Hi  \nx" gives "Hi"; Markdown kept and `\r\n` handled).
- `cargo test`: 952 passed, 0 failed (951 before).
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts:
  - `src/ui/chat.rs` 17: unchanged, after hand-formatting `info_content`.
  - `src/server_edit.rs`, `src/ui/server_dialogs.rs` and `src/ui/profile_card.rs` (unchanged): 0.

### Manual Validation
Deferred to the batched manual pass. Steps:
1. Hover the server's name above the channel list: the pointer, the hover colours and "Server info". Click it with a server that has a banner, an icon and a Markdown description (set them in the web client): all three show, the Markdown renders, and the text can be selected.
2. A server with none of them: no banner, its initials, and "No description." in muted text.
3. Home's "Direct Messages" title does nothing.
4. A channel with a description shows its first line, muted, after its name in the header, with "Click to show full description" on hover. Clicking the name or the line shows Channel info: its icon (or `#`, or the speaker for a voice channel) beside its name, and the whole description. A channel without one shows only its name, still clickable.
5. A long name and a long description truncate cleanly before the search box and buttons, also with the window narrowed. DMs, groups and Saved Notes show no line and aren't clickable.
6. Edit a description, an icon or a banner in the web client while a dialog is open: it updates. Edit the description's first line: the header follows.
7. A link in a description: an external one asks first, an invite link opens Join a server, a channel link opens the channel.
8. Delete the channel, or leave the server, from elsewhere while its info is open (the web client, or another account): the dialog closes.
9. Light and dark themes: both dialogs, "No description." and the header's line are readable.

## Summary

_Completed: 2026-11-07_

### Chunks Completed
1. Create a server from the rail's "+" menu or Stoat → Create Server…, landing in its General channel.
2. Delete a channel from its menu, after confirming, and follow channels deleted elsewhere.
3. Create text and voice channels from the category, channel and channel list menus, in the right category; empty categories show.
4. Create, rename and delete categories (keeping their channels) from the same menus.
5. Leave a server from its rail menu, optionally without the "left" message; owners can't.
6. Server and channel info from the server's name and the channel header, which shows the description's first line.

### Divergences
- Chunk 1:
  - The "+" is the rail's own face made selectable (`RailMenuButton`), not a ghost `Button`.
  - `fetch_own_member` dedupes fetches already on their way, so the response and `ServerCreate` ask once.
  - The dialog's title is "Create a server", to match the menu and "Join a server".
  - Test fixtures carry `approximate_member_count`, which `revolt-models` requires.
- Chunk 2:
  - The menu item uses `Trash` in the danger colour, as "Delete message" does.
  - The confirmation has its own Cancel and Delete, so they can be disabled with a spinner while the request runs, and Escape is off meanwhile.
  - A failed delete leaves the confirmation open, enabled again, beside the toast.
  - The title uses the bare channel name.
  - The store's helper is `drop_channel_state`, apart from `ChatView::forget_channel`.
- Chunk 3:
  - `edit_categories` returns the categories, not the whole `Server`.
  - `channel_created` takes the channel, the category's ID and title, and a window (for the failure toast).
  - The category's title in the dialog and toast is read when the dialog opens.
  - "Channel type" is labelled, and labels are in sentence case.
  - The list's background menu is attached only for those who may manage channels.
- Chunk 4:
  - `with_category_renamed` and `without_category` return `None` for a category that's gone, so nothing is sent.
  - Every edit also drops duplicate channel IDs already in the list.
  - Create server and Create channel weren't folded onto `NameDialog`.
  - `ulid` is 3.0.0.
  - A failed delete leaves the confirmation open, as for channels.
- Chunk 5:
  - The owner guard toasts "You own {name}, so you can't leave it." if the user became the owner meanwhile.
  - A failed leave leaves the confirmation open beside the toast.
  - "Left {name}." is a success toast.
  - The server's name is read when the confirmation opens.
- Chunk 6:
  - One `render_info` with an `InfoContent` serves both dialogs.
  - The dialogs have no title bar, padding or corner ✕, so the banner reaches the edges; the name is beside the icon.
  - Channel info shows the bare name beside its glyph, not `#name`.
  - The header's name is capped at 240 px but can shrink further.
  - Channel mentions in descriptions are links, as in bios.
  - If another dialog is on top of an info dialog when its server or channel goes, that one closes instead, and the info stays open, empty, until dismissed.

### Definition of Done
- [ ] The rail's "+" opens a menu with "Create a server…" and "Join a server…"; Stoat → Create Server… opens the same dialog. Creating sends `{"name":…}`, adds the server once at the end of the rail, and opens General. `TooManyServers` and `CantCreateServers` have their own messages. (Manual pass pending. The body, the single rail entry in either order and the error texts are test-verified.)
- [ ] `ChannelDelete` removes the channel from the sidebar, its categories and every per-channel state, and moves you off it if it was open. (Manual pass pending. The parse and the store's removal are test-verified.)
- [ ] "Delete channel…" shows only with Manage Channels on that channel, always asks first, and removes the channel when Stoat answers 204 or `NotFound`. (Manual pass pending. `can_manage_channel` and the request are test-verified.)
- [ ] "Create channel…" shows only with server-level Manage Channels, in the category menu, the server-channel menu and the list's background; it creates text or voice channels in the category it was started from, and opens them. (Manual pass pending. `can_manage_channels`, the request bodies, `with_channel_in` and both arrival orders are test-verified.)
- [x] Empty categories show; categories whose channels you can't see stay hidden.
- [ ] Categories can be created (appended, with a ULID), renamed and deleted (their channels kept) by those with Manage Channels, each sending the whole list as `{"categories":[…]}`. (Manual pass pending for the menus. The edits, the IDs, the bodies and the uncategorized channels are test-verified.)
- [ ] "Leave server…" shows for every server you don't own and never for one you do; it sends `leave_silently` from the checkbox, removes the server, and toasts. (Manual pass pending. `is_owner`, including after an owner change, and both query values are test-verified.)
- [ ] Clicking the server name above the channel list shows its banner, icon and markdown description; clicking the channel name or its first description line in the header shows its icon and description. (Manual pass pending. `first_line` is test-verified.)
- [x] `cargo test` (952 passed) and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grew.
- [ ] Every chunk's manual check is recorded in `docs/plans/servers-and-channels-implementation.md`. (Manual pass pending. The steps are in each chunk's section and consolidated below.)

### Remaining Manual Steps
One sitting, in this order. You need two accounts on `api.stoat.chat`: **A**, who'll own the test server, and **B**, who'll join it. Sign both in to RuSToat, and sign in to the web client as A in a browser to watch.

**Create a server (chunk 1)**
1. As A, click the rail's "+": a menu with "Create a server…" (plus icon) and "Join a server…" opens to the right of the rail, not clipped by the window's edge. Try again with the window short enough that the "+" is near the bottom.
2. Choose Join a server…: "Join a server" opens. Close it.
3. Choose Create a server…: "Create a server" opens with the field focused. Create is disabled while the name is empty or blank and past 32 characters; the counter shows the count and turns red past 32.
4. The "By creating this server, you agree to the Acceptable Use Policy." line shows (this is stoat.chat), and its link opens `https://stoat.chat/aup` in the browser. (On another instance it wouldn't show.)
5. Name it "M19 Test" and Create: it appears at the end of the rail, General opens, and you can send a message there. No toast.
6. Stoat → Create Server… opens the same dialog. Close it. With the login view on screen (adding an account, say), Stoat → Create Server… does nothing.

**Categories and channels (chunks 3 and 4)**
7. Right-click the channel list's empty space below General: only "Create channel…" and "Create category…". Right-click General: Create invite…, Create channel…, Create category…, Delete channel… (red), a separator, Copy channel ID; the list's own menu doesn't also open.
8. Create category… "Alpha": "Create a new category", Create disabled until the name is valid. Alpha appears at the bottom as a heading with no rows. Create "Beta" and "Gamma" the same way. Fold and unfold Gamma (empty): it toggles.
9. Right-click Alpha's heading: Create channel…, Create category…, Rename category…, Delete category… (red), a separator, Copy category ID.
10. From Alpha's menu, Create channel…: "Create channel", "in ALPHA", Text Channel chosen; the radios switch, and Create follows the same name rules as step 3. Create a text channel "a-text": it lands at the end of Alpha and opens.
11. From Beta's menu, create a text channel "b-text". Then right-click b-text → Create channel…, and create a Voice Channel "b-voice": it lands at the end of Beta with the speaker icon and opens.
12. From the empty space's menu, create "loose": it's uncategorized, at the bottom of the top group, and opens.
13. The web client shows a-text, b-text, b-voice and loose in the same places.
14. Fold Beta, then Rename category… on it: "Rename category" with "Beta" filled in. Rename it "Bravo": the heading changes at once (and in the web client), and it stays folded.
15. Open Create channel… from Gamma's menu and leave it open; delete Gamma in the web client; then Create "orphan": the channel is made, uncategorized, with no toast.
16. Open Rename category… on Alpha and leave it open; delete Alpha in the web client; then Rename: the error line says the category has been deleted. Close it. (Alpha's channels are now uncategorized.)
17. Create a category "Delta" in RuSToat. Open Rename on it, rename it in the web client to "Web", then save "Mine" in RuSToat: "Mine" wins, here and in the web client.
18. Fold Bravo and Delete category… on it: "Delete BRAVO?" with "Its channels won't be deleted. They'll move out of the category." Delete shows a spinner; then b-text and b-voice move to the top group, none deleted, and the web client agrees. Create a category "Echo": it isn't folded.
19. Delete every remaining category, the last one too: everything is uncategorized, with no error.

**Server and channel info (chunk 6)**
20. In the web client, give M19 Test an icon, a banner and a Markdown description with **bold** text, an external link, an invite link and a channel mention. Give General a two-line description whose first line is "Rules".
21. Hover the server's name above the channel list: a pointer, the hover colours and "Server info". Click it: the banner across the top, the icon beside "M19 Test", and the description rendered and selectable. Close, Escape and clicking outside each close it.
22. In Server info, click the external link (a warning asks first), the invite link (Join a server opens) and the channel mention (the channel opens).
23. With Server info open, change the description in the web client: it updates. Remove the banner, icon and description: no banner, the initials "MT", and "No description." in muted text.
24. Home's "Direct Messages" title isn't clickable.
25. Open General: "Rules" shows after its name in the header, muted, with "Click to show full description" on hover. Click the line, then the name: Channel info shows the `#` beside "General" and the whole description. b-voice's info shows the speaker. A channel without a description shows only its name in the header, still clickable, and "No description.".
26. Edit General's first line in the web client: the header follows, and an open Channel info updates. Give General an icon in the web client: Channel info shows it.
27. Give a channel a long name and a long first line, and narrow the window: both truncate cleanly before the search box and the buttons. DMs, groups and Saved Notes show no line and aren't clickable.

**Delete a channel (chunk 2)**
28. Right-click "orphan": Delete channel… is last before Copy channel ID, in red. Choose it: "Delete orphan?" with "Once it's deleted, there's no going back." Cancel keeps it.
29. Open "orphan" and delete it: Delete shows a spinner, the dialog closes, the channel disappears, and the sidebar's first channel opens.
30. Create a category with one channel in it and delete that channel: the category stays, as a heading with no rows, folded or not.
31. Open a channel in RuSToat and delete it from the web client: it disappears within a moment and another channel opens, with no pane or spinner left behind. With its Channel info open instead, the dialog closes.

**A member's view, two accounts (chunks 2–5; plan Manual Tests 2 and 3)**
32. As A, create an invite to M19 Test (the header's Create invite). As B, "+" → Join a server… with it.
33. As B: channel menus have no Create channel…, Create category… or Delete channel…; category headings offer only Copy category ID; right-clicking the empty space does nothing. Server info and Channel info work. DMs and groups never offer Delete channel….
34. Switch between A and B: the "+" menu, Stoat → Create Server… and every server and channel menu act for the account on screen.
35. As B, open a channel; as A, delete it: it disappears for both, and B lands on another channel.
36. As A, right-click M19 Test in the rail: no Leave server… (A owns it).

**Leave a server (chunk 5)**
37. In the web client as A, set M19 Test's "user left" system message channel to General.
38. As B, right-click M19 Test: Leave server… is last before Copy server ID, in red. Choose it: "Leave M19 Test?", "You won't be able to rejoin unless you are re-invited.", and "Don't notify others that you've left", unticked. Cancel keeps the server.
39. With M19 Test on screen, leave with the box unticked: Leave shows a spinner, the server disappears from the rail, Home opens, and "Left M19 Test." shows. The web client shows B left in General.
40. Join again with a new invite and leave with the box ticked: no message in the web client.

**Themes and clean-up (plan Manual Tests 4 and 5)**
41. In light and dark themes: every dialog (Create a server, Create channel, Create a new category, Rename category, Delete channel, Delete category, Leave, Server info, Channel info), the counters, the Acceptable Use Policy link, "in {CATEGORY}", the radios, the checkbox, the red menu items, "No description." and the header's description line are readable.
42. Delete M19 Test from the web client (Server settings): it leaves RuSToat's rail. (RuSToat gets Delete server in milestone 23.)
