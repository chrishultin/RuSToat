# Implementation Record: Member actions and live role edits (milestone 7)

_Plan: docs/plans/member-actions-and-live-roles.md_
_Started: 2026-11-05_

**Supervision:** continuous. The run stops for significant divergences and for test failures that can't be explained. After chunk 1, the user chose to batch the manual checks: each chunk is committed once its automatic checks pass, and every chunk's manual check is walked through together at the end.

**Pre-existing dirty files, kept out of every commit:** `.gitignore` (the same Riptide entry added twice, by tooling) and `.shadow/` (untracked). The plan file was committed on its own first (`797dfd6`), as milestone 6's was.

**Formatting:** `cargo fmt --check` is part of the plan's full-suite command, but it already fails on a clean `HEAD` (`3b89e68`). The drift predates this milestone and is spread over many files (`store.rs`, `chat.rs`, `client.rs`, `events.rs`, `emoji.rs`, `markdown.rs`, `complete.rs`…). The packaging milestone set the precedent: new files go through `rustfmt`, and new code in existing files is laid out as `rustfmt` wants it, but existing files aren't reformatted wholesale. Each chunk checks that the number of `rustfmt --check` hunks in every file it touched is no higher than at `HEAD`.

## Chunk 1: Live role and server edits

### Changes Made
- `src/api/events.rs`:
  - New `ServerEvent` variants: `ServerUpdate { id, data: Box<PartialServer>, clear: Vec<FieldsServer> }`, `ServerRoleUpdate { server, role_id, data: Box<PartialRole>, clear: Vec<FieldsRole> }`, `ServerRoleDelete { server, role_id }` and `ServerRoleRanksUpdate { server, ranks }`.
  - Decoding for each in `decode()`, with `#[serde(default)] clear` as in `ChannelUpdate`. The role is the top-level `role_id`, never `data`'s ID.
  - `session()` forwards the four new events to the UI.
  - Tests: the "unknown event" test now uses `WebhookCreate`, since `ServerRoleUpdate` is decoded now. New parse tests:
    - `ServerUpdate`: with `clear`, and a `default_permissions`-only update.
    - `ServerRoleUpdate`: a role-creation payload carrying the whole role, an edit with `clear: ["Colour"]`, and a permissions-only edit.
    - `ServerRoleDelete` and `ServerRoleRanksUpdate`.
- `src/store.rs`:
  - `apply_server_update(id, data, clear)` merges with `Server::apply_options`, then clears `Description`, `Categories`, `SystemMessages`, `Icon` and `Banner` as listed.
  - `apply_role_update(server, role_id, data, clear)`:
    - A known role is merged with `Role::apply_options`, and `Colour` and `Icon` are cleared as listed.
    - An unknown role is built from the partial. `name` is required; without it the update is logged and ignored (Assumption 9). `permissions` defaults to `{a:0,d:0}`, `rank` to the role count, and `hoist` to false.
    - The role's `id` is always set to the map key.
  - `apply_role_delete(server, role_id)` removes the role from `server.roles`, from the `roles` of every cached member of that server, and from the `role_permissions` of that server's channels.
  - `apply_role_ranks(server, ranks)` sets `rank = index` for each listed role.
  - New `shows_channel(channel)`: a server channel is shown if the user can view it (`permissions::can_view`), or while the user's own membership of its server is unknown (Assumption 8). Other channels are always shown. `server_channels` (both uncategorized channels and categories) and the rail's unread and mention counts in `servers()` both use it.
- `src/permissions.rs`: the module doc no longer says role edits only arrive at reconnect. It lists the live events that are applied.
- `src/ui/chat.rs`:
  - `on_connection_event` applies each of the four events. When the store reports a change, it calls a new `server_permissions_changed(server)`, which does four things:
    1. `forget_member_lists()`.
    2. `sync_pane` on every loaded channel of that server, since `can_pin` on rows depends on permissions.
    3. `drop_stale_selection()`, then `remember_places()` if a selection was dropped.
    4. `ensure_current_loaded`.
  - `drop_stale_selection` now also drops selections of channels that `shows_channel` hides, so a channel that becomes hidden while open is replaced by the server's first visible channel.
