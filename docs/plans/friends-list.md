# Plan: Friends list (RuSToat milestone 8)

## Summary
A Friends view in Home shows your friends, pending requests and blocked people, in the web client's tabs: Online, All, Pending and Blocked.
- **Acting on people:** accept, reject or cancel requests, message friends and unblock people, all from the rows.
- **Adding friends:** send a request by `username#1234`.
- **Being told:** incoming requests count on the Home and Dock badges, and a new request arrives as a toast, or a macOS banner when the window isn't in front.

## Approach
Keep milestone 7's architecture: rules in GPUI-free code with unit tests, and thin views.

**New model, `src/friends.rs`.** GPUI-free, in the style of `src/profile.rs`. It builds the lists from what the store already has. Stoat's `Ready.users` includes everyone the user has a relationship with, with `relationship` set, and milestone 7's `apply_relationship` keeps it live from route answers and the `UserRelationship` event. So the lists need no new fetch.

**Actions reuse milestone 7.** The row buttons and menus call `ChatView::set_relationship` (with Remove friend and Block confirmations), `message_user`, `open_card` and `People::menu`.

**Where the view lives.** It's a "Friends" row pinned at the top of Home's channel list. Choosing it puts the reserved ID `FRIENDS = "friends"` in Home's selection. Stoat IDs are 26-character ULIDs, so it can't clash with a channel. Because the selection is the existing `selected: HashMap<Place, String>`, it's remembered across launches through `places.json` with no format change.
- `current_channel()` returns `None` while it's selected, so the header, message list, composer, side panel and acks, which already handle "no channel", stand down.
- `render_main` draws the Friends page instead.

**Rejected alternative:** a typed selection (`enum HomeSelection { Channel(String), Friends }`) in `places.rs`. It's cleaner in the abstract, but it changes the on-disk format and every `selected` lookup. The reserved ID also gives notifications a free route: a banner tagged with channel `"friends"` already flows through `parse_banner_tag` → `open_channel`.

**Web client reference:** `stoatchat/for-web` `56bc94c` (read 2026-11-06):
- `packages/client/src/interface/Friends.tsx`:
  - The tabs are Online (online friends), All (friends), Pending (an Incoming list, then an Outgoing list) and Blocked. Each list is sorted by display name, with a "Title – count" header and "Nobody here right now!" when empty.
  - Rows show a 36 px avatar, with a presence dot for friends only, and the display name. Clicking a row opens the profile and right-clicking opens the person menu.
  - The Pending tab shows a badge with the incoming count, capped at "99+".
  - An "Add a new friend" button opens a dialog.
- `packages/client/components/modal/modals/AddFriend.tsx`: a `username#1234` field. "Send Request" does `POST /users/friend { username }`.

The web client has no Accept or Reject buttons on its rows (only in the menu). This client adds inline buttons, because the user asked for them.

**Backend reference:** `stoatchat/stoatchat` `df96bca`, `crates/delta/src/routes/users/send_friend_request.rs`. `POST /users/friend` with `DataSendFriendRequest { username }` (`revolt-models` 0.16.2) does this:
- It splits the username on `#`. Without a `#` it returns `InvalidProperty`.
- It looks the user up by username and discriminator. An unknown one is `NotFound`.
- Either side being a bot is `IsBot`.
- Otherwise it calls the same `add_friend` as `PUT /users/{id}/friend`. That gives the same `AlreadyFriends`, `AlreadySentRequest`, `Blocked`, `BlockedByOther` and `TooManyPendingFriendRequests` errors, and it returns the target `User`.

**Codebase context** (on top of milestone 7, `81bb298`):
- **Home:**
  - `ChatView::sections()` lists Home as `store.home_channels()`.
  - `render_channel_list` draws the rows with `render_channel_row`.
  - `current_channel()` returns the selected channel, or else the first one.
  - `drop_stale_selection()` drops selections that `store.shows_channel` rejects. It must learn to keep `FRIENDS`.
