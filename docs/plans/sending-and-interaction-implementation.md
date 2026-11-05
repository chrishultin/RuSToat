# Implementation Record: Sending and interaction (milestone 3)

_Plan: docs/plans/sending-and-interaction.md_
_Started: 2026-11-05_

Pre-existing uncommitted changes, kept out of every commit: `.gitignore` (a tool added a duplicate `.humanlayer/tasks/` entry) and the untracked `.shadow/`.

Supervision: continuous.

## Chunk 1: Message box and sending

### Changes Made
- `src/api/client.rs`: `send_message(channel, content, replies, idempotency_key) -> Message` (`POST /channels/{id}/messages`). The body is built from `DataMessageSend` with its `null` fields removed (`without_nulls`), so a message without replies is exactly `{"content": …}`. `send_raw` now delegates to `send_raw_with(…, headers)`, which adds extra headers (`Idempotency-Key`). Response parsing moved into a shared `parse` helper.
- `src/store.rs`:
  - `PendingMessage { key, nonce, channel, content, replies, state, sent_ms, nonce_used }` and `PendingState { Sending, Failed(String) }`, kept per channel in send order.
  - `add_pending`, `pending`, `fail_pending(channel, key, reason, answered)`, `retry_pending(channel, key, fresh_nonce)` and `discard_pending`.
  - `apply_message` drops the pending entry whose nonce matches a message from `me` (`confirm_pending`), whichever of the POST response or the echo comes first. `set_history` and `prepend_page` do the same for loaded pages (`confirm_pending_in_history`).
  - `MessageRow` gained `key` (the row's identity for `diff_rows`: the nonce for the user's own messages that have one, else the ID) and `pending: Option<PendingState>`. `row_key` uses `key`.
  - `timeline()` was refactored. Row building moved into `message_row()`. Pending messages are turned into synthetic `Message`s (`pending_as_message`) and appended after the loaded ones. Grouping now compares millisecond times, so pending rows (timed by `sent_ms`) group with the user's previous message.
- `src/time.rs`: `millis_time_label(ms)`. `message_time_label` now uses it.
- `src/draft.rs` (new, pure): `check_send` (whitespace-only messages are never sent; leading blank lines and trailing whitespace are trimmed; the 2000-character limit counts characters), `char_count` (shown from 1800), `Drafts` (kept per channel), and `new_nonce(now_ms)` (time, counter and a random part, at most 48 bytes).
- `src/ui/composer.rs` (new): `Composer` view.
  - It owns a `TextareaState` with `auto_grow(1, 8)` and `submit_on_enter(true)`.
  - It emits `ComposerEvent::Send(text)` on `PressEnter { shift: false }`.
  - Drafts are swapped in `set_channel`, with the cursor put at the draft's end.
  - It shows the character count, in red with how many characters are over when past the limit.
- `src/ui/chat.rs`:
  - `ChatView` owns the composer and draws it under the message list for any open channel. `sync_composer` (run from `render`) points it at the current channel with a placeholder ("Message #name", "Message @name", …) and focuses it when the channel changes.
  - `send_message` adds the pending message, syncs, scrolls to the end, then posts. `post_pending` confirms on success, treats `DuplicateNonce` as already sent, and otherwise calls `fail_pending` with `send_error_text` (`MissingPermission` gives "You can't send messages here."). `retry_send` and `RowAction::{RetrySend, DiscardSend}` handle the buttons.
  - Pending rows are drawn at 60% opacity with "Sending…" under them. Failed rows show the reason with Retry and Discard. Pending rows have no reaction button.
  - `close_viewer` gives focus back to the message box.
  - `sync_pane` does nothing for a channel whose history isn't loaded, so sending during a load can't create an empty pane.
- `src/lib.rs` and `src/ui/mod.rs`: register `draft` and `composer`.
- Tests: `tests/api_send.rs` (new), `tests/store_pending.rs` (new). `tests/store_live.rs`'s row helper gained the new fields.

