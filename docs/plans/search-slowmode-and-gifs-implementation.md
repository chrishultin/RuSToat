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
