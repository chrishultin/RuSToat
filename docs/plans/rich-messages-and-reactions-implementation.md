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

### Follow-up fix (after the chunk 3 commit): own emoji-only messages left-aligned
- **Reported by the user:** an emoji-only message they sent showed on the left instead of the right.
- **Cause:** jumbo messages used `BubbleVariant::Ghost`. gpui-component's `Bubble::render` applies `.when(variant == Ghost, |this| this.w_full().max_w_full())`, which stretches ghost bubbles to the full row width (the variant is meant for full-width assistant-style content). The emoji then sat at the left of a full-width bubble.
- **Fix** (`src/ui/chat.rs`): jumbo messages keep their normal variant, so they keep normal shrink-to-fit width (max 80%) and normal alignment. The surface is made invisible with `bg(transparent_black()).p_0()` on `BubbleContent`, which gpui-component applies after the variant's own styles.
- **Plan look-ahead:** chunk 4 said image-only messages would use Ghost. It's amended to use the same transparent-surface approach, with a note not to use Ghost.
- **Verification:**
  - `cargo test`: 111 passed. `cargo clippy --all-targets -- -D warnings`: clean.
  - Relaunched; `Ready` parsed 41 emoji, with no errors.
  - The user confirmed the emoji-only message is now on the right ("it's correctly aligned").
- **Size tweak (user request, "the emoji by itself should be 2x larger"):** `JUMBO_EMOJI_PX` went from 48 to 96 px for custom emoji. For consistency, Unicode emoji in jumbo messages went from `text_3xl` (30 px) to `JUMBO_TEXT_PX` = 60 px. Tests are updated, and 111 pass. The user confirmed the emoji is bigger in the running app.

## Side fix (between chunks 3 and 4): Stoat formatting in a review bot's messages
- **Reported by the user:** "advanced formatting" from a user named Peter didn't come through. Peter is a code-review bot. Logging the raw `content` of its messages showed three things RuSToat got wrong, compared against the web client's pipeline (`stoatchat/for-web` `components/markdown`: `remark-breaks`, `remark-math` with `singleDollarTextMath: false`, `rehype-katex`, and a `sanitise` pass):
  - **Line breaks.** `TextView` collapses a single newline (a CommonMark soft break) to a space. Stoat shows every newline. This affected every multi-line message, not just Peter's.
  - **Quote endings.** A plain line after `> …` lines was pulled into the quote by CommonMark's lazy continuation. Stoat's `sanitise` adds a blank line after a quote's last `>` line.
  - **`$$…$$` KaTeX** (`$$\color{gray}\scriptsize \textsf{17.7s}$$` step timings) showed as raw TeX.
- `src/markdown.rs` `prepare()`:
  - A newline between two non-blank lines becomes a hard break (two trailing spaces) unless it already is one. `\r\n` is normalised first.
  - A quote's last line gets a blank line after it.
  - New `Segment::Math` for `$$…$$` within one line: two or more dollars, closed by the same count, not escaped. Only `<` is escaped inside (`\<`). Mentions, emoji and spoilers aren't rewritten there. A single `$` stays prose, as on Stoat.
- `src/math.rs` (new, pure): `interpret()` reads the formatting part of KaTeX into styled `Run`s, or returns `None`, which leaves the formula as typed. It supports:
  - colours: CSS names via `svgtypes` (already in the tree), `#rgb` and `#rrggbb`
  - size commands, at KaTeX's 1.21× base scale
  - `\text*`/`\math*` fonts and switches
  - TeX inter-atom spacing and explicit spaces
  - `^` and `_` as raised or lowered runs
  - underline and strikethrough, `\left`/`\right`, `\operatorname`
  - Greek letters and common symbols
- `src/ui/math.rs` (new): `MathPlugin`, an inline `MarkdownPlugin`. gpui-base calls the hook an "inline parser"; it claims `InlineMath` nodes whose source starts with `$$`. It draws the runs as one native element (a flex row of text divs) and reports a baseline worked out from font metrics, so each run lines up with the surrounding text. `src/ui/chat.rs` adds `.plugin(MathPlugin)` to the message `TextView`.
- **New `TextView` facts:**
  - Soft breaks collapse to spaces.
  - Inline plugins can claim whole AST nodes (`InlineMath` here) and return any element, which has its own colour and size, unlike HTML or Markdown text.
  - Unclaimed inline math is re-parsed as prose and shown as typed.
  - Block `$$` math becomes a code block unless a block plugin claims it.
- **Limits:**
  - A formula is one element and doesn't wrap. That's fine for short labels, but a long coloured sentence would overflow.
  - Fractions, roots, matrices, accents, `\\` and `\mathbb` are shown as typed.
  - Block `$$` math still renders as a code block.
  - Stoat's extra-blank-line preservation (`​` paragraphs) isn't replicated.
  - Math text uses Times New Roman in place of KaTeX_Main.
