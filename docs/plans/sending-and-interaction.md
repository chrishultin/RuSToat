# Plan: Sending and interaction (RuSToat milestone 3)

## Summary
RuSToat can read Stoat but can't write to it: there's no message box, and the only write calls are read acknowledgements and reactions. This milestone makes it a usable chat client. It adds:
- a message box, and sending (shown at once, confirmed by the server)
- reply previews, and replying
- editing and deleting your own messages
- typing indicators, both ways

## Approach
Keep the architecture of milestones 1 and 2:
- **Pure display model:** `ChatStore` holds all state with no GPUI types, and `timeline()` builds plain rows, so every rule is unit-testable.
- **Thin rendering:** `ChatView` renders those rows inside gpui-component's `MessageScroller`.
- **Backend types:** wire types come from `revolt-models`.
- **Writes are optimistic:** apply locally first, send, and roll back with a toast if Stoat refuses. That's the pattern chunk 7 of milestone 2 established for reactions.

**Facts this plan relies on** (read from `revolt-models` / `revolt-database` 0.16.2, which the app pins):
- **Send:** `POST /channels/{id}/messages`. The body is `DataMessageSend`, of which this milestone uses `content` and `replies: [ReplyIntent { id, mention, fail_if_not_exists }]`. The response is the created `Message`.
  - Duplicates are prevented by an `Idempotency-Key` header of at most 64 bytes. The body's `nonce` is deprecated.
  - **The backend copies the key into the created message's `nonce`** (`revolt-database` `models/messages/model.rs`: "Pass-through nonce value for clients"). So a pending placeholder can be matched to the real message whichever arrives first: the POST response or the live `Message` event.
- **Edit:** `PATCH /channels/{id}/messages/{msg}` with `DataEditMessage { content }`. Content must be at least 1 character.
- **Delete:** `DELETE /channels/{id}/messages/{msg}`.
- **Fetch one message:** `GET /channels/{id}/messages/{msg}`.
- **Limits** (`Revolt.toml`): `message_length = 2000` and `message_replies = 5`.
- **Typing:**
  - The client sends `{"type":"BeginTyping","channel":…}` and `{"type":"EndTyping","channel":…}` over the events websocket (`ClientMessage` in `revolt-database/src/events/server.rs`).
  - The server sends `ChannelStartTyping { id, user }` and `ChannelStopTyping { id, user }`, where `id` is the channel.
- **Already handled:**
  - Live `MessageUpdate` and `MessageDelete` events (milestone 1). Edits and deletes made here echo back through them harmlessly.
  - `ChatStore::apply_message` replaces a message with the same ID, and acks the channel when the author is the user.
- **Message box:** gpui-component has a `Textarea` (`TextareaState`, from gpui-base). It has `auto_grow(min_rows, max_rows)` and emits `InputEvent::PressEnter { secondary, shift }`.
- **Scrolling:** `MessageScrollerState::scroll_to_item(index)` exists, for jumping to a reply's original.

**Message actions** (the user chose both):
- **Hover toolbar:** "React" (the existing "+") and "Reply", in the bubble-corner slot milestone 2 uses for "+".
- **Right-click menu:** Reply, Copy text, and on your own messages Edit and Delete. It uses `ContextMenuExt`, already used by the server rail.

**Rejected alternatives:**
- **Inline editing inside the bubble:** this would need a second text box per row inside a virtualised list. Editing in the message box (with an "Editing message" bar) reuses one input and matches how replies work.
- **Matching pending messages by content and time:** fragile. The idempotency key is round-tripped exactly.
- **A separate websocket for typing:** the existing connection already reconnects and authenticates, so it just needs a way to send messages.