- **Main pane:** `render_main` (`src/ui/chat.rs`) draws the header, messages, composer and side panel for `current_channel()`.
- **Lists:** the member list (`render_member_panel` / `render_member_row`, a `uniform_list` of fixed-height rows with presence dots) is the pattern for friend rows. Rows already open cards and menus through `People` (`open_card`, `menu(user, profile)`).
- **Relationships:**
  - `src/profile.rs` has `RelationshipAction`, `friend_buttons`, `can_message` and `user_menu`.
  - `ChatView::set_relationship(user, action, confirmed)` sends the request, guarded per person by `relationship_pending`.
  - `relationship_changed(user)` applies the result.
- **Badges:**
  - `ChatStore::home_summary()` → the rail's Home badge (`render_rail`).
  - `account_summary()` → the switcher, built on `home_summary`.
  - `attention_count()` → the Dock, summed by `AppRoot::sync_outside`.
- **Notices:**
  - `notify::Notice { tag, channel, title, body }` and `notify::delivery(window_active, banners_available)`.
  - `ChatView::show_toast` (click → `open_channel(notice.channel)`) and `show_banner`, which is tagged `banner_tag(account, notice.channel)` and routed by `AppRoot::open_notice` → `open_channel`.
  - `self.banners` maps channels to messages so banners are dismissed once read (`sync_outside`).
  - `should_notify` suppresses everything while the user is Busy.
- **Input boxes:** the header's search box (`search_input: Entity<InputState>`, `run_search`) is the pattern for the add-friend field.