- **Tests:**
  - `tests/markdown_prepare.rs`: +5. They cover line breaks (blank lines, existing breaks, escaped backslashes, CRLF), breaks next to lists, headings, tables and code blocks, quote endings, `$$` handling (escaping, single `$`, escaped, unclosed and multi-line cases), and a full review-bot message. The multi-byte sweep now includes `$$`, `\r\n` and a quote.
  - New dev-dependency `markdown` (the parser inside `TextView`, already in the tree). The tests use it to check that the prepared output parses into the intended nodes.
  - `tests/markdown_math.rs` (new): 8 tests covering the caption itself, dot-separated captions, colours, sizes and fonts, spacing, symbols and scripts, unsupported input, and a substring sweep.
  - Unit test: `segments` splits `$$…$$`.
- **Verification:**
  - `cargo test`: 134 passed, 0 failed, 5 ignored.
  - `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- **Manual validation:** the user checked Peter's messages in the running app and confirmed "Looks right".

## Chunk 4: Image attachments and file cards

### Changes Made
- `src/attachment.rs` (new, pure):
  - `AttachmentView` is one of two things:
    - `Image { id, url, name, size_label, width, height, too_big }`, drawn inline.
    - `File { name, size_label, url: Option, kind }`, drawn as a card.
  - `AttachmentView::new(file, media, revealed)`: `Metadata::Image` with a file server becomes `Image`. Everything else becomes `File`, including images before `GET /` has loaded (no URL yet).
  - `fit_within(w, h)`: fits within `MAX_IMAGE_WIDTH` × `MAX_IMAGE_HEIGHT` (400×300), never scales up, rounds to whole pixels, and gives at least 1 px. Zero or missing dimensions give `FALLBACK_IMAGE_SIZE` (300×200).
  - `MAX_AUTO_LOAD_BYTES` = 8 MiB. A larger image is `too_big` until revealed.
  - `file_kind(metadata, content_type)`: Image, Video, Audio, Text or File. It uses the metadata first, then the MIME type for uncategorised uploads.
  - `size_label(bytes)`: 1024-based, `B`/`KB`/`MB`/`GB` with one decimal place. Moves up a unit instead of showing `1024.0 KB`.
- `src/store.rs`:
  - `MessageRow.attachments` is now `Vec<AttachmentView>`, replacing the file names.
  - New `revealed_images` set and `reveal_image(file_id)`.
- `src/ui/chat.rs`:
  - `render_attachment` handles three cases:
    - **Images:** `img()` in a box of exactly the precomputed size, `ObjectFit::Contain`, 8 px corners. A blank backdrop shows while loading, and "Couldn't load image" on failure. A click opens the original in the browser.
    - **Large images:** a card with a "Show image" button.
    - **Files:** a card (`file_card`: icon, name, size, a `Download` icon) that opens the file URL on click.
  - Attachments are stacked in a `v_flex` inside the bubble, below the text.
  - A message with attachments but no text gets the invisible surface already used for jumbo emoji (`bg(transparent_black()).p_0()`), not `Ghost`.
  - `ChatView::reveal_image(channel, file)` updates the store, then `sync_pane`. The row is remeasured via `diff_rows`. `render_row` gets a `RevealImage` callback, built in `render_main` from a weak handle to the view.
  - `RowColors` gains `border`, `card_bg` and `image_bg`.
- `src/assets.rs`: adds Lucide `Download`, `FileImage`, `FileMusic` and `FileVideoCamera`. `File` and `FileText` are already default icons. The icon test now covers two of the new ones.
- `src/lib.rs`: `pub mod attachment`.
- `tests/store_attachments.rs` (new), 9 tests:
  - sizing for exact, wide, tall, rounding, no-upscale and extreme shapes
  - the zero-dimension fallback
  - the inline image view
  - the 8 MB guard: at the limit, over it, and revealed
  - file cards for PDF and video, and no link without a file server
  - file kinds from metadata and MIME type
  - size labels
  - timeline order
  - "Show image" flipping the view and giving `RowOp::Remeasure`
- `tests/store_history.rs`: the attachment assertion now expects an `AttachmentView::File` (the fixture store has no file server).

### Divergences
- **Minor, location:** the sizing, guard, kind and label logic lives in a new `src/attachment.rs`, not `store.rs`. Chunk 5 can reuse `fit_within` from there for embed images.
- **Minor, reveal state:** the revealed set is in `ChatStore`, not `ChatView`, so revealing goes through `timeline()` and `diff_rows` like any other row change.
- **Minor, icon names:** Lucide has no `FileVideo` or `FileAudio`, so `FileVideoCamera` and `FileMusic` are used. Text files get `FileText`, which adds a fifth kind beyond the plan's four.
- **Minor, addition:** clicking an inline image opens the original in the browser.
- **Minor:** a message that has only file cards also gets the invisible surface, because the cards have their own border.
- **No effect on later chunks.** Chunk 5 uses `attachment::fit_within` where it planned to use "the chunk 4 sizing helper". The plan file isn't changed.

### Verification Performed
- `cargo test --test store_attachments`: 9 passed. `cargo test --test store_history`: 11 passed.
- `cargo test`: 174 passed, 0 failed, 5 ignored, up from 165.
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live: launched `./target/debug/stoat-gpui`. `Ready` logged 5 servers, 76 channels and 41 emoji; history loaded; there were no warnings or errors beyond GPUI's usual duplicate-font notices.

### Manual Validation
The user checked inline images, file cards and image-only bubbles in the running app and confirmed "Looks good".

## Chunk 4b: In-app image viewer (inserted)

_Added at the user's request after chunk 4: "Is clicking to view full-size on the roadmap?" It wasn't in any plan. The user asked for it now, without zoom and pan or arrowing between images. The plan file gains a chunk 4b section._

### Changes Made
- `src/attachment.rs`:
  - `AttachmentView::Image` now carries `original: (usize, usize)`, the upload's metadata dimensions (zero if unknown).
  - New `fit_into(w, h, max_w, max_h) -> Option<(u32, u32)>`: shrinks to fit without enlarging, rounds, gives at least 1 px, and returns `None` for zero dimensions.
  - `fit_within` is now `fit_into` into 400×300, with the fallback box.
- `src/ui/chat.rs`:
  - The chunk 4 `RevealImage` callback is now `OnRowAction = Rc<dyn Fn(RowAction, &mut Window, &mut App)>`, where `RowAction` is `RevealImage(file_id)` or `ViewImage(ViewedImage { url, name, original })`. `ChatView::on_row_action` handles both and replaces `reveal_image`.
  - New `ChatView.viewer: Option<ViewedImage>` and `viewer_focus: FocusHandle`. Opening the viewer focuses the overlay, so it gets key presses.
  - `render_viewer` draws an absolute `inset_0` overlay over the whole `ChatView`:
    - **Input:** the overlay uses `occlude()` and swallows scroll events, so the list underneath doesn't react.
    - **Backdrop:** black at 85% opacity.
    - **Top bar:** theme background, the file name, a ghost "Open in browser" button (`ExternalLink`) and a close button (`Close`).
    - **Image:** fitted with `fit_into` to the window minus 32 px margins and the 48 px bar. Unknown dimensions fill that space, letterboxed with `ObjectFit::Contain`.
    - **Closing:** Esc (`on_key_down`), the close button, or a left mouse-down on the backdrop. The bar and the image stop propagation, so clicking them doesn't close it.
  - The root render is now a `relative` container, so the overlay can sit over the rail, channel list and chat.
  - Clicking an inline image now opens the viewer instead of the browser. The browser is still one click away, in the viewer's bar.
- `tests/store_attachments.rs`:
  - New `the_viewer_fits_images_to_the_window`: larger than the window (wide and tall), smaller (kept at its own size), a 1×1 window, and zero dimensions giving `None`.
  - The existing image tests now assert `original`.

### Divergences
- **Scope addition (user-directed):** not in the original plan; inserted between chunks 4 and 5.
- **Minor:** `on_scroll_wheel` is deprecated in this gpui and renamed to `on_scroll`, so `on_scroll` is used.
- **No effect on later chunks.** Chunk 5's embed images could open the viewer through `RowAction::ViewImage` if wanted.

### Verification Performed
- `cargo test`: 175 passed, 0 failed, 5 ignored, up from 174.
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live: relaunched. `Ready` logged 5 servers, 76 channels and 41 emoji, and history loaded, with no warnings or errors.

### Manual Validation
The user checked the viewer in the running app and confirmed "lgtm":
- opening from an inline image, fitted to the window
- closing with Esc, the close button and a backdrop click
- "Open in browser"
- no interaction with the list underneath

## Chunk 5: Embeds (link previews and bot embeds)

### Changes Made
- `src/embed.rs` (new, pure). `EmbedView::new(embed, media, names, revealed, key)` gives one of:
  - `Card(EmbedCard)`, a card with:
    - a colour bar, `0xRRGGBBAA`, parsed with `math::parse_color` (`#rgb`, `#rrggbb` and CSS names). Anything else, such as a gradient, uses the theme border.
    - a proxied icon, the site name and the title
    - `link`, http(s) only, from `url` and then `original_url`
    - `description`, either `Plain` (websites) or `Markdown { source, render_key }` (bot embeds, run through `markdown::prepare`)
    - a `thumbnail` (`ImageSize::Preview`, fit into 80×80) or a large `image` (`ImageSize::Large`, fit into 400×300)
    - `playable`, when there's an image and the site has a `video`, or `special` is YouTube, Twitch, Lightspeed or Streamable
    - `media`: a bot embed's uploaded file, as an `AttachmentView` that respects "Show image".
  - `Image(EmbedImage)` for `Embed::Image`, and for websites with `special: GIF`.
  - `Video { url }` for `Embed::Video`, and for a GIF that only has a video.
  - Nothing (`None`) for `Embed::None`, and for cards with nothing beyond a colour.
  - Every remote picture goes through `MediaUrls::external` (the January proxy). Without a proxy, pictures are left out. Missing image dimensions get the fallback box, clamped to the slot.
