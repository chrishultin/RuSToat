# Implementation Record: Friends list (milestone 8)

_Plan: docs/plans/friends-list.md_
_Started: 2026-11-05_

Builds on milestone 7 (`81bb298`) on the same branch, `milestone-7-member-actions-y5xw`, from `6e89d8f` (the commit that added this plan). Run in continuous mode.

**Pre-existing dirty files**, kept out of every commit: `.gitignore` (tooling added a duplicate entry) and `.shadow/`.

**Formatting:** `cargo fmt --check` still fails at `6e89d8f` because of the old drift (see `docs/plans/member-actions-and-live-roles-implementation.md`, "Formatting"). The baseline `rustfmt` hunk counts at `6e89d8f` for files this milestone may touch: `src/store.rs` 15, `src/ui/chat.rs` 18, `src/api/client.rs` 3, `src/api/events.rs` 3, `src/api/mod.rs` 1. `src/notify.rs`, `src/api/error.rs`, `src/ui/app_root.rs`, `tests/store_live.rs`, `tests/notify_rules.rs`, `tests/store_unreads.rs` and `tests/api_users.rs` were clean. Each chunk keeps every touched file at or below its count, and new files are clean.

**Manual checks:** batched, as in milestone 7. They're walked through with the user at the end, together with milestone 7's friend and block checks, which were never done in the app.

## Chunk 1: The Friends page, read-only

### Changes Made
- `src/friends.rs` (new, GPUI-free), registered in `src/lib.rs`:
  - `FRIENDS = "friends"`, the reserved Home selection.
  - `FriendsTab { Online (default), All, Pending, Blocked }`, with `ALL` in tab-bar order and `label()`.
  - `FriendRow { user_id, name, avatar_url, presence, status, relationship }`. `name` is the display name, else the username. `status` is the custom status (trimmed), else the presence label.
  - `FriendLists { online, all, incoming, outgoing, blocked }` and `sections(tab)`, which gives (title, people) per list: Online; All; Incoming and Outgoing; Blocked. These are the web client's titles.
  - `friend_lists(store)` puts Friend → all, Incoming, Outgoing and Blocked in their lists. `None`, `BlockedOther`, `User` and the user themselves go nowhere. `online` is the friends whose `presence_kind` isn't Offline. Every list is sorted by lowercased name, then by ID.
  - `incoming_count(store)`.
- `src/store.rs`: `users()`, an iterator over every cached user.
- `src/ui/friends.rs` (new), registered in `src/ui/mod.rs`: `render_friends_page(lists, tab, incoming, colors, on_tab, people, cx)`.
  - A 48 px "Friends" header with the Users icon.
  - An underline `TabBar` of the four tabs. Pending's suffix is `mention_badge(incoming)`, which caps at "99+".
  - One `uniform_list` of 48 px rows for the tab: each list's "TITLE — count" row, then its people, or "Nobody here right now!".
  - Person rows have a 36 px avatar, with a presence dot for friends only, the name, and (for friends) the status line. Click → `People::open_card`; right-click → `People::menu(user, true)`.
- `src/ui/chat.rs`:
  - `friends_tab: FriendsTab` field (default Online).
  - `friends_open()`: Home is the place and `selected[Home] == FRIENDS`. `current_channel()` returns `None` then, with no fallback to the first conversation.
  - `drop_stale_selection()` keeps Home's `FRIENDS`.
  - `open_channel(FRIENDS)` opens Home on Friends.
  - `render_friends_row()`: a Users-icon "Friends" row at the top of Home's list (once `Ready` is in), selected while Friends is open, with a red `mention_badge` of `incoming_count`. Clicking it calls `select_channel(FRIENDS)`.
  - `render_main()` now returns `AnyElement`, and draws `render_friends()` (the page, with main-pane `MemberColors` and `self.people(None, cx)`) while Friends is open and `Ready` is in.
  - `UserUpdate`: a presence or status change no longer returns early when `friends_lists(id)` holds, meaning the page is open and the user has a relationship with them. It falls through to `cx.notify()`.
  - `People`, `People::open_card`, `People::menu`, `MemberColors` and its fields and `presence()` are now `pub(crate)` for `ui/friends.rs`.
  - The `crate::typing` import moved below `crate::store`, where rustfmt wants it (see Divergences).
