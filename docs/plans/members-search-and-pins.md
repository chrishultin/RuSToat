# Plan: Send button, member list, search and pins (RuSToat milestone 6)

## Summary
This milestone makes channels feel complete:
- **A send button** on the right of the message box, for people who don't press Enter.
- **A right-hand panel**, copied from Stoat's web client, that shows one of three things:
  - the channel's **member list**, with presence, hoisted-role sections and Online/Offline
  - **search results** for the open channel
  - its **pinned messages**
- **Pin and unpin** from a message's right-click menu.
- **Jump to message**, so clicking a search result, a pin or a reply's original scrolls to it even when it's far back in history.

Jump to message comes from the list milestone 5 deferred. The other deferred features stay deferred, as do any member-list actions (profile cards, "Message", right-click menus). That means spoilers, notification settings and sounds, and moderation.

## Approach
Keep the milestone 1–5 architecture:
- **Pure model, thin views:** rules live in GPUI-free code (`ChatStore`, `draft.rs`, and a new `permissions.rs`) and are unit-tested. `ChatView` and `Composer` only wire them up.
- **Backend types:** wire types come from `revolt-models` 0.16.2. Permission maths comes from Stoat's own `revolt-permissions` 0.16.2. It's MIT-licensed, and its only new dependencies are `async-trait`, `auto_ops` and `once_cell`.
- **The web client is the reference for behaviour.** That's `stoatchat/for-web` `main` (read 2026-11-05) on top of the `stoatchat/stoatchat` backend; the live API reports 0.16.2.

