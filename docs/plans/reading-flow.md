# Plan: Reading flow (RuSToat milestone 9)

## Summary
This is the first milestone of the [roadmap](roadmap.md). It makes reading busy servers easier day to day:
- **A "New messages" divider** where you left off in a channel.
- **Mark as unread** on a message, to come back to it later.
- **Copy message link and IDs:** message link and ID; channel, server and category IDs.
- **Collapsible categories** in a server's channel list, remembered per account.
- **Keyboard shortcuts:** move between channels and servers, jump to the newest message, mark a channel or server read, and switch accounts.

## Approach
Everything here builds on the code that exists today. There's no new subsystem.
- **The divider is a new row in the timeline.** `TimelineRow` (`src/store.rs`) gets a `NewMessages` variant. A pure function inserts it after the last read message. The input is a "where you left off" marker that `ChatView` records when a channel opens, *before* the automatic mark-as-read in `maybe_ack_current` moves the read position.
- **Recording the marker first is the only way it works.** Reading the read position at render time would always find the channel already read.
- **Mark as unread** acknowledges the message *before* the chosen one (`PUT /channels/{id}/ack/{message}`). The web clients do the same: for-web's `MessageContextMenu.tsx` and revite's `ContextMenus.tsx`. It then stops RuSToat re-marking the channel read until you leave it.
- **That needs one new store method.** `ChatStore::apply_ack` deliberately never moves the read position back, so a separate method moves it back on purpose. `apply_ack` keeps its guarantee.
- **Message links use the instance's own web address.** That's the root config's `app` field (`https://stoat.chat` on the official instance), followed by for-web's route: `/server/{server}/channel/{channel}/{message}`, or `/channel/{channel}/{message}` for DMs and groups.
- **The `app` field is better than a hardcoded host.** Self-hosted instances then link to their own web client.
- **Collapsed categories go in the per-account places file** (`src/places.rs`), next to the remembered channel. Every account has one already.
- **Shortcuts are GPUI actions.** Each is bound in a `ChatView` key context, or in `AppRoot`'s for switching accounts. That way they show up as actions, and stay out of the way while the emoji picker or a dialog has focus.

