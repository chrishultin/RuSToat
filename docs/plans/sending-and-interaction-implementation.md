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