**Codebase context** (on branch `i-d-like-to-add-a-send-button-to-the-right-side-08ba`, fast-forwarded to milestone 5's `aaa5da8`):
- **The message box** is `src/ui/composer.rs`. `Composer::render` lays out a row of attach button, `Textarea` and emoji `Popover`. Sending happens only on Enter (`on_enter`, then `submit()`), which uses `draft::check_send` (`src/draft.rs`). While a message is being edited, `submit()` saves the edit instead.
- **History** (`src/store.rs`): `History { messages, exhausted }` is always a contiguous run that ends at the newest message:
  - `set_history` loads the latest page.
  - `prepend_page` adds older pages.
  - `apply_message` inserts live messages.

  `ChatView::sync_pane` diffs `timeline()` rows into gpui-component's `MessageScrollerState`, using `diff_rows` and `RowOp`. `maybe_load_older` pages upward. `maybe_ack_current` acks when the list isn't scrolled up.
- **`RowAction::JumpTo(id)`** (reply previews) only scrolls to rows that are already loaded. `StoatClient::fetch_messages_around` (`?nearby=`) already exists; it's used to fetch reply originals.
- **Members:**
  - `ChatView::fetch_members(server)` fetches the whole list once per session (`GET /servers/{id}/members`) for `@` suggestions, and `ChatStore::add_members` stores it.
  - Live `ServerMemberJoin`, `ServerMemberLeave`, `ServerMemberUpdate` and `UserUpdate` are applied.
  - `apply_user_update` returns false for presence-only changes, so nothing re-renders.
- **Message right-click menu:** `render_message_row` builds it with `context_menu` and `PopupMenuItem`, emitting `RowAction`s. Delete confirms through `confirm_delete` unless Shift is held.
- **Preferences:**
  - `src/prefs.rs` `Prefs { skin_tone }` holds app-wide settings, saved as JSON with `#[serde(default)]`.
  - `src/places.rs` holds per-account places.
- **Icons:** extra Lucide icons are declared in `src/assets.rs` (`icon_assets!`). `SendHorizontal`, `Pin`, `PinOff` and `ArrowDown` aren't declared yet.

**API facts** (backend source; full notes are in the research summary in this task's conversation):
- **Pin and unpin:**
  - `POST /channels/{c}/messages/{m}/pin` pins and `DELETE /channels/{c}/messages/{m}/pin` unpins. Both return 204.
  - Both need `ManageMessages`, except in DMs, where either person may pin.
  - Pinning an already-pinned message returns 400 `AlreadyPinned`; unpinning an unpinned one returns 400 `NotPinned`.
  - Pinning sends `MessageUpdate { data: { pinned: true } }` plus a new `SystemMessage::MessagePinned { id, by }` message.
  - Unpinning sends `MessageUpdate { clear: ["Pinned"] }` plus `MessageUnpinned { id, by }`.
- **Search: `POST /channels/{c}/search`** with a `DataMessageSearch` body.
  - It searches **one channel only** and needs `ReadMessageHistory`. Bots get `IsBot`.
  - `query` is 1–64 characters, and the search is MongoDB `$text` (whole words, stemmed, not substrings).
  - `limit` is 1–100, default 50. `sort` is `Relevance`, `Latest` or `Oldest`.
  - `include_users: true` returns `{ messages, users, members }`.
  - **Sending `pinned` alongside `query` is an `InvalidOperation` error**, even `pinned: false`, so `pinned` must be left out of the body.
- **Pins list:** the same search route with `{ pinned: true, sort: "Latest" }` and no `query`. There's no dedicated pins endpoint.
- **Jump:** `GET /channels/{c}/messages?nearby={m}&limit=N&include_users=true` returns up to `N/2+1` messages at or after `m` (including `m` itself) and up to `N/2+1` before it. Newer pages come from `after={id}&sort=Oldest`.
- **Members:** `GET /servers/{id}/members?exclude_offline=true` returns only online members. The web client asks for that when `server.approximate_member_count > 1000`.
- **Rate limits:**
  - Search, pin and unpin share the per-channel `channels` bucket: 15 requests per 10 s.
  - `GET /servers/{id}/members` is in the `servers` bucket: 5 requests per 10 s.

**How the web client lays it out** (`ChannelHeader.tsx`, `TextChannel.tsx`, `MemberSidebar.tsx`, `TextSearchSidebar.tsx`):
- **Header:** the right side has a search box (placeholder "Search messages…", runs on Enter), a pin button ("View pinned messages") and a members toggle.
- **One right sidebar, three states:** default (members), `search` and `pins`.
- **The members toggle is hidden in DMs and Saved Notes**; the pin button shows in every channel.
- **Member sections:**
  - each hoisted role in rank order, then "Online", then "Offline"
  - an online member goes under the first hoisted role they hold; every offline member goes under "Offline"
  - each section is sorted by display name (nickname, else display name, else username)
  - members who can't see a restricted channel are left out
- **Groups:** a flat, name-sorted list of recipients titled "N members".
- **Search results:** a "Latest / Oldest / Relevance" sort (default Latest), with no paging.
- **Clicking a result** loads the messages around it, replacing the list, and scrolls to it. A "jump to bottom" bar appears because the view is no longer at the present.
- **"Pin message" / "Unpin message"** is in the context menu when `channel is DM || ManageMessages`. It confirms in a modal, and Shift skips the modal.

**Jump to message design.** `History` gains `at_present: bool`, meaning the newest loaded message is the channel's newest.
- **A jump to a message that isn't loaded** replaces the channel's history with a window around it (`fetch_messages_around`, 50 messages).
- **Scrolling down in a window that isn't at the present** loads newer pages, until a short page means it has reached the present again.
- **Live messages while away from the present** only update unread state. They aren't inserted, because that would put a gap in the list; they show up once the newer pages reach them.
- **Acks are skipped while away from the present**, so jumping back can't mark unseen messages read.
- **A "Jump to present" pill** reloads the latest page and scrolls to the end.
- **The jumped-to message is highlighted for about 2 s.**
- **Rejected alternative:** a second, separate history alongside the tail, like Discord's detached view. Every part of `ChatStore` (edits, deletes, reactions, pending messages, reply sources) assumes one history per channel. Replacing the window keeps one source of truth, and a reload brings back the present.

**The right panel** is one `SidePanel` state in `ChatView`: `Members`, `Search { … }` or `Pins { … }`, plus whether the member list is open.
- **The open/closed state is app-wide and remembered** in `Prefs.member_list_open` (default on), like the web client's toggle.
- **Search and pins are temporary:** closing them, or switching channel, goes back to members (if open) or closes the panel.
- **The panel has a fixed width of 240 px** with a left border, matching the sidebar's style.

**Permissions** (`src/permissions.rs`, new): an implementation of `revolt_permissions::PermissionQuery` over `ChatStore`, for (user, channel), which `channel_permissions` evaluates. The trait is `async`, but every answer comes from memory, so it runs with `futures::executor::block_on` (`futures` is already a dependency). It's used for:
- "can I pin here?"
- leaving members who can't see a restricted channel out of the list

Rejected: hand-rolling the permission maths. Stoat's own crate is the reference implementation, and it handles owners, timeouts, defaults, role overrides and channel overrides.

## Assumptions
1. **The send button** uses Lucide's `send-horizontal` icon as a ghost button, right of the emoji button.
   - **Disabled** (dimmed) when `check_send` wouldn't send: blank text with no attachments, or over 2000 characters.
   - **While editing** it shows a check icon with the tooltip "Save edit", and saves.
   - Clicking it calls the same `submit()` as Enter, then focuses the box again.
2. **Search runs on Enter, not per keystroke** (as on the web client), with a limit of 50 and no "load more". A query over 64 characters shows a hint instead of sending. The sort is Latest by default, and changing it searches again.
3. **Search results and pins render as compact message rows.** Each row shows its author, time, Markdown body and attachments, but has no hover toolbar, reactions picker or right-click menu. Clicking a row jumps to it.
4. **Switching channel resets search and pins** back to the member list (or a closed panel), and clears the search box. The web client does the same: its sidebar state lives in the channel view.
5. **A jump scrolls the target to the top of the view** (`scroll_to_item`, offset 0) and highlights it. The web client centres it instead; centring needs viewport maths `MessageScrollerState` doesn't expose. Revisit if the top position feels wrong.
6. **Sending while away from the present** returns to the present first, then posts. Pending messages are drawn at the end of the timeline, which would otherwise sit after a gap.
7. **A channel keeps its jumped-to window when you leave it and come back**, with the "Jump to present" pill still showing. Nothing is acked until you return to the present.
8. **Large servers (`approximate_member_count > 1000`)** fetch only online members for the panel (`exclude_offline=true`), and the panel shows no Offline section, with a one-line note saying why. `@` suggestions keep their own on-demand full fetch, unchanged.
9. **Role colour** is the colour of the member's highest-ranked role that has one. Solid CSS colours are parsed with `svgtypes`, which is already used for `$$\color$$`. Gradients and anything else unparseable fall back to the default text colour.
10. **Presence:**
    - "Online" means `user.online` and the presence isn't `Invisible`.
    - The dot colour comes from `status.presence`: Online green, Idle yellow, Focus blue, Busy red, otherwise grey.
    - The second line is `status.text`, else the presence name.
    - Offline rows are drawn muted.
11. **Pin and unpin confirm first, unless Shift is held**, the same as Delete and the web client. Pinning posts a system message everyone sees, so a slip deserves a confirmation.
12. **`AlreadyPinned` and `NotPinned` count as success.** The message is already in the state asked for (another client got there first), so the pins list is simply re-fetched.
13. **The pins list refreshes by re-fetching** (debounced 500 ms) when a `MessageUpdate` in that channel sets or clears `pinned` while the pins panel is open. This is simpler than patching the list, and well within the rate limit.
14. **No pinned marker in the timeline.** The right-click menu's "Unpin message" and the pins panel are enough for now, since you asked to defer extras.

## Open Questions
- *(Deferred, your call)* **Member actions:** a profile card on click, "Message", and right-click menus on members.
- *(Deferred)* **Server-wide search:** Stoat 0.16.2 has no endpoint for it. The web client has an open PR for a Ctrl+K global search.
- *(Deferred)* **Paging past 50 search results.**

## Plan

### Chunk 1: Send button
**Goal:** A send button on the right of the message box sends exactly as Enter does, and saves while editing.
**Files likely touched:**
- `src/assets.rs`: add `SendHorizontal` (and `Check`, if `IconName::Check` isn't already among the defaults) to `ExtraIcons`.
- `src/draft.rs`: `pub fn can_send(text: &str, has_attachments: bool, editing: bool) -> bool`, built on `check_send`. While editing, it's true for non-blank text within the limit.
- `src/ui/composer.rs`: a `Button::new("send")` after the emoji popover in `render`.
  - `.ghost().icon(…)`, with the tooltip "Send" or "Save edit"
  - `.disabled(!can_send(…))`
  - `on_click` calls `submit(window, cx)` then `focus`
- `tests/` or the `draft.rs` unit tests: `can_send` cases.
**Depends on:** None
**Risks:**
- **Focus:** the click takes focus away from the `Textarea`, so `submit()` must run and the box must be focused again, or typing stops working.
- **Icon:** `IconName::SendHorizontal` must be the generated name for `send-horizontal.svg`. Check `IconName::ALL` if it doesn't compile.
**How to verify:**
- `cargo test can_send` covers blank text, blank text with an attachment, whitespace only, 2000 vs 2001 characters, and editing with blank vs non-blank text.
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- `cargo run`, then type a message and click the button: it sends, the box clears and keeps focus.
- The button is dimmed when the box is empty, and enabled with only an attachment.
- Editing a message shows the check icon, and clicking it saves.
**Shippable when:** The button sends and saves exactly like Enter, its disabled state follows `check_send`, and the tests pass.

### Chunk 2: Jump to message
**Goal:** Clicking a reply's original jumps to it even when it isn't loaded. This builds the history-window machinery that search and pins rely on, and is useful on its own.
**Files likely touched:**
- `src/api/client.rs`:
  - generalise `messages_query` to take `before`, `after` and `sort`
  - `fetch_messages_after(channel, after, limit)` with `sort=Oldest`
  - a unit test for the query string
- `src/store.rs`:
  - `History.at_present`
  - `set_history` sets it to true
  - `set_window(channel, page, target, limit)` replaces the history:
    - `exhausted` is true when fewer than `limit/2+1` messages are older than `target`
    - `at_present` is true when fewer than `limit/2+1` are at or after `target`, or the page contains the channel's `last_message_id`
  - `append_page(channel, page, limit) -> bool` adds newer messages, sets `at_present` on a short page, and returns whether anything was added
  - `apply_message` doesn't insert into a history that isn't at the present (it still bumps `last_message_id`, unreads and mentions)
  - `is_at_present(channel)`
  - `MessageRow.highlighted`, set from a `highlight: Option<String>` field via `set_highlight` and `clear_highlight`
- `src/ui/chat.rs`:
  - `jump_to_message(channel, id)`:
    - if the message is loaded, scroll and highlight
    - otherwise fetch around it, `set_window`, sync, then scroll to its row once the reset lands (`Pane.scroll_target`)
  - `RowAction::JumpTo` uses it.
  - `maybe_load_newer`, observed with `maybe_load_older`, runs when the history isn't at the present and the list isn't scrolled up.
  - A bottom-centre "Jump to present" pill (styled like "Loading older messages…") reloads with `load_history` and calls `scroll_to_end`.
  - `maybe_ack_current` returns early when the history isn't at the present.
  - Sending while away from the present reloads the present first (Assumption 6).
  - The highlight is cleared after 2 s.
  - `render_message_row` tints highlighted rows (`theme.accent` at low opacity).
- `src/assets.rs`: `ArrowDown` for the pill.
- `tests/store_history.rs`:
  - window bounds (`exhausted` and `at_present` from counts, and from `last_message_id`)
  - `append_page` reaching the present
  - a live message not inserted away from the present, but unread state updated
  - edits, deletes and reactions still applying inside a window
  - highlight on the row
**Depends on:** None
**Risks:**
- **Tail following:** `sync_pane`'s `RowOp::Reset` calls `scroll_to_end`, and `MessageScrollerState::reset` turns tail-follow mode back on. After a window reset, that must be replaced by `scroll_to_item(target)`, and tail following must not snap the view back to the bottom. If `scroll_to_item` doesn't leave tail mode, the list will jump to the end. In that case, look for a `set_follow_mode`-style hook in `message_scroller.rs` and use it (gpui-component 0.9.0, `src/message_scroller.rs`). If none exists, wrap the jump in a deferred second `scroll_to_item` after layout.
- **Newer-page loop:** "not scrolled up" is also true for a short list, so `maybe_load_newer` needs an in-flight guard (like `loading_older`) and must stop at `at_present`, or it will request in a loop.
- **Pending and reply-source code** assumes the newest message is loaded. Re-run `store_pending` and `store_replies` and fix any that assume `at_present`.
- **Unread state:** live messages away from the present must still mark the channel unread (`bump_last_message`), and must never ack.
**How to verify:**
- `cargo test --test store_history` (new window and append tests)
- `cargo test --test store_pending --test store_replies --test store_live`
- `cargo test --lib client` (query strings)
**Manual check:**
- In a busy channel, reply to a message from a few hundred messages back (or find an old reply). Restart, then click the reply preview: the view jumps to the original and highlights it.
- Scrolling down loads newer pages until the pill disappears.
- "Jump to present" returns to the newest message.
- A message arriving while away from the present makes the pill show and the channel stay unread; it doesn't appear mid-list.
**Shippable when:** Reply previews jump to unloaded originals, newer paging returns to the present, acks are suppressed while away from the present, and the tests pass.

### Chunk 3: Right panel and member list
**Goal:** Server channels and groups show a toggleable member list on the right, grouped as on the web client, with live presence.
**Files likely touched:**
- `Cargo.toml`: `revolt-permissions = { version = "=0.16.2", default-features = false }`, bumped together with the other `revolt-*` crates.
- `src/permissions.rs` (new): `channel_permissions(store, user, channel) -> u64` via a `PermissionQuery` implementation over the store's users, servers, members and channels, plus `can_view(store, user, channel)`. Saved Notes, DMs and groups follow the crate's rules.
- `src/store.rs`:
  - `member_list(channel) -> MemberList { title, rows: Vec<MemberListRow>, offline_hidden: bool }`, where a row is `Header { name, count }` or `Member { user_id, name, avatar_url, colour: Option<String>, presence: PresenceKind, status: String, online: bool }` (Approach and Assumptions 8–10)
  - `role_colour(server, user)`
  - accessors the permissions module needs
- `src/api/client.rs`: `fetch_members(server, exclude_offline: bool)`. Existing callers pass `false`.
- `src/prefs.rs`: `Prefs.member_list_open: bool`, default `true`, so older files load with it on.
- `src/ui/chat.rs`:
  - a `SidePanel` state
  - header buttons on the right: a `Users` toggle, hidden in DMs and Saved Notes
  - a `render_side_panel` function and `render_member_row`: a 32 px avatar with a presence dot, name in role colour, and status line; rows are a fixed 42 px in a `uniform_list` so long lists are virtualised
  - opening a server channel with the panel open triggers a member fetch (online-only for large servers, tracked separately from the `@` fetch)
  - presence-only `UserUpdate`s call `cx.notify()` while the panel is visible
- `src/ui/app_root.rs` (or wherever `Prefs` is saved): pass `member_list_open` through and save it when toggled, the same way `skin_tone` is handled.
- Tests:
  - `tests/permissions.rs` (new): owner, default permissions, role allow and deny, channel override hiding a channel from a role, timed-out member, DM and Saved Notes.
  - `tests/store_members.rs` (new):
    - hoisted-role sections in rank order
    - an online member placed under their first hoisted role
    - offline members under Offline regardless of role
    - name sort using nicknames
    - a restricted channel leaving out members who can't view it
    - large-server `offline_hidden`
    - a group's flat list and its title
    - role colour choice, and gradient fallback
**Depends on:** None. It's independent of chunk 2.
**Risks:**
- **Permission data:** `PermissionQuery` needs the server's `default_permissions` and role permissions, the channel's `role_permissions` and `default_permissions`, and the target member's roles and timeout. All of these are in `Ready` and the member fetch. Check that `ChannelUpdate` and `ServerUpdate` keep them current. If server role edits aren't applied live yet, note it rather than widen scope.
- **Presence updates:** Stoat may only send `UserUpdate { online }` for users it considers related to you. If presence doesn't change live for ordinary server members, that's a backend limit; record it and leave refreshing for later.
- **Huge servers:** even the online-only fetch can be thousands of members. `uniform_list` keeps rendering cheap, but `member_list` must not be rebuilt every frame. Cache it per channel and invalidate it on member, user or role changes.
- **Width:** the panel takes 240 px from the message area. Check that a narrow window still leaves the timeline usable, and hide the panel below some width only if it's clearly needed.
**How to verify:**
- `cargo test --test permissions --test store_members`
- `cargo test` (the prefs round-trip test, with an old file missing the key)
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- Open a server channel: hoisted roles, Online and Offline sections and role colours match the web app for the same channel.
- Toggle the panel off, restart, and it stays off.
- A DM has no toggle; a group lists its members.
- Changing your own status in the web app updates your dot and status line here.
**Shippable when:** The member list matches the web client's grouping for the user's servers, the toggle is remembered, and the tests pass.

### Chunk 4: Search
**Goal:** A search box in the channel header finds messages in that channel. The results show in the right panel, and clicking one jumps to it.
**Files likely touched:**
- `src/api/client.rs`: `search_messages(channel, query, sort, limit) -> BulkMessageResponse`, which is `POST /channels/{c}/search` with `include_users: true` and **no `pinned` key** (build the body through the existing `without_nulls`, and test that `pinned` is absent).
- `src/store.rs`:
  - `absorb_results(page) -> Vec<Message>`, which stores users and members but doesn't touch history
  - `result_rows(channel, &[Message]) -> Vec<MessageRow>`, using the existing `message_row(…, continuation: false)` so every result has its author header
- `src/ui/chat.rs`:
  - a header `Input` with the placeholder "Search messages…" and a search icon; Enter runs the search, Esc clears it and closes the panel
  - `SidePanel::Search { query, sort, state: Loading | Results(rows) | Empty | Error(msg) }`
  - a Latest / Oldest / Relevance button group, where a change searches again
  - results drawn with a `preview` mode of `render_message_row` (Assumption 3), with clicks going to `jump_to_message`
  - an over-64-character hint
  - channel switching resets the panel (Assumption 4)
  - a stale-response guard: a result for an old query or channel is dropped
- `tests/api_search.rs` (new, `wiremock`): the request path, the body (`query`, `sort`, `limit`, `include_users`, no `pinned`), parsing of `{messages, users, members}` and of a bare array, and `InvalidOperation` and rate-limit errors mapped to `ApiError`.
- `tests/store_search.rs` (new): results don't change the history, authors resolve from the result users, and result rows always carry headers.
**Depends on:** Chunk 2 (jumping), Chunk 3 (the panel)
**Risks:**
- **Input focus:** the header `Input` takes keyboard focus. Make sure Esc and Enter there don't reach the composer's captured actions, and that typing in the composer doesn't search.
- **Preview rows:** `render_message_row` takes an `OnRowAction` and adds a context menu and hover toolbar. A `preview` flag must turn those off without duplicating the row code.
- **Search semantics** (`$text` whole words) may surprise people who expect substring matches. Mention it in the empty state ("No messages match. Search matches whole words.").
**How to verify:** `cargo test --test api_search --test store_search` and `cargo clippy --all-targets -- -D warnings`.
**Manual check:**
- In a channel with history, search for a word from an old message: results appear newest first.
- Clicking one jumps there and highlights it.
- Oldest and Relevance reorder the results.
- Switching channel resets the panel.
- Results match the web app's search for the same query.
**Shippable when:** Searching a channel lists matches in the panel, a click jumps to the message, and the tests pass.

### Chunk 5: Pins: list, pin and unpin
**Goal:** See a channel's pinned messages in the right panel, and pin or unpin messages from the right-click menu where permitted.
**Files likely touched:**
- `src/api/client.rs`:
  - `fetch_pins(channel)`: search with `{ pinned: true, sort: Latest, limit: 100, include_users: true }` and no `query`
  - `pin_message(channel, id)` and `unpin_message(channel, id)`, which return `()` on 204
- `src/store.rs`:
  - `can_pin(channel)`: a DM, or `ManageMessages` from `permissions::channel_permissions` for `me`
  - `MessageRow.pinned` (from `message.pinned`)
  - `MessageRow.can_pin`, or pass it beside the rows, whichever keeps `diff_rows` stable
- `src/ui/chat.rs`:
  - a `Pin` header button (tooltip "Pinned messages") in every channel type
  - `SidePanel::Pins { state }`, drawn with the same preview rows as search, newest first; clicks jump to the message
  - `RowAction::TogglePin { id, pin: bool, confirmed }`
  - a context-menu item, "Pin message" / "Unpin message" with the `Pin` / `PinOff` icons, shown when `can_pin`
  - a confirm dialog modelled on `confirm_delete` that shows the message snippet; Shift skips it
  - errors: `AlreadyPinned` and `NotPinned` count as success, and other errors show a toast
  - live refresh: a `MessageUpdate` in the open pins channel whose `data.pinned` is set or whose `clear` includes `Pinned` re-fetches the list (debounced 500 ms); `MessageDelete` drops the row
- `src/api/error.rs`: map `AlreadyPinned` and `NotPinned`, if they aren't already distinct.
- `src/assets.rs`: `Pin` and `PinOff`.
- Tests:
  - `tests/api_pins.rs` (new, `wiremock`): the `POST` and `DELETE` paths with the session header and 204; `AlreadyPinned` and `NotPinned` decoding; the `fetch_pins` body (`pinned: true`, no `query`).
  - `tests/permissions.rs`: `can_pin` true in a DM, in Saved Notes, as owner and with a `ManageMessages` role; false for a plain member and when a channel override denies it.
  - `tests/store_live.rs`: a `MessageUpdate` setting and clearing `pinned` updates `MessageRow.pinned`.
**Depends on:** Chunk 2 (jumping), Chunk 3 (panel and permissions), Chunk 4 (preview rows)
**Risks:**
- **Groups:** a group owner gets every permission, but other members' defaults are unverified. If the server refuses a pin the client allowed, show the error toast. If it allows one the client hid, check the group's `permissions` field against `revolt-permissions`' group rules.
- **System messages:** each pin or unpin adds a "… pinned a message" system row to the timeline (already rendered by `system_text`). That's expected; don't hide it.
- **Row identity:** adding `pinned` or `can_pin` to `MessageRow` changes row equality, so a pin toggle should `Remeasure` the row, not reset the list. Check that `diff_rows` does that.
**How to verify:** `cargo test --test api_pins --test permissions --test store_live` and `cargo clippy --all-targets -- -D warnings`.
**Manual check:**
- In a server where you have Manage Messages, right-click a message and choose "Pin message": it asks first (Shift skips), and the web app shows it pinned.
- The pins panel lists it, and clicking it jumps there.
- Unpinning in the web app removes it from the panel within about a second.
- In a server where you lack the permission, the menu item is absent.
- In a DM, pinning works.
**Shippable when:** Pins list, live-refresh and jump; pin and unpin follow permissions and confirm; and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`. Optional live checks: `cargo test -- --ignored`.

### Unit Tests
- **`can_send`:** blank, whitespace, attachments only, the length limit, and editing.
- **History windows:**
  - `set_window` bounds (`exhausted` and `at_present` from counts and from `last_message_id`)
  - `append_page` reaching the present
  - live messages not inserted away from the present, while unread state still updates
  - edits, deletes and reactions applying inside a window
  - the highlight flag
- **Permissions:** owner, defaults, role allow and deny, channel overrides, timeouts, DMs, Saved Notes and groups. Also `can_view` for restricted channels and `can_pin`.
- **Member list:**
  - hoisted-role sections by rank, Online and Offline placement
  - nickname sort
  - restricted-channel filtering
  - `offline_hidden` for large servers
  - groups
  - role colour, with gradient fallback
- **Search and pins results:** absorbed users without touching history, and rows with headers.
- **Query strings and bodies:** `after` with `Oldest`; search with no `pinned`; pins with no `query`.

### Integration Tests
- **`wiremock`:**
  - `POST /channels/{c}/search`, both response shapes
  - `POST` and `DELETE /channels/{c}/messages/{m}/pin` returning 204
  - `AlreadyPinned` / `NotPinned` / `InvalidOperation` and rate-limit errors
  - `GET /servers/{id}/members?exclude_offline=true`
  - `GET /channels/{c}/messages?after=…&sort=Oldest`
- **Store pipeline:**
  1. Load the latest page.
  2. Jump to an old message, which replaces the history with a window around it.
  3. A live message arrives: unread, not inserted.
  4. Append pages until `at_present`.
  5. The live message is now in the list exactly once.

### Manual Tests
1. **Send button:** it sends, keeps focus, is disabled when empty, and saves edits.
2. **Reply jump:** a reply to an old message jumps and highlights. Scrolling down returns to the present, and "Jump to present" works. Nothing is acked while away from the present.
3. **Member list:** it matches the web app's sections and colours. The toggle is remembered across restarts. DMs have no toggle, and groups list their recipients.
4. **Live presence:** changing your status in the web app updates your row.
5. **Search:** finds old messages; sort changes reorder; a click jumps; switching channel resets.
6. **Pins:** the list matches the web app. Pin and unpin from the menu ask first (Shift skips) and show in both clients, and the panel refreshes live. The menu item is hidden without permission, and pinning works in DMs.
7. **Layout:** a narrow window with the panel open still shows a usable timeline.

## Definition of Done
- [ ] **Send button:** sits right of the emoji button, sends and edits through the same `submit()` as Enter, is disabled exactly when `check_send` wouldn't send, and the message box keeps focus after a click.
- [ ] **Jump to message:**
  - `RowAction::JumpTo`, search results and pins all jump to a message whether loaded or not, scroll it into view and highlight it for about 2 s.
  - Away from the present, scrolling down loads newer pages and a "Jump to present" pill shows.
  - Live messages don't create gaps, and no acks are sent until back at the present (tested).
- [ ] **Right panel:** one 240 px panel shows the member list, search or pins.
  - The members toggle is hidden in DMs and Saved Notes, and its state is saved in `Prefs.member_list_open`.
  - Switching channel resets search and pins.
- [ ] **Member list:**
  - Sections are hoisted roles by rank, then Online, then Offline.
  - Rows are sorted by display name and show role colours, presence dots and status text.
  - Members who can't view a restricted channel are left out (tested with `revolt-permissions`).
  - Large servers (> 1000) list online members only, with a note.
  - Groups show a flat, name-sorted list.
  - Presence updates live.
- [ ] **Search:** `POST /channels/{c}/search` is sent without `pinned`, with a limit of 50 and `include_users`; this is tested.
  - The header box runs it on Enter, with a Latest / Oldest / Relevance sort.
  - Results show as compact rows and jump on click.
  - Queries over 64 characters show a hint.
- [ ] **Pins:**
  - The header button lists pins newest first (`pinned: true`, no `query`), refreshes live on pin and unpin events, and jumps on click.
  - "Pin message" / "Unpin message" appears only when `can_pin`, confirms unless Shift is held, and calls `POST` / `DELETE …/pin`.
  - `AlreadyPinned` and `NotPinned` are treated as success.
- [ ] **Checks:** `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` passes.
