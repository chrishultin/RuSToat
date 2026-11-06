# Plan: Message markup (RuSToat milestone 10)

## Summary
Milestone 10 of the [roadmap](roadmap.md) draws the rest of Stoat's message markup the way Stoat's web client does:
- **Role mentions** (`<%role>`), plus `@everyone` and `@online`. They're drawn in the role's colour and offered in autocomplete.
- **`<t:…>` timestamps** in your time zone, or as relative time ("in 3 hours").
- **Text spoilers** that stay hidden until clicked.
- **A copy button on code blocks.**
- **"Quote"** in the message menu.

## Approach
Everything builds on the existing pipeline. `markdown::prepare` (`src/markdown.rs`) rewrites Stoat's dialect into standard Markdown, and gpui-kit's `TextView` draws it.

**One new inline `TextView` plugin.** The new markup needs pieces that plain Markdown can't express: per-mention colours, live relative times, hover tooltips, and click-to-reveal. So `prepare` rewrites each of them into a link with an internal `stoat:` URL:
- `[@Mods](stoat:role/ID/ff8800)`
- `[@everyone](stoat:everyone)`
- `[…](stoat:time/1762430000/R)`
- `[word ](stoat:spoiler/2)`

A new plugin, `MarkupPlugin` in `src/ui/markup.rs`, claims those `Link` nodes and draws each one as a native GPUI element. `src/ui/math.rs` (`MathPlugin`) already works this way.

**Why this works.** We checked gpui-base 0.9.0's `TextView` source:
- An inline plugin can claim any mdast node.
- The element it returns receives real mouse clicks and can carry `.tooltip(…)`.
- `render_inline` runs again on every layout, so a relative time is recomputed whenever the view repaints.
- gpui-base's own tests claim `Link` nodes with custom schemes (`mention:`, `control:`). It's the established hook.

**Rejected alternatives:**
- **Inline HTML** (`<mark color>`, `<span style>`). `TextView` only pairs `b/i/u/s/mark` tags. `<mark>` forces black or white text, so it can't colour a role name.
- **Plain links.** They are all one colour, and the only hook is `on_link_click`. That can't do colours or tooltips.
- **`RangeHighlight`.** Background only. It's view state, not something `prepare` can produce.

**Limits of plugin elements:**
- **They're atomic.** Each is laid out once at its natural width, and never wraps inside itself. That's fine for mentions and timestamps, but a long hidden spoiler would overflow the bubble.
- **So a hidden spoiler becomes one object per word** (each word carries its trailing space), and lines wrap between words.
- **A revealed spoiler goes back to today's code span,** which wraps normally.

**Reveals live in the store.** Which spoilers are revealed is kept on `ChatStore`, keyed by message and spoiler index. This follows `revealed_images` and `RowAction::RevealImage`. `prepare` takes that set as an input, so the prepared text (and so `render_key`) changes on reveal and `TextView` parses it again.

Keyed GPUI element state was rejected: the timeline is virtualized (`MessageScroller`), so a reveal would be lost after scrolling away.

**Code blocks.** `TextView::code_block_actions(|block, window, cx| …)` is gpui-base's supported hook for buttons on a code block. It gets `CodeBlock::code()` and `lang()`, keeps syntax highlighting, and positions the element at the top-right. It replaces the built-in language label, so our element draws the label next to the button.

