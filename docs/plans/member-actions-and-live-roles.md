# Plan: Member actions and live role edits (RuSToat milestone 7)

## Summary
This milestone makes people in RuSToat clickable, and keeps servers' roles current while the app runs:
- **A profile card** opens when you click a person: in the member list, on a message author's avatar or name, on a reply preview's author, on a name in a system message, or on an `@mention`. It shows their banner, avatar and presence, names, pronouns, status, roles, join dates and bio.
- **"Message"** on the card and in the menu opens (or creates) a DM with them and switches to it.
- **A right-click menu** on the same places offers Profile, Message, Mention, friend requests, block and unblock, Copy username and Copy user ID. The card's "…" button opens the same menu.
- **Live server and role edits:** `ServerUpdate`, `ServerRoleUpdate` (which also announces new roles), `ServerRoleDelete` and `ServerRoleRanksUpdate` are applied as they arrive. Role names, colours, hoisting, order and permissions all update without a reconnect. `ServerCreate` (joining a server, possibly on another device) and `ServerDelete` are handled too.

## Approach
Keep the milestone 1–6 architecture:
- **Pure model, thin views.** Every rule lives in GPUI-free code and is unit-tested. That covers what the card shows, which menu items appear, who can be messaged, and how a role event changes the store. The views only wire it up.
  - A new `src/profile.rs`, in the style of `src/permissions.rs`, holds free functions over `&ChatStore`: `profile_card`, `user_menu`, `can_message` and `dm_with`.
  - `src/store.rs` (2,900 lines) gains only the mutators: `apply_server_update`, `apply_role_update`, `apply_role_delete`, `apply_role_ranks`, `add_server` and `apply_relationship`.
- **A new view file.** `src/ui/chat.rs` is 5,700 lines, so the card's rendering goes in a new `src/ui/profile_card.rs`. `ChatView` owns its state, as it does for the image viewer.
- **Wire types** come from `revolt-models` 0.16.2. Its partial types derive `OptionalStruct`, so `Server::apply_options(PartialServer)` and `Role::apply_options(PartialRole)` exist, like the `Member::apply_options` and `User::apply_options` the store already uses.
- **The web client is the reference for behaviour.** That's `stoatchat/for-web` `56bc94c` and the `stoat.js` SDK `e88f6aa`, on top of the backend `stoatchat/stoatchat` `df96bca`, all read 2026-11-05.