- `tests/friends.rs` (new, 7 tests):
  - membership for every relationship, including `None`, `BlockedOther` and the user
  - an invisible friend isn't online; a custom status wins over the presence label
  - sorting is case-insensitive with an ID tiebreak, and a blank display name falls back to the username
  - `incoming_count`
  - the tabs' section titles and the default tab
  - `UserRelationship` events (through `apply_relationship`) move someone Incoming → Friend → Blocked → None
  - the `Ready` fixture: Ada (online, display name "Ada L.") and Bob (offline) are friends, and only Ada is online

### Divergences
All minor:
- **Status line for friends only.** Incoming, outgoing and blocked rows show just the name. Presence means little there, so the dot is friends-only too, as the plan says.
- **Offline rows aren't dimmed**, unlike the member list. The page has no Online/Offline split for dimming to mark, and Pending and Blocked rows would be dimmed for no reason.
- **Home's "Nothing here yet."** no longer shows, since the Friends row is always there. Servers still show it.
- **Visibility:** `People` and `MemberColors` became `pub(crate)` so the page can live in its own module.
- **Formatting:** my new import split an existing rustfmt import hunk in `chat.rs` in two. I moved the one out-of-order `use crate::typing` line instead of leaving it, which also clears that old hunk.

The look-ahead found nothing to change in the remaining chunks. Chunk 2 can add the row buttons through a `People` method, which keeps `ChatView`'s methods private.

### Verification Performed
- `cargo test --test friends`: 7 passed.
- `cargo test`: 558 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: the new files (`src/friends.rs`, `src/ui/friends.rs` and `tests/friends.rs`) are clean. `src/ui/chat.rs` went from 18 hunks to 16. `src/store.rs` stayed at 15, and `src/lib.rs` and `src/ui/mod.rs` are clean.
- I checked each `current_channel()` caller for the new `None` in Home:
  - `sync_composer` calls `set_channel(None)`, which saves the draft, stops typing and doesn't refocus.
  - These all return early: `sync_side_panel` (which resets search), `maybe_ack_current`, `ensure_current_loaded`, `ensure_members`, `refresh_member_list` and `member_panel_lists`.
  - `render` defers no loads or acks.
  - `sync_card` keeps a card opened on the page (its channel is `None` too) and closes one opened from a channel.
- `places.json` stores Home's selection as an opaque string, so `FRIENDS` round-trips with no format change. `remember_places` saves it like any channel.

### Manual Validation
Deferred to the batched checks at the end (see the header).

## Chunk 2: Inline row actions

### Changes Made
- `src/friends.rs`: `RowAction { Message, Relationship(RelationshipAction) }` and `row_actions(store, user)`. It builds on `profile`'s rules instead of restating them:
  - a Friend gets `Message` when `can_message` holds, which is always true for friends, bots included
  - Incoming and Outgoing get `friend_buttons` (Accept and Reject, or Cancel)
  - Blocked gets `Unblock`
  - everyone else (None, BlockedOther, the user, unknown IDs) gets nothing
- `src/ui/friends.rs`:
  - `RowButtons { actions, pending, opening }`. `render_friends_page` takes `buttons: impl Fn(&str) -> RowButtons`, and each person row carries its own.
  - Buttons at the row's right edge, each small:
    - Accept: primary, UserCheck icon, "Accept"
    - Reject: outline UserX icon, "Reject" tooltip
    - Cancel: outline, "Cancel"
    - Unblock: outline, "Unblock"
    - Message: ghost MessageCircle icon, "Message" tooltip, loading and disabled while its DM is being opened
  - Relationship buttons are disabled while `relationship_pending` holds the person. Every click calls `cx.stop_propagation()` first, so the row doesn't also open the card. Shift is passed through.
- `src/ui/chat.rs`:
  - `People::act(user, action, shift, window, cx)` calls `message_user` or `set_relationship`. Remove friend and Block keep their confirmation, but neither is a row button.
  - `render_friends` builds each row's `RowButtons` from `row_actions`, `relationship_pending` and `dm_opening`.
- `tests/friends.rs`, 2 new tests:
  - `row_actions` for Friend, Incoming, Outgoing and Blocked, and nothing for None, BlockedOther, the user or an unknown ID
  - a bot friend gets Message

### Divergences
Minor:
- **`row_actions` takes a user ID, not a `FriendRow`.** It reads the relationship from the store, as `friend_buttons` does.
- **`render_friends_page` has 8 parameters**, so it carries `#[allow(clippy::too_many_arguments)]`.

The look-ahead found nothing to change in the remaining chunks.

### Verification Performed
- `cargo test --test friends`: 9 passed.
- `cargo test`: 560 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: the new files are clean, and `src/ui/chat.rs` is still at 16 hunks (baseline 18).

### Manual Validation
Deferred to the batched checks at the end. This chunk's check needs a second account.