### Divergences
- **Enter in the textarea (a plan risk):** resolved with `TextareaState::submit_on_enter(true)` (public but `#[doc(hidden)]` in gpui-base 0.9). With it, Enter emits `PressEnter` without inserting a newline and Shift+Enter inserts one. None of the plan's three fallbacks were needed.
- **Retry keys:** Stoat's `IdempotencyKey` guard (`revolt-database` `util/idempotency.rs`) records the key in a cache when the request arrives, before permission checks or creating anything. A later request with the same key gets `409 DuplicateNonce`.
  - Retrying with the same key after Stoat answered with an error would therefore always fail, though nothing was created.
  - So a pending message records whether the last failure was answered by Stoat (`nonce_used`). Retry keeps the key after a network failure, where the outcome is unknown and a duplicate must be impossible. It uses a fresh key after an error response, where nothing was created.
  - A `DuplicateNonce` on retry means an earlier attempt got through, so the pending entry is dropped. The real message shows via its echo or the next history load.
  - The row keeps a stable `key` (its first nonce), so a key change doesn't move the row.
- **Row identity:** `MessageRow.key` was added (not in the plan) so a pending row and its confirmed message are the same list row. `diff_rows` gives `Remeasure` on confirmation, as the plan's tests require.
- **History loads confirm pending messages too** (not in the plan). This covers a send whose echo was missed while the events connection was down and the history was then reloaded.
- **Nonce format:** time, a counter and a random part (`rustoat-{ms}-{n}-{hex}`), not just time and a counter. Stoat's duplicate cache is shared by all clients.

### Verification Performed
- `cargo test --test api_send`: 5 passed.
  - POST path with the session and `Idempotency-Key` headers, and the body exactly `{"content": "hello\nworld"}`.
  - The response parsed into a `Message` with its `nonce`.
  - The `replies` body.
  - 403 `MissingPermission` mapped to `ApiError::Api`.
  - 409 `DuplicateNonce`.
  - 429 `RateLimited` with `retry_after`.
- `cargo test --test store_pending`: 15 passed.
  - Pending after the loaded messages.
  - The echo confirms it.
  - POST response then echo, and echo then POST response, each give one row.
  - Another author's message with the same nonce doesn't confirm ours.
  - A history load confirms it.
  - Fail, then retry (same nonce), then discard.
  - A retry after an answered failure gets a fresh nonce and keeps its row key.
  - Grouping with our last message, and a new group after 7 minutes.
  - `diff_rows` gives `Append` (plus a `Remeasure` of the previous group end) on send, and `Remeasure` on failure and on confirmation.
  - Pending Markdown is prepared like real messages.
  - Message box rules: blank, trimming, the character (not byte) limit, the count threshold, drafts, and nonce uniqueness and length.
