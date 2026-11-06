# Implementation Record: Reading flow (milestone 9)

_Plan: [reading-flow.md](reading-flow.md)_
_Started: 2026-11-06_

Branch: `feature/reading-flow`, from `main` (a6f284d). Running in continuous mode.

Before chunk 1, the uncommitted window-reopen work in `src/main.rs` and `README.md` (from before this plan) was committed on its own as 9520c37, so chunks 5 and 6 can edit `main.rs` cleanly. Kept out of every commit: `.gitignore` (a duplicated Riptide entry added by tooling) and the untracked `.shadow/`.

Formatting: `src/ui/chat.rs`, `src/store.rs` and `src/api/client.rs` already differ from rustfmt's output (17, 15 and 3 hunks at the start). Running `rustfmt` on them would reformat unrelated code, so only the lines each chunk adds are formatted, and checked by confirming those counts don't grow. New files are formatted whole.

## Chunk 1: Copy link and IDs

### Changes Made
- `src/api/client.rs`: `RootConfig.app: Option<String>` (`#[serde(default)]`), the instance's web client.
- `src/links.rs` (new, in `lib.rs`): `message_link(app, server, channel, message)` builds `{app}/server/{server}/channel/{channel}/{message}`, or `{app}/channel/{channel}/{message}` for DMs and groups, dropping a trailing `/` from `app`.
- `src/store.rs`: `ChannelSection.id: Option<String>`, the category's ID (`None` for uncategorized).
- `src/ui/chat.rs`:
  - `ChatView.web_app`, set from `root.app` when `GET /` loads (logged with the media URLs).
  - `RowAction::CopyLink(id)` / `CopyId(id)`. The message menu has "Copy message link" (only when `Pickers.can_link`, i.e. `web_app` is known) and "Copy message ID", after "Copy text".
  - `sections()` returns `Vec<ChannelSection>` instead of tuples, so the channel list has category IDs.
  - Category headers and channel rows have a right-click menu ("Copy category ID", "Copy channel ID") via a shared `copy_id_item`. `render_channel_row` now returns `AnyElement`.
  - `rail_button`'s `menu` is now `Vec<(label, action)>`. Server icons have "Mark as read" and "Copy server ID".
- `src/ui/switcher.rs`: passes the account menu as a one-item list.
- `src/assets.rs`: embeds the Lucide `Link` icon; the icon test covers it.
- `tests/links.rs` (new): server link, DM link, trailing slash, and `app` present (fixture) and missing (older instance).
- `tests/store_ready.rs`: the section shape test also checks category IDs.

### Divergences
- **Minor:** "Copy channel ID" is on Home's DM and group rows too, since they're channel rows.
- **Minor:** the plan's risk about an `/app` prefix was checked before building. stoat.chat's web client serves its files from `/app/`, but its own "Copy link" builds `new URL("/server/…/channel/…/{message}", location.origin)`, and `https://stoat.chat/server/…` serves the client. So no prefix.

### Verification Performed
- `cargo test --test links`: 4 passed.
- `cargo test`: 581 passed, 0 failed, 5 ignored.
- `cargo clippy --all-targets`: no warnings.
- The app's debug log shows `web app: Some("https://stoat.chat")` after `GET /` loads.

### Manual Validation
The user ran the app, copied message links (server channel and DM) and opened them in the browser, and copied message, channel, category and server IDs. They confirmed all of it looks right.

## Chunk 2: The "New messages" divider

### Changes Made
- `src/store.rs`:
  - `TimelineRow::NewMessages`, keyed `"new-messages"` in `row_key` so `diff_rows` inserts and removes it like any row.
  - `ChatStore::read_position(channel)`: the read state's `last_id`.
  - `with_unread_divider(rows, after)`: inserts the divider before the first sent (not pending) message newer than `after` that isn't `mine`. The message after it loses `continuation`, and `mark_group_ends` runs again so the group before it ends.