## Chunk 3: Add a friend by username

### Changes Made
- `src/api/client.rs`: `send_friend_request(username) -> User`, which sends `POST /users/friend` with `DataSendFriendRequest { username }` (from `revolt-models`).
- `src/friends.rs`:
  - `USERNAME_HINT` ("Enter a username and its four-digit tag, like ada#1234.").
  - `check_username(text) -> Result<&str, &'static str>`. It trims the text and splits it at the first `#`, as Stoat does. It needs a non-blank name and exactly four ASCII digits after the `#`, and otherwise returns the hint.
  - `AddFriendState { Sending, Sent(text), Info(text), Error(text) }`.
  - `request_sent(user)`: "Friend request sent to {username}#{tag}.", or "You're now friends with {username}#{tag}." when Stoat's answer is Friend (they'd already asked, and the request accepts theirs).
  - `request_refused(username, err)` gives:
    - NotFound → "There's no one called {typed}."
    - InvalidProperty → the hint
    - AlreadySentRequest → Info "You've already sent them a request."
    - AlreadyFriends → Info "You're already friends."
    - anything else → `err.to_string()`, which `describe` already words well for IsBot, Blocked, BlockedByOther, TooManyPendingFriendRequests and network errors
- `src/ui/friends.rs`:
  - `render_add_friend(input, state, on_send, cx)`: an "ADD FRIEND" label, a 320 px small `Input`, and a primary "Send request" button. Both are disabled while sending, and the button shows a spinner. The state's text goes under them: success in green, information muted, errors red.
  - `render_friends_page` now takes `add_friend: AnyElement`, which goes between the header and the tabs on every tab. The `incoming` parameter is gone; the badge count comes from `lists.incoming.len()`.
- `src/ui/chat.rs`:
  - New fields: `add_friend_input: Entity<InputState>` (placeholder `username#1234`), `add_friend_state: Option<AddFriendState>` and `_add_friend_input: Subscription`.
  - In the field, Enter sends. Any edit clears the state, unless a request is in flight. `set_value` doesn't fire `Change`, so clearing the field after a success keeps the message.
  - `send_friend_request(window, cx)`:
    - A request already in flight makes it do nothing.
    - It runs `check_username` and shows the hint without sending if that fails.
    - Otherwise it sets `Sending` and spawns `POST /users/friend`.
    - On success: `relationship_changed(user)` (they appear under Pending → Outgoing), the field clears, and the `Sent` text shows.
    - On a refusal: `request_refused`. On a task failure: "Couldn't reach Stoat. Try again."
- Tests:
  - `tests/api_users.rs`:
    - the path, the session header, the body (`body_json`) and the returned user
    - NotFound (404), InvalidProperty (400) and IsBot (400) decode, and IsBot reads "Bots can't be friends."
  - `tests/friends.rs`:
    - `check_username` accepts `ada#0001`, `  Ada Lovelace#1234 ` (trimmed) and `ada.l-_#9999`. It rejects empty and blank text, `ada`, `ada#`, `ada#12`, `ada#12345`, `#1234`, `  #1234`, `ada#12a4`, full-width digits and `ada#12#4`.
    - every refusal's text, and which ones are only information
    - `request_sent` for Outgoing and Friend

### Divergences
Minor:
- **`check_username` doesn't check the name's characters.** Stoat's `RE_USERNAME` is `^(\p{L}|[\d_.-])+$`, 2–32 characters, so usernames can't contain spaces. Following the plan's risk note, the client only checks the shape and lets Stoat judge the name. `Ada Lovelace#1234` passes here, and Stoat answers NotFound ("There's no one called Ada Lovelace#1234.").
- **`describe` in `src/api/error.rs` is unchanged.** `InvalidProperty` comes back from many routes, so a username-specific wording there would be wrong elsewhere. The add-friend path words it in `friends::request_refused` instead.
- **`AddFriendState` is in `src/friends.rs` (GPUI-free), not in the UI,** so its wording is unit-tested. `Sent` carries the whole sentence.
- **A request that accepts theirs** reads "You're now friends with …" instead of "Friend request sent to …".
- **The field has an "ADD FRIEND" label** above it.

The look-ahead found nothing to change in the remaining chunks.

### Verification Performed
- `cargo test --test api_users --test friends`: 12 + 12 passed.
- `cargo test`: 565 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `tests/api_users.rs` is clean again, as it was at the baseline. `src/api/client.rs` stayed at 3 hunks and `src/ui/chat.rs` at 16 (baseline 18). The new files are clean.