- `src/api/events.rs`:
  - New `ServerEvent::MessageAppend { id, channel, embeds }`, decoded and forwarded.
  - Embeds are decoded one by one with `lenient_list`, so an embed with an unknown `special` is skipped and logged without losing the others.
  - `lenient_list`'s log message is now generic, so callers pass labels like "user in Ready" or "embed in MessageAppend".
- `src/store.rs`:
  - `apply_append(channel, id, embeds)` extends `message.embeds`. It returns false for an unloaded message or an empty list.
  - New `MessageRow.embeds: Vec<EmbedView>`, keyed `"{message}:embed{ix}"`.
- `src/ui/chat.rs`:
  - Handles `MessageAppend` by calling `apply_append`, then `sync_pane`; `diff_rows` remeasures the row.
  - Chunk 4's image element is factored into `inline_image(id, url, size, colors)`, shared by attachments and embeds.
  - New `render_embed` / `render_embed_card`:
    - the colour bar, plus a site row (16 px icon and name)
    - a bot embed's icon next to its title when there's no site name
    - a linked title (theme `link` colour, underline on hover, opens in the browser)
    - a plain description clamped to 4 lines, or Markdown via `TextView` with the same link handling as messages
    - a thumbnail to the right, a large image below, and media below that
    - large images open in the 4b viewer. Playable ones get a 48 px play badge and open the link instead.
  - Bare image embeds open in the viewer. Video embeds are cards (`FileVideoCamera`, "Video", `ExternalLink`) that open the URL.
  - Embeds stack under any attachments in the bubble. A message with embeds but no text gets the invisible bubble surface.
  - `RowColors` gains `link`.
