# Implementation Record: Search, slowmode and GIFs (milestone 12)

_Plan: docs/plans/search-slowmode-and-gifs.md_
_Started: 2026-11-06_

Run in continuous mode. The user chose to run every chunk's manual check together at the end, so each chunk's "Manual Validation" says pending until then.

The plan and its roadmap link were committed first ("Plan milestone 12: search, slowmode and GIFs"). Already dirty before this work, and kept out of every commit: `.gitignore` (a duplicated Riptide block) and `.shadow/` (untracked).

Starting rustfmt `Diff in` counts (`rustfmt --edition 2024 --check <file>`): `src/search.rs` 0, `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/permissions.rs` 0, `src/store.rs` 15, `src/time.rs` 0, `src/assets.rs` 0, `src/api/events.rs` 3, `src/api/error.rs` 0, `src/ui/composer.rs` 14, `src/api/mod.rs` 7 and `src/ui/mod.rs` 34 (both include their child modules), `src/embed.rs` 0, `tests/api_search.rs` 0, `tests/permissions.rs` 0, `tests/live_events.rs` 0, `tests/api_send.rs` 2, `tests/store_embeds.rs` 0.

## Chunk 1: Search "Load more"

### Changes Made
- `src/search.rs`:
  - `RELEVANCE_LIMIT = 100`. `RESULT_LIMIT` (50) is now documented as the Latest/Oldest page size.
  - `SearchCursor::{Before(id), After(id)}`.
  - `page_limit(&sort)`: 50 for Latest and Oldest, 100 for Relevance.
  - `next_cursor(&sort, &ids)`: `Before(last)` for Latest, `After(last)` for Oldest, when the page is full. `None` for a short page or Relevance.
  - 4 unit tests.
- `src/api/client.rs`: `search_messages` takes `cursor: Option<SearchCursor>`. `search_body` turns it into `before` or `after`, still dropping nulls. Its unit tests pass `None`.
- `src/ui/chat.rs`:
  - `SearchState::Results { rows, more }`, with `MoreState::{None, Ready(cursor), Loading, Failed(cursor, error)}`.
  - `start_search` asks for `page_limit(sort)` with no cursor.
  - `search_page` absorbs a page and returns its rows plus `MoreState`. The cursor comes from the messages as Stoat sent them, before `result_rows` drops any from other channels.
  - `load_more_search` asks for the next page. It is guarded by the panel's `seq`, and appends new rows, skipping IDs already shown. A failure keeps the cursor, for Retry.
  - `render_result_list` takes an optional footer: a "Load more" button, "Loading…", or the error with "Retry". Pins pass `None`.
  - Count line:
    - "The top N matches" for a full Relevance search
    - "N+ matches" while more may exist
    - otherwise "1 match" or "N matches"
- `tests/api_search.rs`: existing calls pass `None`. The first test's exact body still shows no `before`/`after` on a first page. New tests:
  - a Latest cursor sends `before` only
  - an Oldest cursor sends `after` only
  - Relevance sends `limit: 100`

### Divergences
- Minor: no separate `search_more_task`. "Load more" reuses `search_task`, which can't be in use then, because "Load more" only shows once the first page has landed. Every place that drops a search already clears `search_task` and bumps `search_seq`: `start_search` replaces it, and `close_search`, the channel change (`sync_side_panel`), the too-long path and `start_pins` clear it. A change of sort goes through `search_for` → `start_search`, so it drops an in-flight "Load more" too.
- Minor: `MoreState::Failed` keeps the cursor (`Failed(cursor, error)`) so Retry can resend, and `Loading` is a unit variant. The plan had `Failed(SharedString)` and no cursor on the states.
- Minor: added a `search_page` helper next to `result_rows`, which pins still use unchanged.

### Verification Performed
- `cargo test`: 688 passed, 0 failed (5 ignored). This includes the 4 new `search.rs` unit tests and the 3 new `tests/api_search.rs` tests.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts are unchanged: `src/search.rs` 0, `src/api/client.rs` 3, `src/ui/chat.rs` 17 (identical diff to HEAD), `tests/api_search.rs` 0.
- The list is still not virtualized (plan risk). Measuring with ~300 results is part of the manual check.