### Manual Validation
Deferred to the batched checks at the end.

## Chunk 4: Badges for incoming requests

### Changes Made
- `src/store.rs`:
  - `home_summary()` starts its mention count at `friends::incoming_count(self)`, so each waiting request is one mention with no unread dot. `account_summary()` builds on `home_summary()`, so it follows.
  - `attention_breakdown()` adds a "Friend requests" entry with the count when there are any. `attention_count()` sums the breakdown, so the Dock badge follows.
- `tests/store_unreads.rs`, 3 new tests, using the `Ready` fixture and `UserRelationship` events:
  - With everything read, `home_summary`, `account_summary` and `attention_count` each rise by one per incoming request, with no unread dot, and the breakdown entry shows. Accepting one and rejecting the other brings them back to zero and removes the entry.
  - Outgoing requests, friends and blocks add nothing.
  - Requests add to unread mentions: dm-ada's one mention plus one request gives 2.
- No UI change. The Friends row and the Pending tab already show `incoming_count` from chunks 1 and 2.

### Divergences
None. I checked that the counts update live:
- The `UserRelationship` handler ends in `cx.notify()`, and so does `set_relationship`'s answer.
- `AppRoot` observes each view and calls `sync_outside` on every notify (`app_root.rs:426`), which re-sums the Dock badge across accounts.
- The rail, the switcher, the Friends row and the Pending tab are all computed at render.

The look-ahead found nothing to change in chunk 5.

### Verification Performed
- `cargo test --test friends --test store_unreads`: 12 + 19 passed.
- `cargo test`: 568 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/store.rs` stayed at 15 hunks, and `tests/store_unreads.rs` is clean, as it was at the baseline.

### Manual Validation
Deferred to the batched checks at the end.

## Chunk 5: Notices for new requests

### Changes Made
- `src/store.rs`: `apply_relationship` returns `Option<RelationshipStatus>` instead of `bool`: how they related before, or `None` if they weren't known. The type is spelled out in full to keep out of the import block's old rustfmt hunk.
- `src/notify.rs`: `friend_request_notice(store, user, was, viewing_pending, window_active) -> Option<Notice>`.
  - It only notifies when the person (known, and not the user) is now Incoming and `was` wasn't.
  - Nothing while the window is active with Pending in view, or while the user is Busy. Focus still notifies.
  - The notice: `tag` and `channel` are `FRIENDS`, the title is "Friend request", and the body is "{display name} sent you a friend request." (truncated like message bodies).
- `src/ui/chat.rs`:
  - `relationship_changed` returns `apply_relationship`'s previous relationship.
  - The `UserRelationship` handler (route answers don't notify) works out the notice. `viewing_pending` is: this account shown, Friends open, and the Pending tab. It delivers through `notify::delivery`: `show_toast` or the new `show_friend_banner`.
  - `show_friend_banner` shows a system notification tagged `banner_tag(account, FRIENDS)`, with the account label like message banners, and sets the new `friend_banner: bool`. It isn't in `self.banners`, which is keyed by message.
  - `dismiss_friend_banner` takes the banner away if one is showing. It's called:
    - from `sync_outside` once `incoming_count == 0`
    - from the new `friends_seen`, after `open_channel`, `select_channel` and `select_place` land on Friends
    - from the window-activation observer when the window comes to the front with this account shown on Friends
  - `open_channel(FRIENDS)` also sets `friends_tab = Pending`. Toast clicks already call `open_channel(notice.channel)`, and banner clicks reach it through `AppRoot::open_notice`.
- `src/ui/app_root.rs`: unchanged. I checked that `open_notice` parses the tag, shows the account, and calls `open_channel("friends")`.
- Tests:
  - `tests/store_live.rs`: the relationship test now checks the returned previous relationship (`Some(before)`, `Some(Incoming)` the second time, and `None` for a stranger).
  - `tests/notify_rules.rs`, 6 new tests:
    - a new request notifies with the right title, body, tag and channel, whatever it was before
    - a request already Incoming doesn't notify again
    - Busy silences it and Focus doesn't
    - Pending in view only silences it while the window is active
    - Outgoing, Friend, Blocked, BlockedOther, None and Incoming → Friend don't notify, and neither do unknown users or the user
    - the store pipeline: an Incoming event raises `incoming_count`, the Incoming list, the Home badge and the Dock count and gives a notice, and the Friend answer after Accept puts them in All and Online and clears every count

### Divergences
Minor:
- **The banner isn't dismissed just because Friends is open in `sync_outside`.** A banner only shows while the window is in the background, and `sync_outside` runs straight after, so a Friends page that's already open would have taken the banner away at once. It's dismissed when Friends is chosen or opened from a notice, or when the window comes back to the front on it (the activation observer), and once no requests are left. That matches "once the Friends page is open" for a page the user actually sees.
- **`friend_request_notice` treats an unknown previous relationship as `None`.** The caller passes `unwrap_or_default()`, so a request from someone the client didn't know notifies.

### Verification Performed
- `cargo test --test notify_rules --test store_live`: 24 + 33 passed.
- `cargo test`: 574 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/notify.rs` and `tests/notify_rules.rs` are clean, as at the baseline. `src/store.rs` stayed at 15 hunks and `src/ui/chat.rs` at 16 (baseline 18). `tests/store_live.rs` is clean.