- `tests/store_roles.rs` (new, 11 tests):
  - A rename keeps the colour. This confirms that `apply_options` leaves a field alone when the partial's field is `None`, which was a plan risk.
  - A colour change, then a colour clear (Bob falls back to his next role's colour).
  - Hoisting off and on.
  - The whole pipeline: a created role → hoisted and coloured → given to a member through `ServerMemberUpdate` → its section and colour show → a ranks update moves it to the top → a delete puts the member back under Online.
  - An unnamed unknown role, and an unknown server, are ignored.
  - A new role without a rank goes last.
  - A delete strips members and overrides.
  - A ranks update changes both the sections and which colour wins.
  - A role permission edit flips `can_pin` and then `can_view`.
  - `default_permissions` through `ServerUpdate` flips `can_view`.
  - A `ServerUpdate` merges and clears fields.
  - `server_channels` hides channels after a role edit denies ViewChannel, and shows them again when it's undone.
  - The `Ready` fixture guard: every server channel in `tests/fixtures/ready.json` is still shown and listed.
- `tests/fixtures/ready.json`: both servers' `default_permissions` changed from `0` to `2276038022144`, which is `DEFAULT_PERMISSION_SERVER`.

### Divergences
- **The `Ready` fixture's servers had `default_permissions: 0`**, which no real server has. With the new `server_channels` filter, the permission maths correctly worked out that the user (who doesn't own `srv-alpha`) couldn't view any of its channels. Real `Ready` payloads only contain channels the user can view, so the fixture was unrealistic, not the maths. The fix sets both servers to Stoat's default server permissions. Tests that read the fixture (pins, unreads, notifications, completion) still pass: the new value grants no Manage Messages, so `can_pin` stays false in server channels. This counts as minor: a file outside the plan's list, with the same approach and outcome.
- **`servers()` uses `shows_channel` too**, so the rail's unread dot and mention count ignore channels a role edit has hidden. The plan didn't list this, but it keeps the rail consistent with the channel list.
- **`ensure_current_loaded` runs after every server or role change**, not only when a selection was dropped. When the open channel is the server's default first channel rather than a picked one, hiding it makes another channel current without any selection being dropped. The call does nothing if the channel is already loaded.
- **Look-ahead:** chunk 2's `add_server` relies on the "membership unknown" exception in `shows_channel`, which exists now as planned. No other chunk is affected.

