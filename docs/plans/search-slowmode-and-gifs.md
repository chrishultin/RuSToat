# Plan: Search, slowmode and GIFs (RuSToat milestone 12)

## Summary
Milestone 12 of the [roadmap](roadmap.md) finishes three things Stoat's web client (`stoatchat/for-web` v0.16.3) has:
- **Search:** "Load more" past the first 50 results. The sort buttons (Latest, Oldest, Relevance) already exist.
- **Slowmode:** show a channel's slowmode, and count down after you send.
- **GIFs:** a GIF picker in the message box, backed by Gifbox, Stoat's GIF service. Also, GIFs that Gifbox serves only as video show a still preview in chat, not a file card.

## Approach
**What exists already.**
- **Search.** The search panel (`src/ui/chat.rs`, `start_search`, `render_search_panel` ~5149) already has sort buttons and passes `MessageSort` to `StoatClient::search_messages`. The missing piece is paging. `search_body` always sends `before: None, after: None`, and `SearchState::Results` has no cursor.
- **Slowmode.** `ChatStore` keeps each text channel's `slowmode: Option<u64>` (seconds) up to date (`apply_channel_update`), but nothing reads it.
- **GIFs.** There's no GIF code. The emoji picker (`src/ui/emoji_picker.rs`, opened from a `Popover` in `Composer::render`) is the pattern to copy.

**Search paging uses message-ID cursors.** `before` and `after` in `DataMessageSearch` are exclusive `_id` filters, and the server applies them before sorting.
- **Latest:** the next page is `before = last row's ID`.
- **Oldest:** the next page is `after = last row's ID`.
- **Relevance** sorts by text score, so an ID cursor would skip or repeat results, and there is no offset. Relevance asks for 100 results, the most Stoat allows, and offers no "Load more".
- "There may be more" means the last page came back full (`len == limit`).

**Slowmode follows the server, as for-web does.**
- After every send it allows, the backend sends that user a websocket event, `UserSlowmodes { slowmodes: [{ channel_id, duration, retry_after }] }`. It sends the event again on every (re)connect, with the cooldowns still running. That event drives the countdown.
- A send during the cooldown gets HTTP 429 `{"type":"InSlowmode","retry_after":N}`, in seconds. Today `ApiError::from_response` turns every 429 into `RateLimited`. Slowmode refusals must become `ApiError::Api(ErrorType::InSlowmode { .. })` instead, so they start the countdown.
- People with `ChannelPermission::BypassSlowmode` are exempt. That includes owners, through `GrantAllSafe`. `ManageMessages` doesn't exempt anyone.
- Slowmode only applies to server text channels.
- While a countdown runs, the message box won't send. Your text stays in the box rather than becoming a failed message. The server's 429 covers any race.
- Rejected alternative: **starting the countdown only from the local send time.** That would lose cooldowns across reconnects, and get them wrong when you send from another client. The plan also starts a local cooldown on a successful send, so the countdown shows even if the event is late. The event overrides it.

**GIFs come from Gifbox.** The base URL `https://api.gifbox.me` is built into for-web, not read from the instance. Gifbox endpoints:
- `GET /categories?locale=en_US`, `GET /trending?locale=en_US&limit=N` and `GET /search?locale=en_US&query=…&limit=N`
- authenticated with the account's `X-Session-Token`

The live service was probed on 2026-11-06 with a real token. It returned more than the v0.16.2 source suggests:
- Each result has `id`, `url` (`https://gifbox.me/view/{id}`, which is what gets sent) and `media_formats`.
- `media_formats` always has `preview`: a still, `media.gifbox.me/media/post/{id}/poster`, about 15–25 KB, JPEG or WebP.
- Some results also have `gif` / `mediumgif` (`…/gif_high`, `image/gif`, about 0.4–7 MB). Others have only `mp4` / `webm`.
- Categories are `[{ title, image }]`, where `image` is a `…/gif_high` or `…/gif_low` GIF of several MB.
- `next` is a paging cursor (not used here, as in for-web).
- `media.gifbox.me` needs no token.