### Manual Validation
Deferred to the batched checks at the end. This chunk's check needs a second account, and the bundled app (`scripts/run-bundled.sh`) for banners.

## Summary

_Completed: 2026-11-05_

### Chunks Completed
1. The Friends page, read-only: a Friends row in Home and a page with Online, All, Pending and Blocked tabs. Rows open the card and the menu, and the page is kept across reconnects and launches (`7d573af`).
2. Inline row buttons: Accept, Reject, Cancel, Unblock and Message (`12e0567`).
3. Adding a friend by `username#1234`, with readable refusals (`1b0a44f`).
4. Incoming requests count on the Home, switcher and Dock badges (`40a412e`).
5. A toast or banner for a new request, opening Friends → Pending, with banner dismissal (this commit).

### Divergences
All minor; none needed the user to decide:
- Chunk 1:
  - The status line and presence dot are for friends only.
  - Offline rows aren't dimmed.
  - Home no longer says "Nothing here yet."
  - `People` and `MemberColors` became `pub(crate)`.
  - An out-of-order import in `chat.rs` was moved, clearing one old rustfmt hunk.
- Chunk 2: `row_actions` takes a user ID, and `render_friends_page` allows `too_many_arguments`.
- Chunk 3:
  - `check_username` checks only the shape (Stoat judges the name).
  - `InvalidProperty` is worded on the add-friend path, not in the generic `describe`.
  - `AddFriendState` is GPUI-free in `src/friends.rs`.
  - A request that accepts theirs reads "You're now friends with …".
  - The field has an "ADD FRIEND" label.
- Chunk 5: the banner is dismissed when Friends is seen (chosen, opened from a notice, or back in front), not whenever it's open. An unknown previous relationship counts as None.

### Definition of Done
- [x] **The Friends row:** Home's list starts with Friends (with an incoming-request badge), which opens the page.
- [x] **Remembered:** `FRIENDS` survives `drop_stale_selection` (on reconnect and permission changes) and is saved in `places.json`. Still to be checked by hand.
- [x] **Tabs:** Online, All, Pending (Incoming, Outgoing) and Blocked, from `friends::friend_lists`, sorted by name. They're live on `UserRelationship` and `UserUpdate`, and unit-tested for every relationship.
- [x] **Rows:** click opens the card and right-click the menu. The buttons are Accept and Reject, Cancel, Unblock and Message, through `set_relationship` and `message_user`.
- [x] **Add friend:** `username#1234` sends `POST /users/friend`. Every listed refusal shows readable text under the field (tested).
- [x] **Badges:** each incoming request adds one to the Home rail, the switcher and the Dock, and the counts clear when it's handled (tested).
- [x] **Notices:** a new incoming request gives a toast or banner per `notify::delivery`, but not while Busy or with Pending in view. Clicking opens Friends → Pending, and the banner is dismissed once no requests remain or Friends is seen (see Divergences). Banners still need checking by hand, bundled.
- [x] **The rest of the app is unchanged while Friends is open:**
  - `current_channel()` is `None`, so there are no acks, typing, history loads or member fetches.
  - The composer saves its draft through `set_channel(None)` and isn't drawn, so nothing can be sent.
- [~] **Checks:**
  - `cargo test` (574 passed) and `cargo clippy --all-targets -- -D warnings` pass.
  - **Not met:** `cargo fmt --check` still fails, but only because of drift that predates this milestone. No touched file has more hunks than at `6e89d8f` (`chat.rs` went from 18 to 16), and every new file is clean.

### Remaining Manual Steps
Every chunk's manual check and the plan's Manual Tests 1–6, plus milestone 7's batched checks, which were never done in the app (especially its friend and block flows). Chunks 2, 3 and 5 need a second account, and chunk 5's banners need the bundled app (`scripts/run-bundled.sh`).