- `src/ui/chat.rs`:
  - `ChatView.unread_marker: Option<(channel, Option<after>)>`.
  - `sync_unread_marker`, called first thing in `render`, before the deferred `maybe_ack_current`. When the open channel changes, it records the new channel's read position (only if `is_unread`), and re-syncs both panes so the divider appears in one and goes from the other. It waits for Ready and only runs for the account on screen.
  - `ChatView::timeline(channel)` applies the divider; `sync_pane` and `reset_pane` use it instead of `store.timeline`.
  - `clear_unread_divider`, called from `ack_sent`, drops the divider but keeps the channel recorded, so it isn't re-recorded on the next render.
  - `set_active(false)` forgets the marker, so coming back to the account counts as opening the channel again.
  - `render_row` draws the divider as a 1 px danger-coloured line with a bold "New" on the right.
- `tests/store_unreads.rs`: `read_position`; divider placement (and the group ending at it), everything read, only the user's own unread messages, the user's own message before others', a read position older than the loaded history (and the whole-history exception), and one newer than it.
- `tests/store_{attachments,embeds,history,pending,reactions,replies}.rs`: `match` arms on `TimelineRow::StartOfChannel { .. } => None` became `_ => None`, for the new variant.

### Divergences
- **Minor:** `unread_marker` is one `Option<(channel, Option<after>)>`, not the plan's `HashMap<String, String>`. Only the open channel has a divider, and keeping the channel after `ack_sent` clears the position is what stops the marker being recorded again on the next render.
- **Minor:** `with_unread_divider` takes no `me`, because `MessageRow.mine` already says whose a message is.
- **Minor:** when the read position is older than every loaded message but the whole history is loaded (a start-of-channel row), the divider goes above the first message. In that case it's accurate, not misleading.
- **Minor:** the divider is only recorded if the channel is unread when opened. Otherwise messages arriving while you're in a read channel would get a divider.
- **Plan amended (bookkeeping):** chunk 3's description now names `unread_marker` and `sync_unread_marker` as the things it hooks into.

### Verification Performed
- `cargo test --test store_unreads`: 25 passed (7 new).
- `cargo test`: 587 passed, 0 failed, 5 ignored.
- `cargo clippy --all-targets`: no warnings.
- rustfmt drift in `chat.rs` and `store.rs` unchanged (17 and 15). `tests/store_unreads.rs` formatted whole. `tests/store_pending.rs` and `tests/store_replies.rs` have older drift and weren't reformatted.

### Manual Validation
The user had a second account post in a channel, opened it, and confirmed the divider sits above the first new message, stays while reading, and is gone after switching away and back.

## Chunk 3: Mark as unread

### Changes Made
- `src/store.rs`: `ChatStore::set_read_position(channel, Option<&str>)` sets the read position outright, backwards too. Mentions older than it aren't restored (assumption 2). `ChatStore::message_before(channel, id)` returns the previous loaded message, or `None` for the oldest loaded message or an unknown ID.
- `src/ui/chat.rs`:
  - `RowAction::MarkUnread(id)`, and "Mark as unread" (Lucide `Mail` icon) under "Reply" in the message menu.
  - `ChatView.held_unread: Option<String>`. `maybe_ack_current` skips that channel. It's cleared when the open channel changes (in `sync_unread_marker`), when the account is hidden (`set_active(false)`), and by `ack_sent`.
  - `mark_unread` works out the message before the one clicked. If there isn't one in memory, it uses the message itself, as revite does, with a comment saying the divider then falls one message late. It cancels a pending ack for the channel, holds the channel unread, sets the read position, points `unread_marker` at the same message (when it's the open channel), re-syncs the pane, and sends the ack.
  - `post_ack(channel, message, why)` is the ack request and its logging, now shared by `ack_sent` and `mark_unread`.
- `src/assets.rs`: embeds `Mail`; the icon test covers it.
- `tests/store_unreads.rs`: moving the read position back makes the channel unread, `apply_ack` still can't move it back, and `None` means nothing read. Also `message_before` for a middle message, the oldest loaded one, an unloaded ID and a channel with no history.

### Divergences
- **Minor:** the ack request moved into a shared `post_ack` helper instead of being repeated.
- **Minor:** hiding the account (switching accounts) also releases the hold, consistent with chunk 2 treating coming back as opening the channel again.
- The plan's risk that Stoat might ignore a backwards ack didn't happen: the request succeeded (`acked … (marked unread)` in the log), and the web client showed the channel unread. No `tests/api_unreads.rs` case was added, because the request shape is the existing `ack_message`, which is already covered.