GPUI's `img` draws GIFs (animated), WebP and JPEG, but not MP4 or WebM. So:
- Picker tiles show the still `preview`.
- Only the hovered tile plays its `gif`, if it has one. Decoding 50 multi-megabyte GIFs at once (GPUI holds every frame) would cost hundreds of MB.
- Category tiles show the poster of their GIF (the `image` URL with its last path segment replaced by `poster`) with the title over it.

**GIFs in chat.** The probe also showed how Stoat's embed service (January, `proxy.stoatusercontent.com`) handles `gifbox.me/view/{id}` links:
- When a GIF version exists, the link becomes `Embed::Image` with that page URL. RuSToat already draws it, animated, through the proxy.
- Otherwise it becomes `Embed::Video`, which RuSToat shows as an "open in browser" file card.
- This plan draws that video embed as the proxied poster (`…/{id}/poster`) with a "GIF" badge. Playing it waits for milestone 31's AVFoundation work.

## Assumptions
1. **Search** (user decision):
   - Latest and Oldest load 50 at a time, with a "Load more" button at the end of the list. It shows while the last page was full, and is replaced by "Loading…" or an error with Retry.
   - Relevance asks for 100 and has no "Load more". Its count line says "The top 100 matches" when full.
   - The default sort stays Latest.
   - Pins are unchanged (they already fetch the maximum, 100).