### Manual Validation
Pending (batched to the end). Also check scrolling with ~300 results (plan risk: the result list isn't virtualized).

## Chunk 2: Show a channel's slowmode

### Changes Made
- `src/permissions.rs`: `can_bypass_slowmode(store, user, channel)`: a server channel and `BypassSlowmode` (owners have it through `GrantAllSafe`). Manage Messages doesn't count.
- `src/store.rs`:
  - `slowmode(channel) -> Option<u64>`: `Some(secs)` only for a `TextChannel` with `slowmode > 0`.
  - `can_bypass_slowmode(channel)`, the logged-in user's wrapper beside `can_pin`.
- `src/time.rs`:
  - `format_duration_short(secs)`: "5s", "10m", "1m 30s", "1h 30m", "6h". Hours leave out the seconds.
  - `format_countdown(secs)`: "0:24", "12:05", "1:00:00".
  - Unit tests for both, including 0, 59, 60, 3600 and 21600.
- `src/assets.rs`: `Clock` added to the bundled extra icons (gpui-kit has the SVG but doesn't bundle it by default), plus a test that it loads.
- `src/ui/chat.rs`:
  - `render_typing_line` is an `h_flex`: the typing text on the left (`flex_1`, `min_w_0`, truncating), and `render_slowmode_badge` on the right (`flex_none`).
  - The badge is a clock icon with "Slowmode {duration}" or "Slowmode (exempt)". Its tooltip is "Members can send one message every {duration}.". There is no badge without slowmode.
  - Live changes: `ChannelUpdate` handling falls through to the `cx.notify()` at the end of `on_connection_event`, so the badge updates without a reload. The same applies to role and member updates, since the badge reads permissions each render.
- `tests/permissions.rs`: `bypassing_slowmode_takes_its_own_permission` covers:
  - the owner bypasses
  - a role granting it bypasses
  - a channel override denying it wins over the role
  - a plain member doesn't bypass, checked directly and through the store wrapper
  - `ManageMessages` alone doesn't bypass

  `nobody_bypasses_slowmode_outside_a_server` covers DMs, groups and saved notes.
- `tests/store_slowmode.rs` (new): `slowmode()` for a channel with 30, 0 or none set, and for a group, a DM, saved notes and an unknown channel. Also after a `ChannelUpdate` that sets it, one that sets it to 0, `clear: [Slowmode]`, and a parsed live `ChannelUpdate` frame with `"clear": ["Slowmode"]`.

### Divergences
- Minor: the formatting helpers went in `src/time.rs`, as the plan's first option. Chunk 3's cooldown logic hasn't been placed yet.
- Minor: `format_duration_short` also gives "1m 30s" for durations under an hour with leftover seconds. The plan listed only whole units and "1h 30m".
- The permission recomputation per render (plan risk) is left uncached. It is one `channel_permissions` call per render of the open channel.

### Verification Performed
- `cargo test --test store_slowmode --test permissions`: 4 + 24 passed.
- `cargo test`: 697 passed, 0 failed (5 ignored). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts are unchanged: `src/permissions.rs` 0, `src/store.rs` 15, `src/time.rs` 0, `src/assets.rs` 0, `src/ui/chat.rs` 17 (identical diff to HEAD), `tests/permissions.rs` 0. The new `tests/store_slowmode.rs` is formatted and has 0.

### Manual Validation
Pending (batched to the end). This includes checking that the badge fits the 16 px typing line and doesn't push the typing text off when both show.

## Chunk 3: Slowmode countdown

### Changes Made
- `src/api/events.rs`: `ServerEvent::UserSlowmodes(Vec<ChannelSlowmode>)` decodes `{"type":"UserSlowmodes","slowmodes":[…]}` (it used to fall to `Unknown`) and is forwarded to the view. A unit test covers decoding, including an empty list.
- `src/api/error.rs`:
  - `from_response` turns a 429 whose body parses as a Stoat error with `InSlowmode` into `ApiError::Api(InSlowmode { retry_after })`. Any other 429 stays `RateLimited`.
  - New `slowmode_retry_after()`.
  - `describe` reads "Slowmode: try again in {m:ss}".
- `src/slowmode.rs` (new, pure, like `typing.rs`): `Cooldowns`, holding channel → until (ms):
  - `apply(list, now)` replaces only the listed channels; `retry_after` 0 ends a cooldown
  - `start(channel, secs, from)`
  - `left(channel, now)` gives whole seconds, rounded up
  - `prune(now)`
  - `ends(channel, now)`
  - 3 unit tests
- `src/store.rs`: `ChatStore.cooldowns` (kept across a new `Ready`), with `apply_slowmodes`, `start_cooldown`, `cooldown_left`, `prune_cooldowns` and `cooldown_ends`.
- `src/ui/chat.rs`:
  - `UserSlowmodes` events go into this account's store (one `ChatView` per account, each with its own connection, store and composer), then re-arm the tick.
  - `post_pending`'s success arm starts a local cooldown from the message's ULID time when the channel has slowmode and the user can't bypass it.
  - A failure with `InSlowmode` starts a cooldown of `retry_after` from now. Its reason comes from `send_error_text`, which already falls through to the error's text, "Slowmode: try again in 0:12". Retry stays available.
  - `sync_send_block`, called from `sync_composer` on every render, pushes the open channel's countdown into the composer, and arms the tick if needed. `send_block_left` returns nothing when slowmode is now off or the user is exempt.
  - `schedule_slowmode_tick` is a one-shot timer, re-armed each time, that wakes when the shown second changes (`(ends - now) % 1000`). It prunes, re-arms and notifies, and goes away when the open channel has nothing left.
  - The badge shows the countdown ("0:24") during a cooldown.
  - A `now_ms()` helper returns `u64`.
- `src/ui/composer.rs`:
  - `send_block: Option<SharedString>` and `set_send_block`.
  - A blocked `submit` keeps the text, replies and files, and sets the hint "Slowmode: wait {m:ss}". The hint follows the countdown and goes at 0:00.
  - While blocked, the send button is disabled and shows the countdown instead of its icon, with a "Slowmode" tooltip.
  - Edits aren't blocked.
- `tests/api_send.rs`: a 429 `{"type":"InSlowmode","retry_after":12,…}` from `send_message` is `InSlowmode { retry_after: 12 }`, with readable text. The existing plain-429 test also checks `slowmode_retry_after()` is `None`.
- `tests/store_slowmode.rs`: 5 more tests:
  - an event counts down and expires
  - a later event replaces a local cooldown
  - a cooldown can start from a ULID's time
  - cooldowns are per channel, and an event leaves unlisted channels alone
  - cooldowns survive a new `Ready`

### Divergences
- Minor: the `UserSlowmodes` decode test is a unit test in `src/api/events.rs`, with the other frame-decoding tests, not in `tests/live_events.rs`, which only holds ignored tests against the live service.
- Minor: `next_cooldown_end()` became `cooldown_ends(channel, now)`. The ticker only counts down the open channel, so it needs that channel's end time to wake on whole-second boundaries.
- Minor: `cooldown_left` doesn't prune; it takes `&self` so render can call it. The tick calls `prune_cooldowns`.
- Minor: an event *merges* into what's known (only listed channels change), rather than replacing all cooldowns. A local cooldown the server doesn't list just runs out on its own.
- Minor: `send_error_text` needed no new arm: `InSlowmode` already falls through to `err.to_string()`, which `describe` now makes readable.
- Minor: the tick re-renders `ChatView` (as the typing tick does). It doesn't call `sync_pane`, so the timeline isn't resynced.
- No pure composer helper for "a blocked submit keeps the text". `submit` is GPUI-bound, so this is left to the manual check, as the plan allowed.

### Verification Performed
- `cargo test`: 707 passed, 0 failed (5 ignored). `cargo clippy --all-targets -- -D warnings`: clean.
- No other code matches 429 by status alone. `chat.rs`'s retries match `ApiError::RateLimited`, which slowmode refusals no longer are.
- rustfmt `Diff in` counts are unchanged:
  - `src/api/events.rs` 3 (its import was hand-wrapped to match)
  - `src/api/error.rs` 0
  - `src/store.rs` 15 (the new imports sit inside its already-drifted import hunk)
  - `src/ui/chat.rs` 17
  - `src/ui/composer.rs` 14 (identical diff)
  - `tests/api_send.rs` 2
  - new `src/slowmode.rs` and `tests/store_slowmode.rs` are formatted, with 0

### Manual Validation
Pending (batched to the end).

## Chunk 4: Gifbox client and GIF picker

### Changes Made
- `src/api/gifbox.rs` (new, `pub mod gifbox` in `src/api/mod.rs`):
  - `GIFBOX_URL`, `GIF_LIMIT = 50` and locale `en_US`.
  - `GifboxClient`: its own `reqwest::Client` (30 s timeout, the `StoatClient` user agent), sending `x-session-token`.
    - `for_account(api_url, token)` returns `Some` only for `DEFAULT_API_URL`, ignoring a trailing slash.
    - `categories()`, `trending(limit)` and `search(query, limit)`. Query strings are built with `serde_urlencoded`, because reqwest 0.13's `.query()` needs a feature the crate doesn't enable.
  - Types: `GifCategory { title, image }`; `GifResult { id, url, media_formats }`; `GifMedia { url, dimensions }`, where `dimensions` also accepts `dims`; `GifPage { results, next }`.
  - Lenient parsing:
    - results and categories that don't parse are skipped
    - a media format that doesn't parse is dropped
    - a result with no formats is kept
    - a non-array categories response gives none, as for-web does
    - `next` may be a string or a number
  - Helpers:
    - `still()` (`preview`), `animated()` (`gif`, else `mediumgif`) and `size()`
    - `category_poster`, which uses the `url` crate to replace the last segment with `poster` and drop the query
  - 5 unit tests, including that a self-hosted or lookalike API URL gets no client.
- `src/ui/gif_picker.rs` (new): `GifBrowser`.
  - Search field: focused on open and cleanable. Its text is trimmed and lower-cased, then debounced 250 ms through a replaceable `Task`. Enter skips the wait.
  - A back button shows while a query is shown.
  - Categories page: a 2-column grid with a "Trending" tile first, then poster tiles with the title over a dark scrim.
  - Clicking a category puts its title in the field and searches it. A change event for text the picker set itself is ignored, by comparing with `typed`.
  - Results: a 2-column grid of 120 px tiles. Each shows the still, swapped for the animated GIF while hovered (`hovered: Option<usize>`), or the GIF's ID when there's no still.
  - States for loading, "No GIFs found" and errors with Retry. A 429 says "Too many searches, try again in a moment."
  - Stale answers are dropped by `seq`.
  - `GifCache` (categories and trending) is shared per account, so reopening the picker doesn't refetch.
  - The picker is 400 px wide, and its body is 340 px tall and scrolls.
- `src/ui/composer.rs`:
  - New events `ComposerEvent::OpenGifPicker` and `SendGif { url, replies }`.
  - New state: `gif_picker` and `gifs_available`.
  - `set_gifs_available`; `open_gif_picker(client, cache)`, which closes the emoji picker; `close_gif_picker`.
  - `pick_gif`:
    - takes the reply intents, emits `SendGif` and closes the picker
    - leaves the text alone
    - during a slowmode cooldown, sets the "Slowmode: wait" hint instead of sending
  - A "GIF" ghost button in a `Popover`, before the emoji button. It is shown only when available and disabled while editing.
  - Opening the emoji picker closes the GIF picker.
  - Esc closes the GIF picker first.
  - `emoji_picker_open()` became `picker_open()`, covering both pickers, for Up in the box and `ChatView::shortcuts_blocked`.
- `src/ui/chat.rs`:
  - `Gifs { client, cache }` per `ChatView` (per account), built in `new` only through `GifboxClient::for_account`.
  - `OpenGifPicker` opens it.
  - `SendGif` calls the existing `send_message(channel, url, replies, vec![], …)`, so a GIF goes through the pending, retry and slowmode paths like any message.
  - `sync_composer` sets `gifs_available` on every render (when there's a client and `can_send_embeds`), so permission changes apply live.
- `src/permissions.rs` and `src/store.rs`: `can_send_embeds`.
- `tests/api_gifbox.rs` (new, wiremock): 6 tests.
  - categories send the token and locale, parse the list and skip a bad item
  - non-list categories give none
  - trending sends `limit` and parses the fixture
  - search sends `limit` and escapes `&`/spaces (`cats+%26+dogs`)
  - a 401 `NotAuthenticated` is `Unauthorized`
  - a 429 is `RateLimited`
- `tests/fixtures/gifbox_trending.json` (new).
- `tests/permissions.rs`: `sending_embeds_follows_its_permission`.

### Divergences
- Fixture: the plan says to copy `tests/fixtures/gifbox_trending.json` from the 2026-11-06 probe, but that output wasn't available in this session. The fixture follows the shape the plan records:
  - `id`, `url` and `media_formats`
  - `preview` posters
  - `gif`/`mediumgif` at `gif_high`
  - a video-only result
  - `next`

  It also adds a result with no formats and one missing its `url`. The `dims` key for dimensions is a guess from Tenor's format; `dimensions` is accepted too. **Worth replacing with a real response.**
- Minor: a 401 `NotAuthenticated` from Gifbox becomes `ApiError::Unauthorized`, not `ApiError::Api` (`from_response` already maps it). The picker shows it as "Couldn't load GIFs." and doesn't log the account out.
- Minor: the cache lives in a shared `Rc<RefCell<GifCache>>` held by `ChatView`, not in the browser entity. The browser is recreated on each open, as the emoji picker is, so an entity cache wouldn't survive a reopen.
- Minor: the GIF button is disabled while editing a message (as the attach button is).
- Minor: picking a category puts its title in the search field, and Trending leaves the field empty with the back button showing. for-web puts "trending" in the field.
- for-web today previews with `tinywebm`, and doesn't send `limit`. The plan's stills plus hover-GIF approach was kept, since GPUI can't draw WebM.
- `GifResult::size()` is tested but not used by the picker: tiles are a fixed 120 px and the image is `ObjectFit::Contain`.

### Verification Performed
- `cargo test`: 719 passed, 0 failed (5 ignored). This includes 5 `gifbox` unit tests, 6 `tests/api_gifbox.rs` tests and 1 new permissions test.
- `cargo clippy --all-targets -- -D warnings`: clean, after folding the Enter check into a match guard.
- rustfmt `Diff in` counts are unchanged:
  - `src/ui/chat.rs` 17
  - `src/ui/composer.rs` 14 (the `gif_picker` import sits inside its already-drifted import hunk)
  - `src/store.rs` 15
  - `src/permissions.rs` 0
  - `tests/permissions.rs` 0
  - new `src/api/gifbox.rs`, `src/ui/gif_picker.rs` and `tests/api_gifbox.rs` are formatted, with 0

### Manual Validation
Pending (batched to the end). The plan's risks to check by hand:
- memory after hovering ~30 tiles (if it grows without bound, give the picker its own `ImageCache`)
- whether the JPEG-bytes-as-`image/webp` poster draws (if not, use the proxied poster)
- the popover's size and placement

## Chunk 5: Gifbox video GIFs in chat

### Changes Made
- `src/embed.rs`:
  - `EmbedView::GifPoster { image: EmbedImage, link: String }`.
  - `pub fn gifbox_id(url) -> Option<&str>`:
    - matches only `https://gifbox.me/view/{id}`
    - the ID is ASCII letters, digits, `_` and `-`
    - a trailing `/`, query or fragment is ignored
  - The `Embed::Video` arm tries `gif_poster` first:
    - for a Gifbox page it builds an `EmbedImage` of `https://media.gifbox.me/media/post/{id}/poster` through `media.external` (the proxy), sized from the video's width and height with the usual 400×300 cap, and links to the page
    - other videos, or no proxy, fall back to `EmbedView::Video` as before
  - A unit test checks `gifbox_id`, including lookalike hosts, `http`, `user@host`, an empty ID, `..` and media URLs.
- `src/ui/chat.rs`: `render_embed` draws `GifPoster` with `inline_image`, so it gets the same loading and "Couldn't load image" fallback. It has a small black "GIF" badge at the bottom left, and a click sends `RowAction::OpenLink(page)`, which goes through `open_external` and the safety prompt.
- `tests/store_embeds.rs`:
  - `a_video_only_gifbox_gif_shows_its_still_badged`: a proxied poster at 400×225 from 640×360, linked to the page. Without a proxy it's still a `Video`.
  - `other_videos_and_gifbox_images_are_unchanged`: other hosts and lookalikes stay `Video`, and a Gifbox `Image` embed is drawn as before.

### Divergences
- Minor: the variant carries the page URL beside the image (`GifPoster { image, link }`), because `EmbedImage.url` is the proxied poster and the click needs the page.
- Minor: only the `Embed::Video` arm is changed, as planned. A `Website` embed with `special: GIF` and only a video still falls back to `Video`.

### Verification Performed
- `cargo test`: 722 passed, 0 failed (5 ignored). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt `Diff in` counts are unchanged: `src/embed.rs` 0, `tests/store_embeds.rs` 0, `src/ui/chat.rs` 17.

### Manual Validation
Pending (batched to the end).

## Fixes from manual testing

### GIF picker: hovered GIFs didn't play, then flashed before playing
- **Found:** hovering a result requested its `gif_high` file (in the log) but the tile never animated.
- **Cause:** GPUI's `img` keeps its frame index in element state, so an `img` without an element id stays on frame 0 and never asks for animation frames. Chat's `inline_image` gives its `img` an id, which is why GIFs in chat already animate.
- **Fix 1:** result images get `.id(("gif-image", ix))`.
- **Found next:** the tile flashed to its dark background before the GIF played.
- **Cause:** for its first 200 ms of loading (`LOADING_DELAY`), an `img` draws nothing, not even its `with_loading` element.
- **Fix 2:** `render_results` asks `window.use_asset::<ImgResourceLoader>` for the hovered GIF's animation. This starts the load and redraws the picker when it lands. The tile shows the still until the animation is decoded, then switches straight to it. `with_loading` is no longer used.
- **Files:** `src/ui/gif_picker.rs` (`render` passes `window` to `render_results`).
- **Verified:** the user confirmed hovering now plays with no flash. `cargo test` passed 722, 0 failed. Clippy is clean, and `src/ui/gif_picker.rs` has 0 rustfmt drift.