- Full suite: `cargo test`, 230 passed, 0 failed, 5 ignored (live checks). `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
The user ran the app (`cargo run`). They sent a message in a DM and a multi-line message. Both appeared at once and were confirmed (log: `sent 01M6PBV7… in 01M4PQSA…`, `sent 01M6PBVQ… in 01M6N0BJ…`). The user confirmed it "looks good". The offline send and Retry wasn't exercised in this session (no failure in the log). It's carried to the plan's final Manual Tests, item 2.

## Chunk 2: Reply previews

### Changes Made
- `src/api/client.rs`: `fetch_messages_around(channel, message, limit) -> BulkMessageResponse`, which is `GET /channels/{id}/messages?nearby={message}&limit=…&include_users=true`.
- `src/markdown.rs`: `snippet(content, names, max_chars)`, a one-line plain-text preview.
  - It takes the first non-empty line, skipping lone code fences.
  - It strips heading, quote and list prefixes (`strip_line_prefix`), and inline markers: `**`, `__`, `~~`, `||`, `$$`, `*`, backticks, and `[text](url)` becomes `text` (`strip_markers`). A lone `_` stays, so `snake_case` survives.
  - Mentions become `@Name` (via `plain_text`). Whitespace is collapsed, and the text is cut at `max_chars` characters with "…".
- `src/store.rs`:
  - `MessageRow.replies: Vec<ReplyPreview { id, author, avatar, snippet, mentions_author, state }>`, with at most `MAX_REPLIES` (5). `ReplyState` is `Loaded`, `Loading`, `Deleted` or `Unavailable`.
  - The original is looked up in the loaded history first (`find_message`, binary search), then in a new `reply_sources` cache (private `ReplySource::{Found(Box<Message>), Deleted, Unavailable}`).
  - `missing_reply_sources(channel)` lists originals of loaded and pending replies, each once. `add_reply_sources(page, wanted)` caches the page's messages and authors, and marks any wanted ID missing from it as deleted. `reply_source_unavailable(id)` records a failed fetch.
  - `clear_histories` drops `Unavailable` entries, so they're retried after a reconnect. `apply_delete` marks deleted IDs as deleted in the cache, with no refetch. It also returns true when a cached original was deleted. `apply_update` also updates a cached original.
  - `snippet()` handles system messages (their text), attachment-only originals ("[image]" or "[file]"), and embed-only originals ("[embed]").
  - A reply always starts a new group (`continuation` is false).
- `src/ui/chat.rs`:
  - `fetch_reply_sources(channel)` runs at the end of `sync_pane`. It keeps at most `REPLY_FETCHES` (4) in flight, tracked in `reply_fetches`, and each result re-syncs the channel, which starts the next ones. A rate-limited fetch waits for `retry_after` (2 s by default) and is tried again. Other errors mark the original unavailable.
  - `render_reply_preview` draws the line above the bubble, inside `MessageContent`. It has a fixed `REPLY_LINE_HEIGHT` (18 px) in every state: the corner-up-right icon, a 14 px avatar, the name (`@` prefixed when the reply mentioned them), and the truncated snippet. The other states read "Loading…", "Original message was deleted" and "Couldn't load the original message".
  - Clicking a loaded preview sends `RowAction::JumpTo(id)`, which calls `scroll_to_item` if the original is in the pane's rows.
- `src/assets.rs`: registers the `CornerUpRight` icon.
- Tests: `tests/store_replies.rs` (new), `tests/api_send.rs` (+1). `tests/store_live.rs`'s row helper gained `replies`.

### Divergences
- **How originals are fetched:** the plan named `fetch_message` (`GET /channels/{c}/messages/{id}`). Stoat's single-message route returns no `user` or `member`, so an original whose author isn't cached would show "Unknown user", which is common in servers.
  - `GET …/messages?nearby={id}&limit=1&include_users=true` returns the message, its authors' users and members, and a neighbour on each side, in one request. In `revolt-database`, `nearby` runs `$gte id` limit 1 and `$lt id` limit 1.
  - A deleted original is simply missing from the result. That replaces the plan's 404 handling (`reply_source_missing`), which became `add_reply_sources(page, wanted)`.
- **Grouping (the plan left this open):** a reply starts a new group, so its preview sits under a name and time header and is never inside a group.
- **Unavailable state** (not in the plan): a failed fetch (not a deleted original) says "Couldn't load the original message", rather than "Loading…" forever. It's retried after a reconnect.
- **Live edits and deletes of cached originals** (not in the plan) keep their previews current.

### Verification Performed
- `cargo test --test store_replies`: 11 passed.
  - A reply to a loaded message.
  - An unloaded original listed once for two replies, then loaded with an author the cache didn't have.
  - A fetch without the original marks it deleted, and it isn't refetched.
  - Deleting a loaded original marks its replies with no fetch.
  - A failed fetch shows unavailable and is retried after a reconnect.
  - The `mentions_author` flag from `Message.mentions`.
  - Attachment-only originals ("[image]", "[file]").
  - At most 5 previews, and replies start a group.
  - Snippet stripping: bold, italic, underline, strike, spoiler, code, links, headings, quotes, lists, fences, mentions, `snake_case` and whitespace.
  - Cutting on character boundaries (é, 🦦).
  - A live edit of a cached original.
  - `diff_rows` gives `Remeasure` for both replies when the source arrives.
- `cargo test --test api_send`: 6 passed. The new test checks `nearby`, `limit=1` and `include_users=true`, and that the response parses with its users.
- Full suite: `cargo test`, 242 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings` was clean after boxing `ReplySource::Found` (`large_enum_variant`).

