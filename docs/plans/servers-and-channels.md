# Plan: Create, leave and organise servers and channels (RuSToat milestone 19)

## Summary
Milestone 19 of the [roadmap](roadmap.md) lets you make and organise the places you talk in:
- **Create a server:** the rail's "+" becomes a menu with "Create a server…" and "Join a server…". Stoat → Create Server… does the same.
- **Create a channel** (text or voice) **or a category,** and **rename or delete a category.** These are in the category menu, the channel menu and a right-click on the channel list's empty space, for people with Manage Channels.
- **Delete a channel,** with confirmation, for people with Manage Channels on it. Deleting already happens in other clients; today RuSToat ignores it and keeps showing the channel.
- **Leave a server** from its rail menu, optionally without telling anyone. Owners can't leave.
- **Server and channel info:** the server's name above the channel list opens its icon, banner and description. The channel's name in the header opens its icon and description, and the description's first line is shown next to the name.

**Deleting a server moves to milestone 23** (Server Settings), confirmed by typing the server's name (Decision 3).

The references are Stoat's server (`stoatchat/stoatchat` v0.16.2: `crates/delta/src/routes/servers/{server_create,channel_create,server_edit,server_delete}.rs`, `crates/delta/src/routes/channels/channel_delete.rs`, `crates/core/database/src/models/{servers,channels,server_members}/model.rs`, `crates/core/database/src/models/channels/ops/mongodb.rs`, `crates/core/database/src/events/client.rs`, `crates/delta/src/util/ratelimits.rs`, `crates/core/config/Revolt.toml`) and the web client (`stoatchat/for-web` `stoat-for-web-v0.16.3`: `components/modal/modals/{CreateOrJoinServer,CreateServer,CreateChannel,CreateCategory,EditCategory,DeleteCategory,LeaveServer,DeleteChannel,ServerInfo,ChannelInfo}.tsx`, `components/app/menus/{ServerContextMenu,ChannelContextMenu,CategoryContextMenu,ServerSidebarContextMenu}.tsx`, `src/interface/navigation/channels/ServerSidebar.tsx`, `src/interface/channels/ChannelHeader.tsx`). Both were read on 2026-11-07 from shallow clones of those tags.

## Approach
**What the API does** (from the server source):
- **Create a server:** `POST /servers/create` with `DataCreateServer { name (1–32), description?, nsfw? }` returns `CreateServerLegacyResponse { server, channels }`.
  - Checks, in order: a bot account (`IsBot`); `CantCreateServers` (403) when the instance restricts creation to a list of users; `FailedValidation`; then `TooManyServers { max }` (400).
  - The new server has one text channel, "General", and no categories, roles or system-message channels.
  - Stoat then sends **`ServerCreate` to you only**, as when joining. Your own `ServerMemberJoin` goes to a topic you aren't subscribed to yet.
- **Create a channel:** `POST /servers/{id}/channels` with `DataCreateServerChannel { type: "Text" | "Voice" (default Text), name (1–32), description?, nsfw?, voice? }` returns the `Channel`.
  - Needs **server-level `ManageChannel`**, else `MissingPermission` (403). `TooManyChannels { max }` (400) above `server_channels` (200 on stoat.chat).
  - A voice channel is a `TextChannel` with `voice: {}`.
  - The channel is appended to `server.channels`. **It isn't put in any category.**
  - Stoat sends `ServerUpdate { data: { channels } }`, then `ChannelCreate`, to the server.
- **Categories have no endpoint of their own.** They're `server.categories: [{ id (1–32), title (1–32), channels }]`, replaced whole by `PATCH /servers/{id}` with `{ "categories": [...] }`. The response is the full `Server`.
  - `categories` alone needs **`ManageChannel`**. (Name, description, icon and `remove` need `ManageServer`.) Clearing them with `remove: ["Categories"]` needs `ManageServer`, so deleting the last category sends `"categories": []` instead.
  - A channel listed twice is `InvalidOperation` (400). Unknown channel IDs are silently dropped. Empty categories and duplicate category IDs are allowed. There's no limit on the number of categories.
  - The client makes the category ID. The web client uses a ULID.
  - Stoat sends `ServerUpdate { data: { categories } }` to the server.
- **Leave a server:** `DELETE /servers/{id}?leave_silently=true|false`, 204. No permission is needed.
  - **If you're the owner, the same request deletes the server.** So "Leave" must never be offered to the owner.
  - `NotFound` if you aren't a member (already gone).
  - Stoat sends `ServerMemberLeave { id, user, reason: "Leave" }`, which you also receive, and a `UserLeft` system message unless silent (and only if the server has a "user left" channel set).
- **Delete a channel:** `DELETE /channels/{id}`, 204. Needs `ViewChannel`, then **`ManageChannel` on the channel**, else `MissingPermission`.
  - Stoat removes the ID from `server.channels` and clears any system-message setting pointing at it. **It doesn't remove the ID from categories** (a TODO in the server), and **sends no `ServerUpdate`.**
  - It sends **`ChannelDelete { id }` to the channel's topic**, so only people who could see it are told.
  - The same route closes a DM or leaves a group. That's milestone 20.
