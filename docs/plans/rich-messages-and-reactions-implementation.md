# Implementation Record: Rich messages and reactions (RuSToat milestone 2)

_Plan: docs/plans/rich-messages-and-reactions.md_
_Started: 2026-11-04_

## Chunk 1: Avatars and media URLs

### Changes Made
- `Cargo.toml`: added `url = "2"` (already in the dependency tree) for percent-encoding.
- `src/api/client.rs`: `RootFeatures.january`, `RootConfig::proxy_server()`, and a shared `enabled_url` helper.
- `src/media.rs` (new): `MediaUrls { api, files, proxy }` with trailing slashes trimmed.
  - `from_root`
  - `file(&File)` gives `{files}/{tag}/{id}`
  - `file_by_id(tag, id)`
  - `emoji(id)` gives `{files}/emojis/{id}`
  - `default_avatar(user)` gives `{api}/users/{id}/default_avatar`
  - `external(url)`: own file-server URLs are used directly; anything else goes through `{proxy}/proxy?url=<form-encoded>`. A look-alike host such as `cdn….evil.example` isn't treated as ours, because it's matched on `"{files}/"`. With no proxy, external images aren't loaded.
- `src/store.rs`:
  - `ChatStore.media`, `set_media()` and `media()`.
  - `user_avatar_url(id)` returns the user's avatar or their generated default.
  - `avatar_url(&Message)` follows plan Assumption 7: masquerade avatar (via `external`), then webhook avatar (`avatars/{id}`, or `None` because webhooks have no default), then server member avatar (embedded or from the store), then the embedded `message.user` avatar (only if the user isn't cached), then `user_avatar_url`. System messages get `None`.
  - `MessageRow.avatar` and `ChannelEntry.avatar`: DMs carry the other person's avatar, groups their icon file, and text channels and Saved Notes `None`.
- `src/ui/chat.rs`:
  - `ChatView.media: MediaUrls` replaces `file_server`. It's seeded with the API base at construction, so default avatars work at once. `set_media()` runs after `GET /` loads and re-syncs loaded panes, so avatar fields fill in. Server icons now use `media.file()`.
  - New `avatar(url, name, size, bg, fg)`: a round `img()` with an initial-letter placeholder for loading, fallback, or no URL.
  - `render_message_row` is now `h_flex[gutter 36 px, content]`. Header rows show the avatar; continuation rows keep an empty gutter so text lines up. System rows put the "→" in the same gutter.
  - DM and group rows in the channel list show a 20 px avatar instead of the glyph when there is one. The user panel shows your 32 px avatar next to your name and status.
- `tests/media_urls.rs` (new), 5 tests:
  - file, emoji and default-avatar URLs, including with no file server
  - external proxying and encoding (space, `&`, `é`), own-CDN passthrough, the look-alike host, and no-proxy giving `None`
  - avatar precedence for masquerade, webhook with and without an avatar, member over user in a server, user in a DM, the default, and system
  - DM entry avatars
  - timeline rows carrying avatars
- `tests/live_media.rs` (new): 2 opt-in live tests. `default_avatar` returns an image without auth for any ULID-shaped ID, and the January proxy serves a public PNG.
- `tests/store_live.rs`: the test row helper gains `avatar: None`.

### Divergences
- **Minor: webhook avatar tag.** `MessageWebhook.avatar` is a bare file ID, not a `File`, so the URL is built as `{files}/avatars/{id}`, assuming webhook avatars use the `avatars` tag. This was not separately verified live, because no webhook avatar ID was at hand.
- **Minor:** the embedded `message.user` avatar is a fallback for authors who aren't in the cache. It isn't in the plan's precedence list.
- **Minor: the live test uses a made-up ID.** `default_avatar` works for any ULID-shaped ID (the server generates the image), so the live test doesn't contain a real account ID.
- No effect on later chunks. Chunk 3 can use `MediaUrls::emoji`, and chunk 5 can use `MediaUrls::external`, as planned.

### Verification Performed
- `cargo test --test media_urls`: 5 passed.
- `cargo test --test live_media -- --ignored`: 2 passed against the live services.
- `cargo test`: 86 passed, 0 failed, 5 ignored:
  - 27 unit tests
  - 9 `api_login`
  - 7 contract
  - 5 `media_urls`
  - 10 `session_restore`
  - 8 `store_history`
  - 12 `store_live`
  - 8 `store_ready`
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live run: the app logged `media: MediaUrls { api: "https://api.stoat.chat", files: Some("https://cdn.stoatusercontent.com"), proxy: Some("https://proxy.stoatusercontent.com") }`. It then fetched 7 uploaded avatars (`cdn…/avatars/<file>`) and 1 generated default (`api…/users/<id>/default_avatar`), with no warnings or errors.

### Manual Validation
The user checked the running app and confirmed "Looks right": avatars match the web app.

## Chunk 2: Markdown message bodies

### Changes Made
- `src/markdown.rs` (new):
  - `prepare(content, &impl Names) -> String` turns Stoat text into standard Markdown.
  - `segments()` splits content into code (fenced blocks with ``` or ~~~, which may be unclosed, and inline spans with matching backtick runs) and text. Only text is rewritten.
  - In text:
    - `<@id>` becomes `[@Name](stoat:user/id)` and `<#id>` becomes `[#name](stoat:channel/id)`, with the name escaped via `escape()`, which backslash-escapes every ASCII punctuation character.
    - Unknown mentions keep their raw form, with the `<` escaped.
    - `||spoiler||` becomes an inline code span (`code_span()` picks a long enough fence), with mentions inside rendered as plain `@Name`.
    - Autolinks (`<https://…>`, `<http://…>`, `<mailto:…>`) are kept.
    - **Every other `<` is escaped as `\<`.** gpui-base's Markdown parser renders raw HTML (`Node::Html`, which goes to `html::parse`), and Stoat doesn't. Without this, `<b>` would render as bold, and `<not a mention>` would vanish as an HTML tag.
  - Also: the `Names` trait, `is_external_link()` (http, https and mailto only), and `INTERNAL_SCHEME = "stoat:"`.
  - 3 unit tests: segments, code spans, escaping.
- `src/store.rs`:
  - `render_content` is removed, superseded by `markdown::prepare`.
  - `MessageRow.body` is now Markdown for normal messages, and still plain text for system messages.
  - New `MessageRow.render_key = render_key(id, body)`, which is `"{id}:{hash of body}"`. It changes when the drawn text changes: an edit, or a renamed mention.
  - `impl markdown::Names for ChatStore`.
- `src/ui/chat.rs`: message bodies render as `TextView::markdown(render_key, body)` with `.w_full().text_sm().selectable(true)`. `.on_link_click(…)` opens only external links via `cx.open_url`; `stoat:` mention links do nothing. "(edited)" stays a small muted line under the body.
- `tests/markdown_prepare.rs` (new), 8 tests:
  - mention links with escaped names
  - hostile names (`*](javascript:x)[*`)
  - unknown mentions and stray `<` stay literal (`<b>`, `1 < 2`)
  - autolinks kept
  - ordinary Markdown passes through unchanged
  - nothing rewritten inside inline code, fenced blocks or multi-backtick spans
  - spoilers (including mentions inside, unclosed and empty markers)
  - the external-link allowlist (rejecting `javascript:` and `file:`)
- `tests/store_history.rs`: the expected body is now the Markdown form, plus a new `render_key_changes_when_the_drawn_text_changes` test.
- `tests/store_live.rs`: the row helper sets `render_key`.

### Divergences
- **Minor, discovered: escape all `<`.** The plan only mentioned escaping inside replacements. But `TextView` renders raw HTML, so all `<` outside code and autolinks must be escaped to match Stoat, which shows typed HTML literally.
- **Minor: `render_key` hashes the prepared body** instead of using the edit timestamp. That covers edits and also mention names that change, as the plan allowed ("edit timestamp (or a content hash)").
- **Minor: a spoiler wrapping inline code** (`||a `b` c||`) is left as typed, because code spans are split out first. This is documented in the module and pinned by a test. Spoilers can't be hidden yet anyway (Assumption 2).
- **Minor: kept the default `TextViewStyle`** (theme colours) with only `text_sm` sizing. No custom style pass has been needed so far; the visual check will confirm.
- No effect on later chunks. Chunk 3 adds custom-emoji rewriting inside `markdown::rewrite_text`, and its image sizing remains the risk the plan names.
- **Bug found in live testing, fixed: crash on multi-byte text.** Opening a channel whose message began with `⚠` panicked the app (`src/markdown.rs:172`: "start byte index 1 is not a char boundary; it is inside '⚠'"). `rewrite_text` advanced past the first character with `rest[1..]`, which is one *byte*, not one character. The fix advances by `char::len_utf8()` of the first character. Regression test `multibyte_text_never_panics_and_survives_intact` checks the exact case, then runs `prepare` over every char-boundary substring of a string mixing emoji, CJK and accented characters with every piece of syntax the module handles. I also audited the module's other slices: they all cut at an ASCII delimiter or at a `find` result, so they're always on a boundary.

### Verification Performed
- `cargo test --test markdown_prepare`: 8 passed, after fixing two of my own mistakes. The first run found a real bug: the `<…>` close-bracket filter included the opening `<`, so no mention resolved. It also caught a wrong expected string (`:` is escaped too).
- After the crash fix, `cargo test`: 99 passed, 0 failed, 5 ignored. Before it, 98 passed:
  - 30 unit tests
  - 9 `api_login`
  - 7 contract
  - 8 `markdown_prepare`
  - 5 `media_urls`
  - 10 `session_restore`
  - 9 `store_history`
  - 12 `store_live`
  - 8 `store_ready`
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live: the app connected (`Ready`: 5 servers, 75 channels, 0 skipped) and loaded the first conversation, with no warnings or errors.

### Manual Validation
The first check found the multi-byte crash, now fixed. After the fix, the user reopened the channel that crashed and confirmed "Looks right": formatting renders correctly in the running app.

## Chunk 2b: Message bubbles and grouping (inserted)

_Added at the user's request before chunk 3: "a 'bubble' or some other identifier around messages, including some signifier/grouping of sequential messages by one user." Asked to pick a style (chat bubbles, group cards, or an accent bar), the user chose **chat bubbles**._

### Changes Made
- `src/store.rs`:
  - New `MessageRow.group_end` (the next row doesn't continue this one) and `MessageRow.mine` (`author == me`).
  - `mark_group_ends()` fixes up `group_end` after the timeline is built.
  - Appending a live message to a group flips the previous row's `group_end`, so `diff_rows` remeasures that row; no special handling is needed.
- `src/ui/chat.rs`:
  - **Message rows** are built from gpui-component's `Message` (aliased `MessageLayout`, to avoid confusion with the API model), `MessageAvatar`, `MessageHeader`, `MessageContent`, `Bubble` and `BubbleContent`.
    - Others are left-aligned with `BubbleVariant::Secondary`. Yours are right-aligned with `BubbleVariant::Tinted`, and the header says "You".
    - The header is shown only on a group's first bubble. The 32 px avatar is on the group's last bubble (`AVATAR_SIZE` was 36 and now matches the component's `size-8` slot). Earlier bubbles get an empty `MessageAvatar` slot, so they align.
    - Corners: `BUBBLE_RADIUS` 16 px outside, and `JOINED_RADIUS` 4 px on the sender's side wherever bubbles from one person touch (top when not first, bottom when not last).
    - Spacing is 12 px above a group's first bubble and 2 px between bubbles within a group.
    - The Markdown `TextView`, attachment placeholders and "(edited)" now live inside the bubble surface. The `TextView` no longer forces `w_full`, so short messages can get short bubbles.
  - **System rows** are centred, small, italic and muted, with the time.
- `tests/store_history.rs`: `groups_know_where_they_end` checks (continuation, group_end) for every fixture row. `own_messages_are_marked_mine` checks the same history from "u-me" (none mine) and from "u-ada" (her three messages mine).
- `tests/store_live.rs`: the row helper sets the new fields.
- `docs/plans/rich-messages-and-reactions.md`: added chunk 2b. Look-ahead amendments:
  - Chunk 4: images go inside the bubble, and image-only messages use the `Ghost` variant.
  - Chunk 6: reaction chips use `Bubble::reactions(BubbleReactions)`.

### Divergences
- **Scope addition (user-directed):** this chunk wasn't in the plan. It was inserted between chunks 2 and 3, and the plan file is updated.
- **Fix after user feedback ("too much space between the messages"):**
  - `MessageScroller` wraps every row in `px_3` and `pb_8`, which is 32 px under every row. That spacing is designed for whole chat turns, not single messages, and with the original 12 px / 2 px it put at least 34 px between consecutive bubbles. It's now overridden with `.with_row_style(StyleRefinement::default().pb(0).px(0))`.
  - `Message`'s header-to-bubble gap is cut from its default 10 px (`rems(0.625)`) to `HEADER_GAP` = 2 px via `with_stack_style`.
  - Spacing is now named constants: `GROUP_GAP` = 8 px between senders, `BUBBLE_GAP` = 2 px within a group.
- **Minor:** `BubbleGroup`/`MessageGroup` from gpui-component aren't used. Both are plain vertical stacks with no corner shaping, and `MessageScroller` renders one row per message, so a group can't be a single element. Grouping is done per row with corner radii and spacing instead.

### Verification Performed
- `cargo test`: 101 passed, 0 failed, 5 ignored. That's 2 more than before: the new `store_history` tests.
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live: relaunched; connected (`Ready`: 5 servers, 75 channels); no warnings, errors or panics.
- Not verifiable by me: how it looks. In particular, whether bubbles shrink to short messages or always take the 80% maximum width depends on how gpui's layout measures `TextView`.

### Manual Validation
- **First look:** the user reported "too much space between the messages", which was fixed as described under Divergences.
- **After the fix:** the user confirmed "Looks right" in the running app.

## Chunk 3: Custom emoji

### Changes Made
- **Sizing spike** (the plan's main risk), resolved by reading gpui-base's source:
  - gpui-base's `ImageNode` has explicit `width`/`height` fields, filled only from HTML `<img width height>` attributes. Markdown `![](url)` leaves them `None`, which means intrinsic size.
  - Inline HTML inside a paragraph is merged into the paragraph by `append_inline_html_blocks`, images included.
  - So a generated `<img … width=20 height=20>` renders as a sized inline image. No `image_source` resolver is needed; the `src` is the CDN URL, loaded via the chunk 1 HTTP client.
- `Cargo.toml`: added `unicode-properties = "0.1"` (already in the dependency tree) for emoji character classes.
- `src/time.rs`: `pub fn is_ulid()`, which requires 26 upper-case Crockford characters, plus a test.
- `src/api/events.rs`:
  - `"emojis"` is added to `READY_FIELDS`, and `Ready.emojis` is parsed leniently into `revolt-models` `Emoji`.
  - New `ServerEvent::EmojiCreate(Box<Emoji>)` and `EmojiDelete { id }`, decoded and forwarded.
  - The `Ready` log line now includes the emoji count.
  - New unit test `emoji_events_decode`.
- `src/store.rs`:
  - an `emojis` map, filled by `apply_ready`, plus `add_emoji`, `remove_emoji` and `emoji()`
  - `Names::emoji_name` (from the store) and `Names::emoji_url` (`MediaUrls::emoji`)
  - `MessageRow.jumbo`, which is `markdown::is_jumbo(content)` for non-system messages
- `src/markdown.rs`:
  - `Names` gains default `emoji_name`/`emoji_url` methods.
  - `:ULID:` in text becomes `<img src="…" alt=":name:" title=":name:" width=20 height=20>`. Attributes are HTML-escaped, and unknown emoji get the name `emoji` but still render from the CDN. The ID is validated, so `:smile:` and `12:30:45` are left alone, and the byte check happens before slicing.
  - `is_jumbo()`: whitespace plus 1 to 10 emoji (custom `:ULID:`, or non-ASCII Unicode emoji characters; joiners, modifiers and variation selectors aren't counted separately) makes a message jumbo, and its custom emoji are 48 px.
  - Inside spoilers, custom emoji become `:name:` text.
  - Code is still untouched.
- `src/ui/chat.rs`: applies `EmojiCreate`/`EmojiDelete`. Jumbo messages render their `TextView` at `text_3xl` in a `BubbleVariant::Ghost` bubble (no surface).
- Tests:
  - `tests/markdown_prepare.rs`: +5 tests (sized images for known and unknown emoji; non-emoji colons, near-miss IDs and times left alone; emoji in code and spoilers; jumbo detection including skin tone, VS16, 10 vs 11, and ASCII digits/`#` excluded; 48 px in jumbo messages). The multi-byte substring sweep now includes an emoji.
  - `tests/store_emoji.rs` (new): 3 tests (Ready emoji with a broken one skipped, not fatal; create and delete events; the jumbo row flag and 48 px).

### Divergences
- **Minor, simpler than planned:** emoji are sized HTML `<img>` tags with the CDN URL directly, instead of a `stoat-emoji:` scheme plus a `TextView::image_source` resolver. The spike showed only HTML images carry a size. This is safe because `prepare` escapes every user `<`, so only HTML generated here reaches the parser.
- **Minor, extended:** jumbo detection also covers Unicode emoji (drawn at `text_3xl`), not just custom ones. Jumbo messages use a Ghost bubble, following chunk 2b's bubble design.
- **Minor:** `EmojiCreate`/`EmojiDelete` are handled (the plan made this optional). Already-rendered rows aren't rebuilt when an emoji is added, because their image URL doesn't depend on the emoji list, only the alt text does.
- No effect on later chunks. Chunk 6's custom-emoji reaction chips can use `MediaUrls::emoji` and `ChatStore::emoji()`.

### Verification Performed
- `cargo test`: 111 passed, 0 failed, 5 ignored:
  - 32 unit tests
  - 9 `api_login`
  - 7 contract
  - 14 `markdown_prepare`
  - 5 `media_urls`
  - 10 `session_restore`
  - 3 `store_emoji`
  - 11 `store_history`
  - 12 `store_live`
  - 8 `store_ready`
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live: `events: Ready (5 servers, 75 channels, 29 users, 41 emoji, 0 skipped)`, so the live emoji payload parses cleanly. The app is running with no warnings or errors.

### Manual Validation
The user checked the running app and confirmed "Looks right": custom emoji render at the right size.