**Codebase context** (branch `milestone-7-member-actions-y5xw`, on top of milestone 6's `3b89e68`):
- **Events** (`src/api/events.rs`):
  - `ServerEvent` is a hand-written enum decoded by `decode()`. Unknown types become `ServerEvent::Unknown(kind)`.
  - The test at ~line 930 asserts that `ServerRoleUpdate` is `Unknown`, so it must change.
  - `ChatView::on_connection_event` (`src/ui/chat.rs` ~line 476) applies each event. `ConnectionEvent::Event(_) => return` swallows the rest.
- **Member list:**
  - `ChatStore::member_list` / `server_member_list` (`src/store.rs` ~line 2171) build `MemberListRow`s from `server.roles` (hoist, rank) and `role_colour`.
  - `ChatView.member_lists` caches them per channel. `forget_member_lists()` empties the cache.
  - Rows are drawn by `render_member_row` (`src/ui/chat.rs` ~line 3884) inside a `uniform_list`. They have no click handlers yet.
- **Permissions** (`src/permissions.rs`): the `revolt-permissions` maths over the store. Its module doc says `ServerUpdate` and `ServerRoleUpdate` aren't applied yet. That note goes away in this milestone.
- **Message rows:**
  - `MessageRow` has `author` (a display name) and `avatar`, but **no author ID**.
  - `ReplyPreview` likewise has no author ID. `render_reply_preview` makes the whole line jump to the original.
  - System messages are plain text from `ChatStore::system_text`.
  - Mentions render as `[@Name](stoat:user/{id})` links (`src/markdown.rs` ~line 412, `INTERNAL_SCHEME = "stoat:"`). `on_link_click` currently ignores them.
- **Menus and dialogs:**
  - Right-click menus use gpui-component's `context_menu` with `PopupMenuItem`; see `render_message_row`.
  - Confirmations use `window.open_alert_dialog`; see `confirm_delete` (~line 1774). Shift skips the confirmation.
  - Toasts use `window.push_notification(Notification::error(..))`.
- **Overlays:** the image viewer is `ChatView.viewer: Option<ViewedImage>`, drawn last in `render` over everything else, with its own focus handle for Esc. The card follows the same pattern.
- **Channels:**
  - `ChatView::open_channel(channel)` switches place and selects a channel.
  - `ChatStore::add_channel` adds one.
  - DMs with `active: false` are hidden from Home.
- **Composer:** mentions are chips (`with_chips`, `learn_labels`). There's no public "insert text" API yet; `insert_emoji` is the closest pattern.
- **Users:** `User.relationship` (the logged-in user's relationship to that user) is in the store, but nothing outside `permissions.rs` reads it.
- **Time:** `src/time.rs` has `ulid_local_time(id)`, for "account created" dates.

**API facts** (backend source; the full research is in this task's conversation):
- **`ServerUpdate`:** `{ type, id, data: PartialServer, clear: ["Description"|"Categories"|"SystemMessages"|"Icon"|"Banner"] }`.
  - It's sent for `PATCH /servers/{id}`, for ownership transfer, and for `PUT /servers/{id}/permissions/default` (`data: { default_permissions }`).
  - It never carries `roles` in this version, but the client should tolerate it if it does.
- **`ServerRoleUpdate`:** `{ type, id: server, role_id, data: PartialRole, clear: ["Colour"|"Icon"] }`.
  - **A newly created role arrives this way, with the whole role in `data`:** `name`, `permissions: {a, d}`, `hoist`, `rank` (= the old role count, so it's the lowest priority), and `colour` and `icon` only if set.
  - There's no separate "role created" event, so an unknown `role_id` must be inserted from the partial. Use the top-level `role_id`, not `data`'s id key (whether that key is `id` or `_id` is unverified).
  - An edit sends only the changed fields among `name`, `colour`, `hoist` and `icon`.
  - Setting a role's permissions (`PUT /servers/{id}/permissions/{role}`) sends `data: { permissions: {a, d} }`.
  - Rank changes never come through this event.
- **`ServerRoleDelete`:** `{ type, id, role_id }`. **Nothing else is sent.** The backend removes the role from members and channel overrides in its database, but sends no `ServerMemberUpdate` or `ChannelUpdate`, so the client must remove the role ID itself.
- **`ServerRoleRanksUpdate`:** `{ type, id, ranks: [role_id, …] }`.
  - It lists every role, and a role's index is its rank. **Index 0 is the highest priority**, and a smaller rank wins.
- **`ServerCreate`:** `{ type, id, server: Server, channels: [Channel], emojis: [Emoji], voice_states: [...] }`, sent to the joining user's own session.
  - **The joiner's own session never receives its own `ServerMemberJoin`**: it's published to the server topic before that session subscribes.
  - So fetch your own member with `GET /servers/{id}/members/{me}`. It returns a bare `Member`, is in the `servers` rate-limit bucket, and returns 404 if you're not a member.
- **`ServerDelete`:** `{ type, id }`. Only sent when the owner deletes the server. Leaving, kicks and bans send `ServerMemberLeave`, which is already handled.
- **`GET /users/{id}/dm`** returns a `Channel`:
  - for another user, `DirectMessage`, created with `active: true` if it's new
  - for yourself, `SavedMessages`

  More details:
  - **It needs `SendMessage` on the user:** you're friends, or one of you is a bot and you share a server or group. **For a human who isn't your friend it returns 403 `MissingPermission { permission: "SendMessage" }`, even when you share a server.** A blocked user fails the same way.
  - An existing DM is returned unchanged, even when it's inactive, and nothing is sent over the websocket.
  - A new DM sends `ChannelCreate` to both people.
  - stoat.js reuses a cached DM and sets `active = true` itself.
- **`GET /users/{id}/profile`** returns `UserProfile { content?: markdown (≤ 2000 chars), background?: File }`.
  - It needs `ViewProfile`: a friend, a shared server, group or DM, or a bot. Otherwise it returns 403 `MissingPermission`.
- **Friends and blocks.** Each returns 200 with the **target `User`**, whose `relationship` is the new one:
  - `PUT /users/{id}/friend`: send a request, or accept one.
  - `DELETE /users/{id}/friend`: reject, cancel or remove.
  - `PUT /users/{id}/block` and `DELETE /users/{id}/block`.
  - **Errors:**
    - `AlreadyFriends`, `AlreadySentRequest` and `Blocked`: 409.
    - `BlockedByOther`: 403.
    - `NoEffect`, `IsBot` (friending a bot, or as a bot) and `TooManyPendingFriendRequests { max }`: 400.
  - **Bots** can be blocked but not friended.
- **`UserRelationship`:** `{ type, id: recipient, user: User }`, where `user` is the other person, with `relationship` as the recipient sees it. There's no `status` field. Both people receive one for every friend or block change.
- **`Ready.users`** already includes everyone you have a relationship with (friends, requests and blocks), with `relationship` set.
- **Rate limits:**
  - `/users/*` (DM, profile, friend and block): 20 per 10 s, shared.
  - `/servers/{id}/*`: 5 per 10 s per server.

**How the web client behaves** (`UserCard.tsx`, `ProfileActions.tsx`, `UserContextMenu.tsx`, `MemberSidebar.tsx`, `Message.tsx`):
- **Click opens the card, right-click opens the menu.** This works on member rows, message author avatars and names, system-message users, reply-preview authors and mentions.
- **The card fetches `GET /users/{id}/profile` when it opens**, and shows:
  - the banner
  - a 48 px avatar with presence
  - the display name (only if it differs from the username)
  - `username#discriminator`, which you click to copy
  - pronouns (member first, then user)
  - actions
  - roles, highest first, each with a colour dot
  - badges
  - the custom status
  - join dates: the account, and the server when there's a membership
  - the bio as Markdown
- **Card actions:** Add Friend, Accept/Cancel request, Message (friends only), Add Bot, and a "…" button that opens the context menu.
- **The context menu**, in order (only the items in this milestone's scope):
  1. Profile
  2. Message (friend or bot)
  3. Mention (server channels)
  4. friend requests (Add, Accept and Reject, Cancel)
  5. Remove friend, Block or Unblock
  6. Copy user ID

**Rejected alternatives:**
- **A gpui-component `Popover` attached to each clickable element.** Mentions are links inside a `TextView`, which has no element to attach a popover to. Rows in the member list's `uniform_list` and the message scroller are also recycled. One `ChatView`-owned card, placed at the click position with `anchored()`, works the same from every trigger.
- **Rebuilding servers from a fresh `Ready` on every role event** (a forced reconnect). That's what users see today, and it discards loaded history.

## Assumptions
1. **"Message" shows for friends, bots, and anyone you already have a DM with, but never for yourself** (your answer to question 1).
   - With an existing DM, the client opens it locally. If it's inactive, it sets `active = true` locally, as stoat.js does, so it shows in Home. No request is made.
   - Otherwise it calls `GET /users/{id}/dm`, adds the channel and opens it.
   - A 403 or other error shows a toast: "You can only message friends and bots."
2. **The right-click menu** is Profile, Message, Mention, the friend actions, Block or Unblock, Copy username and Copy user ID (your answer to question 2, option c).
   - **Not included:** moderation (kick, ban, timeout), edit identity, edit roles, report, and group member removal or ownership transfer.
   - Each item's visibility follows the web client:
     - **Mention:** only in server channels and groups, and not for yourself.
     - **Add friend:** relationship is `None`, and it isn't a bot or yourself.
     - **Accept / Reject:** relationship is `Incoming`.
     - **Cancel request:** relationship is `Outgoing`.
     - **Remove friend:** relationship is `Friend`.
     - **Block:** not yourself, and not already `Blocked`.
     - **Unblock:** relationship is `Blocked`.
     - **Copy username and Copy user ID:** always.
   - The web client hides Copy user ID behind a developer setting; this client always shows it, since there's no such setting.
3. **The card and menu open from everywhere the web client opens them** (your answer to question 3, option c): member rows, message author avatars and names, reply-preview authors, user names in system messages, and `@mention` links.
   - On a reply preview, the avatar and name open the card; the rest of the line still jumps to the original.
   - For mentions only left-click works (it opens the card). A link inside a `TextView` can't take a right-click menu, so the card's "…" button gives mentions the menu.
   - The author of a masqueraded message (a bridge or bot posting under another name) opens the card of the real author ID. The web client does the same.
4. **Remove friend and Block ask for confirmation first, unless Shift is held**, like Delete and Pin. The other friend actions take effect at once.
   - The success toast is skipped, because the card and menu change right away.
   - Errors show Stoat's message as a toast. `AlreadyFriends`, `AlreadySentRequest` and `NoEffect` count as success, since another client got there first.
5. **Card contents:** everything the web card shows except badges and the "Add Bot" button. Badges need icon assets, and inviting bots is out of scope.
   - A bot shows a "Bot" tag next to its name instead.
   - The roles list is shown only for a server channel's members.
   - The bio is rendered with the same Markdown pipeline as messages (`markdown::prepare` plus `TextView`), so mentions in a bio are links too.
6. **The profile is fetched every time the card opens.**
   - The last result for that user (kept for the session) shows meanwhile, so reopening doesn't flash.
   - A 403 hides the bio and banner, and shows no error.
   - This is well within the `users` limit of 20 per 10 s.
7. **The card is placed at the click position**, snapped to stay inside the window (`anchored().snap_to_window()`). From the member list that puts it over the panel's left edge. It closes on a click outside it, Esc, a channel switch, or a reconnect.
8. **Role edits that hide a channel from you hide it at once.** `server_channels` leaves out channels the user can't view (`permissions::can_view`).
   - Channels a role edit makes visible appear after the next reconnect, since `Ready` is the only source of the channel objects.
   - While your own membership of a server isn't known yet (just after a `ServerCreate`), that server's channels aren't filtered.
9. **A `ServerRoleUpdate` for an unknown role without a `name`** is logged and ignored. The next reconnect corrects it. A role can't be built without a name.
10. **A `ServerCreate` appends the server to the end of the rail.** The user's saved ordering (`user_settings[ordering]`) isn't re-read live.
11. **Copy username** copies `username#discriminator`, matching the card's click-to-copy.

## Open Questions
- *(Deferred)* **Moderation** in the menu: timeout, kick, ban, and group member removal.
- *(Deferred)* **Edit identity and Edit roles.**
- *(Deferred)* **Report user, Add Bot, badges, and mutual servers or friends.** The web client shows mutuals only in the full profile modal.
- *(Deferred)* **Channels that become visible after a role edit**, without a reconnect. The backend doesn't seem to send `ChannelCreate` for them; this is unverified.

## Plan

### Chunk 1: Live role and server edits
**Goal:** Role names, colours, hoisting, order and permissions, and server name, icon and default permissions, all update live, so the member list, role colours, "can pin" and the channel list stay correct without a reconnect.
**Files likely touched:**
- `src/api/events.rs`:
  - new variants:
    - `ServerUpdate { id, data: Box<PartialServer>, clear: Vec<FieldsServer> }`
    - `ServerRoleUpdate { server, role_id, data: Box<PartialRole>, clear: Vec<FieldsRole> }`
    - `ServerRoleDelete { server, role_id }`
    - `ServerRoleRanksUpdate { server, ranks: Vec<String> }`
  - decoding in `decode()`, in the style of `ChannelUpdate` (`#[serde(default)] clear`)
  - replace the `ServerRoleUpdate`-is-Unknown test with parse tests for all four, including a creation payload carrying a full role
- `src/store.rs`:
  - `apply_server_update(id, data, clear)`: uses `Server::apply_options`, then clears the `FieldsServer` fields.
  - `apply_role_update(server, role_id, data, clear)`:
    - merges into an existing role with `Role::apply_options`, and clears `Colour` / `Icon`
    - an unknown role is built from the partial: `name` is required; `permissions` defaults to `{a:0,d:0}`, `rank` to the role count, `hoist` to false (Assumption 9)
  - `apply_role_delete(server, role_id)`: removes the role from `server.roles`, from every cached member of that server's `roles`, and from that server's channels' `role_permissions`.
  - `apply_role_ranks(server, ranks)`: sets `rank = index` for each listed role.
  - `server_channels`: leaves out channels `me` can't view, except while `me`'s membership of the server is unknown (Assumption 8).
- `src/permissions.rs`: update the module doc, which still says roles aren't applied live.
- `src/ui/chat.rs`: in `on_connection_event`, apply each event, then `forget_member_lists()`. Re-`sync_pane` each loaded channel of that server, since `can_pin` on rows depends on permissions. If the open channel has become hidden, `drop_stale_selection()`.
- `tests/store_roles.rs` (new):
  - a role rename, colour change and colour clear, hoist on and off: `member_list` sections and `role_colour` follow
  - a created role inserted from a partial
  - an unknown role without a name ignored
  - a deleted role gone from members, sections and colours
  - a ranks update reorders hoisted sections and changes which colour wins
  - a role permission change flips `permissions::can_pin` and `can_view`
  - `default_permissions` via `ServerUpdate` flips `can_view`
  - `server_channels` hides a channel after a role edit denies `ViewChannel`
  - every channel in a `Ready` fixture stays visible (a guard against wrong permission maths)
**Depends on:** None
**Risks:**
- **Hiding channels from the list depends on the permission maths matching the backend's exactly.** If it hides a channel the user can in fact see, that's a visible regression. The `Ready` fixture test guards against it. If the maths disagrees with real data in the manual check, drop the `server_channels` filter and keep everything else; don't ship the filter half-working.
- **`OptionalStruct`'s `apply_options` field semantics:** confirm that `PartialRole.colour: Option<String>` sets the colour when `Some` and leaves it alone when `None`. Check this in a test before relying on it.
- **`decode_as` failure:** if `PartialRole` doesn't parse (for example, `permissions` isn't `{a, d}`), the event becomes `Unknown` and is silently lost. The creation-payload parse test covers this.
**How to verify:**
- `cargo test --test store_roles`
- `cargo test --lib events`
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- In a server you own, with the member list open in RuSToat and the web app open beside it:
  - Rename a hoisted role, change its colour, turn hoist off and on, and drag it above another role. RuSToat follows each change within a second.
  - Create a role, assign it and hoist it; then delete it.
  - Take Manage Messages from your own role in a test channel (or as a second account): "Pin message" disappears from the right-click menu.
**Shippable when:** All four events are decoded and applied, the member list, colours and permissions follow them live, and the tests pass.

### Chunk 2: Joining and deleted servers, live
**Goal:** A server you join or create (here or on another device) appears in the rail at once, with its channels usable. A server its owner deletes disappears.
**Files likely touched:**
- `src/api/events.rs`:
  - `ServerCreate { server: Box<Server>, channels: Vec<Channel>, emojis: Vec<Emoji> }`, with each list read through `lenient_list` and `voice_states` ignored
  - `ServerDelete { id }`
  - parse tests for both
- `src/api/client.rs`: `fetch_member(server, user) -> Member` (`GET /servers/{s}/members/{u}`).
- `src/store.rs`: `add_server(server, channels, emojis)`.
  - It inserts or replaces the server, its channels and its emojis, and appends the server to `server_order` if it's new (Assumption 10).
  - Replacing an existing server keeps its loaded histories.
- `src/ui/chat.rs`:
  - on `ServerCreate`: call `add_server`, then fetch your own member and apply it with `apply_member_join`. Log a failure and leave it; the next reconnect brings the membership.
  - on `ServerDelete`: call the existing `leave_server`.
- `tests/store_servers.rs` (new):
  - `add_server` adds the server to `servers()` and its channels to `server_channels`
  - the channels aren't filtered before your membership is known (Assumption 8)
  - a duplicate `ServerCreate` replaces the server without losing history
- `tests/api_users.rs` (new, `wiremock`): `fetch_member`'s path, and parsing a bare `Member`.
**Depends on:** Chunk 1 (the `server_channels` filter and its "membership unknown" exception)
**Risks:**
- **The rail order** isn't saved here; the server appears last. If the user then reorders servers in the web app, that comes in through `UserSettingsUpdate`, which only reads `notifications` today. That's acceptable; note it.
- **`ServerCreate`'s `channels`** may include channels the user can't see. Once your membership arrives, the chunk 1 filter hides them.
**How to verify:**
- `cargo test --test store_servers --test api_users`
- `cargo test --lib events`
**Manual check:**
- With RuSToat running, join a server through an invite in the web app: it appears in RuSToat's rail, and its channels open and load.
- Create a server in the web app: it appears.
- Delete it in the web app: it disappears from RuSToat, and Home opens if you were in it.
**Shippable when:** Joined, created and deleted servers show up or go away live, and the tests pass.

### Chunk 3: Profile card from the member list
**Goal:** Clicking someone in the member list opens a profile card with their banner, avatar, presence, names, pronouns, status, roles, join dates and bio.
**Files likely touched:**
- `src/profile.rs` (new, GPUI-free):
  - `profile_card(store, user, channel) -> Option<ProfileCard>` returns:
    - `name` (nickname, else display name, else username)
    - `username_tag` (`username#discriminator`)
    - `avatar_url` (the member's avatar first)
    - `presence`
    - `status`
    - `pronouns` (the member's first)
    - `bot`
    - `roles: Vec<(name, Option<colour>)>`, highest first, in server channels only
    - `joined_server: Option<(server name, date)>`
    - `created` (from `ulid_local_time`)
    - `relationship`
    - `is_me`
  - It reuses `presence_kind`, `role_colour`-style colour parsing, and `name_in`.
- `src/lib.rs`: add `pub mod profile;`.
- `src/api/client.rs`: `fetch_profile(user) -> UserProfile`.
- `src/ui/profile_card.rs` (new): `render_profile_card(card, profile, …)`, a fixed 340 px wide card:
  - the banner, if any
  - a 48 px avatar with a presence dot
  - the name, with a "Bot" tag where it applies
  - `username#discriminator`, which you click to copy
  - pronouns, status and roles (colour dots)
  - "Member since" for Stoat and for the server
  - the bio, through `markdown::prepare` and `TextView`
  - an actions row, empty for now (chunks 5–7 fill it)
- `src/ui/mod.rs`: register the module.
- `src/ui/chat.rs`:
  - `ChatView.card: Option<OpenCard { user, channel, position, profile: ProfileState }>`
  - `ChatView.profiles: HashMap<String, UserProfile>`, the per-session cache (Assumption 6)
  - `open_card(user, position, cx)`, which starts the fetch
  - drawn last in `render`, like the viewer, with `deferred(anchored().position(p).snap_to_window())`
  - closes on a click outside, Esc, a channel switch, or `Ready`
  - `render_member_row`'s `Member` rows get an `id`, `cursor_pointer` and `on_click` that call `open_card` with `window.mouse_position()`
- `tests/profile.rs` (new):
  - the name and pronoun precedence (member over user)
  - the role order (highest first, after a ranks change), with colours and gradient fallback
  - no roles outside server channels
  - the joined date from the member, and the created date from the ID
  - the bot flag
  - an unknown user gives `None`
- `tests/api_users.rs`: the `fetch_profile` path and parsing, and a 403 `MissingPermission` mapped to `ApiError`.
**Depends on:** None. It's independent of chunks 1–2, but chunk 1 makes the roles live.
**Risks:**
- **Closing on a click outside:** GPUI's `on_mouse_down_out` on the card must not fire for the click that opened it. If it does, the card opens and closes at once. Open it on mouse-up (`on_click`), or ignore mouse-downs for one frame after opening.
- **Esc:** focus must move to a card focus handle (as with `viewer_focus`), so that Esc closes the card and isn't taken by the composer. Give focus back to the composer when the card closes.
- **Bio Markdown:** `TextView` keeps its parse per element ID. Key the bio on the user plus a hash of the content, or a reopened card shows a stale bio.
**How to verify:**
- `cargo test --test profile --test api_users`
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- Click several people in a server's member list. Compare the card with the web app's card for the same person: roles and colours, pronouns, status, join dates, bio and banner.
- The card stays inside the window near the edges.
- A click outside the card, or Esc, closes it.
**Shippable when:** Member-list clicks open an accurate card that closes cleanly, and the tests pass.

### Chunk 4: Open the card from messages, replies, system messages and mentions
**Goal:** The card opens from everywhere the web client opens it, not only the member list (Assumption 3).
**Files likely touched:**
- `src/store.rs`:
  - `MessageRow.author_id: Option<String>`, which is `None` for pending rows that have no real author yet, else `message.author`
  - `ReplyPreview.author_id: Option<String>`
  - `system_text` gains a Markdown form: each user name becomes `[Name](stoat:user/{id})`, escaped as `markdown.rs` escapes mention names
    - keep the plain-text form for notifications and snippets
    - system rows render the Markdown form through `TextView`
- `src/markdown.rs`: a `parse_internal_link(url) -> Option<InternalLink::User(id) | Channel(id)>`, beside `INTERNAL_SCHEME`, plus a test.
- `src/ui/chat.rs`:
  - a `RowAction::OpenCard { user }`, which `ChatView` turns into `open_card` at `window.mouse_position()`
  - the author avatar and name in `render_message_row` emit it; when the row is a `preview` (search or pins), only the name and avatar do
  - in `render_reply_preview`, the avatar and name emit it and stop propagation, and the rest of the line keeps jumping
  - every `on_link_click` (message bodies, the bio, system rows) maps `stoat:user/{id}` to `OpenCard`, and still opens external links in the browser
- Tests:
  - `tests/store_replies.rs` and `tests/store_live.rs` check that `author_id` is set
  - the system-message Markdown form escapes names and links their IDs
  - `diff_rows` stays stable: `author_id` never changes for a row, so no extra remeasure
**Depends on:** Chunk 3
**Risks:**
- **`TextView` system rows:** system messages are drawn as plain text today. Switching them to `TextView` must not change how they look (size, colour, italics). If it does, style the `TextView` to match, rather than keeping two code paths.
- **Click areas:** on continuation rows (no header), only the avatar, if any, is clickable. That's fine; don't add a hidden target.
**How to verify:**
- `cargo test --test store_replies --test store_live`
- `cargo test --lib markdown`
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- Click an author's avatar and name, a reply's author, a name in "X added Y", and an `@mention`: each opens the right person's card.
- Clicking the snippet in a reply still jumps to the original.
- External links still open the browser.
**Shippable when:** Every place listed in Assumption 3 opens the card, and nothing that clicked before has changed.

### Chunk 5: "Message": open a DM
**Goal:** "Message" on the card opens your DM with that person, creating it when Stoat allows it, and switches to it.
**Files likely touched:**
- `src/profile.rs`:
  - `dm_with(store, user) -> Option<channel id>`: any `DirectMessage` whose recipients are you and `user`, active or not
  - `can_message(store, user) -> bool`: not you, and `Friend`, a bot, or `dm_with` is `Some` (Assumption 1)
- `src/store.rs`: `activate_dm(channel)`, which sets `active = true` locally.
- `src/api/client.rs`: `open_dm(user) -> Channel` (`GET /users/{id}/dm`).
- `src/ui/chat.rs`: `message_user(user, window, cx)`:
  - With an existing DM, call `activate_dm` and `open_channel`.
  - Otherwise, request the DM, then `add_channel` and `open_channel`. If its `ChannelCreate` arrives too, `add_channel` must be idempotent.
  - On error, show a toast: 403 `MissingPermission` gets the Assumption 1 text; anything else, the error's own message.
  - Close the card when it succeeds.
- `src/ui/profile_card.rs`: a primary "Message" button in the actions row, shown when `can_message`, disabled while the request is in flight.
- Tests:
  - `tests/profile.rs`: `can_message` is true for a friend, a bot, and a non-friend you already have a DM with. It's false for yourself, a plain server member, and a blocked user without a DM.
  - `tests/profile.rs`: `dm_with` finds an inactive DM.
  - `tests/api_users.rs`: `open_dm`'s path, and parsing both `DirectMessage` and `SavedMessages`. A 403 `MissingPermission { permission: "SendMessage" }` decodes.
  - `tests/store_live.rs`: a `ChannelCreate` for a DM already added by `open_dm` doesn't duplicate it.
**Depends on:** Chunk 3
**Risks:**
- **An inactive DM that's activated only locally** may be hidden again by a later `ChannelUpdate { active: false }` from another client. That's correct behaviour, but check that the Home list re-sorts and doesn't leave the selection pointing at a hidden channel.
- **`open_channel` for a DM switches to Home.** If the card was opened from a server channel, the user lands in Home. That's intended, as on the web client; make sure the side panel resets (milestone 6 Assumption 4).
**How to verify:**
- `cargo test --test profile --test api_users --test store_live`
**Manual check:**
- From a friend's card in a server, choose Message: Home opens on your DM with them.
- With a friend you've never messaged, the DM is created and opens, and the web app shows it too.
- A non-friend server member has no Message button.
- A non-friend you already have a DM with does have one, and it opens that DM.
**Shippable when:** Message opens or creates the DM exactly when Assumption 1 says it should, failures show a toast, and the tests pass.

### Chunk 6: Right-click menu on people
**Goal:** Right-clicking a person (in the member list, on an author, on a reply's author, on a system-message name), or the card's "…" button, opens a menu with Profile, Message, Mention, Copy username and Copy user ID.
**Files likely touched:**
- `src/profile.rs`: `user_menu(store, user, channel) -> Vec<UserMenuItem>` and the `UserMenuItem` enum: `Profile`, `Message`, `Mention`, `Separator`, `CopyUsername`, `CopyUserId`. It applies Assumption 2's visibility rules, in the web client's order. Chunk 7 adds the friend and block items.
- `src/ui/composer.rs`: `pub fn insert_mention(&mut self, user: &str, window, cx)`.
  - It inserts `<@id>` (plus a trailing space) at the cursor, as a chip through the existing `with_chips` / `learn_labels` path, modelled on `insert_emoji`.
  - Then it focuses the box.
- `src/ui/profile_card.rs`: a "…" ghost button at the end of the actions row, with a dropdown menu built from `user_menu` (without `Profile`).
- `src/ui/chat.rs`:
  - a shared `user_context_menu(menu, items, user, on_action)` that turns `UserMenuItem`s into `PopupMenuItem`s
  - `context_menu` added on member rows, author avatars and names, reply-preview authors and system-message names (the same elements as chunks 3–4)
  - a `RowAction::UserMenu(UserMenuAction)`, or a direct `cx.listener`, handles each item:
    - Profile opens the card at the menu position
    - Message calls `message_user`
    - Mention calls `composer.insert_mention`
    - Copy writes to the clipboard
- `tests/profile.rs`, for `user_menu`:
  - yourself: no Message or Mention
  - a server channel and a group: Mention; a DM: no Mention
  - a friend: Message
  - a plain member: no Message
  - the item order
- Composer unit test, if the existing composer tests allow one without a window: `insert_mention` produces `<@id> ` at the cursor.
**Depends on:** Chunk 4 (author IDs and click targets), Chunk 5 (Message)
**Risks:**
- **A context menu on a `uniform_list` row** must be keyed by a stable element ID (for example, `member-{user_id}`), or the menu opens on a recycled row. Message rows already use stable IDs for their own menu.
- **Nested context menus:** the author name sits inside the message row, which has its own menu (Reply, Copy text…). The inner menu must win on the name and avatar, and the outer one keep the rest of the row. Check gpui-component's `context_menu` on a child stops the event; if it doesn't, stop propagation in the child's right-mouse-down.
- **Mention chips:** the composer turns `<@id>` into a chip only when it knows the label. Call `learn_labels` with the user's name first, or a raw `<@01H…>` shows.
**How to verify:**
- `cargo test --test profile`
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- Right-click a member, an author's name, a reply's author and a name in a system message: each shows the menu, with items matching the rules.
- Mention puts a chip in the message box, which sends as a mention.
- Copy username gives `name#1234`, and Copy user ID gives the ID.
- The card's "…" shows the same items.
- Right-clicking a message's body still shows the message menu.
**Shippable when:** The person menu appears on every target with the right items, each item works, and the tests pass.

### Chunk 7: Friend requests and blocking
**Goal:** Send, accept, reject, cancel and remove friend requests, and block and unblock, from the card and the menu. Relationship changes from anywhere (another client, the other person) update live.
**Files likely touched:**
- `src/api/client.rs`:
  - `add_friend(user)`: `PUT /users/{id}/friend`
  - `remove_friend(user)`: `DELETE /users/{id}/friend`
  - `block_user(user)`: `PUT /users/{id}/block`
  - `unblock_user(user)`: `DELETE /users/{id}/block`

  Each returns the target `User`.
- `src/api/error.rs`: make sure `AlreadyFriends`, `AlreadySentRequest`, `Blocked`, `BlockedByOther`, `NoEffect`, `IsBot` and `TooManyPendingFriendRequests` give readable messages.
- `src/api/events.rs`: `UserRelationship { user: Box<User> }`, ignoring the top-level `id` (it's the recipient), plus a parse test.
- `src/store.rs`: `apply_relationship(user: User)`. It upserts the user and returns whether their relationship changed. The API response and the event both go through it.
- `src/profile.rs`: `user_menu` gains `AddFriend`, `AcceptFriend`, `RejectFriend`, `CancelFriend`, `RemoveFriend`, `Block` and `Unblock`, by relationship (Assumption 2). `profile_card` exposes the card's friend button with the same rules.
- `src/ui/profile_card.rs`: the friend button before Message ("Add Friend", "Accept" plus a reject icon, "Cancel request").
- `src/ui/chat.rs`:
  - `set_relationship(user, action, confirmed)`, which calls the API, applies the returned user, and maps errors (Assumption 4)
  - confirm dialogs for Remove friend and Block, modelled on `confirm_delete`, with Shift to skip
  - the `UserRelationship` event goes through `apply_relationship`, then refreshes the open card, `forget_member_lists()` and calls `cx.notify()`
- Tests:
  - `tests/api_users.rs`: each route's method and path; the `User` response parsing; `AlreadyFriends` (409), `BlockedByOther` (403) and `TooManyPendingFriendRequests` (400) decoding.
  - `tests/profile.rs`: the menu and card buttons for each `RelationshipStatus` (`None`, `Incoming`, `Outgoing`, `Friend`, `Blocked`, `BlockedOther`, `User`), and for a bot (Block, but no friend items).
  - `tests/store_live.rs`: `apply_relationship` updates an existing user and adds an unknown one; `can_message` turns true after it becomes `Friend`.
**Depends on:** Chunk 6 (the menu), Chunk 5 (`can_message` reads the relationship)
**Risks:**
- **Blocking changes what the permission maths allows** (`user_relationship` in `permissions.rs`). After a block, Message disappears, unless a DM already exists (Assumption 1 keeps it). Confirm this matches Stoat: sending in a DM with a blocked user fails server-side, which is acceptable.
- **The event and the response race:** whichever arrives second sets the same relationship, so applying both is harmless. Don't skip one.
**How to verify:**
- `cargo test --test api_users --test profile --test store_live`
- `cargo test --lib events`
**Manual check (needs a second account):**
- From A's card, add B as a friend: B's web app shows the request, and A's card shows "Cancel request".
- Accept in B's web app: A's card changes to Message plus Remove friend, live.
- Remove friend asks first (Shift skips).
- Block B: the card shows Unblock, and B's messages still display (the client doesn't hide blocked users' messages yet).
- Unblock.
- Try adding a bot: there's no Add Friend item.
**Shippable when:** Every friend and block action works from the card and the menu, relationship changes update live from either side, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`. Optional live checks: `cargo test -- --ignored`.

### Unit Tests
- **Event decoding** (`src/api/events.rs` tests):
  - `ServerUpdate` with `clear`
  - `ServerRoleUpdate` for a creation (full role) and for an edit (partial plus `clear: ["Colour"]`)
  - `ServerRoleDelete`
  - `ServerRoleRanksUpdate`
  - `ServerCreate` with an unparseable channel skipped
  - `ServerDelete`
  - `UserRelationship` without `status`
- **Store roles** (`tests/store_roles.rs`):
  - merge, create from a partial, and an unnamed unknown role ignored
  - delete strips members and overrides
  - ranks reorder sections and colours
  - permission edits flip `can_pin` and `can_view`
  - `server_channels` filtering, with the `Ready` fixture guard
- **Store servers** (`tests/store_servers.rs`): `add_server` (new, and a replacement keeping history), and no filtering before membership is known.
- **Profile model** (`tests/profile.rs`):
  - `profile_card` fields and precedence
  - `can_message` and `dm_with`
  - `user_menu` for every relationship, bots, yourself, and each channel type
- **Markdown:** `parse_internal_link`, and system-message Markdown escaping.

### Integration Tests
- **`wiremock`** (`tests/api_users.rs`):
  - `GET /servers/{s}/members/{u}`
  - `GET /users/{id}/profile`, including a 403
  - `GET /users/{id}/dm`, both channel shapes and a 403 `SendMessage`
  - `PUT` and `DELETE /users/{id}/friend`
  - `PUT` and `DELETE /users/{id}/block`
  - the relationship errors
- **Store pipeline:**
  1. `Ready`.
  2. Fetch members.
  3. A `ServerRoleUpdate` creates a hoisted, coloured role.
  4. A `ServerMemberUpdate` gives it to a member.
  5. The member list shows the section and colour.
  6. A `ServerRoleRanksUpdate` moves the role.
  7. A `ServerRoleDelete` removes the section, and the member falls back to Online.
- **Relationship pipeline:**
  1. A non-friend: `can_message` is false.
  2. `UserRelationship` (`Friend`): `can_message` is true, and the menu shows Message and Remove friend.

### Manual Tests
1. **Live roles:** in the web app, rename, recolour, hoist, reorder, create and delete roles, and edit role permissions and the server's default permissions. RuSToat's member list, colours, "Pin message" and channel list follow, with no reconnect.
2. **Servers:** join, create and delete a server in the web app; RuSToat's rail follows.
3. **Card:** open it from the member list, an author, a reply's author, a system-message name and a mention. Its contents match the web card. It stays inside the window, and Esc or a click outside closes it.
4. **Message:** it opens an existing DM, creates one with a friend, and is absent for non-friends you have no DM with.
5. **Menu:** the items follow the rules on every target. Mention inserts a chip. The Copy items work.
6. **Friends and blocks:** the full request cycle with a second account, live on both sides. Remove friend and Block confirm first.

## Definition of Done
- [ ] **Live server edits:**
  - `ServerUpdate`, `ServerRoleUpdate` (including creation), `ServerRoleDelete` and `ServerRoleRanksUpdate` are decoded and applied.
  - The member list's sections, colours and order, and `can_pin` and `can_view`, update without a reconnect (tested in `tests/store_roles.rs`).
  - `permissions.rs`'s doc no longer says roles are only applied at reconnect.
- [ ] **Hidden channels:** a channel a role edit hides from the user leaves the channel list at once. A `Ready` fixture test proves no `Ready` channel is wrongly hidden.
- [ ] **Joining and deleted servers:**
  - `ServerCreate` adds the server, its channels and emojis, and fetches the user's own member with `GET /servers/{id}/members/{me}`.
  - `ServerDelete` removes the server via `leave_server`.
- [ ] **Profile card:**
  - It opens on left-click from member rows, author avatars and names, reply-preview authors, system-message names and `stoat:user/` mention links.
  - It fetches `GET /users/{id}/profile` on open, shows the fields in Assumption 5, and closes on a click outside, Esc or a channel switch.
- [ ] **"Message":**
  - It's shown exactly when `can_message` (a friend, a bot, or an existing DM; never yourself).
  - It opens an existing DM locally, or creates one with `GET /users/{id}/dm`, then switches to it.
  - A 403 shows the "only friends and bots" toast.
- [ ] **Right-click menu:**
  - It appears on the same targets as the card (except mentions), and the card's "…" shows the same items.
  - Its items and order follow `user_menu`, which is unit-tested for every relationship and channel type.
  - Mention inserts a `<@id>` chip; the Copy items copy `username#discriminator` and the ID.
- [ ] **Friends and blocks:**
  - Add, accept, reject, cancel and remove friend, block and unblock call the documented routes and apply the returned `User`.
  - Remove friend and Block confirm unless Shift is held.
  - `UserRelationship` events update the card and menu live.
- [ ] **Checks:** `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` passes.