### Verification Performed
- `cargo test --test store_unreads`: 27 passed (2 new).
- `cargo test`: 589 passed, 0 failed, 5 ignored.
- `cargo clippy --all-targets`: no warnings.
- rustfmt drift in `chat.rs` and `store.rs` unchanged (17 and 15); `assets.rs` and `tests/store_unreads.rs` clean.

### Manual Validation
The user marked a message unread and confirmed the following. The unread dot and the moved divider appear. The channel stays unread while open, and the web client shows it unread. It's read again after leaving and coming back. The app log shows Stoat accepted the backwards ack.

## Chunk 4: Collapsible categories

### Changes Made
- `src/places.rs`: `Places.collapsed: HashSet<String>`. On disk it's `collapsed`, a sorted list (`BTreeSet`) so the file doesn't churn, with `#[serde(default)]`. The sample in the unit tests includes it. New tests check that a file without `collapsed` loads with nothing folded, and that folded IDs are saved sorted. The existing round-trip and per-account tests cover the rest.
- `src/store.rs`:
  - `fold_sections(sections, collapsed, open)` cuts a folded category down to the open channel and channels with `mentions > 0`. Its heading stays even when empty.
  - `ChatStore::has_category(id)`.
- `src/ui/chat.rs`:
  - `ChatView.collapsed`, loaded from and saved with places (`remember_places`).
  - `drop_stale_selection`, run on Ready, drops IDs that are no longer categories.
  - `visible_sections()`, the folded list, is meant for chunk 5's walkers too.
  - `toggle_category` toggles and saves.
  - Headings are clickable rows with `ChevronDown`/`ChevronRight` and a hover colour, and keep the "Copy category ID" menu. The "Nothing here yet" check uses the unfolded sections.
- `src/assets.rs`: the icon test covers `ChevronDown` and `ChevronRight`. Both are default component icons, so no embedding was needed.
- `tests/store_ready.rs`: tests for `fold_sections` (nothing folded; open channel and mention kept; a folded category emptied keeps its heading; unknown IDs are ignored) and `has_category`.

### Divergences
- **Minor:** the folding filter is a pure `fold_sections` in `store.rs`, next to `ChannelSection`, so it can be tested and reused by chunk 5.
- **Minor:** the plan said to look for existing places tests in `tests/`. They're unit tests inside `src/places.rs`, so the new ones went there.

### Verification Performed
- `cargo test --lib`: 104 passed, 1 ignored (2 new places tests).
- `cargo test --test store_ready`: 10 passed (2 new).
- `cargo test`: 593 passed, 0 failed, 5 ignored.
- `cargo clippy --all-targets`: no warnings.
- `src/places.rs` and `tests/store_ready.rs` were rustfmt-clean before, and were formatted whole. Drift in `chat.rs` and `store.rs` is unchanged (17 and 15).

### Manual Validation
The user folded a category (its channels hid except the open one), had a mention reappear in a folded category, and relaunched with the category still folded. All confirmed.

## Chunk 5: Keyboard shortcuts in a channel

### Changes Made
- `src/nav.rs` (new, in `lib.rs`): `neighbour(items, current, step, eligible)` finds the eligible item `step` places away, wrapping round and never returning `current`. With no current (or one not listed) it starts from the matching end. Plain data, no GPUI.
- `src/ui/chat.rs`:
  - `actions!(chat, [PrevChannel, NextChannel, PrevUnread, NextUnread, PrevServer, NextServer, JumpToPresent, MarkServerRead])`.
  - `chat::init(cx)` binds ⌥↑/↓, ⌥⇧↑/↓, ⌘⌥↑/↓, Esc and ⇧Esc in the `ChatView` key context. It binds ⌥⇧↑/↓ and ⌘⌥↑/↓ again in `ChatView > Input`, so that inside the chat view they beat the text input's add-cursor-above/below bindings for the same keys. That works because GPUI prefers the later binding at equal depth, and `chat::init` runs after `gpui_kit::init`.
  - The render root has `key_context("ChatView")` and the action handlers. It also handles the input's `Escape` action when it bubbles up unhandled.
  - `move_channel(step, unread_only)` walks `visible_sections()` (folded channels skipped), with Home's Friends row first. `move_server(step)` walks Home then `store.servers()`. Both select through the existing `select_channel` and `select_place`.
  - `go_to_present`: at the present it scrolls the pane to the end; otherwise it calls the existing `jump_to_present`. Then `mark_read_now` acks the latest message straight away (cancelling any pending ack and releasing a "Mark as unread" hold).
  - ⇧Esc calls the existing `mark_server_read` for the open server; on Home it does nothing.
  - `shortcuts_blocked` makes every handler `cx.propagate()` while the image viewer or emoji picker is open, so their own Esc and arrow keys still work. GPUI dispatches actions before key-down listeners, and the viewer closes on a key-down.