### Manual Validation
The user ran the chunk 2 build and confirmed reply previews work ("lgtm"). The log shows no fetch failures or rate limiting.

## Chunk 3: Message actions and replying

### Changes Made
- `src/draft.rs`:
  - `ReplyTarget { id, author, mention }`, `TooManyReplies`, and `Draft { text, replies }`.
  - `Draft`'s methods: `is_empty`, `add_reply` (replying to the same message twice is a no-op, and a sixth past `MAX_REPLIES` is an error), `toggle_mention`, `remove_reply`, and `reply_intents()` (`ReplyIntent`s with `fail_if_not_exists: None`).
  - `Drafts` now keeps a `Draft` per channel. A draft with replies but no text still counts.
- `src/ui/composer.rs`:
  - The composer holds the current `Draft`'s replies. `set_channel` saves and restores text and replies together.
  - `add_reply(target)` returns `Err(TooManyReplies)` past 5 and focuses the box.
  - `ComposerEvent::Send { text, replies }` carries the reply intents, and replies are cleared after sending.
  - The reply bar draws one line per reply: the Reply icon, "Replying to **Name**", an "@ On"/"@ Off" toggle (primary or ghost, with a tooltip) and ×.
  - Esc (the `input::Escape` action, which the textarea passes on) clears all replies. Without replies it keeps propagating.
- `src/ui/chat.rs`:
  - `send_message` takes the replies, and they're stored on the `PendingMessage`.
  - `reaction_button` became `message_toolbar`: the "+" picker and a Reply button in an `h_flex`, `TOOLBAR_OFFSET` (50 px) outside the bubble, shown on hover or while the picker is open, and not shown on pending rows.
  - Message rows have an ID (`row-{key}`) and a `context_menu` with Reply and Copy text. Pending rows have no menu.
  - New `RowAction::Reply(ReplyTarget)`: the composer adds the reply, or a warning toast "You can reply to up to 5 messages at once." New `RowAction::CopyText(id)`: `store.raw_content` goes to the clipboard.
  - `reply_target(row)`: the mention is on for others' messages and off for the user's own ("Replying to yourself").
- `src/store.rs`:
  - `raw_content(channel, id)` gives the Markdown source of a loaded message, or a pending one by key. It was planned for chunk 4 but was needed now for Copy text.
  - `pending_message()` turns a pending message into a `Message` whose `mentions` are the authors (via `original_author`) of the originals its replies ping, so pending previews show "@".
- `src/assets.rs`: registers the `Reply` icon. `Copy` and `Close` are defaults.
- Tests: `tests/store_pending.rs` (the drafts test was rewritten for `Draft`, plus 3 new tests), and `tests/api_send.rs` (the replies body now has a mention-off reply too).

### Divergences
- **Right-click (a plan risk):** the risk didn't happen. gpui-component's context menu listens for right-button presses on the whole window and checks which element is under the cursor, and `TextView` doesn't handle the right button. The menu is on the whole message row, so it works on the text, the padding and the avatar. No "More…" button was needed.
- **Replying to your own message** starts with the mention off. The plan says mentions default to on; that still holds for others' messages.
- **`raw_content`** was brought forward from chunk 4, for Copy text. Chunk 4's plan entry needs no change.