- **Descriptions** (`server.description`, `TextChannel.description`) are plain strings of up to 1024 characters. The web client renders them as markdown.
- **Rate limits** (10 s windows): `/servers/*` is 5 per server ID (`/servers/create` counts as one "server"), `/channels/*` is 15 per channel. A 429's `retry_after` is already read into `ApiError::RateLimited`.
- **Manage Channels isn't in Stoat's default permissions** (`DEFAULT_PERMISSION_SERVER`), so ordinary members see none of the create, rename or delete items. Owners have every permission.

**What the web client does:**
- **"+" in the server rail:** "Create or join a server". A chooser opens Create server or Join a server.
- **Create server:** "Create server", one "Server Name" field (1–32, with a counter), the line "By creating this server, you agree to the Acceptable Use Policy." (linking `https://stoat.chat/aup`), Close and Create. On success it opens the server.
- **Create channel:** "Create channel", "Channel Name" (1–32) and a Text Channel / Voice Channel choice, Close and Create. No description or NSFW (those are channel settings).
  - Started from a category (or a channel in one), it then PATCHes the categories to move the new channel into that category, without waiting.
  - Then it opens the channel.
- **Create category:** "Create a new category", one "Name" field; appends `{ id: ulid(), title, channels: [] }`.
- **Rename category:** "Rename category", "New name" prefilled.
- **Delete category:** "Delete category", "Once it's deleted, there's no going back." Its channels become uncategorised; none are deleted.
- **Entry points,** all gated by `ManageChannel`:
  - category menu: Create channel (in that category), Create category, Rename category, Delete category
  - channel menu: Create channel (in the channel's category), Create category, then Delete channel (with `ManageChannel` on that channel)
  - right-click on the channel list's background: Create channel, Create category
  - nothing in the server's rail menu, and no "+" buttons.
- **Delete channel:** "Delete {name}?", "Once it's deleted, there's no going back.", Cancel and Delete. It stays open and shows the error if the request fails.
- **Leave server** (server menu, hidden for the owner): "Leave {name}?", "You won't be able to rejoin unless you are re-invited.", a "Don't notify others that you've left" checkbox, Cancel and Leave.
- **Server info:** clicking the server name above the channel list. Title: the name; body: the description as markdown; buttons: Settings, Edit Identity, Report, Close. No icon or banner (the banner is the sidebar header's background).
- **Channel info:** clicking the channel name in the header, or the description's first line shown next to it (tooltip "Click to show full description"). Title `#name`, body the description as markdown, and Close.
- No toasts after any of these.

**What exists already** (line numbers as of 2026-11-07):
- **Client** (`src/api/client.rs`): `send` (~1038) and `send_raw` (~1047). `create_invite` (~810) sends a private `Serialize` struct; `edit_member` (~678) sends a revolt_models type through `edit_body` (~1121), which drops nulls and an empty `remove`. `delete_message` (~482) is the model for a DELETE that returns `()`. The hand-formatted `revolt_models::v0` import list is at ~4–11.
- **Events** (`src/api/events.rs`):
  - `ServerCreate` (~195), `ServerUpdate` (~166), `ServerDelete` (~201), `ServerMemberLeave` (~146) and `ChannelCreate` (~132) are parsed.
  - **`ChannelDelete` isn't.** It falls to `Unknown` (~653) and is dropped. A new event also needs adding to the forwarded list (~1030–1056).
  - Parse tests are at the bottom of the file (~1220–1520).
- **Event handling** (`src/ui/chat.rs` `on_connection_event`):
  - `ChannelCreate` (~965) only calls `store.add_channel`.
  - `ServerMemberLeave` for you (~1017) and `ServerDelete` (~1114) call `leave_server` (~1192). It removes the server and clears every per-channel state (panes, loading, errors, jumps, banners, `pending_ack`), then runs `drop_stale_selection` (~1285), `remember_places` and `ensure_current_loaded`.
  - `ServerCreate` (~1101): `add_server`, then `fetch_own_member`.
- **Store** (`src/store.rs`):
  - `add_channel` (~1941) appends a server channel's ID to `server.channels` if absent.
  - **There's no `remove_channel`.** `remove_server` (~3004) shows what a channel's removal must clear (`histories`, `read_states`, `pending`).
  - `apply_server_update` (~2837) applies `categories` and `clear: Categories`.
  - `add_server` (~2981) replaces a known server and appends a new one to the rail.
  - `server_channels` (~3239) builds the sidebar: uncategorised first, then categories, **leaving out categories with no visible channels** (~3271).
  - `ChannelEntry` (built in `channel_entry`, ~3322) has no description.
- **Permissions** (`src/permissions.rs`): `server_permissions` (~45; the owner gets everything), `channel_permissions`, `has`, the private `in_server` (~155) and `can_invite` (~139). `ChatStore` wrappers are at `store.rs` ~2500–2560. There's no `ManageChannel` helper.
- **UI** (`src/ui/chat.rs`):
  - `render_rail` (~5546) ends with the "+" join button `rail-join` (~5586), whose click calls `open_join_dialog(None, ..)` (~4287). `rail_button(...)` (~9358) takes a right-click `MenuBuilder`.
  - `server_menu` (~3463) and `channel_menu` (~3392) build their items lazily from the store.
  - `render_channel_list` (~5624): the category header (~5647) has a one-item menu, "Copy category ID". The server title above the list (~5699) is a plain `div`. The scrolling list is `channel-list`, and the "Nothing here yet." line is drawn when every section is empty.
  - The channel header is in `render_main` (~5940): the channel icon and name, then search, Create invite, Pins and Members.
  - Dialogs:
    - `JoinDialog` and `CreateInviteDialog` (`src/ui/invites.rs`) are their own entities, with an `InputState`, a busy flag, an error line, a `WeakEntity<ChatView>` and a cloned client. They're opened with `window.open_dialog` at a width of 420. Copy their shape.
    - Confirmations use `window.open_alert_dialog` with `ButtonVariant::Danger`, as `confirm_delete` (~2656) and `confirm_relationship` (~3173) do.
    - The external-link prompt (~2495) has a checkbox in an alert dialog: the model for "Don't notify others".
  - `invite_joined` (~4332) is the model for "a server was added; show it". `open_channel` (~4477) opens a channel and switches to its server.
  - A markdown body outside a message is the profile card bio: `markdown::prepare` plus `store::render_key` (`chat.rs` ~3936), drawn with `TextView::markdown` in `profile_card.rs` (~222), with links through `People::link_handler`.
  - Banners: `profile_card.rs` draws one from a URL (~65). `MediaUrls::file` makes the URL.
- **App menu:** `set_menus` (`src/main.rs` ~27), "Join Server…" at ~43 with its handler at ~152 routing to `root.shown_chat()`; actions in `src/ui/actions.rs` (~17).
- **Error text:** `profile_edit::error_text(err, fallback)` (`src/profile_edit.rs` ~384) handles rate limits and `FailedValidation`. `invites::join_error_text` (~475) already words `TooManyServers`.

**Decisions** (user, 2026-11-07):
1. **The rail's "+"** opens a small menu, "Create a server…" and "Join a server…". The Stoat menu gets "Create Server…" beside "Join Server…".
2. **Deleting a server** is confirmed by typing its name.
3. **Deleting a server waits for milestone 23** and goes in Server Settings, as in the web client. This milestone offers no way to delete a server.
4. **Renaming and deleting categories are included,** as in the web client's category menu. Milestone 26 keeps reordering channels and categories.
5. **Info dialogs** follow the web client (server name above the channel list; channel name in the header; the description's first line in the header), and **also show the icon and banner.**
6. **The rest follows the web client:** where Create channel and Create category appear, the "Don't notify others that you've left" checkbox, and deleting a channel always asking (Shift doesn't skip it).

**Rejected alternatives.**
- **Creating a channel straight into a category in one request.** Stoat has no such field. The create-then-PATCH pair is the only way.
- **Waiting for `ServerCreate` before opening a new server.** The response already has the server and General. `add_server` replaces, so adding it at once and then again from the event is safe, as for joins.
- **Making category IDs from the clock alone,** or a counter. Stoat only checks the length, but two clients creating at the same moment must not collide. A ULID (time plus 80 random bits) is what the web client makes.
- **`remove: ["Categories"]` to delete the last category.** That needs Manage Server; `"categories": []` needs only Manage Channels and has the same effect.

## Assumptions
1. **Empty categories are shown.** A category is left out only when it lists channels that exist on the server (`server.channels`) and you can see none of them. A brand-new category, or one whose channels were all deleted, shows with no rows. The web client shows them all; this keeps a category of private channels hidden from people who can't see them.
2. **Deleted channels are removed locally** from `server.channels`, every category, and every per-channel state, both on `ChannelDelete` and after your own delete succeeds (whichever comes first; the second does nothing). Stoat leaves stale IDs in categories, so `server_channels` already ignores IDs with no channel. The next category edit sends the cleaned list.
3. **The server's system-message channels aren't cleared locally** when one is deleted. Nothing in RuSToat reads them yet. Milestone 23's overview edits them, and fetches the server first.
4. **Create channel in a category:** after the create succeeds, build the categories from the store, remove the new ID everywhere, append it to the target category, and PATCH. If the category was deleted meanwhile, skip the PATCH. If the PATCH fails, keep the channel and toast "Created #name, but couldn't move it into {category}."
5. **Category edits are built from the store's current categories,** then sent whole. A concurrent edit by someone else in the same second can be lost. That's how the web client works, and Stoat offers nothing finer.
6. **After a category PATCH,** the response's `categories` are put in the store at once, so the sidebar updates without waiting for `ServerUpdate`.
7. **Where the new channel or category appears:**
   - From a category's menu, "Create channel" goes into that category, and the dialog says "in {CATEGORY}".
   - From a channel's menu, it goes into the channel's category, or uncategorised.
   - From the list's background, uncategorised.
   - "Create category" always appends at the end.
   - Creating a channel opens it. Creating a category doesn't move you.
8. **The category ID** is a new ULID from the `ulid` crate (its `rand` is already in the tree). If adding the crate is a problem, write a 26-character Crockford base32 ULID from `chrono` and `getrandom`'s bytes.
9. **Who sees what:**
   - Create channel and Create category, and Rename and Delete category: server-level `ManageChannel` (`server_permissions`), which is what Stoat checks.
   - Delete channel: `ManageChannel` on that channel (`channel_permissions`), as Stoat does.
   - Leave server: everyone but the owner.
   - A server whose membership isn't known yet shows none of these.
10. **Create server** has only a name field. The web client's line "By creating this server, you agree to the Acceptable Use Policy." (with the link) is shown **only on the official instance** (`app` is `https://stoat.chat`); self-hosted instances have their own rules. After creating: add the server, `fetch_own_member`, open General, and no toast.
11. **Create channel** has the name and a Text / Voice choice (gpui-component's `Radio`), as in the web client. There's no description or NSFW: those come with channel settings (milestone 26).
12. **Leave server:** after the 204, run `leave_server` at once (the `ServerMemberLeave` that follows does nothing). `NotFound` counts as done. Toast "Left {name}." to confirm the server didn't just vanish.
13. **Delete channel:** the confirmation stays open, disabled, while the request runs, and shows the error in a toast on failure. `NotFound` counts as done. If you were in the channel, the sidebar's first channel opens (`current_channel` already falls back).
14. **Error texts:**
    - **Create server:** `TooManyServers { max }`: "You're in {max} servers, the most Stoat allows. Leave one to create another." `CantCreateServers`: "This Stoat instance doesn't let you create servers."
    - **Create channel:** `TooManyChannels { max }`: "This server already has {max} channels, the most Stoat allows." `MissingPermission`: "You can't create channels in this server."
    - **Categories:** `MissingPermission`: "You can't manage channels in this server." `InvalidOperation`: "Stoat refused that change to the categories. Try again."
    - **Delete channel:** `MissingPermission`: "You can't delete this channel."
    - **Anything else:** `profile_edit::error_text(err, "Couldn't … Try again.")`, which already says how long to wait after a rate limit, and words `FailedValidation`.
    - Names are trimmed and checked for 1–32 characters before sending, with the button disabled until they are, so `FailedValidation` is unexpected.
15. **The description's first line** in the header is the first non-empty line, as plain text (markdown left as typed), truncated to the header's width. It isn't shown for DMs, groups or Saved Notes. Groups' descriptions are milestone 20.
16. **Info dialogs:**
    - **Server info** (440 px): the banner across the top (if any), then the icon (48 px, or initials) beside the name, then the description as markdown (selectable, links through `People::link_handler`), or "No description." in muted text. Close.
    - **Channel info** (440 px): the channel's icon (or its `#` / speaker glyph) beside `#name`, then the description the same way. Close.
    - The web client's Settings, Edit Identity and Report buttons aren't added. Edit identity is already in the server menu; settings are milestone 23; reports are milestone 22.
    - Both re-read the store when drawn, so a live edit shows, and close themselves if the server or channel disappears.
17. **No keyboard shortcuts** for any of this.
18. **Logging:** server, channel and category IDs at debug level. Names aren't sensitive.

## Plan

### Chunk 1: Create a server
**Goal:** The rail's "+" offers "Create a server…" and "Join a server…", and Stoat → Create Server… opens the same dialog, which creates a server and opens it.
**Files likely touched:**
- `src/api/client.rs`: `create_server(name) -> CreateServerLegacyResponse` (`POST /servers/create`), sending a private struct with just `name` (`DataCreateServer` would send `"description": null`). Add the imports.
- `src/server_edit.rs` (new, pure, no GPUI):
  - `valid_name(text) -> Option<String>`: trimmed, 1–32 characters (Stoat counts chars, check with `chars().count()`)
  - `create_server_error_text(&ApiError) -> String` (Assumption 14)
- `src/ui/server_dialogs.rs` (new): `CreateServerDialog`, its own entity like `JoinDialog`: the name field with a "{n}/32" counter, the AUP line on the official instance, Close and Create (`loading` while creating), the error line.
- `src/ui/chat.rs`:
  - `open_create_server(window, cx)`
  - `server_created(response, cx)`: `add_server(server, channels, [])`, `fetch_own_member`, then `open_channel` on the first channel (or `select_place(Place::Server)` if none). Share the body with `invite_joined` where they overlap.
  - `rail-join` becomes `rail-add`: a left click opens a `PopupMenu` with "Create a server…" (`IconName::Plus`) and "Join a server…" (`IconName::LogIn` or similar). Tooltip "Create or join a server".
- `src/ui/actions.rs`, `src/main.rs`: a `CreateServer` action and "Create Server…" before "Join Server…", routed through `root.shown_chat()` like `JoinServer`.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.

**Depends on:** None.
**Risks:**
- **A left-click menu on the rail.** `rail_button` only wires right-click menus. If gpui-component can't anchor a `PopupMenu` to a plain `div` on left click, make the "+" a ghost `Button` styled like the rail and use `.dropdown_menu(...)`, as `CreateInviteDialog`'s pickers do. Check the menu opens to the right of the rail and isn't clipped by the window edge.
- **Response then event.** `add_server` from the response, then `ServerCreate`, must give one rail entry and one `fetch_own_member`. `tests/store_servers.rs` already covers this for joins; add a case with the create response's shape (no categories key).
- **The menu-bar item with no account on screen** (the login view) must do nothing, as Join Server… does.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/server_edit.rs` unit tests: `valid_name` for "", "   ", " Lounge " ("Lounge"), 32 and 33 characters (including multi-byte ones: 32 "é" pass); `create_server_error_text` for `TooManyServers { max: 100 }`, `CantCreateServers` and a rate limit.
- `tests/api_servers.rs` (new, wiremock): `POST /servers/create` sends exactly `{"name":"Lounge"}` with the session token and parses a `{ server, channels }` response with one "General" channel; a 400 `{"type":"TooManyServers","max":100}` stays `ApiError::Api`.
- `tests/store_servers.rs`: the create response then `ServerCreate`, and the reverse, give one entry at the end of the rail with General.

**Manual check:**
- Click "+": the menu shows both items. Create a server: it appears at the end of the rail, General opens, and you can send a message.
- Stoat → Create Server… opens the same dialog. Join a server… still works.
- The Create button is disabled for an empty name and past 32 characters; the counter shows it.
- On stoat.chat the Acceptable Use Policy line shows and its link opens in the browser.

**Shippable when:** you can create a server from the rail or the menu bar and land in its General channel, and the tests pass.

### Chunk 2: Delete a channel, and follow deletions made elsewhere
**Goal:** Channels deleted anywhere disappear from RuSToat, and people with Manage Channels can delete one from its menu after confirming.
**Files likely touched:**
- `src/api/events.rs`: `ServerEvent::ChannelDelete { id }`, parsed and forwarded, with a parse test.
- `src/store.rs`:
  - `remove_channel(id) -> bool`: removes the channel, its ID from its server's `channels` and every category's `channels`, and its `histories`, `read_states` and `pending`. Works for any channel kind (groups and DMs come in milestone 20).
  - `server_channels` ignores category entries with no known channel (already true through `channel_entry`; keep it so).
  - `can_manage_channel(channel)`: `in_server && has(channel_permissions, ManageChannel)`.
- `src/permissions.rs`: `can_manage_channel(store, user, channel)`.
- `src/api/client.rs`: `delete_channel(id)` (`DELETE /channels/{id}`, no query, returns `()`).
- `src/server_edit.rs`: `delete_channel_error_text(&ApiError)`.
- `src/ui/chat.rs`:
  - factor `leave_server`'s per-channel cleanup into `forget_channel(id)`, used by both
  - the `ChannelDelete` handler: `store.remove_channel`, `forget_channel`, `drop_stale_selection`, `remember_places`, `ensure_current_loaded`
  - "Delete channel…" (destructive, `IconName::Delete`) at the end of `channel_menu`, before Copy channel ID's separator, when `can_manage_channel`
  - `confirm_delete_channel(id, window, cx)`: "Delete {name}?", "Once it's deleted, there's no going back.", Cancel and a Danger "Delete". On OK it sends; on success it removes the channel locally (Assumption 2); `NotFound` counts as success; other errors toast.

**Depends on:** None.
**Risks:**
- **Open panes and the selection.** Deleting the open channel must leave no pane, loading flag or pending ack behind, and must open another channel. Reuse exactly what `leave_server` clears per channel, so nothing is missed.
- **The channel's server.** `remove_channel` needs the server ID before the channel is gone: read it first.
- **A deleted channel in a folded category** must not leave the category header empty and stuck. Chunk 3 shows empty categories, which is fine; until then it disappears.
- **Drafts and per-channel settings** (mutes, notification levels, the draft store) keyed by the channel stay behind. They're harmless; don't chase them unless something reads them for a missing channel.
- **Banners in Notification Center** for the deleted channel: clear them as `leave_server` does.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/api/events.rs`: `{"type":"ChannelDelete","id":"c1"}` parses to `ChannelDelete`.
- `tests/store_channels.rs` (new, from the `ready.json` fixture): removing a categorised channel takes it out of `server.channels`, its category, `server_channels`, and its history; removing it twice returns false the second time; the category stays.
- `tests/api_servers.rs`: `DELETE /channels/c1` with the token and an empty body; a 403 `MissingPermission` stays an `Api` error.
- `tests/permissions.rs`: `can_manage_channel` is false with the default permissions, true for the owner, true for a role granting `ManageChannel`, false when a channel override denies it, and false in a DM.

**Manual check:**
- As a server's owner, delete a channel from its menu: after the confirmation it disappears, and if it was open, the first channel opens.
- Delete a channel from the web client: it disappears from RuSToat within a moment.
- As a member without Manage Channels, the menu has no Delete.

**Shippable when:** channels deleted anywhere disappear from RuSToat, and those allowed can delete one with confirmation, and the tests pass.

### Chunk 3: Create a channel, and show empty categories
**Goal:** People with Manage Channels can create a text or voice channel from the category menu, the channel menu or the channel list's background, landing in the right category.
**Files likely touched:**
- `src/api/client.rs`:
  - `create_channel(server, name, voice: bool) -> Channel` (`POST /servers/{id}/channels`, `{"type":"Text"|"Voice","name":…}`)
  - `edit_categories(server, categories: &[Category]) -> Server` (`PATCH /servers/{id}`, exactly `{"categories":[...]}`)
- `src/server_edit.rs`:
  - `with_channel_in(categories, channel, category_id) -> Option<Vec<Category>>` (Assumption 4; `None` if the category is gone)
  - `create_channel_error_text`, `categories_error_text`
- `src/store.rs`:
  - `server_channels` shows empty categories (Assumption 1)
  - `set_categories(server, categories)` (Assumption 6)
  - `can_manage_channels(server)`: server-level `ManageChannel`
  - `category_of(server, channel) -> Option<String>`
- `src/permissions.rs`: `can_manage_channels(store, user, server)`.
- `src/ui/server_dialogs.rs`: `CreateChannelDialog`: "Create channel", "in {CATEGORY}" when there's one, the name field with a counter, Text Channel / Voice Channel radios, Close and Create.
- `src/ui/chat.rs`:
  - `open_create_channel(server, category: Option<String>, window, cx)`
  - `channel_created(server, channel, category, cx)`: `add_channel`, the category PATCH if any, then `open_channel`
  - "Create channel…" in the category header's menu (above Copy category ID), in `channel_menu` (server channels), and in a new right-click menu on `channel-list`, each when `can_manage_channels`
  - "Nothing here yet." only when there are no sections at all

**Depends on:** None. (It touches the same menus as chunk 2.)
**Risks:**
- **The background menu catching right-clicks meant for rows.** Rows and category headers have their own `context_menu`. Check that right-clicking a row opens only the row's menu, and right-clicking the space below the last row opens the list's. If gpui-component opens both, put the list's menu on a filler `div` that takes the remaining height instead of on `channel-list`.
- **`ChannelCreate` arrives before the response, or after.** `add_channel` appends to `server.channels` only if absent, so either order gives one row. Test it.
- **The PATCH and `ServerUpdate` racing** with the store's own `set_categories`: both carry the same list, so the order doesn't matter.
- **Empty categories and folding.** A folded empty category must still toggle, and `drop_stale_selection` must not prune its fold (it checks `has_category`, which is fine).
- **Voice channels can't be joined yet** (milestone 33). Creating one is still useful from RuSToat; it shows with the voice icon, as voice channels already do.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/server_edit.rs`: `with_channel_in` moves a channel from one category to another, adds a new one at the end of its category, removes duplicates, and returns `None` for a missing category.
- `tests/api_servers.rs`: `POST /servers/s1/channels` sends exactly `{"type":"Voice","name":"Lobby"}` and parses a `TextChannel` with `voice: {}`; `PATCH /servers/s1` sends exactly `{"categories":[{"id":"k1","title":"Chat","channels":["c1"]}]}`; a 400 `TooManyChannels { max: 200 }` stays an `Api` error.
- `tests/store_ready.rs`: a category with no channels, and one whose only channel was deleted, show as empty sections; a category of channels you can't see stays hidden.
- `tests/store_channels.rs`: `ChannelCreate` then `add_channel` (and the reverse) gives one row; `set_categories` moves the row.
- `tests/permissions.rs`: `can_manage_channels` with the default permissions (false), as owner (true), and with a role granting it (true).

**Manual check:**
- From a category's menu, create a text channel: it appears in that category and opens.
- From a channel's menu in another category, create a voice channel: it lands in that category with the voice icon.
- From the list's empty space, create one: it's uncategorised, at the bottom of the top group.
- The web client shows the same placement.
- A member without Manage Channels sees none of the three items.

**Shippable when:** channels can be created in the right place from all three entry points, empty categories show, and the tests pass.

### Chunk 4: Create, rename and delete categories
**Goal:** People with Manage Channels can add a category, rename one, or delete one (keeping its channels) from the same menus.
**Files likely touched:**
- `Cargo.toml`: `ulid` (Assumption 8).
- `src/server_edit.rs`:
  - `new_category_id()`
  - `with_category_added(categories, id, title)`, `with_category_renamed(categories, id, title)`, `without_category(categories, id)`
- `src/ui/server_dialogs.rs`: one `NameDialog` for "Create a new category" ("Name", Create) and "Rename category" ("New name", prefilled, Rename). If chunk 1's and 3's dialogs share most of it, fold them onto it too, but don't force it.
- `src/ui/chat.rs`:
  - "Create category…" in the category, channel and background menus; "Rename category…" and "Delete category…" (destructive) in the category menu; each when `can_manage_channels`
  - `confirm_delete_category(server, id, window, cx)`: "Delete {TITLE}?", "Its channels won't be deleted. They'll move out of the category.", Cancel and Danger "Delete"
  - all three send `edit_categories` and apply the response with `set_categories`; errors toast

**Depends on:** Chunk 3 (`edit_categories`, `set_categories`, `can_manage_channels`, the background menu, empty categories).
**Risks:**
- **A collapsed category deleted,** or one renamed while folded. The fold is keyed by ID, so renaming keeps it, and `drop_stale_selection` prunes the fold of a deleted one.
- **The last category.** `without_category` can return an empty list; send `"categories": []`, not `remove` (it needs Manage Server).
- **Stale channel IDs** in the categories (Assumption 2) are sent back as they are; Stoat drops unknown IDs, so they clean themselves up. Don't send a channel twice: `with_channel_in` and these helpers must keep each ID once.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/server_edit.rs`: adding appends with an empty channel list; renaming changes only that title; deleting removes only that category; a missing ID changes nothing; `new_category_id` is 26 characters and two calls differ.
- `tests/api_servers.rs`: `PATCH /servers/s1` with `[]` sends exactly `{"categories":[]}`.
- `tests/store_channels.rs`: `set_categories` without a category moves its channels to the uncategorised section.

**Manual check:**
- Create a category: it appears at the bottom, empty. Create a channel in it from its menu.
- Rename it: the header changes here and in the web client.
- Delete it: its channels move to the top group, and none are deleted.
- Fold a category, rename it: it stays folded.

**Shippable when:** categories can be created, renamed and deleted by those allowed, and the tests pass.

### Chunk 5: Leave a server
**Goal:** Anyone but the owner can leave a server from its rail menu, optionally without the "left" message.
**Files likely touched:**
- `src/api/client.rs`: `leave_server(id, silently: bool)` (`DELETE /servers/{id}?leave_silently=true|false`, returns `()`). Name it `leave_server`, and document that for the owner the same request deletes the server; the UI must never call it for the owner.
- `src/server_edit.rs`: `leave_error_text`.
- `src/store.rs`: `is_owner(server) -> bool`.
- `src/ui/chat.rs`:
  - "Leave server…" (destructive, at the end of `server_menu`, before the separator) when you aren't the owner and your membership is known
  - `confirm_leave_server(id, window, cx)`: "Leave {name}?", "You won't be able to rejoin unless you are re-invited.", a "Don't notify others that you've left" checkbox (unchecked), Cancel and Danger "Leave". On success: `leave_server` locally, then toast "Left {name}." (Assumption 12)

**Depends on:** None.
**Risks:**
- **The owner check must be right.** If `server.owner` is ever stale (an ownership transfer the store missed), "Leave" would delete the server. `ServerUpdate` carries `owner`, and `apply_server_update` applies it; add a test that the store follows an owner change. As a second guard, re-read the owner from the store at the moment of sending, not when the menu was built.
- **The checkbox in an alert dialog.** The external-link prompt already does this (`link_prompt`); copy its state handling.
- **Leaving the server on screen** must go Home, as `leave_server` already does for `ServerMemberLeave`.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `tests/api_servers.rs`: `DELETE /servers/s1?leave_silently=true` and `=false` (match the query), empty body, 204 → `Ok(())`.
- `tests/store_servers.rs`: `is_owner` for an owned and a joined server, and after a `ServerUpdate` changing `owner`.

**Manual check:**
- Leave a server you joined, unchecked: it disappears, the toast shows, and the web client shows "{you} left" in its system channel (if it has one).
- Join again and leave with the box checked: no message.
- On a server you own, the menu has no Leave.

**Shippable when:** non-owners can leave with or without notice, owners can't, and the tests pass.

### Chunk 6: Server and channel info
**Goal:** The server's name above the channel list opens its banner, icon and description. The channel's name in the header opens its icon and description, and the header shows the description's first line.
**Files likely touched:**
- `src/server_edit.rs` (or `src/markdown.rs`): `first_line(description) -> Option<&str>` (Assumption 15).
- `src/ui/server_dialogs.rs`: `render_server_info` and `render_channel_info` bodies (Assumption 16), using `MediaUrls::file` for the icon and banner and the profile card's markdown setup for the description.
- `src/ui/chat.rs`:
  - the channel list's title becomes clickable (`cursor_pointer`, hover colour, tooltip "Server info") when a server is shown, opening `open_server_info(id)`
  - in the header, the channel name and, after it, the description's first line (muted, normal weight, truncated, tooltip "Click to show full description") open `open_channel_info(id)`, for server channels only
  - the dialogs re-read the store each render and close if their server or channel is gone

**Depends on:** None.
**Risks:**
- **Header width.** The name plus the description must truncate cleanly before the search box and buttons. Give the name `flex_none` with a max width and the description `flex_1 min_w_0 truncate`.
- **Banner loading.** Reuse the profile card's banner element (fixed height, cover) so a slow image doesn't reflow the dialog.
- **Markdown cache keys.** Use `render_key("server-info-{id}", ...)` and `"channel-info-{id}"` so an edited description re-renders.
- **Links in descriptions** go through `People::link_handler`, so external links get the usual warning and invite links open the Join dialog.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `first_line`: `None` for `None`, "" and "\n\n"; "Rules\nmore" gives "Rules"; "\n  Hi  \nx" gives "Hi".

**Manual check:**
- Click a server's name with a banner, an icon and a markdown description: all three show, and the markdown renders.
- A server with none of them: initials and "No description."
- A channel with a description shows its first line in the header; clicking the name or the line shows it all.
- Edit a description in the web client while the dialog is open: it updates.
- Light and dark themes are readable.

**Shippable when:** both info dialogs show icon, banner (servers) and description, the header shows the first line, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`. Never run it on a `mod.rs`, `lib.rs` or `main.rs`, since it formats their child modules too. For `src/main.rs`, use `--check` and hand-format.
- A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines (from `rustfmt --edition 2024 --check <file>`) hasn't grown. Starting counts on 2026-11-07:
  - with drift: `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15
  - clean: `src/permissions.rs`, `src/main.rs`, `src/ui/actions.rs`, `src/ui/invites.rs`, `src/invites.rs`, `src/ui/profile_card.rs`, `tests/store_servers.rs`, `tests/permissions.rs`, `tests/store_ready.rs`
  - New files must be clean.

### Unit Tests
- **`src/server_edit.rs`:** name validation (trimming, 1–32 characters, multi-byte), category list edits (add, rename, delete, move a channel in, missing IDs, no duplicates), category IDs, the description's first line, and every error text.
- **`src/api/events.rs`:** `ChannelDelete` parses.

### Integration Tests
- **`tests/api_servers.rs`** (new, wiremock): create server, create channel (text and voice), edit categories (a list, and `[]`), delete channel, leave server (both query values); exact bodies and paths; `TooManyServers`, `TooManyChannels` and `MissingPermission` stay `Api` errors.
- **`tests/store_channels.rs`** (new): removing a channel, `ChannelCreate` and the create response in either order, `set_categories`.
- **`tests/store_servers.rs`:** the create response and `ServerCreate` in either order; `is_owner`, including after an owner change.
- **`tests/store_ready.rs`:** empty categories shown; categories of channels you can't see hidden.
- **`tests/permissions.rs`:** `can_manage_channel` and `can_manage_channels`.

### Manual Tests
You need an `api.stoat.chat` account that owns a test server (chunk 1 makes one), a second account (or the web client) to watch from, and a server you joined but don't own.
1. Run every chunk's manual check.
2. With two accounts in RuSToat, the "+" menu, the menu-bar item and every server and channel menu act for the account on screen.
3. Delete a channel the other account has open: it disappears there too and another channel opens.
4. Light and dark themes: every dialog, the radios, the checkbox and the header's description line are readable.
5. Clean up: delete the test server from the web client (RuSToat gets that in milestone 23).

## Definition of Done
- [ ] The rail's "+" opens a menu with "Create a server…" and "Join a server…"; Stoat → Create Server… opens the same dialog. Creating sends `{"name":…}`, adds the server once at the end of the rail, and opens General. `TooManyServers` and `CantCreateServers` have their own messages.
- [ ] `ChannelDelete` removes the channel from the sidebar, its categories and every per-channel state, and moves you off it if it was open.
- [ ] "Delete channel…" shows only with Manage Channels on that channel, always asks first, and removes the channel when Stoat answers 204 or `NotFound`.
- [ ] "Create channel…" shows only with server-level Manage Channels, in the category menu, the server-channel menu and the list's background; it creates text or voice channels in the category it was started from, and opens them.
- [ ] Empty categories show; categories whose channels you can't see stay hidden.
- [ ] Categories can be created (appended, with a ULID), renamed and deleted (their channels kept) by those with Manage Channels, each sending the whole list as `{"categories":[…]}`.
- [ ] "Leave server…" shows for every server you don't own and never for one you do; it sends `leave_silently` from the checkbox, removes the server, and toasts.
- [ ] Clicking the server name above the channel list shows its banner, icon and markdown description; clicking the channel name or its first description line in the header shows its icon and description.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/servers-and-channels-implementation.md`.