## Assumptions
1. **Enter sends, Shift+Enter adds a new line**, as in Stoat's web client. If `Textarea` can't be stopped from inserting the newline on Enter, the fallback is described in chunk 1's risks.
2. **Drafts are kept per channel** in memory while the app runs. Switching channels swaps them; they aren't saved to disk.
3. **Reply mentions default to on**, as in Stoat. The toggle is per reply in the "Replying to" bar.
4. **Up to 5 replies per message** (Stoat's limit). Choosing Reply on a sixth message is refused with a toast.
5. **Delete asks for confirmation**, and Shift+click skips it, as in Stoat's web client. Only your own messages can be deleted. Deleting others' messages with Manage Messages is out of scope.
6. **Your typing is shared with others.** Other Stoat clients do this, and there's no setting for it.
7. **Clicking a reply preview jumps to the original only if it's loaded.** Loading history around an older message ("jump to message") is out of scope.
8. **Out of scope for this milestone (user decision):**
   - file uploads and pasted images
   - @mention autocomplete
   - :emoji: autocomplete, or an emoji button in the message box

   These are good candidates for milestone 4. Mentions typed as `<@id>` still work.
9. **Send permission isn't precomputed** (no `revolt-permissions`), the same as for reactions. If Stoat refuses (`MissingPermission`), the pending message shows as failed with the reason.
10. **Unicode emoji shortcodes** (`:smile:`) aren't converted on send. Stoat clients send Unicode, and that's out of scope with autocomplete.

## Open Questions
- *(Deferred, not blocking)* **Typing timing:** how often to resend `BeginTyping`, and how long an incoming indicator lasts, aren't documented in the crates. Chunk 6 starts with web-client-like values (resend every 3 s, expire after 5 s without a refresh) and confirms them live.

## Plan

### Chunk 1: Message box and sending
**Goal:** Type in a message box under the conversation and send. The message appears at once (muted, "Sending…") and becomes the real message when Stoat confirms it. If sending fails, it stays with Retry and Discard. This is the milestone's core: RuSToat becomes a chat client.
**Files likely touched:**
- `src/api/client.rs`:
  - `send_message(channel, content, replies, idempotency_key) -> Message`
  - `send_raw` gains a way to add headers (the `Idempotency-Key`)
- `src/store.rs`:
  - `PendingMessage { nonce, channel, content, replies, state: Sending | Failed(String) }`, kept per channel in send order
  - `add_pending`, `fail_pending`, `retry_pending` (back to Sending) and `discard_pending`
  - `apply_message` drops the pending entry whose `nonce` matches a message from `me`. This works for both the POST response and the echo, in either order.
  - `timeline()` appends pending rows after the loaded messages, as `MessageRow`s with a new `pending: Option<PendingState>` field. They're grouped with the user's previous message like any other.
  - Pending content goes through `markdown::prepare`, like real messages.
- `src/ui/composer.rs` (new): the message box view.
  - It owns a `TextareaState` (`auto_grow(1, 8)`, placeholder "Message #channel" or "Message @name").
  - Enter sends; Shift+Enter adds a new line.
  - Whitespace-only messages are never sent.
  - A character count appears from 1800 characters; over 2000 shows an error and blocks sending.
  - Drafts per channel (Assumption 2).
  - It emits `ComposerEvent::Send(String)`.
- `src/ui/chat.rs`:
  - The composer goes under the message list in `render_main`, with drafts swapped on `select_channel`.
  - `send_message` makes a nonce (time plus a counter, kept under 64 bytes), adds the pending message, scrolls to the end, then posts. On failure it calls `fail_pending` with the reason: `MissingPermission` gives "You can't send messages here."
  - Pending rows draw muted with "Sending…". Failed rows show the reason plus Retry and Discard.
- `tests/api_send.rs` (new, wiremock), and `tests/store_pending.rs` (new)
**Depends on:** None (builds on milestone 2)
**Risks:**
- **Enter in `Textarea` (spike first):** `PressEnter` is emitted, but the textarea may already have inserted the newline. Options, in order:
  1. Intercept the Enter key action before the textarea handles it (a key binding in a context the composer owns).
  2. Strip a trailing newline on `PressEnter { shift: false }` and send.
  3. Fall back to a single-line `InputState`, plus Shift+Enter via the same interception.

  Record which one worked.
- **Echo before response:** the live `Message` event usually arrives before the POST response. Matching on `nonce` must work in both orders, and the response arriving second must not re-add the message. `apply_message` replaces by ID, which covers that; test both orders.
- **Scroll position:** sending while scrolled up should jump to the end, like other chat apps. Make sure an incoming message doesn't also force a jump.
- **Focus:** the full-size image viewer (milestone 2, chunk 4b) takes focus for Esc. Closing it should return focus to the composer.
**How to verify:**
- `cargo test --test api_send`:
  - `POST /channels/{id}/messages` with the session and `Idempotency-Key` headers, and the JSON body `{"content": …}` with no `replies` key when there are none
  - the response parsed into a `Message`
  - a 403 `MissingPermission` mapped to `ApiError::Api`
- `cargo test --test store_pending`:
  - pending, then the echo with a matching nonce, gives one real row
  - pending, then the POST response, then the echo, gives one row
  - a message from someone else with the same nonce text doesn't confirm ours
  - fail, then retry, gives Sending again; discard removes the row
  - pending rows group with the user's last message
  - `diff_rows` gives `Append` when sending and `Remeasure` when confirmed or failed
**Manual check:** In a DM with the test account, send "hello" and a multi-line message (Shift+Enter). Both appear at once, look normal within a second, and show in the web app. Turn Wi-Fi off and send: the message shows as failed with Retry. Turn Wi-Fi on and Retry: it sends once, not twice.
**Shippable when:** Messages can be typed and sent in any channel, appear instantly, never duplicate, and failures can be retried or discarded.

### Chunk 2: Reply previews
**Goal:** Show what a reply is replying to: a small "↱ Name: snippet" line above the bubble. Today replies render with no context at all. Clicking the preview jumps to the original when it's loaded.
**Files likely touched:**
- `src/api/client.rs`: `fetch_message(channel, id) -> Message`
- `src/store.rs`:
  - `MessageRow.replies: Vec<ReplyPreview { id, author, snippet, mentions_author, state: Loaded | Loading | Deleted }>`, built from `Message.replies`, up to 5
  - The original is looked up in the loaded history first, then in a new `reply_sources: HashMap<id, Message>` cache.
  - `missing_reply_sources(channel) -> Vec<id>` lists what needs fetching. `add_reply_source(message)` and `reply_source_missing(id)` (for a 404) update the cache.
  - The snippet is plain text: the first non-empty line, Markdown markers stripped (a small pure function in `src/markdown.rs`), mentions resolved to `@Name`, and cut at about 100 characters on a character boundary. Messages with only attachments get "[image]" or "[file]".
- `src/ui/chat.rs`:
  - After `sync_pane`, fetch missing reply sources (each ID once, tracked in flight), then re-sync.
  - Draw the preview line above the bubble: a small avatar, the name, then the muted snippet.
  - A click calls `scroller.scroll_to_item` if the original is in the timeline.
- Tests: `tests/store_replies.rs` (new), and `tests/api_send.rs` gains `fetch_message`.
**Depends on:** None (independent of chunk 1)
**Risks:**
- **Fetch volume:** a page of 50 messages could reference many unloaded originals. Dedupe IDs, keep one request in flight per ID, and cap concurrency (for example 4 at a time). Watch for `RateLimited`.
- **Row height:** the preview adds a line above the bubble. It must be present on first layout, even as a "Loading…" placeholder of the same height, so rows don't jump when the fetch completes.
- **Grouping:** a reply shouldn't be drawn as a continuation without its preview. Decide whether replies break a group: Stoat's web client shows the preview even inside a group.
**How to verify:** `cargo test --test store_replies` covers:
- a reply to a loaded message, and to a cached one
- missing sources listed once
- a deleted original (404) shown as "Original message was deleted"
- snippet stripping (bold, code, links, mentions, multi-byte truncation)
- attachment-only originals
- the `mentions_author` flag from `Message.mentions`
- `diff_rows` giving `Remeasure` when a source arrives
**Manual check:** Reply to a recent message and to a much older one from the web app. Both show previews here, the older one after a moment. Clicking the recent one's preview scrolls to it.
**Shippable when:** Every reply shows what it replies to, without rows jumping, and the tests pass.

### Chunk 3: Message actions and replying
**Goal:** Reach actions on a message, and reply to it.
- **Hover toolbar:** React and Reply.
- **Right-click menu:** Reply and Copy text.
- **Replying:** puts a "Replying to Name" bar above the message box, with an @-mention toggle and a close button. Sending includes the replies.

**Files likely touched:**
- `src/ui/chat.rs`:
  - `reaction_button` becomes a small toolbar in the same bubble-corner slot, with "+" and Reply (`IconName::Reply`, added to `src/assets.rs` if it isn't a default icon), shown on hover.
  - Right-click on a bubble opens a `context_menu` with Reply and Copy text (copies the message's raw `content` via `cx.write_to_clipboard`).
  - New `RowAction::Reply(message_id)`.
- `src/ui/composer.rs`:
  - The reply bar: an "@ on/off" toggle per reply, × to remove one, and Esc to clear all.
  - Up to 5 replies (Assumption 4).
  - The draft and replies are kept per channel.
- `src/store.rs`: `PendingMessage.replies` carries the `ReplyIntent`s, so pending rows show reply previews (chunk 2's builder, using the loaded originals).
- `tests/api_send.rs`: the `replies` body. `tests/store_pending.rs`: pending replies.
**Depends on:** Chunks 1 and 2
**Risks:**
- **Right-click in rows:** right-click on a selectable `TextView` may already be claimed for text selection. If so, attach the menu to the bubble's padding and the avatar, or add "More…" (⋯) to the hover toolbar to open the same menu. Record which.
- **Toolbar placement:** the bubble-corner slot sits 28 px outside the bubble. Two buttons need about 56 px. Check narrow windows and your own messages (left side).
**How to verify:** `cargo test` covers:
- `send_message` with `replies: [{"id": …, "mention": true}]`
- the mention toggle giving `mention: false`
- a sixth reply refused
- pending rows showing reply previews
**Manual check:** Hover a message and click Reply. The bar appears. Toggle @ off, send, and in the web app the reply shows without pinging. Right-click, then Copy text, and paste somewhere: it's the raw text.
**Shippable when:** You can reply (with or without a mention) from the toolbar or the menu, and copy a message's text.

### Chunk 4: Editing your messages
**Goal:** Edit your own messages. Choosing Edit (right-click) or pressing Up in an empty message box loads the message's text into the box under an "Editing message" bar. Enter saves and Esc cancels. The change shows at once and rolls back if refused.
**Files likely touched:**
- `src/api/client.rs`: `edit_message(channel, id, content) -> Message`
- `src/store.rs`:
  - `apply_local_edit(channel, id, content) -> Option<previous content>` for the optimistic change and rollback
  - `last_own_message(channel)` for the Up-arrow shortcut (newest message by `me`, not pending)
  - `raw_content(channel, id)` returns the original Markdown source, not the prepared form.
- `src/ui/composer.rs`:
  - an edit mode that stashes the draft and replies, and restores them after saving or cancelling
  - Up-arrow on an empty box
  - an empty edit shows "To delete the message, use Delete" and isn't sent (Stoat requires at least 1 character)
- `src/ui/chat.rs`: "Edit" in the right-click menu (own messages only), and the save, rollback and toast flow
- Tests: `tests/api_send.rs` (`PATCH` path and body), and `tests/store_pending.rs` or a new `tests/store_edit.rs`
**Depends on:** Chunks 1 and 3 (message box and menu)
**Risks:**
- **Echo:** the live `MessageUpdate` for our own edit must not undo or double-apply the optimistic change. `apply_update` merges server data, which is the same content, so check that `render_key` and the edited flag settle.
- **Draft safety:** a half-written draft must survive starting and cancelling an edit.
**How to verify:** `cargo test` covers:
- `PATCH /channels/{c}/messages/{m}` with `{"content": …}`
- a local edit, then a refusal, restoring the old text and `render_key`
- the echo after a local edit changing nothing visible
- `last_own_message` skipping others, system messages and pending rows
- the draft stashed and restored
**Manual check:** Press Up in an empty box, change the text, and press Enter. The message updates here and in the web app, marked "(edited)". Edit another and press Esc: the original text and your draft are unchanged.
**Shippable when:** Your own messages can be edited from the menu or with Up, failures roll back, and drafts are never lost.

### Chunk 5: Deleting your messages
**Goal:** Delete your own messages from the right-click menu, after a confirmation dialog (Shift+click skips it). The message disappears at once and comes back in place if Stoat refuses.
**Files likely touched:**
- `src/api/client.rs`: `delete_message(channel, id)`
- `src/store.rs`: `remove_local(channel, id) -> Option<(index, Message)>` and `restore_local(channel, message)`. Restoring goes back in ID order, so the position is exact.
- `src/ui/chat.rs`:
  - "Delete message" in the menu, own messages only, styled as destructive
  - A confirmation dialog (gpui-component dialog, via `WindowExt`) previewing the message's snippet, with Cancel and Delete. Shift held on the menu item skips it.
  - The optimistic remove, send, then restore and toast on failure
  - Discarding a failed pending message (chunk 1) stays separate and needs no dialog.
- Tests: `tests/api_send.rs` (the `DELETE` path), and store tests
**Depends on:** Chunk 3 (the menu)
**Risks:**
- **Dialog API:** find gpui-component's dialog entry point (`window.open_dialog` or similar) and its keyboard handling: Enter confirms, Esc cancels.
- **Reply previews:** messages whose reply source was just deleted should show "Original message was deleted" (chunk 2's state) without a refetch loop.
**How to verify:** `cargo test` covers:
- `DELETE /channels/{c}/messages/{m}` with the session header
- remove then restore returning the message to its exact index
- the `MessageDelete` echo after a local remove being a no-op
- `diff_rows` giving `Remove`, then `Insert` on restore
**Manual check:** Delete one of your messages with confirmation, and another with Shift held. Both vanish here and in the web app. A reply to one of them now says the original was deleted.
**Shippable when:** Your own messages can be deleted safely, with confirmation and rollback.

### Chunk 6: Typing indicators
**Goal:** Show "Name is typing…" above the message box when others type, and tell others when you're typing.
**Files likely touched:**
- `src/api/events.rs`:
  - `Connection` gains an outbound queue (an `mpsc` sender) read by the connection task alongside the socket and ping timer, plus `begin_typing(channel)` and `end_typing(channel)`.
  - Messages queued while disconnected are dropped. Typing state isn't worth replaying after a reconnect.
  - New `ServerEvent::ChannelStartTyping { channel, user }` and `ChannelStopTyping`, decoded from `id`/`user`.
- `src/typing.rs` (new, pure):
  - `TypingState`: incoming typists per channel with an expiry, using injected `Instant`s for testing. `start`, `stop`, `typists(channel, now)` (excluding `me`) and `next_expiry`.
  - `OutgoingTyping`: decides when to send Begin (text non-empty, at most every 3 s) and End (sent, cleared, channel switched, or 5 s idle).
  - `typing_label(names)`: "Ada is typing…", "Ada and Bob are typing…", "Several people are typing…".
- `src/ui/chat.rs` and `src/ui/composer.rs`:
  - composer `Change` events feed `OutgoingTyping`
  - the indicator line sits just above the box, a fixed height, empty when nobody is typing, so the list doesn't jump
  - a timer re-renders at `next_expiry`
  - a new message from a typist removes them at once
- `tests/typing.rs` (new)
**Depends on:** Chunk 1 (the message box)
**Risks:**
- **Connection task change:** adding an outbound queue touches the reconnect loop that milestone 1 hardened. Keep the existing `live_events` handshake test passing, and add a unit test that `BeginTyping` frames are serialised exactly (`{"type":"BeginTyping","channel":"…"}`).
- **Timing values** (Open Question): confirm live that a typist's indicator in the web app stays steady with a 3 s resend, and that ours clears promptly after sending.
**How to verify:** `cargo test --test typing` covers:
- start and stop
- expiry after 5 s with no refresh
- our own user ignored
- several typists' labels
- outgoing Begin throttled to once per 3 s
- End on send, clear and idle

Plus the frame serialisation unit test in `events.rs`, and the `parse_frame` decoding for both events.

**Manual check:** Type in the web app: "Name is typing…" appears here within a second, and clears when they send or stop. Type here: the web app shows you typing, and it clears when you send.
**Shippable when:** Typing shows both ways without flicker or leftover indicators, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`. Optional live checks: `cargo test -- --ignored`, which runs the API drift, events handshake, Keychain and media checks.

### Unit Tests
- **Pending messages:** add, confirm by nonce (POST first or echo first), fail, retry, discard, grouping, and no confirmation from another author's message.
- **Reply previews:** source lookup (history, then cache, then missing), deleted originals, snippet stripping and multi-byte truncation, the `mentions_author` flag.
- **Composer rules:** whitespace-only messages aren't sent, the 2000-character limit, at most 5 replies, the empty-edit hint, drafts stashed and restored.
- **Edits:** local edit and rollback restoring `render_key`; the echo is a no-op.
- **Deletes:** remove and restore to the exact index; the echo is a no-op.
- **Typing:** `TypingState` expiry and labels, `OutgoingTyping` throttling and End triggers, the `BeginTyping`/`EndTyping` frame JSON.

### Integration Tests
- **`wiremock`:**
  - `POST` send (headers and body, with and without replies)
  - `GET` one message
  - `PATCH` edit
  - `DELETE` message
  - `MissingPermission` and `RateLimited` mapping
- **`parse_frame`:** `ChannelStartTyping` and `ChannelStopTyping` with real field names.
- **Store pipelines:** send (pending), then a `Message` event with a nonce, then `timeline()` has one row and `diff_rows` gives `Remeasure`. A reply whose source is fetched gives the row with its preview.
- **Opt-in live:** the existing `live_events` handshake still passes with the outbound queue.

### Manual Tests
1. Send plain, multi-line and Markdown messages in a DM and in a server channel. They appear instantly and match the web app.
2. Go offline and send, then go online and Retry. The message sends exactly once.
3. Reply with and without a mention, and check in the web app that only the first pings.
4. Edit with Up, then Enter, and edit and cancel with Esc; your draft is unchanged.
5. Delete with confirmation, and with Shift. A reply to a deleted message says so.
6. Type in the web app and here at the same time. Both indicators appear and clear.
7. Try to send in a read-only channel. The message fails with "You can't send messages here." and can be discarded.
8. Light and dark mode. The message box, the reply and edit bars, pending and failed rows, and the typing line are all readable.

## Definition of Done
- [ ] **Sending:** a message box under every channel. Enter sends, Shift+Enter adds a new line, whitespace-only messages aren't sent, and over 2000 characters is blocked with a visible count. Drafts are kept per channel.
- [ ] **Pending messages:** sent messages appear immediately as pending and are confirmed by `Idempotency-Key`/`nonce` matching. There's never a duplicate, whether the POST response or the live event arrives first (tested both ways).
- [ ] **Failures:** a failed send shows the reason with Retry (same key, sent at most once) and Discard. `MissingPermission` reads "You can't send messages here."
- [ ] **Reply previews:** replies show "↱ Name: snippet" above the bubble. Unloaded originals are fetched (deduplicated, without rows jumping), and deleted ones say so. Clicking jumps to a loaded original.
- [ ] **Replying:** Reply is in the hover toolbar and the right-click menu. The "Replying to" bar has a per-reply mention toggle, allows up to 5 replies, and Esc cancels. Sent replies carry the right `replies` body (tested).
- [ ] **Copy text:** the right-click menu copies a message's raw content.
- [ ] **Editing:** Edit (menu) and Up (empty box) edit your own messages in the message box, saved with `PATCH`. Refusals roll back. Drafts survive starting and cancelling an edit.
- [ ] **Deleting:** your own messages can be deleted, with confirmation that Shift skips. They're removed optimistically and restored in place on failure.
- [ ] **Typing:** others' typing shows above the message box and clears on stop, send or a 5 s timeout. Ours is sent throttled and ended on send, clear, channel switch or idle. Both were checked live.
- [ ] **Checks:** `cargo test && cargo clippy --all-targets -- -D warnings` passes, and `cargo test -- --ignored` (live checks, including the events handshake) passes.