**Already done: counting role and everyone mentions.** `ChatStore::is_mentioned` (`src/store.rs:1380`) already counts:
- `role_mentions`
- `MessageFlags::MentionsEveryone`
- `MentionsOnline` (only while you're online)

Unread mention counts and notification rules go through it, and `tests/store_unreads.rs:103-144` covers it. This milestone only adds notification-rule tests for those cases.

**Quote** follows revite (`MessageBox.tsx` `append(…, "quote")`):
- Prefix every line of the message's raw content with `> `.
- Add a blank line after it.
- Append it to whatever is in the message box, on a new line.

## Assumptions
1. **Timestamp syntax and formats follow Stoat's web client** (`for-web` `plugins/timestamps.tsx`). The syntax is `<t:SECONDS>` or `<t:SECONDS:X>`, matching `/<t:([0-9]+)(?::(\w))?>/`. We use the app's 24-hour clock rather than the web client's `hh:mm`, which reads as a 12-hour clock with no AM/PM:

   | Tag | Shows | Example |
   |---|---|---|
   | `t` | time | `14:05` |
   | `T` | time with seconds | `14:05:09` |
   | `D` | date | `6 November 2026` |
   | `F` | day, date and time | `Friday, 6 November 2026 14:05` |
   | `R` | relative | `in 3 hours`, `5 minutes ago` |
   | anything else, or none | date and time | `6 November 2026 14:05` |

   Hovering shows the full date and time with seconds and the time zone.
2. **`@everyone` and `@online` are always drawn as mention pills when typed,** as for-web does (its `RE_MENTIONS` matches them in any text), whether or not the server honoured them. Whether they count as a mention for you is the server's call, through the message flags.
3. **Role mentions look like for-web's.** The pill has a dot in the role's colour and the role name in that colour. A role with no colour (or a gradient) uses the muted foreground.
   - **An unknown role stays as typed,** as unknown user mentions do today.
   - **Role IDs are unique across servers** (they're ULIDs), so a role is found by searching every server's roles. The `Names` trait doesn't need to know the channel.
4. **Role mentions, `@everyone` and `@online` are only offered in server channels,** and only with the matching permission:
   - `MentionRoles` (bit 38) for roles
   - `MentionEveryone` (bit 37) for `@everyone` and `@online`

   They aren't offered in DMs or groups. Typing them by hand is still allowed; the server decides what they do.
5. **A revealed spoiler stays revealed for the rest of the session,** across scrolling and channel switches, like `revealed_images`. It's hidden again after a relaunch. Clicking a hidden word reveals the whole spoiler, not just that word.
6. **Spoiler text never leaks into banners or reply previews.** `plain_text` and `snippet` show it as `[spoiler]`. Today `strip_markers` drops the `||` and shows the text.
7. **Relative times update once a minute,** while the open channel has an `R` timestamp. There's one repeating timer in `ChatView`, modelled on `schedule_typing_tick`.
8. **The code-block copy button is always visible,** not only on hover. `code_block_actions` puts it inside a container we don't control, so we can't make it a hover group. After a click it shows a check mark for 2 seconds.
9. **"Quote" appears only for messages with text,** not for system messages or attachment-only messages. It sits in the right-click menu after Reply, with no hover-toolbar button. It quotes the raw Stoat text, so mentions in it become chips in the message box.
10. **Other `TextView`s get the plugin and the copy button too:** embed descriptions, profile bios and system messages. A role mention in a bio then looks the same as in a message.

## Open Questions
None that block the work. *(Can wait)* Our own bubbles are tinted with the accent colour. A role colour may be hard to read on them. Chunk 5's manual check looks at this; if it's a problem, add a contrast fallback like `<mark>`'s black/white rule.

## Plan

### Chunk 1: Copy button on code blocks
**Goal:** Every fenced code block in a message has a button that copies its code, so you don't have to select it by hand.
**Files likely touched:**
- `src/ui/markup.rs` (new):
  - `code_block_actions` renderer: a ghost icon `Button` (`IconName::Copy`), and the block's `lang()` label in muted text to its left.
  - On click it does `cx.write_to_clipboard(ClipboardItem::new_string(block.code()))`.
  - The icon swaps to `IconName::Check` for 2 seconds, through `window.use_keyed_state` and a timer.
- `src/ui/mod.rs`: `mod markup;`
- `src/ui/chat.rs`: add `.code_block_actions(…)` to every `TextView::markdown` (message body ~5788, embed description ~6935, system message ~5558)
- `src/ui/profile_card.rs`: the bio `TextView` (~223)
**Depends on:** None.
**Risks:**
- **The button sits over the code's first line.** `code_block_actions` places it at `top_2().right_2()`. A long first line runs under the button.
  - If that's ugly, give the block extra top padding through `TextViewStyle::code_block`, or move the button down to the line of the language label.
- **A click on the button may start a text selection in the selectable `TextView`.** Stop it with `on_mouse_down(…, |_, _, cx| cx.stop_propagation())`, as gpui-base's own plugin test does.
**How to verify:** `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`.
**Manual check:**
- Send a fenced block with a language and one without. Each has a button, and the first also shows its language.
- Click a button and paste: you get the block's code exactly, with no fences.
- The icon turns into a check mark, then back.
- Dragging to select text inside the block still works.
**Shippable when:** code blocks in messages, embeds and bios have a working copy button, and the language label still shows.

### Chunk 2: Quote a message into the message box
**Goal:** "Quote" in a message's right-click menu adds that message to the message box as a Markdown quote.
**Files likely touched:**
- `src/draft.rs` (or `src/markdown.rs`): a pure `pub fn quote(content: &str) -> String`. Each line gets `> ` (a blank line becomes `>`), and the result ends with `"\n\n"`. Also `pub fn append_quote(existing: &str, quote: &str) -> String`, which adds a newline when `existing` is non-empty and doesn't end with one.
- `src/ui/chat.rs`:
  - `RowAction::Quote(String)`
  - A menu item after "Reply" (`IconName` quote, or the closest available), only when the message has text
  - In `on_row_action`, read `self.store.raw_content(channel, &id)`, as `CopyText` does, and call the composer
- `src/ui/composer.rs`: `pub fn append_text(&mut self, text, window, cx)`. It builds the new value from `input.value()` and `append_quote`, learns labels for mentions in the quote (as the draft-restore path does before `set_text`), then calls `set_text`. That keeps existing chips and makes new ones. It then focuses the box.
**Depends on:** None.
**Risks:**
- **`set_text` replaces the whole value.** Any undo history in the input is lost.
- **Labels for mentions in the quote must be learnt first.** Otherwise chips show the "unknown" label. Use the same labeler that `start_edit` uses.
**How to verify:** `cargo test`, with unit tests for `quote` and `append_quote`:
- one line, several lines, blank lines inside, a trailing newline
- appending to an empty box, to text without a final newline, and to text with one
**Manual check:**
- Quote a multi-line message that has a mention. The box shows `> ` lines with a mention chip, and the cursor is at the end.
- Quote again: the second quote is appended below the first.
- Send it: it renders as a block quote here and in the web client.
- Attachment-only and system messages have no Quote item.
**Shippable when:** Quote works from the menu and its output renders as a quote.

### Chunk 3: The markup plugin, and `<t:…>` timestamps
**Goal:** Timestamps show in your time zone (or relative, for `R`), with the full date on hover. This chunk also adds the `MarkupPlugin` that chunks 4 and 5 extend.
**Files likely touched:**
- `src/time.rs`: pure, testable formatters that take `now`:
  - `format_timestamp(when: DateTime<Local>, style: Option<char>, now) -> String`, for assumption 1's table
  - `relative_time(when, now) -> String`: "just now", "in 5 minutes", "3 hours ago", "in 2 days", "a month ago". Round as dayjs's `fromNow` does.
  - `timestamp_tooltip(when) -> String`: e.g. "Friday, 6 November 2026 at 14:05:09 GMT+1"
- `src/markdown.rs`:
  - In `angle_bracket`, before the autolink check, match `t:` + digits + optional `:X`. Rewrite it to `[<escaped source>](stoat:time/SECONDS/X)`, or `…/SECONDS` with no style.
  - Out-of-range seconds (`chrono` gives `None`) stay as typed.
  - `InternalLink::Time { seconds, style }` in `parse_internal_link`. The link handler ignores it, as it does channels.
  - `plain_text` and `snippet` show the absolute format (they can't update live).
  - Update the module doc.
- `src/ui/markup.rs`:
  - `pub struct MarkupPlugin` (`Send + Sync`). `parse` claims `markdown_ast::Node::Link` whose `url` parses as `InternalLink::Time` and stores it in `MarkdownNode` data, with `.text()` set to the source (copy keeps `<t:…>`).
  - `render_inline` formats against `Local::now()` and draws a small pill: a muted background, `context.text_style()` for font and size, and `.id(…).tooltip(…)` with `timestamp_tooltip`.
  - Use `.with_baseline` as `MathPlugin` does, so it sits on the text line.
- `src/ui/chat.rs` and `src/ui/profile_card.rs`: `.plugin(MarkupPlugin)` next to `MathPlugin` on every `TextView`
- `src/ui/chat.rs`: a once-a-minute `relative_tick` task that calls `cx.notify()` while the open channel's rows contain a relative timestamp. Model it on `schedule_typing_tick`. A `ChatStore::has_relative_times(channel)` (rows whose `body` contains `stoat:time/` and ends `/R)`) decides.
**Depends on:** None. Chunk 1's `markup.rs` file is shared but not needed.
**Risks:**
- **The `Link` url may reach `parse` normalised** (percent-encoded, or with the scheme lower-cased).
  - Keep URLs to `[a-z0-9/]` so nothing gets encoded.
  - Test the round trip with the `markdown` dev-dependency, as `tests/markdown_math.rs` does.
- **The inline element is laid out at `MaxContent` and never wraps.** That's fine for timestamps, which are short.
- **`.tooltip` on an inline plugin element isn't covered by gpui-base's tests** (only `HoverCard` is). If it doesn't show, wrap the pill in a `HoverCard`, which is tested.
- **A repaint may not lay inline objects out again.** If relative times don't update on `cx.notify()`, put the minute in the prepared text instead (fall back to bumping `render_key` with a minute counter). That costs a re-parse once a minute, for that channel only.
**How to verify:** `cargo test`:
- `tests/markdown_prepare.rs`:
  - `<t:1762430000>`, `<t:1762430000:R>`, `<t:abc>` (as typed), `<t:1762430000:R` with no `>` (as typed)
  - one inside a code span (untouched)
  - one next to a mention
- `src/time.rs` unit tests for each style and for relative times on both sides of `now`
- `parse_internal_link` round-trips
**Manual check:**
- Send `<t:NOW+3h:R>`, `<t:NOW:F>` and `<t:NOW:t>`. They show "in 3 hours", the full date, and the time. Hover shows the full date.
- Wait a minute: "in 3 hours" becomes "in 2 hours" when it should.
- Copying the message text gives back `<t:…>`.
**Shippable when:** timestamps render and update as in assumption 1, and every `TextView` has `MarkupPlugin`.

### Chunk 4: Spoilers hidden until clicked
**Goal:** `||text||` shows as a blank bar until clicked. Then it shows as today's muted span, and stays revealed for the session.
**Files likely touched:**
- `src/markdown.rs`:
  - `prepare` gains a way to know which spoilers are revealed. Add `pub fn prepare_with(content, names, revealed: impl Fn(usize) -> bool)`, and keep `prepare` as "none revealed" so existing callers and tests stand.
  - Spoilers are numbered in order across the message, counting only those `rewrite_text` turns into spoilers.
  - Hidden: each word, with its trailing space, of the flattened text (`plain_mentions`, as today) becomes `[escaped word ](stoat:spoiler/N)`.
  - Revealed: today's `code_span`.
  - `InternalLink::Spoiler(usize)`.
  - `plain_text` and `snippet`: `[spoiler]` (assumption 6).
- `src/store.rs`:
  - `revealed_spoilers: HashSet<(String, usize)>` and `reveal_spoiler(message, index)`, next to `revealed_images` (~500, 1905)
  - `message_row` (~2078) calls `prepare_with(c, self, |n| self.revealed_spoilers.contains(&(id, n)))`
- `src/ui/markup.rs`:
  - `MarkupPlugin` gets an optional `reveal: Option<(String /*message id*/, WeakEntity<ChatView>)>`, so it's built per message row.
  - A claimed spoiler word draws its text with `text_color(transparent)` on a solid muted-foreground background, rounded only at the ends of the run. It has `cursor_pointer`, and an `on_click` that calls `view.update(cx, |this, cx| this.reveal_spoiler(message, n, cx))`, plus `stop_propagation` on mouse-down.
  - With no `reveal` (bios, embeds), the bar can't be clicked.
- `src/ui/chat.rs`: `ChatView::reveal_spoiler` calls `store.reveal_spoiler` and then `sync_pane`, as `RowAction::RevealImage` does
**Depends on:** Chunk 3 (`MarkupPlugin`).
**Risks:**
- **`MarkdownPlugin` must be `Send + Sync`.** `WeakEntity<T>` holds a `Weak<RwLock<…>>` and should be both, but check it at compile time.
  - If it isn't, the plugin holds a `futures::channel::mpsc::UnboundedSender<(String, usize)>` instead, drained by a task in `ChatView`.
- **Per-word objects leave a seam at each space.** Including the trailing space in each object keeps the bar continuous within a line.
- **CommonMark may trim a link text's trailing space.** If it does, pad the space inside the element instead.
- **Selecting across a hidden spoiler copies its words** (atomic objects copy their `.text()`). Set `.text()` to `""` or `"[spoiler]"` for hidden words.
- **`render_key` must change on reveal.** It hashes the prepared Markdown, which now differs, so it should. A test on `prepare_with` covers this.
**How to verify:** `cargo test`:
- `tests/markdown_prepare.rs`: hidden and revealed forms; two spoilers numbered 0 and 1; a mention inside a spoiler (flattened); a spoiler containing inline code (left as typed, as today); `||` with nothing inside
- `plain_text` and `snippet` show `[spoiler]`
- A store test: `reveal_spoiler` changes that row's `body` and `render_key`, and not other rows'
**Manual check:**
- A long spoiler wraps across lines as a bar. Clicking any word reveals all of it.
- Scroll away and back, or switch channels: it's still revealed.
- A reply preview and a banner for a spoiler message say `[spoiler]`.
**Shippable when:** spoilers hide and reveal as described, and no plain-text surface leaks them.

### Chunk 5: Draw role mentions, `@everyone` and `@online`
**Goal:** `<%role>` shows as the role's name in its colour, and `@everyone` and `@online` show as mention pills. Banners for them follow your notification settings.
**Files likely touched:**
- `src/markdown.rs`:
  - `Names::role(&self, id) -> Option<(String /*name*/, Option<String> /*#rrggbb*/)>`, with a default of `None`
  - In `angle_bracket`: `<%ID>` with a known role becomes `[@{escape(name)}](stoat:role/ID)`, or `stoat:role/ID/rrggbb` when coloured
  - In `rewrite_text`: `@everyone` and `@online`, when not part of a longer word (the previous character isn't alphanumeric and the next isn't a word character), become `[@everyone](stoat:everyone)` and `[@online](stoat:online)`. The fast-path `find` set needs `'@'`.
  - `plain_mentions` turns `<%ID>` into `@RoleName`
  - `InternalLink::{Role{id, colour}, Everyone, Online}`
  - Update the module doc
- `src/store.rs`:
  - `impl Names for ChatStore` gets `role`, searching `servers.values()` for the role and colouring it through `solid_colour`
  - A role rename or recolour already resyncs rows (`server_permissions_changed`), and the colour is in the prepared text, so `render_key` changes
- `src/ui/markup.rs`: `MarkupPlugin` draws:
  - Roles: a pill with a 10px dot in the colour (or muted) and `@Name` in the colour, using `hex_colour` (`src/ui/chat.rs:5165`, which may need to move to `ui/mod.rs` or `markup.rs`)
  - `@everyone` and `@online`: the same pill in the theme's accent colour, with no dot
- `tests/notify_rules.rs`: banners for a role mention you hold, a role you don't hold, `MentionsEveryone`, and `MentionsOnline` while online and while invisible, under Level::Mention
**Depends on:** Chunk 3 (`MarkupPlugin`).
**Risks:**
- **Role colours are drawn on our own accent-tinted bubbles here for the first time.** See Open Questions.
- **`@everyone` inside an email address or a word** (`foo@everyone.com`) must stay plain. Test that.
- **Searching every server for a role** costs servers × roles per mention. That's negligible, but if profiling says otherwise, add a role → server index in the store.
**How to verify:** `cargo test`:
- `tests/markdown_prepare.rs`: known, unknown and colourless roles; a hostile role name (escaped); `@everyone` and `@online` at the start, middle and end, after punctuation, inside a word, and inside code (untouched)
- A role mention in `plain_text`
- The new `notify_rules` cases
**Manual check:**
- From the web client with a second account, mention a coloured role, a colourless role, `@everyone` and `@online`.
- Each draws as a pill. The coloured role is readable in your own bubble and in others'.
- With Level::Mention on that server, the role you hold and `@everyone` give banners; the others don't.
- Recolour the role in the web client: the pill changes colour live.
**Shippable when:** role, everyone and online mentions render, and the new notification tests pass.

### Chunk 6: Offer roles, `@everyone` and `@online` in autocomplete
**Goal:** Typing `@` in a server channel suggests roles, `@everyone` and `@online` next to people, if you're allowed to mention them.
**Files likely touched:**
- `src/complete.rs`:
  - `TokenRef::Role(String)`: `wire_text` gives `<%ID>`, `token_id` gives `role:ID`, and `from_token_id` reads it back
  - A `<%` arm in `mention_spans`
  - `Suggestion::role(id, name, colour)`, which inserts a chip
  - `Suggestion::everyone()` and `Suggestion::online()`, which insert `Insertion::Text("@everyone")` and `"@online"`
  - One combined ranking for people, roles and the two specials, so a bare `@` still shows a mix. Rank everything with `rank` by name, then break ties people → roles → specials, capped at `MAX_SUGGESTIONS`.
- `src/permissions.rs`: `can_mention_roles` and `can_mention_everyone` (`has(channel_permissions(..), ChannelPermission::MentionRoles / MentionEveryone)`). `src/store.rs` gets wrappers, as `can_pin` has.
- `src/store.rs`:
  - `role_candidates(channel)`: the server's roles by rank, without the default role
  - `token_label` gets a `TokenRef::Role` arm (`@Name`)
- `src/ui/composer.rs`: `unknown_label` gets a Role arm (`@unknown-role`). `render_chip` may tint role chips with the role colour; that's optional and only if it's cheap.
- `src/ui/chat.rs`: in `suggest()`'s `QueryKind::Mention` branch (~1432), merge in roles and the specials for server channels, gated on the permissions above
**Depends on:** Chunk 5 (otherwise the role mentions you send draw as `<%…>` here).
**Risks:**
- **Adding `TokenRef::Role` breaks every exhaustive match.** The compiler lists them: `unknown_label`, `token_label`, `wire_text`, `token_id`.
- **Mixed ranking could push people down.** Ties go to people.
- **Some queries hide the specials.** With an empty query and 8+ members, `@everyone` won't show until you type `e`. That's acceptable, and matches the web client.
**How to verify:** `cargo test`:
- `tests/complete.rs`:
  - `mention_spans` finds `<%ULID>`
  - the `TokenRef::Role` round-trip
  - the role suggestion inserts a chip, and the everyone suggestion inserts text
  - mixed ranking order
- `token_label` for a role
- `tests/permissions.rs`: `can_mention_roles` and `can_mention_everyone` with and without the bit, for the owner, and for a channel override that denies them
**Manual check:**
- With permission: `@mo` suggests the Mods role; accepting gives a chip. Sending it pings the role in the web client.
- `@ev` suggests `@everyone`. Sending it notifies the second account.
- Without the permissions (a plain member account): no roles or specials are suggested.
- In a DM: none either.
**Shippable when:** role and everyone/online suggestions appear only where allowed, and what they insert is sent correctly.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: drift from before milestone 7 makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`.
- Check that `src/ui/chat.rs`'s drift count (17 at milestone 13) doesn't grow.

### Unit Tests
- **`prepare` / `prepare_with`** (`tests/markdown_prepare.rs`): for timestamps, spoilers (hidden or revealed, numbering), role mentions, and `@everyone`/`@online`:
  - every case inside code and math, where nothing changes
  - hostile names, escaped
  - malformed input, left as typed
- **Parsing:** URLs parse as `Link` nodes with the URL intact, using the `markdown` dev-dependency (as `tests/markdown_math.rs` does).
- **`plain_text` / `snippet`:** `@Role`, an absolute time, `[spoiler]`.
- **`parse_internal_link`:** every new variant, and malformed URLs.
- **`time.rs`:** each timestamp style, relative times past and future, rounding edges.
- **`quote` / `append_quote`.**
- **`complete.rs`:** role tokens, spans, suggestions, mixed ranking.
- **`permissions.rs`:** `MentionRoles` and `MentionEveryone`.

### Integration Tests
- **`tests/notify_rules.rs`:** role, everyone and online mentions under Level::Mention.
- **Store tests:** spoiler reveal changes only that row's body and `render_key`. A role recolour changes the row's prepared body (in `tests/store_roles.rs` style).

### Manual Tests
You need a second account (as in milestone 8's checks), in a server where you can grant and remove `MentionRoles` and `MentionEveryone`.
1. Post, from the web client: a code block, a long spoiler, `<t:…:R>`, `<t:…:F>`, a role mention, `@everyone` and `@online`. Check each against chunks 1, 3, 4 and 5.
2. Quote a message with a mention and send it.
3. Mention roles and everyone from RuSToat, with and without the permission.
4. Leave the channel open for two minutes: relative times move on.
5. Light and dark themes: pills, spoiler bars and the copy button are readable in both, in your own bubbles and in others'.

## Definition of Done
- [ ] Every fenced code block in messages, embeds and bios has a copy button that copies exactly its code. The language label still shows.
- [ ] "Quote" in the message menu appends `> `-prefixed lines and a blank line to the message box, with mention chips. It's missing for attachment-only and system messages.
- [ ] `<t:N>` and `<t:N:X>` render in local time per assumption 1. Hover shows the full date. `R` updates at least once a minute. Malformed tags stay as typed.
- [ ] `||…||` renders as a bar that wraps. A click reveals the whole spoiler for the session. Banners and reply previews say `[spoiler]`.
- [ ] `<%role>` renders as the role name in its colour (muted if none). `@everyone` and `@online` render as pills. Unknown roles stay as typed.
- [ ] Banners fire for role, everyone and online mentions that include you, under Level::Mention (tests in `tests/notify_rules.rs`).
- [ ] `@` suggests roles, `@everyone` and `@online` only in server channels and only with `MentionRoles` or `MentionEveryone`. A role suggestion sends `<%ID>`.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. `src/ui/chat.rs` rustfmt drift doesn't grow.
- [ ] Every chunk's manual check is recorded in `docs/plans/message-markup-implementation.md`.