### Verification Performed
- `cargo test --test store_pending`: 18 passed. New tests:
  - The 5-reply limit, a repeated reply as a no-op, the mention toggle and removal, giving the right `ReplyIntent`s.
  - Drafts kept with their replies.
  - A pending reply showing previews of loaded originals, with the "@" from the reply's mention choice, and starting its own group.
  - Copy text giving the typed Markdown, for loaded and pending messages.
- `cargo test --test api_send`: 6 passed. The replies body is checked as `[{id, mention: true}, {id, mention: false}]`, with no `fail_if_not_exists` key.
- Full suite: `cargo test`, 245 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
The user ran the chunk 3 build, used it (log: two sends in a DM), and said to continue. The log shows no errors.

## Chunk 4: Editing your messages

### Changes Made
- `src/api/client.rs`: `edit_message(channel, id, content) -> Message`. It sends `PATCH /channels/{c}/messages/{m}` with `DataEditMessage { content }`, nulls removed, so the body is exactly `{"content": …}`.
- `src/store.rs`:
  - `apply_local_edit(channel, id, content) -> Option<LocalEdit>` sets the content, and `edited` to now (built through serde, since `Timestamp` isn't a direct dependency). It returns the previous message.
  - `undo_local_edit(channel, id, edit)` puts back the old content and `edited` flag, but only if the content is still the edit's (no newer change).
  - `last_own_message(channel)`: the newest loaded message by `me`, not a system or webhook message. Pending messages are separate, so they're never picked.
- `src/draft.rs`: `EditStash` (`begin(id, current)`, `editing()`, `end()`) sets the draft aside during an edit. Switching edits keeps the first draft.
- `src/ui/composer.rs`:
  - Edit mode: `start_edit(id, content)` and `cancel_edit`. While editing, the "Editing message" bar replaces the reply bar.
  - Enter emits `ComposerEvent::Edit { id, content }` and restores the draft. An empty edit shows the hint "To delete the message, use Delete." and isn't sent.
  - Esc cancels the edit first, otherwise clears replies. Changing channel cancels an edit.
  - Up in an empty box emits `ComposerEvent::EditLast`. It's caught by `capture_action::<MoveUp>` and stops propagation only in that case.
  - The hint clears when the text changes.
- `src/ui/chat.rs`:
  - `start_edit` (from `RowAction::Edit` or `EditLast`) loads `raw_content` into the composer.
  - `edit_message`: an unchanged edit does nothing. Otherwise it applies the edit locally, syncs and sends `PATCH`. On failure it undoes the edit and shows a toast (`edit_error_text`: `MissingPermission` or `CannotEditMessage` gives "You can't edit this message."). On success the response is ignored, because the `MessageUpdate` echo applies the same text.
  - "Edit" was added to the right-click menu for own messages, after a separator.
- `src/assets.rs`: registers the `Pencil` icon.
- Tests: `tests/store_edit.rs` (new), `tests/api_send.rs` (+2).

### Divergences
- **Up arrow:** the textarea's `MoveUp` handler doesn't propagate, so the shortcut is a capture-phase action listener on the composer. It acts only when the box is empty and not already editing; otherwise Up moves the cursor as usual.
- **`EditStash`:** the draft set-aside logic is a pure type in `draft.rs`, not composer internals, so the plan's "draft stashed and restored" check is a unit test.
- **The PATCH response isn't applied.** The live `MessageUpdate` echo carries the same change, and a reconnect reloads history if the echo is missed.
- **A refusal doesn't undo a newer change** (not in the plan): if the message changed again before the refusal arrived, it's left as it is.
- **`raw_content`** was already added in chunk 3.

### Verification Performed
- `cargo test --test store_edit`: 6 passed.
  - A local edit changes the body, `edited` and `render_key`, and a refusal restores the row exactly.
  - A refusal after a newer change leaves the newer change.
  - The echo (with Stoat's own edited time) leaves the timeline unchanged.
  - Editing an unloaded message does nothing.
  - `last_own_message` skips others' messages, our system message and pending ones.
  - `EditStash` keeps the original draft when switching edits.
- `cargo test --test api_send`: 8 passed. New: the PATCH path, session header and body, and a 403 `CannotEditMessage`.
- Full suite: `cargo test`, 253 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
The user ran the chunk 4 build. The first launch waited on a macOS Keychain prompt for the rebuilt binary. They edited a message (log: `edited 01M6PCP7… in 01M6N0BJ…`) and confirmed "LGTM".

## Chunk 5: Deleting your messages

### Changes Made
- `src/api/client.rs`: `delete_message(channel, id)`, which sends `DELETE /channels/{c}/messages/{m}`.
- `src/store.rs`:
  - `remove_local(channel, id) -> Option<Message>` takes a loaded message out and marks it `Deleted` in the reply-source cache.
  - `restore_local(channel, message)` puts it back in ID order (its exact place), clears the cache mark, and doesn't add it twice.
  - `message_snippet(channel, id)` gives the dialog's preview.
- `src/ui/chat.rs`:
  - The right-click menu on own messages gained a red **Delete message** item (an element item with the `Trash` icon). Its click sends `RowAction::Delete { id, confirmed: event.modifiers().shift }`.
  - `confirm_delete` opens an `AlertDialog`: `.confirm()`, the title "Delete message", a description mentioning Shift, the snippet in a bordered box (at most 3 lines), and a `Danger` Delete button. gpui-base binds Enter to confirm and Esc to cancel.
  - `delete_message` stops any edit of that message (`Composer::stop_editing`), removes the message locally, syncs, then sends `DELETE`.
    - Success, or a 404 `NotFound` (already gone), keeps it removed.
    - Other failures restore it, sync, and show a toast (`delete_error_text`: `MissingPermission` gives "You can't delete this message.").
    - The `MessageDelete` echo is a no-op afterwards.
- `src/ui/composer.rs`: `stop_editing(id)`.
- `src/assets.rs`: registers the `Trash` icon.
- `docs/plans/sending-and-interaction.md`: corrected chunk 5's reply-preview risk note to match.
- Tests: `tests/store_delete.rs` (new), `tests/api_send.rs` (+1).

### Divergences
- **`remove_local` returns `Option<Message>`**, not `(index, Message)`. Restoring by ID order makes the index unnecessary.
- **Reply-source mark:** `remove_local` marks the message deleted in the reply-source cache, so replies don't fetch it while the delete is in progress, and `restore_local` clears the mark. My chunk 2 note in the plan had said the opposite; it was corrected.
- **A 404 on delete counts as success**, and **deleting a message being edited cancels the edit** (neither was in the plan).

### Verification Performed
- `cargo test --test store_delete`: 5 passed.
  - Remove then restore returns the history and timeline exactly to before. `diff_rows` gives `Remove { at: 2 }` plus a `Remeasure` of the reply, then `Insert { at: 2 }` plus a `Remeasure`.
  - The `MessageDelete` echo after a local remove returns false and leaves the timeline unchanged.
  - The reply's preview goes Loaded, then Deleted (with no missing source, so no fetch), then Loaded on restore.
  - Removing unloaded messages does nothing, and restoring twice adds the message once.
  - The dialog snippet.
- `cargo test --test api_send`: 9 passed. New: `DELETE` with the session header.
- Full suite: `cargo test`, 259 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
The user deleted two messages: one through the dialog and one with Shift (log: `deleted 01M6PCP7…`, `deleted 01M6PCBG…`). They confirmed it works ("beautiful").

## Chunk 6: Typing indicators

### Changes Made
- `src/api/events.rs`:
  - `Connection` holds an `outbound` sender (`mpsc::UnboundedSender<String>`), with `begin_typing(channel)` and `end_typing(channel)`. `connect` passes the receiver to `run`, which lends it to each `session`.
  - The session's `select!` gained an `outbound.next()` arm. It sends the frame only once authenticated, and treats a closed queue as `ClientGone`. At the start of each session, the queue is drained with `try_recv`, so frames queued while disconnected are dropped.
  - Added `begin_typing_frame` and `end_typing_frame`.
  - New `ServerEvent::ChannelStartTyping { channel, user }` and `ChannelStopTyping`, decoded from `id`/`user` and forwarded to the UI.
  - The old "unknown event" unit test used `ChannelStartTyping` as its example, so it now uses `ServerMemberJoin`.
- `src/typing.rs` (new, pure):
  - Constants: `EXPIRE_AFTER` 5 s, `RESEND_EVERY` 3 s, `IDLE_AFTER` 5 s.
  - `TypingState`: `new(me)`, `start` (ignores `me`, refreshes the expiry and keeps start order), `stop`, `typists(channel, now)`, `next_expiry`, `prune`, `clear`.
  - `OutgoingTyping`: `on_input(channel, empty, now)` returns `Begin` throttled to 3 s, `End` when cleared, and `End` then `Begin` when the channel changes. Also `stop()` for send, switch or edit, `on_tick(now)` for idle, and `deadline()`.
  - `typing_label(names)`.
- `src/ui/composer.rs`: emits `ComposerEvent::Input { empty }` on text changes, except while editing.
- `src/store.rs`: `name_in(channel, user)` gives the server nickname, else the display name, else "Someone".
- `src/ui/chat.rs`:
  - New fields `typing`, `outgoing_typing` and `typing_tick`. `_connection` was renamed to `connection`.
  - `send_typing(signals)` sends the frames and reschedules. `schedule_typing_tick` sleeps until the nearest typist expiry or the user's idle deadline, then prunes, sends any idle `End`, and redraws.
  - Typing ends on send (`send_message`), channel switch (`sync_composer`) and starting an edit (`start_edit`).
  - Incoming `ChannelStartTyping` and `ChannelStopTyping` update `typing`. A `Message` from a typist stops them, and `Disconnected` clears everyone.
  - `render_typing_line` draws a 16 px line between the list and the message box, always present and empty when nobody's typing.
- `src/lib.rs`: registers `typing`.
- Tests: `tests/typing.rs` (new), plus 2 unit tests in `events.rs`.

### Divergences
- **Ending typing when an edit starts** was added (not in the plan's list of End triggers). Editing isn't typing a message, and the composer reports no input while editing.
- **The typing timing values (the plan's Open Question)** were confirmed live: 3 s resend, 5 s expiry, 5 s idle. In the log, `Begin` went out on typing and `End` after sending or going idle.

### Verification Performed
- `cargo test --test typing`: 7 passed.
  - Start and stop, kept per channel.
  - Expiry after 5 s, and a refresh extends it while keeping order.
  - `next_expiry`, `prune` and `clear`.
  - Our own user ignored.
  - Labels for 0, 1, 2 and 3 people.
  - `Begin` throttled to once per 3 s.
  - `End` on send (only once), on clear, on channel switch (`End` then `Begin`), and on 5 s idle, with `deadline`.
- `cargo test --lib events`: 15 passed. New: `typing_frames_match_the_protocol` (byte for byte `{"type":"BeginTyping","channel":"ch"}`, plus `EndTyping`) and `typing_events_decode_with_the_channel_as_id`.
- `cargo test --test live_events -- --ignored`: `live_events_handshake_succeeds` passed with the outbound queue.
- Full suite: `cargo test`, 268 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
The user typed in both clients and confirmed the indicators appear and clear both ways ("lgtm"). The log shows `typing: Begin(…)` at 17:58:46 and `typing: End(…)` at 17:58:52.