- `src/lib.rs`: `pub mod embed`.
- `tests/store_embeds.rs` (new), 12 tests:
  - `MessageAppend` decoding, including an unknown `special` and a missing `append.embeds`
  - a full website card (proxied icon and image, colour, sizing)
  - Preview thumbnails
  - the YouTube play badge, and none without a picture
  - GIF, GIF video and image-link embeds, plus the zero-dimension fallback
  - video links (web only)
  - bot embeds: Markdown with HTML escaped, colour name, media
  - missing and odd fields: empty cards, title-only previews, colour formats, `javascript:`/`mailto:` links, the `original_url` fallback
  - no proxy, so no pictures
  - a live append through `parse_frame`, `apply_append` and `timeline`, giving a `Remeasure`
  - ignored appends
  - the order of embeds already on a message
- `tests/store_live.rs`: the row helper sets `embeds`.

### Divergences
- **Minor, location:** the embed model lives in its own `src/embed.rs`, like chunk 4's `attachment.rs`, not in `store.rs`.
- **Minor, extended colour parsing:** the plan said hex only. Reusing `math::parse_color` also accepts CSS colour names, at no extra cost. CSS4 names such as `rebeccapurple` aren't known to `svgtypes`.
- **Minor, additions:**
  - GIF websites show as the bare picture.
  - Video previews get a play badge.
  - Embed pictures open in the 4b viewer.
  - Unknown embed kinds are skipped individually.
- **Process: another session in this worktree.** Partway through the chunk, a separate session made uncommitted "remember the open place" changes here and then committed them as `03288e3`. I paused, the user said to continue, and I confirmed that commit contains none of this chunk's changes and that what remained uncommitted was only this chunk. The full suite then passed on top of `03288e3`. Its 4 unit tests in `places.rs` are why the library's unit test count went from 40 to 44.
- **No effect on later chunks.**

### Verification Performed
- `cargo test --test store_embeds`: 12 passed. The first run had one failure in the test itself: `svgtypes` doesn't know `rebeccapurple`, so it was changed to `teal`.
- `cargo test`: 191 passed, 0 failed, 5 ignored. That's +12 from `store_embeds` and +4 from the other session's `places` unit tests.
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live: relaunched on top of `03288e3`. `Ready` logged 5 servers, 76 channels and 41 emoji, and three channels loaded, with no warnings or errors.

### Manual Validation
The user checked embeds in the running app and confirmed "Embeds look good".
