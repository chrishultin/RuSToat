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