- `src/ui/composer.rs`: `emoji_picker_open()`. Esc already fell through (`on_escape` calls `cx.propagate()` when there's no edit or reply to cancel, and completion has its own capture), so nothing else changed.
- `src/main.rs`: calls `ui::chat::init(cx)` right after `gpui_kit::init(cx)`.
- `tests/nav.rs` (new): next and previous wrap, no current item, unread-only filtering (the current item never counts), Home-then-servers order, and single or empty lists.

### Divergences
- **Minor (Open Question resolved):** the input binds nothing to ⌥↑/⌥↓, so those keys don't move the cursor in the message box. The channel moves therefore work everywhere in the chat view, including the message box. The plan's fallback ("only when the box is empty or unfocused") wasn't needed.
- **Minor:** the text input binds ⌥⇧↑/↓ and ⌘⌥↑/↓ to multi-cursor editing, which the plan didn't anticipate. Those keys were also bound in the `ChatView > Input` context, so the shortcuts win inside the chat view. The search and dialog inputs elsewhere keep the input's own behaviour only outside the chat view.
- **Wraps or stops:** every walker **wraps round**, as the manual check asked us to decide.
- **Minor:** Home's Friends row counts as a channel for ⌥↑/↓, but it's never "unread" for ⌥⇧↑/↓.
- **Minor:** the walker is `crate::nav::neighbour`, in a non-UI module so `tests/` can reach it.

### Verification Performed
- `cargo test --test nav`: 5 passed.
- `cargo test`: 598 passed, 0 failed, 5 ignored.
- `cargo clippy --all-targets`: no warnings.
- `src/nav.rs`, `tests/nav.rs` and `src/main.rs` are rustfmt-clean. Drift in `chat.rs` and `composer.rs` is unchanged (17 and 14).

### Manual Validation
The user went through ⌥↑/↓, ⌥⇧↑/↓, ⌘⌥↑/↓, Esc (scrolled up, and in the message box while editing, replying and autocompleting) and ⇧Esc, and confirmed they all work.

## Chunk 6: Switch accounts with ⌘1–⌘9

### Changes Made
- `src/ui/app_root.rs`:
  - `show_account_at(index)`: shows the nth account in the switcher; past the end it does nothing.
  - `account_names()`: every account's name in the switcher's order.
  - `account_name(entry)`, factored out of `render_switcher`, gives the name and avatar the switcher shows.
- `src/main.rs`:
  - Nine unit actions, `SwitchAccount1`–`SwitchAccount9`, bound to ⌘1–⌘9 with no context, and handled at the app level like Log Out and Add Account.
  - `set_menus(accounts)` builds the menu bar with a new **Accounts** menu listing each account by name. macOS shows each item's ⌘-digit shortcut from the keymap. The menu is greyed out when there are none.
  - `MainWindow` observes the `AppRoot` and rebuilds the menus only when the names change (`menu_accounts`, `sync_menus`). It clears them when the window closes.
  - `MainWindow::update_root` updates the `AppRoot` entity without its window. The switch handlers use it.

### Divergences
- **Minor:** nine unit actions instead of `SwitchAccount(usize)`, as the plan allowed. The `Action` derive for an action with a field emits `gpui::` paths, which don't resolve through the gpui_kit facade.
- **Minor:** the actions live in `main.rs` with the other app-level actions, not in `app_root.rs`. `AppRoot` only gained `show_account_at`.
- **Minor:** the Accounts menu lists the accounts by name, kept current by observing `AppRoot`.
- **Bug found in manual testing, fixed:** ⌘1/⌘2 did nothing at first. A key binding's action is dispatched while GPUI is updating the window, and `MainWindow::update` → `window.update` refuses a window already on the update stack (silently, through `.ok()`). The menu items didn't have this problem, because menu actions arrive from outside the window. The fix was `MainWindow::update_root`, which updates the `AppRoot` entity directly; switching accounts doesn't need the `Window`.

### Verification Performed
- `cargo build`; `cargo test`: 598 passed, 0 failed, 5 ignored. `cargo clippy --all-targets`: no warnings.
- `src/main.rs` and `src/ui/app_root.rs` are rustfmt-clean.

### Manual Validation
Run from the signed bundle (`scripts/run-bundled.sh`). After the fix, the user confirmed: ⌘1 and ⌘2 switch between their two accounts, ⌘3 does nothing, and the Accounts menu lists them.

Also from this chunk: earlier manual checks launched `target/debug/stoat-gpui` directly, which skips the signing in `scripts/cargo-runner.sh`, so the Keychain asked after every rebuild. Running through `scripts/run-bundled.sh` (or `cargo run`) signs with the Apple Development identity under a fixed identifier, so "Always Allow" sticks.

## Summary

_Completed: 2026-11-06_

### Chunks Completed
1. Copy link and IDs: message link and ID; channel, category and server IDs (0f01797).
2. "New messages" divider where you left off (cc50b22).
3. Mark as unread, held until you leave the channel; Stoat accepts the backwards ack (113d1b7).
4. Collapsible categories, remembered per account (d462fb1).
5. Keyboard shortcuts in a channel: ⌥↑/↓, ⌥⇧↑/↓, ⌘⌥↑/↓, Esc, ⇧Esc (af6c532).
6. ⌘1–⌘9 to switch accounts, plus an Accounts menu (this commit).

Before chunk 1, pre-existing uncommitted work (window reopening from the Dock) was committed separately as 9520c37.

### Divergences
- Chunk 1: "Copy channel ID" is on DM and group rows too. The link format was checked: no `/app` prefix.
- Chunk 2: the marker is one `Option<(channel, Option<after>)>`, not a map. The divider uses `MessageRow.mine`, not a `me` argument. It shows above the first message when the whole history is loaded. It's only recorded if the channel is unread when opened.
- Chunk 3: a shared `post_ack` helper; hiding the account also releases the hold.
- Chunk 4: the folding filter is a pure `fold_sections` in `store.rs`. The places tests are unit tests in `src/places.rs`.
- Chunk 5: ⌥↑/↓ work in the message box too, because the input doesn't bind them, so the Open Question's fallback wasn't needed. ⌥⇧↑/↓ and ⌘⌥↑/↓ override the input's multi-cursor bindings inside the chat view. The walkers wrap round. Friends counts as Home's first row. The walker lives in `src/nav.rs`.
- Chunk 6: nine unit actions in `main.rs`, and an Accounts menu by name. A dispatch bug, where a key action couldn't re-enter the window, was found in manual testing and fixed.
- Formatting: `chat.rs`, `store.rs`, `client.rs`, `composer.rs` and a few test files already differ from rustfmt's output. Only lines this work added were formatted, and the drift counts are unchanged.
- The plan was amended once (chunk 3's description, after chunk 2).

### Definition of Done
- [x] The message menu has "Copy message link" (only when the instance sends `app`) and "Copy message ID". Channel rows, category headers and server icons have "Copy … ID".
- [x] A copied message link opens that message in Stoat's web client (user-checked).
- [x] Opening a channel with unread messages from others shows the divider before the first of them. Leaving the channel or sending a message removes it.
- [x] "Mark as unread" makes the channel unread here and in the web client, and RuSToat doesn't mark it read again until you leave it.
- [x] Category headers fold and unfold. The state survives a relaunch, and channels with mentions still show.
- [x] ⌥↑/↓, ⌥⇧↑/↓, ⌘⌥↑/↓, Esc, ⇧Esc and ⌘1–9 work as in assumption 4. Esc still cancels edits, replies and autocomplete.
- [x] `cargo test` (598 passed, 5 ignored) and `cargo clippy --all-targets` pass, with no new warnings.
- [x] Every chunk's manual check is recorded in this file.

### Remaining Manual Steps
None required. Every plan-level manual test (1–5) was covered by the per-chunk checks. Optional:
- Unsent-message case in chunk 2: confirm sending a reply clears the divider.
- Remaining Open Question: should the shortcuts be listed somewhere, such as a Help menu or milestone 13's settings window?