## Assumptions
1. **The Friends row sits at the top of Home's list, above Saved Notes**, and opens the Friends page in the main pane (the user's answer to question 1).
   - It shows a red badge with the number of incoming requests.
   - Home with nothing selected still opens its first conversation, as today. Friends only opens when chosen, from a notification, or when it was the last thing open (it's restored from `places.json`).
2. **No "ignore"** (the user's answer to question 2). Incoming requests offer Accept and Reject only.
3. **Tabs as in the web client** (the user's answer to question 3): Online, All, Pending (Incoming, then Outgoing) and Blocked.
   - The Online tab opens by default.
   - The tab isn't saved across launches.
   - A notification click opens Pending.
4. **Each row has inline buttons for its tab:**
   - Online and All: Message (`message_user`).
   - Incoming: Accept (primary) and Reject (icon).
   - Outgoing: Cancel.
   - Blocked: Unblock.

   Clicking the rest of a row opens the profile card; right-clicking opens the person menu. Remove friend and Block stay in the menu, with milestone 7's confirmation. Buttons are disabled while that person's request is pending (`relationship_pending`).
5. **"Online" follows the member list's presence rule** (`presence_kind != Offline`), so invisible friends count as offline. The web client uses `user.online` only. Matching this client's member list matters more than matching the web client.
6. **Sorting:** each list is sorted by display name, case-insensitively, then by ID, as the web client does. The presence dot shows for friends only, as on the web.
7. **Add friend** (the user's answer to question 4a) is an "Add friend" button in the page header, on every tab. It opens a dialog with a `username#1234` field, Close and "Send request", as the web client does. *(Amended during manual testing: this was first a field always visible at the top of the page. The user asked for a dialog instead.)*
   - The dialog opens empty with the field focused, and stays open after a request goes out, so another can follow.
   - It checks the shape first: a non-empty name, `#`, and four digits. The field shows what went wrong under it.
   - Server errors also show there, in plain words: NotFound → "There's no one called {name}."; InvalidProperty → the shape hint; IsBot → "Bots can't be friends."; plus milestone 7's relationship messages.
   - When it succeeds, the field clears, "Friend request sent to {name}." shows under it, and the person appears under Pending → Outgoing.
   - `AlreadySentRequest` and `AlreadyFriends` aren't errors. They show "You've already sent them a request." and "You're already friends.", as information.
8. **Badges** (the user's answer to question 4b): each incoming request counts as one mention.
   - It counts toward Home's rail badge (`home_summary`), the account switcher (`account_summary`, through `home_summary`) and the Dock (`attention_count`).
   - It doesn't add an unread dot, only the count, the way mentions do.
   - Blocked people's requests can't exist, since Stoat removes them on block.
9. **Notices** (the user's answer to question 4c): a `UserRelationship` event that turns someone's relationship into `Incoming`, from anything else, gives a notice: title "Friend request", body "{display name} sent you a friend request."
   - **Not for:** requests already pending at `Ready`, the user's own Outgoing requests, or when the user is Busy (as messages). Focus still notifies, since a request is aimed at the user, like a mention. And not when the window is active with the Friends page on Pending (they're looking at it).
   - **Delivery:** `notify::delivery` picks a toast or a banner, as for messages.
   - All friend-request notices of an account share one tag (channel `"friends"`), so a newer one replaces the older.
   - Clicking it opens Friends on the Pending tab.
   - A friend-request banner is dismissed once no incoming requests are left, or once the Friends page is open.
10. **Remove friend and Block keep milestone 7's behaviour** and confirmations. This plan adds no new relationship rules.

## Open Questions
- *(Deferred)* **Ignore.** This is a local-only hide, which Stoat doesn't support server-side. The user chose not to have it.
- *(Deferred)* **Mutual friends and servers on rows**, the way the profile modal on the web shows them (`GET /users/{id}/mutual`).
- *(Deferred)* **Create group from the Friends page.** The old Revolt client had this.

## Plan

### Chunk 1: The Friends page, read-only
**Goal:** a Friends row at the top of Home opens a page with Online, All, Pending and Blocked tabs. It lists the right people live. Rows open the profile card on click and the person menu on right-click, so requests can already be handled through milestone 7's menu.
**Files likely touched:**
- `src/friends.rs` (new, GPUI-free), and `src/lib.rs` to register it:
  - `pub const FRIENDS: &str = "friends"`, the reserved Home selection.
  - `FriendsTab { Online, All, Pending, Blocked }`.
  - `FriendRow { user_id, name, avatar_url, presence, status, relationship }`. `status` is the custom status, else the presence label, as `MemberListRow` has it.
  - `friend_lists(store) -> FriendLists { online, all, incoming, outgoing, blocked }`, each sorted (Assumption 6).
  - `incoming_count(store)`.
- `src/store.rs`: `users()`, an iterator over every known user. Reuse `presence_kind`, `display_name` and `user_avatar_url`.
- `src/ui/friends.rs` (new), and `src/ui/mod.rs` to register it: `render_friends_page(lists, tab, colors, on_tab, people)`. It draws:
  - a 48 px header "Friends" with the Users icon
  - a tab bar, with the Pending tab showing the incoming count (capped at "99+")
  - for each list: a "TITLE — count" header, then a `uniform_list` of fixed-height (48 px) rows, or "Nobody here right now!"
  - rows like `render_member_row`: a 36 px avatar with a presence dot for friends only, the name, and the status line
- `src/ui/chat.rs`:
  - `friends_tab: FriendsTab` (default `Online`).
  - In `render_channel_list`, the Friends row above the Home sections, for `Place::Home` only. It's selected when Home's selection is `FRIENDS`, has a Users icon and a red badge (`mention_badge`) with `incoming_count`, and clicking it calls `select_channel(FRIENDS)`.
  - `current_channel()` returns `None` when `selected[Home] == FRIENDS` (no fallback to the first DM).
  - `drop_stale_selection()` keeps `FRIENDS`.
  - `open_channel(FRIENDS)` opens Home with Friends. Chunk 5 uses this.
  - `render_main` draws `render_friends_page` while Friends is selected, instead of the header, messages, composer and side panel.
  - Rows: click → `People::open_card`, right-click → `People::menu(user, true)`, built with `self.people(None, cx)`. There's no channel, so Mention doesn't appear.
  - `UserUpdate` for someone on the open Friends page (anyone with a relationship) must `cx.notify()`. Today it returns early for other people's presence changes unless the member panel lists them.
- `tests/friends.rs` (new):
  - each list's membership by relationship: Friend → all; online friends → online; Incoming, Outgoing, Blocked; `None`, `BlockedOther` and `User` → nowhere
  - an invisible friend isn't online
  - sorting by display name, case-insensitive, with an ID tiebreak
  - `incoming_count`
  - a `UserRelationship` (through `apply_relationship`) moves someone between lists
  - the `Ready` fixture: build lists from `tests/fixtures/ready.json`, adding relationship users to it if it has none
**Depends on:** milestone 7 (`People`, `apply_relationship`, the `UserRelationship` event).
**Risks:**
- **`current_channel()` returning `None` in Home is new.** Until now that only happened before `Ready`, or in an empty place. Check every caller: `sync_composer`, `sync_side_panel`, `maybe_ack_current`, `ensure_current_loaded`, `render`'s deferred `maybe_load_older/newer`, typing, `member_panel_lists` and `sync_card`. Each must do nothing quietly. In particular, the composer must not keep the previous channel's draft live, and must not send typing events.
- **`drop_stale_selection` and `remember_places`** must keep `FRIENDS`. If they don't, Friends is lost on every reconnect, or never restored at launch.
- **Live presence:** if the `UserUpdate` early return isn't widened, the Online tab goes stale until something else redraws.
**How to verify:**
- `cargo test --test friends`
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- Click Friends in Home. Compare the four tabs with the web app's Friends page for the same account: the same people, order and counts.
- Have a friend go online or offline: the Online tab follows.
- Right-click an incoming request and choose Accept friend request: they move to All.
- Quit and relaunch with Friends open: it reopens.
- Open a DM, then Friends: the composer and member panel go away, and come back when a channel opens.
**Shippable when:** the Friends page shows correct, live lists in four tabs, rows open the card and menu, Friends survives a relaunch, and the tests pass.

### Chunk 2: Inline row actions
**Goal:** every row has the buttons for its tab (Assumption 4), so requests are handled in one click.
**Files likely touched:**
- `src/friends.rs`: `row_actions(store, row) -> Vec<RowAction>`, where `RowAction` is `Message` or a `Relationship(RelationshipAction)`:
  - a friend gets `Message`
  - Incoming gets Accept and Reject
  - Outgoing gets Cancel
  - Blocked gets Unblock

  These are the same rules as `profile::friend_buttons` plus `can_message`; reuse them, don't restate them.
- `src/ui/friends.rs`: buttons at the row's right edge.
  - Accept: primary, small, labelled. Reject: an outline icon button (UserX) with a tooltip. Cancel: outline, labelled "Cancel". Unblock: outline, labelled. Message: a ghost icon button (MessageCircle) with a tooltip.
  - Each click calls `stop_propagation`, so the row doesn't also open the card.
- `src/ui/chat.rs`: wire the buttons to `set_relationship(user, action, shift)` and `message_user(user)`, disabled while `relationship_pending` holds the user.
- `tests/friends.rs`: `row_actions` for each relationship and for bots. A bot friend can only exist as `Friend` through a bot invite, and still gets Message.
**Depends on:** Chunk 1.
**Risks:**
- **Clicks on buttons inside a `uniform_list` row** must stop propagation. Otherwise Accept also opens the card. `person_target` in milestone 7 is the pattern.
- **Optimistic feel:** nothing moves until Stoat answers (about 100 ms), which matches milestone 7. Don't add optimistic moves. A refusal would then have to put the row back.
**How to verify:**
- `cargo test --test friends`
- `cargo clippy --all-targets -- -D warnings`
**Manual check (needs a second account):**
- Accept, Reject and Cancel requests from the rows: each moves to the right list at once, on both accounts.
- Unblock from Blocked.
- Message from Online opens the DM.
- Clicking a button doesn't open the card.
**Shippable when:** each tab's buttons perform their action, rows update live, and the tests pass.

### Chunk 3: Add a friend by username
**Goal:** send a friend request by `username#1234` from the Friends page.
**Files likely touched:**
- `src/api/client.rs`: `send_friend_request(username) -> User` (`POST /users/friend`, body `DataSendFriendRequest { username }`).
- `src/friends.rs`: `check_username(text) -> Result<&str, &'static str>`. It trims the text, then requires `name#dddd`, with a non-empty name and exactly four ASCII digits. The error is the hint text.
- `src/ui/friends.rs` and `src/ui/chat.rs`:
  - `add_friend_input: Entity<InputState>` (placeholder `username#1234`), with a "Send request" primary button. Enter submits too, as `run_search` does.
  - `add_friend_state: Option<AddFriendState { Sending, Sent(name), Info(text), Error(text) }>`, shown under the field.
  - On success: `relationship_changed(user)`, clear the field, show "Friend request sent to {name}." (Assumption 7).
  - Errors map as in Assumption 7. `relationship_already_done()` errors are information, not errors.
  - Editing the field clears the message.
- `src/api/error.rs`: a readable `InvalidProperty` message, if `describe` doesn't have one ("That doesn't look like username#1234.").
- Tests:
  - `tests/api_users.rs`: the path and body (`body_json`), the returned `User`, and `NotFound` (404), `InvalidProperty` (400) and `IsBot` (400) decoding.
  - `tests/friends.rs`: `check_username` accepts `ada#0001` and `  Ada Lovelace#1234 ` (usernames can contain spaces; check what the backend allows and match it). It rejects `ada`, `ada#`, `ada#12`, `ada#12345`, `#1234` and `ada#12a4`.
**Depends on:** Chunk 1.
**Risks:**
- **Username rules:** confirm what characters a Stoat username may contain (backend `revolt-database` username validation, or `revolt-models` `RE_USERNAME`) before tightening `check_username`. A wrong client-side check blocks real usernames, so err towards letting the server decide.
- **Focus:** the field must take keyboard focus and keep Enter away from the (hidden) composer.
**How to verify:**
- `cargo test --test api_users --test friends`
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- Add a second account by `name#1234`: it appears under Pending → Outgoing, and the web app shows the request.
- Try a name that doesn't exist, one without `#`, a bot, and someone you've already asked: each shows the right message under the field.
**Shippable when:** requests go out by username, every refusal reads clearly, and the tests pass.

### Chunk 4: Badges for incoming requests
**Goal:** incoming requests count on the Friends row, the Pending tab, Home's rail badge, the account switcher and the Dock (Assumption 8).
**Files likely touched:**
- `src/store.rs`:
  - `home_summary()` adds `friends::incoming_count(self)` to its mention count, and `account_summary()` follows from it.
  - `attention_count()` / `attention_breakdown()` add one entry, "Friend requests", with that count. The Dock badge's debug breakdown then explains it.
- `src/ui/chat.rs` and `src/ui/friends.rs`: the Friends row and Pending tab already show `incoming_count` from chunk 1. Check that they update on `UserRelationship`.
- Tests (`tests/store_unreads.rs` or `tests/friends.rs`):
  - `home_summary`, `account_summary` and `attention_count` each rise by one per incoming request, and fall when it's accepted or rejected
  - outgoing requests and friends add nothing
  - the breakdown has a "Friend requests" entry only when there are any
**Depends on:** Chunk 1.
**Risks:**
- **The Dock badge is summed across accounts** in `AppRoot::sync_outside`. Two accounts with requests should add up, which `attention_count` already handles.
- **The badge must update when the event arrives.** Check that `UserRelationship` leads to `cx.notify()`, and so to the owner's `sync_outside`. Milestone 7's handler does notify.
**How to verify:**
- `cargo test --test friends --test store_unreads`
- `cargo clippy --all-targets -- -D warnings`
**Manual check:**
- Send a request from a second account. Home's rail badge, the Friends row, the Pending tab and the Dock each show 1.
- Accept it: they all clear.
**Shippable when:** every badge counts incoming requests and clears when they're handled, and the tests pass.

### Chunk 5: Notices for new requests
**Goal:** a new incoming request shows a toast, or a macOS banner when RuSToat isn't in front. Clicking it opens Friends → Pending (Assumption 9).
**Files likely touched:**
- `src/notify.rs`: `friend_request_notice(store, user, was: &RelationshipStatus, viewing_pending: bool, window_active: bool) -> Option<Notice>`.
  - It returns `None` unless the relationship went from something other than `Incoming` to `Incoming`. Also `None` while the user is Busy, and `None` when the window is active and Pending is open.
  - The notice: `channel: FRIENDS`, `tag: FRIENDS`, title "Friend request", body "{display name} sent you a friend request." (truncated like message bodies).
- `src/store.rs`: `apply_relationship` returns the previous relationship (`Option<RelationshipStatus>`) instead of `bool`, so the event handler can tell a new request apart. Update milestone 7's callers and tests.
- `src/ui/chat.rs`:
  - In the `UserRelationship` handler only (not route answers; the user's own actions never make an Incoming), compute the notice and deliver it through `notify::delivery`.
    - For a toast: `show_toast`. Its click already calls `open_channel(notice.channel)`. Make `open_channel(FRIENDS)` also set `friends_tab = Pending`.
    - For a banner: a new `show_friend_banner` that doesn't go in `self.banners`, which assumes a message ID. It records `friend_banner: bool`.
  - In `sync_outside`, dismiss the friends banner (`banner_tag(account, FRIENDS)`) once `incoming_count == 0` or Friends is open.
- `src/ui/app_root.rs`: nothing should be needed. `open_notice` parses the tag and calls `open_channel(FRIENDS)`. Verify it.
- `tests/notify_rules.rs`:
  - a new request notifies, with the right title and body
  - a request already Incoming before doesn't
  - Busy suppresses it, and Focus doesn't
  - looking at Pending with the window active suppresses it
  - Outgoing, Friend and Blocked transitions don't notify
**Depends on:** Chunk 1 (`FRIENDS` and `open_channel`), and Chunk 4 for the dismissal condition.
**Risks:**
- **Banners need the app bundle** (`scripts/run-bundled.sh`). Under `cargo run` only toasts show, as for messages. Test banners bundled.
- **Changing `apply_relationship`'s return type** touches milestone 7's code and tests (`relationship_changed`, `tests/store_live.rs`). Keep the change mechanical.
- **Toast IDs:** `show_toast` IDs toasts by `banner_tag(account, notice.tag)`. One shared `FRIENDS` tag means a second request replaces the first toast, which is intended.
**How to verify:**
- `cargo test --test notify_rules --test store_live`
- `cargo clippy --all-targets -- -D warnings`
**Manual check (needs a second account; bundled for banners):**
- With RuSToat in front, send a request from the second account: a toast shows, and clicking it opens Friends → Pending.
- With RuSToat in the background (bundled): a banner shows, and clicking it opens the right account on Friends → Pending.
- Accept the request: the banner goes away.
- Set Busy: no notice.
**Shippable when:** new requests notify exactly as Assumption 9 says, clicks land on Pending, banners clear when handled, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`. Optional live checks: `cargo test -- --ignored`.

`cargo fmt --check` already fails on `main` because of formatting drift in many files that predates milestone 7. Follow milestone 7's practice: new files must be rustfmt-clean, and every touched file's `rustfmt --check` hunk count must be no higher than before the chunk. See `docs/plans/member-actions-and-live-roles-implementation.md`, "Formatting". If that drift has been fixed on `main` by the time this runs, require a clean `cargo fmt --check` instead.

### Unit Tests
- **`src/friends.rs` model** (`tests/friends.rs`):
  - list membership for every `RelationshipStatus`
  - Online uses `presence_kind`
  - sorting and the ID tiebreak
  - `incoming_count`
  - `row_actions` per tab and for bots
  - `check_username` accepts and rejects the right shapes
- **Badges:** `home_summary`, `account_summary`, `attention_count` and `attention_breakdown` with incoming requests (Chunk 4).
- **Notices:** `friend_request_notice` transitions, Busy and Focus, and suppression while viewing Pending (Chunk 5).
- **Errors:** `InvalidProperty` and `NotFound` read well on the add-friend path.

### Integration Tests
- **`wiremock`** (`tests/api_users.rs`): `POST /users/friend` with the body checked, the returned `User`, and the NotFound, InvalidProperty and IsBot refusals.
- **Store pipeline:**
  1. Build from `Ready`.
  2. A `UserRelationship` (Incoming) arrives. Incoming lists them, `incoming_count` is 1, the Home badge goes up, and a notice is produced.
  3. The route answer after Accept (Friend) arrives. They're in All and in Online if online, the count is 0, and the badges clear.

### Manual Tests
1. **Lists:** the Friends tabs match the web app's for the same account, and presence changes follow live.
2. **Actions:** Accept, Reject, Cancel, Unblock and Message from rows, plus Remove friend and Block from the menu with confirmation. Both accounts update live.
3. **Add friend:** by `name#1234`, and each refusal's message.
4. **Badges:** an incoming request raises the Friends row, Pending tab, Home rail, account switcher and Dock badges. Handling it clears them all.
5. **Notices:** a toast while in front and a banner in the background (bundled). Clicks open Friends → Pending, Busy suppresses them, and the banner clears once the request is handled.
6. **Navigation:** Friends survives relaunch and reconnect. Switching between Friends and conversations hides and restores the composer and side panel, with drafts intact.

## Definition of Done
- [ ] **The Friends row:** Home's list starts with a Friends row (with an incoming-request badge) that opens the Friends page.
- [ ] **Remembered:** the open Friends page is restored at launch and kept on reconnect.
- [ ] **Tabs:** the page has Online, All, Pending (Incoming and Outgoing) and Blocked, built by `friends::friend_lists`, sorted by name, live on `UserRelationship` and `UserUpdate`, and unit-tested for every relationship.
- [ ] **Rows:** clicking opens the profile card, right-clicking opens the person menu, and the inline buttons are Accept and Reject, Cancel, Unblock, and Message, through `set_relationship` and `message_user`.
- [ ] **Add friend:** `username#1234` sends `POST /users/friend`. Successes and refusals (NotFound, InvalidProperty, IsBot, AlreadySentRequest, AlreadyFriends, Blocked, BlockedByOther, TooManyPendingFriendRequests) show readable text under the field.
- [ ] **Badges:** each incoming request adds one to the Home rail badge, the account switcher and the Dock, and the counts clear when it's handled (tested).
- [ ] **Notices:** a new incoming request (not one already pending at `Ready`) gives a toast or banner per `notify::delivery`, not while Busy or while Pending is in view. Clicking opens Friends → Pending, and the banner is dismissed once no requests remain or Friends is open.
- [ ] **The rest of the app is unchanged** while Friends is open: no acks, typing or sends, and the composer drafts are kept.
- [ ] **Checks:**
  - `cargo test` and `cargo clippy --all-targets -- -D warnings` pass.
  - `cargo fmt --check` passes, or, while `main` still has its old drift, no touched file has more `rustfmt --check` hunks than before and new files are clean.