### Verification Performed
- `cargo test --test store_roles`: 11 passed.
- `cargo test --lib events`: 24 passed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test` (full): 505 passed, 0 failed.
- `rustfmt --check` hunks in the touched files are the same as at `HEAD` (`events.rs` 3, `store.rs` 15, `chat.rs` 19, `permissions.rs` 0). The new `tests/store_roles.rs` is rustfmt-clean.

### Manual Validation
Deferred to the end of the milestone (manual checks are batched).

## Chunk 2: Joining and deleted servers, live

### Changes Made
- `src/api/events.rs`:
  - New `ServerCreate { server: Box<Server>, channels: Vec<Channel>, emojis: Vec<Emoji> }`. Channels and emojis are read with `lenient_list`, so an unparseable channel costs only itself. `voice_states` is ignored.
  - New `ServerDelete { id }`.
  - Both are forwarded to the UI.
  - Parse tests: a `ServerCreate` with an unknown channel type skipped, one with no lists, and `ServerDelete`.
- `src/api/client.rs`: `fetch_member(server, user) -> Member` (`GET /servers/{s}/members/{u}`).
- `src/store.rs`: `add_server(server, channels, emojis) -> bool` (true if new).
  - It inserts or replaces the server and adds each channel (through `add_channel`) and emoji.
  - A new server is appended to `server_order` (Assumption 10).
  - Histories and members aren't touched, so a repeated `ServerCreate` keeps loaded history.
- `src/ui/chat.rs`:
  - On `ServerCreate`: `add_server`, then, if the user's own membership isn't known yet, `fetch_own_member`. That fetches `GET /servers/{id}/members/{me}`, applies it with `apply_member_join`, and calls `server_permissions_changed`, so the chunk 1 channel filter now applies. If the server was left before the membership arrived, the result is dropped. A failure is logged and left for the next reconnect.
  - On `ServerDelete`: the existing `leave_server`.
- `tests/store_servers.rs` (new, 4 tests):
  - A joined server goes at the end of the rail, with its channels and emojis.
  - Channels aren't filtered until the membership is known, and a role-restricted channel disappears once it is.
  - A repeated `ServerCreate` replaces the server (renamed, not duplicated) and keeps history.
  - `ServerDelete` → `remove_server` removes the channels.
- `tests/api_users.rs` (new, 2 tests): `fetch_member`'s path and token, parsing a bare `Member`, and a 404 → `ApiError::Api(NotFound)`.

### Divergences
None. The rail-order risk was accepted as planned: the server appears last, and the saved ordering isn't re-read live.

### Verification Performed
- `cargo test --test store_servers --test api_users`: 6 passed.
- `cargo test --lib events`: 26 passed.
- `cargo test` (full): 513 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --check` hunks are unchanged from `HEAD` in `events.rs`, `store.rs`, `chat.rs` and `client.rs`. The new test files are rustfmt-clean.

### Manual Validation
Deferred to the end of the milestone (manual checks are batched).

## Chunk 3: Profile card from the member list