2. **Slowmode display** (as for-web's `CompositionInfo.tsx`, adapted to RuSToat's layout):
   - The indicator sits at the right end of the typing line, the strip just above the message box. It shows a clock icon with:
     - "Slowmode 30s" (the channel's delay, formatted as `5s`, `10m`, `1h`, `6h`) normally
     - the remaining time as `m:ss` (or `h:mm:ss`) during your cooldown
     - "Slowmode (exempt)" if you have Bypass Slowmode
   - Its tooltip: "Members can send one message every {duration}."
   - The roadmap says "count down in the message box". The countdown also replaces the send button's icon, so it is in the message box itself. Enter during a countdown shows the composer's hint line: "Slowmode: wait {m:ss}".
3. **Slowmode blocks sending client-side:**
   - Submitting or picking a GIF during a known cooldown does nothing except show the hint. Your text stays.
   - Edits, reactions and uploads aren't affected; the backend only applies slowmode to new messages.
   - A send that still gets `InSlowmode` (a race) fails as today, with the reason "Slowmode: try again in {m:ss}", and starts the countdown. Retry stays available.
4. **The GIF picker only appears for accounts on `https://api.stoat.chat`** (`DEFAULT_API_URL`; user decision). Self-hosted accounts don't get it: their session token would be sent to a third party, which couldn't use it anyway. It also needs `SendEmbeds` in the channel, as in for-web.
5. **Picking a GIF sends it at once** (user decision), as for-web:
   - The message content is the result's `url`.
   - If the reply bar is set, the GIF is sent as that reply and the bar is cleared.
   - The text in the box is left alone. That's why the GIF button stays visible while you type (for-web hides it).
   - The picker closes after a pick.
6. **Picker contents** (as for-web's `GifPicker.tsx`):
   - A search box (debounced 250 ms, lower-cased).
   - With no text: a 2-column grid of category tiles, led by a "Trending" tile.
   - Picking a category searches its title. Trending calls `/trending`.
   - A back button returns to the categories.
   - 50 results per query, with no paging (the `next` cursor is ignored, as for-web).
   - "No GIFs found" when empty.
   - Locale `en_US`.
   - No "powered by Gifbox" explainer card.
7. **Gifbox media loads directly from `media.gifbox.me`,** not through Stoat's proxy, as in for-web. Gifbox is Stoat's own service, and the account's token is already sent to `api.gifbox.me`. The poster in chat (assumption 8) goes through the proxy, like every other embed.
8. **GIFs in chat:** an `Embed::Video` whose URL is `https://gifbox.me/view/{id}` draws the proxied poster at the video's size (capped like other embed images), with a small "GIF" badge. Clicking it opens the link through the safety prompt (`open_external`). Other videos are unchanged.

## Plan

### Chunk 1: Search "Load more"
**Goal:** Latest and Oldest searches can load past the first 50 results, and Relevance shows the top 100.
**Files likely touched:**
- `src/search.rs`:
  - `RESULT_LIMIT` (50) stays the page size for Latest and Oldest. Add `RELEVANCE_LIMIT: i64 = 100`.
  - `fn page_limit(sort) -> i64`.
  - `fn next_cursor(sort, rows_ids) -> Option<SearchCursor>`, where `SearchCursor` is `Before(id)` or `After(id)`, and Relevance is `None`.
  - Update the doc comment that says there's no "load more".
- `src/api/client.rs`: `search_messages` gains `cursor: Option<SearchCursor>`, and `search_body` sets `before`/`after` from it (still null-stripped).
- `src/ui/chat.rs`:
  - `SearchState::Results` carries `rows`, `more: MoreState` (`None | Ready(SearchCursor) | Loading | Failed(SharedString)`).
  - `start_search` uses `page_limit(sort)`.
  - A `load_more_search(cx)` sends the next page, guarded by `seq` as `start_search` is. The task goes in `search_more_task`, which `start_search`, `close_search` and the channel change drop.
  - New rows are built with `store.absorb_results` and `result_rows`, and appended. Drop any ID already present (defensive).
  - `render_result_list` gets an optional footer: a "Load more" button, "Loading…", or the error with "Retry".
  - The count line becomes "{n} matches" or, when more may exist, "{n}+ matches"; Relevance shows "The top 100 matches" when full.
**Depends on:** None.
**Risks:**
- **The result list isn't virtualized** (`render_result_list` maps every row). After several pages, hundreds of preview rows render each frame. If scrolling gets sluggish, move it to `uniform_list` or `list`. Rows vary in height, so it would have to be `list`. Measure with ~300 results first.
- **Exact-body wiremock tests** in `tests/api_search.rs` assert the whole JSON body. They need updating for the new parameter, and must still show no `before`/`after` on a first page.
- **Changing sort while a "Load more" is in flight** must drop it. `set_search_sort` goes through `start_search`, which bumps `seq` and replaces the tasks; check it.
**How to verify:** `cargo test`:
- `src/search.rs` unit tests:
  - `page_limit` is 50 for Latest and Oldest, and 100 for Relevance
  - `next_cursor` gives `Before(last)` for Latest, `After(last)` for Oldest and `None` for Relevance
  - no cursor when the page is short
- `tests/api_search.rs`:
  - a Latest page with a cursor sends `"before":"<id>"` and no `after`
  - Oldest sends `"after"`
  - a first page sends neither
  - Relevance sends `"limit":100`
**Manual check:** Search a busy channel for a common word, sorted by Latest. Click "Load more" until it disappears, and check the results continue in date order with no repeats. Do the same with Oldest. Relevance shows up to 100 results and no button. Switching sort or closing the panel mid-load doesn't add stray rows.
**Shippable when:** Latest and Oldest page through all matches, Relevance shows the top 100, and the tests pass.

### Chunk 2: Show a channel's slowmode
**Goal:** Text channels with slowmode show it beside the message box, and exempt users see that they're exempt.
**Files likely touched:**
- `src/permissions.rs`: `can_bypass_slowmode(store, user, channel)` using `has(perms, ChannelPermission::BypassSlowmode)`, next to `can_pin`.
- `src/store.rs`:
  - `slowmode(channel) -> Option<u64>`: `Some(secs)` only for a `TextChannel` with `slowmode > 0`
  - `can_bypass_slowmode(channel)`, as the `can_pin` wrapper (~2317)
- `src/time.rs`:
  - `format_duration_short(secs)` gives "5s", "10m", "1h 30m", "6h"
  - `format_countdown(secs)` gives "0:24", "12:05", "1:00:00"
  - put them here, or in a new `src/slowmode.rs` if chunk 3 grows the module
- `src/ui/chat.rs`:
  - `render_typing_line` becomes an `h_flex`: typing text on the left (truncating), and on the right `render_slowmode_badge(channel)` (clock icon, "Slowmode 30s" or "Slowmode (exempt)", with a tooltip).
  - Nothing shows when the channel has no slowmode.
  - A live `ChannelUpdate` changing slowmode re-renders already, through `cx.notify()`; confirm it.
- `src/assets.rs`: a clock icon, if gpui-component lacks one (check `IconName` first).
**Depends on:** None.
**Risks:**
- **The typing line is a fixed height** (`TYPING_LINE_HEIGHT`). The badge must fit it at `text_xs`, and must not push the typing text off when both show.
- **Permission recomputation per render.** `channel_permissions` runs the crate's calculation under `block_on`. It's cheap, but it happens on every render. If profiling shows it, compute once per `sync_composer`/channel change and cache it on `ChatView`.
**How to verify:** `cargo test`:
- `tests/permissions.rs`:
  - an owner bypasses
  - a member with a role granting `BypassSlowmode` bypasses
  - a default member doesn't
  - a channel override denying it wins over the role
  - `ManageMessages` alone doesn't bypass
- Store tests (`tests/store_slowmode.rs`, new): `slowmode()` for a text channel with 30, with 0, with none, after a `ChannelUpdate` setting it, after `remove: ["Slowmode"]`, and `None` for DMs and groups.
- `time.rs` formatting unit tests, including 0, 59, 60, 3600 and 21600.
**Manual check:** In the web client, set a test channel's slowmode to 30 seconds. RuSToat shows "Slowmode 30s" by the message box without a reload, with the tooltip. As the server owner you see "Slowmode (exempt)". Turning it off removes the badge live.
**Shippable when:** the badge reflects each channel's slowmode and your exemption live, and the tests pass.

### Chunk 3: Slowmode countdown
**Goal:** After you send in a slowmode channel, the message box counts down and won't send until the cooldown ends.
**Files likely touched:**
- `src/api/events.rs`: `ServerEvent::UserSlowmodes(Vec<ChannelSlowmode>)`, decoding `revolt_models::v0::ChannelSlowmode` (`channel_id`, `duration`, `retry_after` in seconds). Today it falls to `Unknown`.
- `src/api/error.rs`: in `from_response`, a 429 whose body has `"type":"InSlowmode"` becomes `ApiError::Api(ErrorType::InSlowmode { retry_after })`. Any other 429 stays `RateLimited`. Add an `ApiError::slowmode_retry_after() -> Option<u64>` helper.
- `src/store.rs` (or `src/slowmode.rs` with pure logic, as `src/typing.rs`):
  - `cooldowns: HashMap<String, u64 /* until_ms */>`
  - `apply_slowmodes(list, now_ms)`, which sets `until = now + retry_after*1000`
  - `start_cooldown(channel, secs, now_ms)`
  - `cooldown_left(channel, now_ms) -> Option<u64 /* secs, rounded up */>`, which prunes expired entries
  - `next_cooldown_end()`, for the ticker
- `src/ui/chat.rs`:
  - Handle `ServerEvent::UserSlowmodes` for the right account's store.
  - On a successful send (`post_pending`'s success arm): if `store.slowmode(ch)` is `Some(secs)` and you can't bypass, `start_cooldown(ch, secs, ulid_millis(message.id))`. The event, when it arrives, overrides it.
  - On a failed send with `InSlowmode`: `start_cooldown(ch, retry_after, now)`, with the failure reason "Slowmode: try again in {m:ss}".
  - `send_error_text` gains the `InSlowmode` arm.
  - `schedule_slowmode_tick`: a one-second repeating `Task` while the open channel has a cooldown, modelled on `schedule_typing_tick`. It stops when the cooldown ends, and is re-armed on channel switch and on new cooldowns.
  - `sync_composer` pushes the state into the composer each tick.
  - The badge shows the countdown during a cooldown.
- `src/ui/composer.rs`:
  - `set_send_block(Option<SharedString>)`: the countdown text, or `None`.
  - While blocked:
    - `submit` doesn't clear or send, and sets the hint "Slowmode: wait {m:ss}"
    - the send button is disabled and shows the countdown text instead of its icon
  - Attachments queued while blocked stay queued.
**Depends on:** Chunk 2.
**Risks:**
- **The event's unit and timing.** `retry_after` is seconds (backend `SET … EX`). The event comes before the HTTP response, or soon after. Starting the local cooldown on success covers a late event; an event arriving after the local start resets it to the server's figure, which is correct.
- **Multiple accounts.** Cooldowns live in each account's `ChatStore`, so another account's event never blocks this one. Check that the event goes to the store the socket belongs to.
- **Existing 429 handling.** Other code paths (history paging, `chat.rs` ~3718/3770) retry on `RateLimited`. An `InSlowmode` only comes from sending, but check nothing else matches 429 by status alone.
- **The composer re-renders every second during a cooldown.** Notify only the composer and the typing line's owner. Don't resync the timeline.
**How to verify:** `cargo test`:
- `tests/live_events.rs`: a `UserSlowmodes` frame decodes into `ServerEvent::UserSlowmodes` with its fields.
- `tests/api_send.rs`:
  - a 429 `{"type":"InSlowmode","retry_after":12}` from `send_message` is `ApiError::Api(InSlowmode { retry_after: 12 })`
  - a 429 with only `{"retry_after":1200}` is still `RateLimited { retry_after_ms: Some(1200) }`
- `tests/store_slowmode.rs`:
  - `apply_slowmodes` then `cooldown_left` counts down and expires
  - a later event replaces an earlier cooldown
  - `start_cooldown` from a ULID timestamp
  - cooldowns are per channel
- `src/ui/composer.rs`: if the composer has pure helpers (`check_send`), add a case that a blocked submit keeps the text. Otherwise cover it by the manual check.
**Manual check:**
- In a 30 s slowmode channel, as a non-exempt member (use the second account), send a message. The badge and the send button count down from 0:30.
- Enter during the countdown shows the hint and keeps your text. Sending works again at 0:00.
- Relaunch mid-countdown: the countdown resumes, from the server's reconnect event.
- Send from the web client: RuSToat's countdown starts too.
- As the owner, nothing counts down.
**Shippable when:** sending in a slowmode channel counts down in the message box and blocks until it ends, refusals start the countdown, and the tests pass.

### Chunk 4: Gifbox client and GIF picker
**Goal:** A GIF button in the message box opens a picker with categories, trending and search. Picking a GIF sends it.
**Files likely touched:**
- `src/api/gifbox.rs` (new, `pub mod gifbox` in `src/api/mod.rs`):
  - `GIFBOX_URL = "https://api.gifbox.me"`
  - `GifboxClient::new(base, token)`, with its own `reqwest::Client` (30 s timeout, the same user agent as `StoatClient`), sending `X-Session-Token`
  - `categories()`, `trending(limit)`, `search(query, limit)`, with locale `en_US`
  - types:
    - `GifCategory { title, image }`
    - `GifResult { id, url, media_formats: HashMap<String, GifMedia> }`
    - `GifMedia { url, dimensions: Option<[u32; 2]> }`
    - `GifPage { results, next: Option<String> }`
  - helpers:
    - `GifResult::still()`: `preview`, else `None`
    - `GifResult::animated()`: `gif`, else `mediumgif`, else `None`
    - `GifResult::size()`
    - `category_poster(image_url)`: replaces the last path segment with `poster`
  - errors reuse `ApiError` (`from_response` handles Gifbox's `{"type":"NotAuthenticated"}` bodies and 429s)
- `src/ui/gif_picker.rs` (new, `pub mod` in `src/ui/mod.rs`): `GifBrowser`, modelled on `EmojiBrowser`.
  - A search `Input` (focused on open, cleanable, debounced 250 ms through a replaceable `Task`) and a back button when filtered.
  - The body:
    - categories: a 2-column grid, with a "Trending" tile first, then poster tiles labelled with their titles
    - results: a 2-column grid of fixed-height tiles; each tile's `img(still)` is sized from `dimensions`
    - plus states for loading, "No GIFs found" and errors (with Retry)
  - Hovering a result swaps its `img` to the animated URL, if any. Track `hovered: Option<usize>`.
  - Scrolling is `overflow_y_scroll`. 50 tiles don't need virtualizing.
  - Stale responses are dropped by a request `seq`, as in search.
  - `on_pick(url)`.
  - Size: about 400×400, like for-web.
- `src/ui/composer.rs`:
  - A `Popover::new("gif-picker")` with a ghost "GIF" button, beside the emoji button, mirroring `emoji_picker` (`gif_picker: Option<Entity<GifBrowser>>`, `open_gif_picker`, `close_gif_picker`, `ComposerEvent::OpenGifPicker`).
  - `set_gifs_available(bool)` shows or hides the button.
  - A pick emits `ComposerEvent::SendGif { url, replies }`, taking the reply intents and leaving the text alone.
  - Blocked by `set_send_block`, as submit is.
  - Only one of the emoji and GIF pickers is open at a time.
- `src/ui/chat.rs`:
  - Build a `GifboxClient` per account, only when the account's API URL is `DEFAULT_API_URL`, using the session token.
  - `sync_composer` sets `gifs_available` when there is a client and the user has `SendEmbeds` in the open channel. A `can_send_embeds` wrapper goes in `permissions.rs` and `store.rs`.
  - `ComposerEvent::OpenGifPicker` creates the browser with the client.
  - `ComposerEvent::SendGif` calls the existing `send_message(channel, url, replies, vec![], …)`.
**Depends on:** Chunk 3, for the send block. Without it, drop that line and the chunk stands alone.
**Risks:**
- **Memory.** One hovered GIF can be 7 MB compressed, with every frame decoded. Only one animates at a time, but GPUI's asset cache keeps decoded images after hovering. Watch memory after hovering 30 tiles. If it grows without bound, give the picker its own `ImageCache` (gpui `image_cache(...)` on the browser's root), dropped when the picker closes.
- **Poster format.** The poster is JPEG bytes served as `image/webp`. GPUI should sniff the format from the bytes; confirm it draws. If not, use the proxied poster URL, which the proxy re-encodes as real WebP.
- **The Gifbox API isn't versioned or documented.** The live response already differs from the v0.16.2 source (more formats, `next`). Parse leniently: every `media_formats` entry is optional, and unknown fields are ignored. A result with no `preview` gets a text tile with its ID trimmed, rather than being dropped.
- **Rate limits** (about 2/10 s for categories, 10/10 s for search). Debounce typing. Cache categories and trending in the browser entity for the session, so reopening the picker doesn't refetch. A 429 shows "Too many searches, try again in a moment."
- **The token goes to a second host.** Keep it to `DEFAULT_API_URL` accounts (assumption 4), and add a test that a client isn't built for other instances.
**How to verify:** `cargo test`, with a new `tests/api_gifbox.rs` (wiremock):
- `categories` sends `X-Session-Token` and `locale=en_US`, and parses the array
- `trending` and `search` send `limit` and `query`, URL-encoded with spaces and `&`, and parse the probed shape from a fixture (`tests/fixtures/gifbox_trending.json`, copied from the 2026-11-06 probe)
- `still()` and `animated()` choices, including a result with no `gif`
- `category_poster`
- a 401 `NotAuthenticated` becomes `ApiError::Api`
- a 429 becomes `RateLimited`
- a unit test that a self-hosted API URL gets no Gifbox client
**Manual check:**
- On an `api.stoat.chat` account, the GIF button shows beside the emoji button. It doesn't show on a self-hosted account, or where you lack Send Embeds.
- Open it: category posters and "Trending" show.
- Search "cat": stills appear, and hovering one with a GIF animates it.
- Click one: it sends at once, the picker closes, and the GIF shows in the channel, animated if it has a GIF version.
- With a reply set, the GIF is sent as that reply.
- Text in the box is kept.
- Check Activity Monitor after browsing a few searches.
**Shippable when:** official-instance accounts can search and send GIFs from the message box, and the tests pass.

### Chunk 5: Gifbox video GIFs in chat
**Goal:** Gifbox GIFs that only exist as video show as a still with a "GIF" badge, not as a file card.
**Files likely touched:**
- `src/embed.rs`:
  - `EmbedView::GifPoster(EmbedImage)` (or `Image` with a `gif_badge: bool`)
  - In the `Embed::Video` arm: when `gifbox_id(&video.url)` matches `https://gifbox.me/view/{id}`, build an `EmbedImage` from `https://media.gifbox.me/media/post/{id}/poster` through `media.external` (the proxy), sized from the video's `width`/`height` with the usual cap.
  - `gifbox_id(url) -> Option<&str>`.
- `src/ui/chat.rs`:
  - `render_embed` draws `GifPoster` like `viewable_embed_image`, with a small "GIF" badge in a corner.
  - A click sends `RowAction::OpenLink { url: page URL }`, which goes through `open_external`.
  - URL-only messages still show their link text, as other GIF embeds do today.
**Depends on:** None (independent of chunk 4, but most useful after it).
**Risks:**
- **January's choice between `Image` and `Video` may change.** Today it is `Image` when Gifbox has a GIF version, else `Video`. The `Image` path already works. This only changes the `Video` path for Gifbox URLs, so it can't break other videos.
- **Poster URL guessing.** `…/poster` was present for every probed result, but it's an undocumented path. If it 404s, `inline_image` already shows its "Couldn't load image" fallback, so nothing breaks badly.
**How to verify:** `cargo test`, in `tests/store_embeds.rs`:
- a `Video` embed for `https://gifbox.me/view/abc` becomes a poster through the proxy with the badge
- a `Video` embed for another host is still `Video`
- an `Image` embed for a Gifbox page is unchanged
- `gifbox_id` rejects lookalike hosts (`gifbox.me.evil.com`, `notgifbox.me`)
**Manual check:** Send `https://gifbox.me/view/smGy30kWfDtNbJyWRnq8a`, which is video-only. After the embed arrives, it shows a still with a "GIF" badge, and clicking it asks before opening the browser. `https://gifbox.me/view/cXQwxRj5VVmb4q_qmUAg6` animates as before.
**Shippable when:** video-only Gifbox GIFs show as badged stills, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`.
- A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines (from `rustfmt --edition 2024 --check <file>`) hasn't grown. Record each touched file's starting count before chunk 1.

### Unit Tests
- **`search.rs`:** `page_limit`, `next_cursor` for each sort, a short page.
- **`time.rs`** (or `slowmode.rs`): duration and countdown formatting; cooldown arithmetic (rounding up, expiry).
- **`api/gifbox.rs`:** `still()`, `animated()`, `category_poster`, lenient parsing of a result missing formats.
- **`embed.rs`:** `gifbox_id`.

### Integration Tests
- **`tests/api_search.rs`:** cursor bodies for Latest and Oldest, Relevance limit 100, no cursor on a first page.
- **`tests/api_send.rs`:** `InSlowmode` 429 versus a plain 429.
- **`tests/live_events.rs`:** `UserSlowmodes` decoding.
- **`tests/permissions.rs`:** `BypassSlowmode` from owner, role and override; `ManageMessages` alone doesn't bypass.
- **`tests/store_slowmode.rs`** (new): `slowmode()` across channel kinds and updates; cooldown lifecycle.
- **`tests/api_gifbox.rs`** (new): headers, query encoding, response parsing from the probed fixture, error mapping.
- **`tests/store_embeds.rs`:** Gifbox video poster.

### Manual Tests
You need an `api.stoat.chat` account that owns a test server, plus a second, non-owner account in it.
1. Run every chunk's manual check.
2. Slowmode with two accounts signed in to RuSToat at once: the member's countdown never blocks the owner's message box, and switching accounts mid-countdown shows each account's own state.
3. A GIF picked during a slowmode countdown is blocked with the hint, as typed messages are.
4. Light and dark themes: the slowmode badge, the countdown on the send button, the GIF picker tiles and the "GIF" badge are readable.
5. A self-hosted account (if available) shows the slowmode features but no GIF button.

## Definition of Done
- [ ] Latest and Oldest searches show "Load more" while the last page was full, and page with `before`/`after` cursors without duplicates. Relevance fetches 100 results and has no "Load more".
- [ ] A text channel with slowmode shows "Slowmode {duration}" by the message box, or "Slowmode (exempt)" for users with Bypass Slowmode, and updates live when the slowmode changes.
- [ ] After a send in a slowmode channel, the badge and send button count down, sourced from `UserSlowmodes` events (and a local start on success). The box won't send until 0:00, and the typed text is kept.
- [ ] A 429 `InSlowmode` is parsed as `ErrorType::InSlowmode` and starts the countdown. Other 429s are still `RateLimited`.
- [ ] Accounts on `api.stoat.chat` with Send Embeds get a GIF button. The picker shows categories, trending and search results as stills, animates the hovered GIF, and sends the GIF's `gifbox.me/view/…` URL immediately on pick, as a reply when one is set.
- [ ] No Gifbox request is made for accounts on other instances.
- [ ] Video-only Gifbox embeds show as badged stills that open through the link safety prompt.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/search-slowmode-and-gifs-implementation.md`.