## Assumptions
1. **The divider sits before the first unread message that isn't yours,** and only if there is one. It stays put while you're in the channel, as in the web client. It disappears when you leave the channel, or when you send a message there (Discord does the same; it saves a divider sitting above your own reply).
2. **Mark as unread moves the read position back but doesn't restore mention badges** for older mentions. The store only keeps mention IDs newer than the read position. The web client doesn't restore them either.
3. **Collapsing a category hides its channels, except the open one and any with unread mentions.** That's so you can't lose a ping. The web client hides everything except the open channel; ours is a little kinder.
4. **Shortcuts follow Discord on macOS where the web client clashes with macOS.** The web client's server shortcut, Ctrl+↑/↓, is Mission Control on a Mac.
   - **⌥↑ / ⌥↓:** previous or next channel in the list (as in the web client)
   - **⌥⇧↑ / ⌥⇧↓:** previous or next *unread* channel
   - **⌘⌥↑ / ⌘⌥↓:** previous or next server (Home counts as the first)
   - **Esc:** jump to the newest message and mark the channel read (the web client's "jump to end"). In the message box, Esc keeps its current meaning first: cancel an edit, a reply or autocomplete.
   - **⇧Esc:** mark the whole server read
   - **⌘1–⌘9:** switch to the nth account in the switcher
5. **Copy IDs appear in every menu:** channel ID and category ID on channel rows and category headers, and server ID on the server icon. The web client hides these behind a developer setting; we have no settings yet, and they're harmless.

## Open Questions
- *(Can wait)* Should the shortcuts be listed somewhere, such as a "Keyboard Shortcuts" item in a Help menu? Milestone 13's settings window could list them instead.
- *(Can wait)* `⌥↑` inside the message box moves the cursor to the start of the paragraph on macOS. Chunk 5 binds the shortcut only outside the message box unless the box is empty, as Discord does. Revisit this if it feels wrong.

## Plan

### Chunk 1: Copy link and IDs
**Goal:** Copy a message's link or ID, and a channel's, category's or server's ID, from their right-click menus.
**Files likely touched:**
- `src/api/client.rs`: `RootConfig` gets `app: Option<String>`
- `src/media.rs`, or a small new `src/links.rs`: build a message link from `app`, server, channel and message
- `src/ui/chat.rs`: add "Copy message link" and "Copy message ID" to the message menu (`render_message_row`); a new right-click menu on channel rows (`render_channel_row`) and category headers (`render_channel_list`); "Copy server ID" next to "Mark as read" on the server icon (`render_rail`)
- `src/store.rs`: `ChannelSection` gets the category's ID
**Depends on:** None
**Risks:**
- **The link might need `/app` in front of the path.** Check by opening a copied link in a browser. Older instances may not send `app`; in that case leave out "Copy message link" rather than guess.
- **The server icon's menu takes one item today** (`menu: Option<(label, action)>` in `render_rail`), so it needs to take a list.
**How to verify:** `cargo test`, with new unit tests for the link builder: a server channel, a DM, no `app`, and `app` with a trailing slash. Then `cargo clippy --all-targets`.
**Manual check:** Copy a message link, paste it into a browser, and confirm it opens that message in Stoat's web client. Copy each kind of ID, and paste it into a text editor to check.
**Shippable when:** every menu item copies the right text, and links open in the web client.

### Chunk 2: The "New messages" divider
**Goal:** Opening a channel with unread messages shows a "New messages" line above the first one.
**Files likely touched:**
- `src/store.rs`: `TimelineRow::NewMessages`; a pure `with_unread_divider(rows, after: &str, me: &str) -> Vec<TimelineRow>`; `ChatStore::read_position(channel)`; `diff_rows` keys the new row
- `src/ui/chat.rs`: `ChatView.unread_markers: HashMap<String, String>`, recorded when a channel opens (before `maybe_ack_current` can run) and cleared when you leave it or send a message (`ack_sent`); pane rows go through `with_unread_divider`; `render_row` draws the divider (a thin line in the theme's danger colour, with "New" on the right)
- `tests/store_unreads.rs`
**Depends on:** None
**Risks:**
- **The marker has to be recorded before the first ack.** If the ack wins, there's no divider.
- **A channel opened before its history loads** gets its marker straight away. The divider shows once the history arrives.
- **The read position can be older than all the loaded history.** In that case, no divider: it would sit above the oldest message loaded, which is misleading. Jumping there is milestone 12's business, or later.
**How to verify:** `cargo test --test store_unreads` with new cases:
- the divider sits before the first unread message
- no divider when everything is read
- no divider when the only unread messages are yours
- no divider when the read position is older than every loaded message
- no divider when the read position is newer than every loaded message

Then `cargo test`.
**Manual check:** Have a second account post three messages in a channel. Open the channel: the divider is above the first of the three. Switch away and back: the divider is gone.
**Shippable when:** the divider shows in the cases above, stays put while you read, and goes when you leave the channel.

### Chunk 3: Mark as unread
**Goal:** "Mark as unread" on a message makes the channel unread from that message, here and in other clients.
**Files likely touched:**
- `src/store.rs`: `ChatStore::set_read_position(channel, Option<&str>)`, which can move backwards, and `ChatStore::message_before(channel, id)`
- `src/ui/chat.rs`: a new `RowAction::MarkUnread(id)` and its menu item; `ChatView.held_unread: Option<String>`, which makes `maybe_ack_current` skip that channel, and is cleared on leaving the channel and by `ack_sent`. It cancels any pending ack, sets the read position and the divider marker (from chunk 2) to the previous message, and sends the ack.
- `tests/store_unreads.rs`; `tests/api_unreads.rs` if the ack request needs a new case
**Depends on:** Chunk 2. Mark as unread also places the divider.
**Risks:**
- **The oldest loaded message has no "previous" in memory.** In that case, acknowledge the message itself, as revite does (the divider then falls one message late), and leave a comment saying so.
- **Stoat might ignore an ack that moves backwards.** Confirm with the manual check below before building more on this. If it does ignore it, the fallback is local-only "unread", which other clients won't see. Note that in the implementation record.
- **The server echoes `ChannelAck` with the older ID.** That's harmless, since `apply_ack` doesn't move back.
**How to verify:** `cargo test --test store_unreads`: setting the read position backwards makes the channel unread; `message_before` handles the first loaded message and a missing ID.
**Manual check:**
- Mark a message unread. The channel's unread dot appears and the divider moves to that message.
- Stay in the channel: it stays unread. Leave and come back: it's read again.
- Open Stoat's web client before coming back: it shows the channel unread too.
**Shippable when:** the channel stays unread until you leave it, and the web client agrees.

### Chunk 4: Collapsible categories
**Goal:** Click a category header to fold its channels away. Each account remembers which categories are folded.
**Files likely touched:**
- `src/places.rs`: `Places.collapsed: HashSet<String>` (category IDs), saved with `#[serde(default)]` so existing files still load
- `src/ui/chat.rs`: `render_channel_list` makes headers clickable, with a chevron. A folded section shows only its open channel and channels with unread mentions (assumption 3). Saving reuses the places file's existing save path.
- `tests/` for places round-trips (look for existing places tests first)
**Depends on:** Chunk 1, which adds category IDs to `ChannelSection`.
**Risks:**
- **A category deleted while folded** leaves a stale ID. Drop unknown IDs on `Ready`, as `drop_stale_selection` does for channels.
- **Keyboard navigation (chunk 5) must skip hidden channels.**
**How to verify:** `cargo test`: places files with and without `collapsed` load, and collapsed IDs round-trip.
**Manual check:**
- Fold a category: its channels disappear except the open one.
- Have a second account mention you in a hidden channel: it reappears with its badge.
- Quit and relaunch: the category is still folded.
**Shippable when:** folding works, survives a relaunch, and never hides a mention.

### Chunk 5: Keyboard shortcuts in a channel
**Goal:** Move between channels, unread channels and servers, jump to the newest message, and mark a channel or server read, all from the keyboard.
**Files likely touched:**
- `src/ui/chat.rs`: `actions!(chat, [PrevChannel, NextChannel, PrevUnread, NextUnread, PrevServer, NextServer, JumpToPresent, MarkServerRead])`; `key_context("ChatView")` on the root element; `cx.bind_keys` from a `chat::init(cx)` called in `main.rs`; handlers that walk the visible list built by `sections()`, skipping folded channels from chunk 4, and the rail's order of Home then servers. Esc reuses the existing "Jump to present" plus an immediate ack; ⇧Esc reuses `mark_server_read`.
- `src/ui/composer.rs`: let Esc fall through when nothing in the message box uses it
- `src/main.rs`: call `chat::init(cx)`
**Depends on:** Chunk 4, for skipping folded channels.
**Risks:**
- **gpui-component's text input may claim ⌥↑/⌥↓ and Esc first.** Check with `window.dispatch_keystroke`-style tests or by hand.
- **Fallback if the input wins:** bind channel moves only when the message box is empty or unfocused (see Open Questions), and say so in the implementation record.
- **Esc already cancels edits, replies and autocomplete.** Those must keep working.
**How to verify:** `cargo test`, with unit tests for the pure "next channel / next unread / next server" walkers. Factor them out of `ChatView` (for example `fn neighbour(list, current, step, filter)`) so they can be tested.
**Manual check:**
- ⌥↓ through a server's channels; it wraps around or stops at the end (pick one and note it).
- ⌥⇧↓ lands only on unread channels.
- ⌘⌥↓ goes Home → first server → ….
- Scroll up and press Esc: you're at the newest message and the channel is read.
- ⇧Esc clears the server's badges.
- Esc still cancels an edit, a reply and autocomplete.
**Shippable when:** every shortcut works, and Esc still does its old jobs in the message box.

### Chunk 6: Switch accounts with ⌘1–⌘9
**Goal:** ⌘1 to ⌘9 switch to the nth account in the switcher.
**Files likely touched:**
- `src/ui/app_root.rs`: an action `SwitchAccount(usize)` (`#[derive(Action)]` with a field, or nine unit actions if that's simpler with GPUI's macros), handled with `show_account`
- `src/main.rs`: bindings, and a "Window" or "Accounts" menu listing them
**Depends on:** None. This chunk is independent of 1–5.
**Risks:**
- **An index past the end** does nothing.
- **A dimmed (signed-out) account** still switches, as clicking does today.
**How to verify:** `cargo build`, then `cargo test`.
**Manual check:** With two accounts, ⌘1 and ⌘2 switch between them, and ⌘3 does nothing.
**Shippable when:** the shortcuts match the switcher's order.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets`. Don't run `cargo fmt` across the crate: it reformats unrelated files, because of drift from before milestone 7. Format only the files you touch, with `rustfmt --edition 2024 <file>`.

### Unit Tests
- **Message link builder:** a server channel, a DM or group, no `app`, and `app` with or without a trailing slash.
- **`with_unread_divider`:** placement, no unread messages, only your own unread messages, a read position before all loaded history, a read position after it.
- **`set_read_position` / `message_before`:** moving back marks the channel unread; the first loaded message; an unknown ID.
- **Places:** `collapsed` round-trips; older files without the field load.
- **Neighbour walkers:** next or previous channel, unread-only, servers including Home, folded channels skipped.

### Integration Tests
- `tests/api_unreads.rs`: if mark as unread adds a new request shape, cover `PUT /channels/{id}/ack/{message}` with an older ID, following the file's existing mock-server pattern.

### Manual Tests
You need a second account (as in milestone 8's checks), in a server with categories and several channels.
1. The second account posts in two channels. Open each one: the divider is in the right place, and gone after switching away.
2. Mark an older message unread. The channel stays unread while you're there, is read after you leave and come back, and shows unread in the web client before that.
3. Copy a message link and open it in a browser. Copy each kind of ID.
4. Fold a category. A mention in it still shows. Relaunch: still folded.
5. Go through every shortcut in chunks 5 and 6, including Esc inside the message box while editing, replying and autocompleting.

## Definition of Done
- [ ] The message menu has "Copy message link" (only when the instance sends `app`) and "Copy message ID". Channel rows, category headers and server icons have "Copy … ID".
- [ ] A copied message link opens that message in Stoat's web client.
- [ ] Opening a channel with unread messages from others shows the divider before the first of them. Leaving the channel or sending a message removes it.
- [ ] "Mark as unread" makes the channel unread here and in the web client, and RuSToat doesn't mark it read again until you leave it.
- [ ] Category headers fold and unfold. The state survives a relaunch, and channels with mentions still show.
- [ ] ⌥↑/↓, ⌥⇧↑/↓, ⌘⌥↑/↓, Esc, ⇧Esc and ⌘1–9 work as in assumption 4. Esc still cancels edits, replies and autocomplete.
- [ ] `cargo test` and `cargo clippy --all-targets` pass, with no new warnings.
- [ ] Every chunk's manual check is recorded in `docs/plans/reading-flow-implementation.md`.