### Changes Made
- `src/profile.rs` (new, GPUI-free), registered in `src/lib.rs`: `profile_card(store, user, channel: Option<&str>) -> Option<ProfileCard>`. A `ProfileCard` has:
  - `user_id` and `name` (the nickname in the channel's server, else the display name, else the username).
  - `username_tag` (`username#discriminator`).
  - `avatar_url` (the server avatar first).
  - `presence`, and `status` (trimmed; a blank one is `None`).
  - `pronouns` (the member's first, then the user's).
  - `bot`.
  - `roles: Vec<CardRole { name, colour }>`, highest first. These appear only in server channels. Roles that no longer exist are left out, and a gradient colour becomes `None`.
  - `joined_server: Option<(server name, DateTime<Utc>)>`. The epoch placeholder from old `ServerMemberJoin` fallbacks counts as unknown.
  - `created` (from the ULID).
  - `relationship` and `is_me`.
  - `None` for an unknown user.
- `src/store.rs`: `presence_kind` is now `pub`. The colour parsing in `role_colour` moved into a new `pub fn solid_colour(css)`, which the card reuses.
- `src/api/client.rs`: `fetch_profile(user) -> UserProfile` (`GET /users/{id}/profile`).
- `src/ui/profile_card.rs` (new), registered in `src/ui/mod.rs`: `render_profile_card(content, actions, on_link, cx) -> Div`. The card is a fixed 340 px wide and at most 560 px tall; a long bio scrolls inside it. It draws:
  - the banner, or a plain strip without one
  - a 48 px avatar with a presence dot
  - the name, with a "Bot" tag where it applies
  - `username#discriminator`, which you click to copy
  - Status, Pronouns, and Roles (as chips with colour dots)
  - "Member since" for Stoat and for the server, as local dates
  - the bio ("About me") through `TextView::markdown`
  - an actions row, empty for now. `CardContent` / `CardBio` carry the data, and `OnLink` is the link-click callback.
- `src/ui/chat.rs`:
  - New state: `card: Option<OpenCard { user, channel, position }>`, `card_focus` and `profiles: HashMap<String, Option<UserProfile>>`, the per-session cache, where `None` means Stoat refused.
  - `open_card(user, position)` focuses the card and always fetches the profile again. A 403 `MissingPermission` stores `None`, so no bio or banner and no error. Other errors are logged.
  - `close_card(refocus)`: Esc gives focus back to the composer. A click elsewhere closes the card without moving focus.
  - `sync_card()`, called every render, closes a card opened from a different channel. `Ready` also closes it.
  - `render_card` builds `CardContent` from the store and the cached profile. The bio goes through `markdown::prepare`, keyed by `render_key("bio-{user}", bio)`. It's drawn as `deferred(anchored().position(p).snap_to_window_with_margin(8px))` at priority 1, with `occlude`, `track_focus`, Esc on `on_key_down`, `on_mouse_down_out` to close, and scrolling stopped.
  - Member rows get a stable `member-{user_id}` ID, a pointer cursor, a hover background (`sidebar_accent`, through a new `MemberColors.hover`) and `on_click`, which opens the card at `window.mouse_position()` through an `OpenCardAt` callback.
  - `hex_colour` is now `pub(crate)` so the card can share it.
- `tests/profile.rs` (new, 9 tests):
  - nickname and member pronouns win in a server
  - display name and own pronouns elsewhere, with a blank nickname falling back
  - roles highest first, with only solid colours and unknown roles left out
  - roles follow a ranks change
  - no roles or server join date outside server channels
  - join dates from the membership and the ULID
  - the bot flag and a blank status
  - `is_me`
  - an unknown user gives `None`
- `tests/api_users.rs`: 3 tests for `fetch_profile`: the path, token and parsing with a banner; an empty profile; and a 403 `MissingPermission { permission: "ViewProfile" }`.

### Divergences
- **`profiles` holds `Option<UserProfile>`**, where `None` means 403. The plan sketched a separate `ProfileState` on `OpenCard`. The outcome is the same: the last result shows while the card fetches again, and a 403 hides the bio and banner without an error.
- **The card's Markdown callback is `Arc<… + Send + Sync>`**, because `TextView::on_link_click` requires `Send + Sync`. Chunk 4 will route `stoat:user/` links through a `WeakEntity<ChatView>`, which is `Send + Sync`.
- **The heading always shows the best name.** The web card shows the display-name line only when it differs from the username, but here it's the card's heading and `username#discriminator` is always shown below it. This is minor and only changes the presentation.
- **Small helper refactors:** `solid_colour` extracted from `role_colour`, `presence_kind` made `pub`, and `hex_colour` made `pub(crate)`.
- **Look-ahead:** chunk 4's link handling has to build an `OnLink` (Send + Sync) for message bodies too. That's already the shape `TextView` requires there, so the plan needs no change. Chunks 5–7 add their buttons through `render_profile_card`'s `actions` parameter, as planned.

### Verification Performed
- `cargo test --test profile --test api_users`: 14 passed.
- `cargo test` (full): 525 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --check`: the new files are clean. `chat.rs` (19), `store.rs` (15) and `client.rs` (3) are back to their `HEAD` hunk counts. From here on, a scratch script (`/tmp/fmtmine/fmtmine.py`, outside the repo) applies rustfmt's layout only to hunks containing lines this milestone added.
- The UI can't be driven from tests. Placement, closing, and focus are part of the batched manual check.

### Manual Validation
Deferred to the end of the milestone (manual checks are batched).

## Chunk 4: Open the card from messages, replies, system messages and mentions

### Changes Made
- `src/store.rs`:
  - `MessageRow.author_id: Option<String>`: the real `message.author`, including under a masquerade. It's `None` for system messages and webhooks, through a new `person_id(message)`.
  - `ReplyPreview.author_id: Option<String>`: the same, only when the original is `Loaded`.
  - `system_text` and a new `system_markdown` share a free `describe_system(system, name, text)`:
    - In the plain form, names are plain and other text passes through. This is still used by notifications and reply snippets.
    - In the Markdown form, each known person is `[escaped name](stoat:user/{id})`, an unknown one is plain "Someone", and any other text (a new channel name, a `Text` system message, the fallback description) is escaped with `markdown::escape`.
  - System rows' `body` is now the Markdown form.
- `src/markdown.rs`:
  - `InternalLink::{User, Channel}` and `parse_internal_link(url)`, beside `INTERNAL_SCHEME`. It rejects external URLs, empty or nested IDs, and unknown kinds.
  - A unit test.
- `src/ui/chat.rs`:
  - `RowAction::OpenCard { user }`, which `on_row_action` turns into `open_card` at `window.mouse_position()`.
  - New `person_target(id, user, on_action, child)` wraps an author's name or avatar in a clickable element. It opens the card and calls `stop_propagation`, so a preview row's jump and a reply line's jump don't fire as well. Without a person, it's a plain wrapper.
  - In `render_message_row`, the avatar slot and the header name use it. The name underlines on hover. This applies to timeline rows and to search and pins previews alike.
  - In `render_reply_preview`, the avatar and name are one `person_target`, and the rest of the line still jumps.
  - New `ChatView::link_handler(cx) -> OnLink`, `Send + Sync` through a `WeakEntity`:
    - `stoat:user/{id}` opens that card at the mouse.
    - `stoat:channel/…` does nothing yet.
    - External links open in the browser.
    - It's threaded through `render_row`, `render_preview_row`, `render_system_row` and `render_message_row`, and also used for the card's bio.
  - `render_system_row` draws its body through `TextView::markdown` (keyed by the row's `render_key`, `text_xs`), inside the same italic, system-coloured wrapper as before.
- Tests:
  - `tests/store_live.rs` has 3 new tests:
    - `author_id` for a normal author, a masqueraded one (the real author), a webhook and a system message (both `None`).
    - Adding a live message is one `Append` with no other row changes.
    - The system-message Markdown links and escapes names (one display name contains Markdown and a URL), an unknown person is unlinked, a renamed channel's name is escaped, and `system_text` stays plain.
  - The `diff_rows` helper `row()` sets `author_id`.
  - `tests/store_replies.rs`: a loaded preview's `author_id`, plus a new test where a webhook original and a loading original have no author ID and the reply row has one.
  - `tests/store_history.rs`: the system row's body is now `[Carol](stoat:user/u-carol) joined`.

### Divergences
- **`author_id` is `None` for system messages and webhooks, not for pending rows.** The plan said `None` for pending rows "that have no real author yet". In practice a pending row's author is always the user (`pending_as_message`), so it gets `Some(me)`, and the value stays the same when the pending row is confirmed. Webhooks and system messages are what really have no person behind them.
- **Embed descriptions keep external-only link handling.** The plan's "every `on_link_click`" named message bodies, the bio and system rows. Those three use `link_handler`.
- **Clicking a mention in a search or pins result** opens the card and may also jump to the result, since the row's own click isn't stopped by `TextView`. Both are harmless; this needs a look in the manual check.
- **Look-ahead:** chunk 6 adds context menus to the same `person_target` elements. Putting the menu on that wrapper keeps the nested-menu risk to one place. No plan change is needed.

### Verification Performed
- `cargo test --test store_replies --test store_live --test store_history`: 12 + 29 + 28 passed.
- `cargo test --lib markdown`: 5 passed.
- `cargo test` (full): 530 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --check` hunk counts are at their `HEAD` values in every touched file.
- Whether system rows look the same through `TextView` (size, colour, italics, no paragraph spacing) can only be checked by eye. That's part of the batched manual check.

### Manual Validation
Deferred to the end of the milestone (manual checks are batched).

## Chunk 5: "Message": open a DM

### Changes Made
- `src/profile.rs`:
  - `dm_with(store, user) -> Option<String>`: the `DirectMessage` whose recipients are the user and `user`, open or closed. Never for yourself.
  - `can_message(store, user) -> bool`: not yourself, and a `Friend`, a bot, or someone `dm_with` finds (Assumption 1).
- `src/store.rs`:
  - New `channels()`, an iterator over every known channel, for `dm_with`.
  - New `activate_dm(channel) -> bool`, which sets `active = true` locally on a closed DM, as stoat.js does.
  - `shows_channel` now returns false for closed DMs, matching what `home_channels` lists.
- `src/api/client.rs`: `open_dm(user) -> Channel` (`GET /users/{id}/dm`).
- `src/ui/chat.rs`:
  - New `dm_opening: Option<String>`, the user whose DM is being requested.
  - `message_user(user)`:
    - With a known DM: `activate_dm`, close the card (focus returns to the composer), then `open_channel`, which moves to Home. The side panel resets through the existing `sync_side_panel`.
    - Otherwise it asks Stoat once (a repeat click while waiting does nothing). On success it does `add_channel` (idempotent, since it's a map insert), `activate_dm`, closes the card and opens the DM.
    - Errors become toasts. A 403 `MissingPermission` gets "You can only message friends and bots." (`CANNOT_MESSAGE_TEXT`). Other API errors get their own message, and a failed task gets "Couldn't open the conversation."
  - New `card_actions(user)` builds the card's actions row: a primary "Message" button (MessageCircle icon), shown when `can_message`, loading and disabled while its request is in flight.
  - `ChannelUpdate` now also runs `drop_stale_selection` and, if a selection was dropped, `remember_places` and `ensure_current_loaded`. A DM closed by another client, or a channel hidden by an override, then can't stay selected. This was the plan's risk for locally activated DMs.
- Tests:
  - `tests/profile.rs`: the fixture gains a plain member, a blocked member and a closed DM. New tests:
    - `can_message` is true for a friend without a DM, a bot, and non-friends with an open or a closed DM.
    - It's false for yourself, a plain server member, a blocked user without a DM, and an unknown user.
    - `dm_with` finds open and closed DMs, and not yourself.
  - `tests/api_users.rs`: `open_dm`'s path and token with a `DirectMessage`, a `SavedMessages` for yourself, and a 403 `MissingPermission { permission: "SendMessage" }`.
  - `tests/store_live.rs`:
    - The DM from `open_dm` plus its `ChannelCreate` gives exactly one Home entry.
    - `activate_dm` shows a closed DM in Home (and is a no-op when it's already open or not a DM), and a later `ChannelUpdate { active: false }` hides it again.

### Divergences
- **`shows_channel` hides closed DMs, and `ChannelUpdate` drops stale selections.** The plan only asked to check that the selection doesn't stay on a hidden DM. Without these, a locally reopened DM closed by another client would have stayed open while missing from the Home list. As a side effect, a channel hidden by a live channel-override edit now also leaves the selection.
- **Look-ahead:** chunk 6's menu "Message" item reuses `message_user` and `can_message`. Chunk 7 adds its friend button to `card_actions`. No plan change is needed.

### Verification Performed
- `cargo test --test profile --test api_users --test store_live`: 12 + 8 + 31 passed.
- `cargo test` (full): 538 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --check` hunk counts are at their `HEAD` values in every touched file, and the new files are clean.

### Manual Validation
Deferred to the end of the milestone (manual checks are batched).

## Chunk 6: Right-click menu on people

### Changes Made
- `src/profile.rs`:
  - `UserMenuItem { Profile, Message, Mention, Separator, CopyUsername, CopyUserId }`.
  - `user_menu(store, user, channel)` builds the menu in the web client's order, with separators between non-empty groups:
    - Profile always.
    - Message when `can_message`.
    - Mention only for others, in server channels and groups.
    - Copy username and Copy user ID always.
- `src/ui/composer.rs`: `insert_mention(user, label)` inserts a `<@id>` chip at the cursor and then a space, through the same `replace_with_token` path the emoji picker uses. It records the label first (so no raw `<@01H…>` shows) and focuses the box.
- `src/ui/chat.rs`:
  - New `People { view: WeakEntity<ChatView>, channel }` replaces chunk 4's `OnLink` parameter and `ChatView::link_handler`. It's built by `ChatView::people(channel)` and provides:
    - `open_card(user)`, at the mouse
    - `link_handler()` (chunk 4's link behaviour)
    - `menu(user, profile)`: a `PopupMenu` builder that reads `user_menu` from the store when the menu opens, so relationship changes show without re-rendering the row. `profile = false` leaves out Profile, for the card.
  - `user_menu_label` gives each item's label and icon (CircleUser, MessageCircle, AtSign, Copy, IdCard).
  - `on_user_menu(user, channel, item)`:
    - Profile opens the card at the mouse.
    - Message calls `message_user`.
    - Mention closes the card and calls `composer.insert_mention` with `store.token_label(channel, @user)`.
    - Copy username writes `username#discriminator`, and Copy user ID writes the ID.
  - The menu is attached with `context_menu(people.menu(user, true))` to:
    - member rows, keyed by their stable `member-{user_id}` ID
    - `person_target`, which covers author avatars and names (timeline and previews) and reply-preview authors
  - `person_target` and the member rows now open the card through `People` rather than `RowAction`, so chunk 4's `RowAction::OpenCard` is removed. `render_row`, `render_preview_row`, `render_system_row`, `render_message_row`, `render_reply_preview` and `render_member_row` take `&People`.
  - `card_actions` adds a ghost "…" button (Ellipsis icon, "More" tooltip) with `dropdown_menu(people.menu(user, false))`.
  - **Closing on an outside click** is now two steps. A mouse-down outside sets `OpenCard.closing`, and a left or right mouse-up outside (`finish_closing_card`) then closes the card. Closing on the press removed the card and its "…" menu before a click on a menu item (the menu is outside the card's bounds) could land.
- `tests/profile.rs`: a `menu` module with 4 tests:
  - a friend in a server channel gets every item, in order
  - Mention appears in a group, not in a DM or with no channel
  - your own menu has no Message or Mention
  - Message follows `can_message` (a plain member doesn't get it, a bot does), with the exact item list for a plain member

### Divergences
- **System-message names don't get a right-click menu.** The plan listed them among the menu targets, but chunk 4 made them links inside a `TextView`, like mentions, so they can't hold a context menu. Left-click opens their card, and its "…" shows the same items. This is the same trade-off the plan accepted for mentions (Assumption 3).
- **No composer unit test for `insert_mention`.** The repo has no GPUI window-backed tests, and the composer's text box needs a `Window`. The plan made this test conditional ("if the existing composer tests allow one without a window"). It's covered by the batched manual check (Mention puts a chip in the box, and it sends as a mention).
- **The card's outside-click close happens on mouse-up**, as described above. This is the plan's chunk 3 risk about outside clicks, solved differently because the "…" menu arrived.
- **The `People` refactor** replaces chunk 4's separate `on_link` parameter. The behaviour is unchanged.
- **Look-ahead:** chunk 7's friend and block items extend `UserMenuItem` and `user_menu`, which `People::menu` and `on_user_menu` already route, and its card button joins `card_actions`. No plan change is needed.

### Verification Performed
- `cargo test --test profile`: 16 passed.
- `cargo test` (full): 542 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --check` hunks: `chat.rs` 18 (`HEAD` 19, because a line this chunk edited had old drift), `composer.rs` 14 (unchanged). The new code in `profile.rs` and `tests/profile.rs` is rustfmt-clean.
- The format helper script once dropped an import while applying an import-reordering hunk. The build caught it, and the import was restored. The script now refuses any hunk that changes more than whitespace, braces and commas.

### Manual Validation
Deferred to the end of the milestone (manual checks are batched).
