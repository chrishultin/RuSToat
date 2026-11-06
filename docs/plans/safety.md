# Plan: Safety (RuSToat milestone 11)

## Summary
Milestone 11 of the [roadmap](roadmap.md) adds three safeguards that Stoat's web client has:
- **Blocked users' messages** collapse into "N blocked messages", which you can click to show.
- **External links** ask before opening, with a stronger warning when a link's text is a different address.
- **Mature (NSFW) channels** are behind an 18+ gate.

## Approach
Each feature follows the web client (`stoatchat/for-web` v0.16.3), apart from the decisions listed under Assumptions. The research is summarised there.

**Blocked messages: rows built in the store.** `ChatStore::timeline` already builds the rows the view draws (`TimelineRow`). Runs of consecutive messages from `Blocked` authors become one `TimelineRow::Blocked { ids }` row, unless they've been revealed.
- Reveals live on the store for the session (`revealed_blocked: HashSet<message id>`), like `revealed_images` and `revealed_spoilers`. They survive scrolling and channel switches.
- The rejected alternative was folding rows in the view: `with_unread_divider`, `diff_rows` and jumping to a message all work on store rows, so the view would have to re-teach each of them.

**Link warnings: one place opens external URLs.** Today `cx.open_url` is called from about six places in `src/ui/chat.rs` (the Markdown link handlers, embed titles, thumbnails and video cards). They all go through a new `ChatView::open_external(url, text, …)`. It opens trusted origins at once and asks about everything else.

**Giving the dialog the link's text.** `TextView::on_link_click` only receives the URL, not the link's text, and the dialog needs the text to spot deceptive links. So `markdown::prepare` gets a final pass, `guard_links`:
- It parses the prepared Markdown with the `markdown` crate (the parser inside `TextView`, already in the tree; it moves from dev-dependencies to dependencies).
- It finds external links whose text isn't their address, and rewrites each to `[same text](stoat:link?url=…&text=…)`, form-urlencoded.
- The link handler decodes it through `parse_internal_link`.
- Bare URLs and autolinks show their address, so they're left alone, and the pass only runs when the content has `](` or `]:`.

Rejected alternatives:
- **Claiming links in `MarkupPlugin`.** Plugin elements are atomic and don't wrap, and links are often long.
- **Matching `[text](url)` by hand.** It breaks on reference links, titles and nested brackets.

**Trust and age confirmations are saved per account.** A new `SafetyFile` (`src/safety.rs`) is modelled on `PlacesFile`: JSON in the app folder, keyed by API URL and user ID. It holds:
- `trusted_origins`
- `adult_confirmed`
- `mature_channels` (channels entered)

It isn't synced, as in the web client (localforage, per user, not in `SYNCED_KEYS`).

**The age gate replaces the channel's main area.** `render_main` draws the gate instead of the message list and the message box while the open channel is mature and not yet allowed. The channel's history isn't loaded or marked read until you enter it.

## Assumptions
1. **Blocked messages** (as for-web's `Messages.tsx` and `BlockedMessage.tsx`, plus click-to-show from the roadmap):
   - Only `RelationshipStatus::Blocked` authors are collapsed, not `BlockedOther`, as in the web client.
   - Each run of consecutive messages from blocked authors is one row: "1 blocked message" or "N blocked messages". Any other message ends the run. System messages and the user's own pending messages count as other messages.
   - Clicking the row shows that run's messages until the app quits. The web client's row can't be clicked; the roadmap asked for it.
   - A reply preview whose original is by a blocked user shows "Blocked user" and no snippet, as in for-web's `MessageReply.tsx`.
   - Search results and the pins list are unchanged: you asked for those messages. Banners already skip blocked authors (`notify.rs`).
   - Unblocking someone (`RelationshipUpdate`) resyncs loaded panes, so their messages show at once. Blocking someone collapses them.
2. **Link warnings follow the web client** (user decision: every external link, as in for-web's `anchors.tsx`, `LinkWarning.tsx` and `LinkSafety.ts`):
   - "External" means `http`, `https` and `mailto` (as `is_external_link`).
   - Links to the user's own instance don't ask: the web client's in-app links and Stoat's own media (`MediaUrls` hosts). Nor do attachments and the image viewer's "Open in browser", which open Stoat-hosted files.
   - **Every other external link asks first,** unless its origin (scheme, host and port) is trusted.
   - The dialog's title is "External links can be dangerous!" and its body "Are you sure you want to go to {url}?" Buttons: Cancel and Continue.
   - There are three levels, as for-web's "scrutiny":
     - **Text is the address** (after normalising a missing scheme and a trailing `/`): a "Don't ask me again for {origin}" checkbox. Ticking it and continuing trusts the origin.
     - **Text is something else** (`[docs](…)`): it also says `You clicked on "{text}"`. There's no trust checkbox.
     - **Text is a different web address or domain** (it parses as a URL, or as a host with a dot, whose host differs from the destination's): it says "Be careful! This is not the same as the link that was displayed:" and shows the text. Continue stays disabled until "I understand the risk" is ticked, and ticking it doesn't trust the origin.
   - `mailto:` links have no origin, so they always ask, with no trust checkbox.
   - Trusted origins are saved per account and never synced. There's no way to un-trust one yet; a settings section can come with milestone 14.
3. **Mature channels** (as for-web's `AgeGate.tsx`):
   - A channel is mature when it's a server text channel or a group with `nsfw: true`. Server `nsfw` is ignored, as the web client ignores it. DMs and Saved Notes can't be mature.
   - The gate shows a warning icon, the channel's name, "This channel is marked as mature.", a checkbox "I confirm that I am at least 18 years old." and an "Enter channel" button. The button stays disabled until the box is ticked.
   - **No "Back" button.** RuSToat has no navigation history; the channel list is right beside the gate.
   - Entering saves both the 18+ confirmation and that channel, per account, across relaunches (user decision). The checkbox starts ticked once you've confirmed before, but each mature channel still needs "Enter channel" once.
   - **No region check** (user decision). The web client asks `geo.revolt.chat` and blocks mature channels in age-restricted regions; RuSToat doesn't.
   - Until a mature channel is entered, it isn't loaded, isn't marked read, and its search and pins panels aren't offered. Its unread dot and banners are unchanged, as in the web client.
4. **Turning a channel mature while it's open** (a live `ChannelUpdate` with `nsfw: true`) puts the gate up, unless that channel was already entered.

## Plan

### Chunk 1: Collapse blocked users' messages
**Goal:** Runs of messages from people you've blocked show as one "N blocked messages" row, which you can click to show them.
**Files likely touched:**
- `src/store.rs`:
  - `TimelineRow::Blocked { ids: Vec<String> }`
  - `revealed_blocked: HashSet<String>` and `reveal_blocked(ids)`, next to `revealed_spoilers`
  - the run-folding in `timeline()`, after the rows are built and before `mark_group_ends`
  - `row_key` for the new row (its first ID)
  - `with_unread_divider` must treat a blocked row's IDs as messages (the divider goes before the row if its first ID is after `after`)
  - `ReplyPreview` from a blocked author: author "Blocked user", empty snippet, no avatar
  - `is_blocked(user)`
- `src/ui/chat.rs`:
  - `render_row`'s `TimelineRow::Blocked` arm: a muted, centred row with `IconName::UserX`, "N blocked messages" and a pointer cursor. Clicking it sends `RowAction::RevealBlocked(ids)`, which calls `store.reveal_blocked` and then `sync_pane`.
  - `jump_to_message` to a hidden message (a reply's original, a search result) reveals its run first, so the jump lands.
  - Resync loaded panes when a relationship changes to or from `Blocked`, if that isn't already done.
**Depends on:** None.
**Risks:**
- **Every `match` on `TimelineRow` gains an arm.** The compiler lists them.
- **The unread divider and "jump to present" key off row IDs.** A divider whose anchor is a collapsed message must still land, or the row must be revealed. Test it.
- **`diff_rows` keys rows by `row_key`.** Revealing a run replaces one row with several, so check it emits ops the scroller handles. A `Reset` is acceptable but loses the scroll position; prefer the existing path for replaced rows.
**How to verify:** `cargo test`, with a new `tests/store_blocked.rs`:
- consecutive blocked messages fold into one row with their IDs
- another author's message, or a system message, splits the run
- `BlockedOther` isn't folded
- `reveal_blocked` shows that run only
- the unread divider before and inside a collapsed run
- a reply preview of a blocked author's message
- unblocking unfolds
**Manual check:** Block the second account and post three messages from it, then one from a third person, then two more from it. You should see "3 blocked messages", the other message, then "2 blocked messages". Click one: those messages show, and stay shown after switching channels. A reply to a hidden message previews "Blocked user".
**Shippable when:** blocked authors' runs collapse and reveal as described, and the new store tests pass.

### Chunk 2: Ask before opening external links
**Goal:** Clicking any external link asks first, unless you've said not to ask for that site.
**Files likely touched:**
- `src/safety.rs` (new):
  - `SafetySettings { trusted_origins: BTreeSet<String>, adult_confirmed: bool, mature_channels: BTreeSet<String> }`
  - `SafetyFile::for_account(instance, user)` with `load` and `save`, modelled on `PlacesFile` (same per-account JSON shape, file `safety.json`)
  - `origin(url) -> Option<String>`, using the `url` crate (`None` for `mailto:`)
  - `LinkCheck` (`Plain`, `Text(String)`, `Mismatch(String)`)
  - `classify(url, text: Option<&str>) -> LinkCheck`. Chunk 2 only passes `None`, which is `Plain`.
- `src/lib.rs`: `pub mod safety;`
- `src/ui/chat.rs`:
  - a `safety: SafetySettings` field loaded with the account, as `places` is
  - `fn open_external(&mut self, url: &str, text: Option<String>, window, cx)`: own-instance URLs and trusted origins open with `cx.open_url`; anything else opens the dialog (`window.open_alert_dialog`, as `confirm_delete` does), with a `Checkbox` for "Don't ask me again for {origin}". Continue opens the URL, and trusts and saves the origin when ticked.
  - Route every external `cx.open_url` through it: the Markdown link handler (`People::link_handler`), the embed description and bio handlers, embed titles, playable thumbnails and video cards. Attachments, file cards and the image viewer keep opening directly.
  - `People`/`OnLink` reach the view through its `WeakEntity`, as they do now.
**Depends on:** None.
**Risks:**
- **Not every handler can reach `ChatView`.** Embed and bio handlers are free functions with no view handle; `render_embed_card` gets `on_action`. A new `RowAction::OpenLink { url, text }` routes embed links, and the bio handler goes through `People`.
- **`open_alert_dialog` with a checkbox needs state.** Keep the ticked flag in an `Entity<bool>` or `Rc<Cell<bool>>` captured by `on_ok`, and re-render on toggle. Chunk 3's "I understand" checkbox reuses it.
- **Own-instance detection.** `MediaUrls` knows the CDN host, and the web app URL comes from `root.app`. If neither is known yet, ask.
**How to verify:** `cargo test`:
- unit tests in `src/safety.rs`: `origin` for http, https with a port, and mailto; trusted-origin matching; `SafetySettings` round-trips through `SafetyFile` in a temp directory, an older file without the field loads, and accounts don't share settings
- an `is_own_instance` unit test
**Manual check:**
- Click a bare URL: the dialog shows it. Continue opens it.
- Tick "Don't ask me again" and continue. The next link to that site opens at once, even after a relaunch.
- A `mailto:` link always asks, with no checkbox.
- An attachment opens without asking.
**Shippable when:** every external link asks, unless its origin is trusted, and trust is saved per account.

### Chunk 3: Warn harder when a link's text is something else
**Goal:** The dialog shows what the link said, and makes you confirm when the text is a different address.
**Files likely touched:**
- `Cargo.toml`: move `markdown = "1"` from `[dev-dependencies]` to `[dependencies]`. It's the same version already in `Cargo.lock`, through gpui-base.
- `src/markdown.rs`:
  - `guard_links(prepared) -> String`, run at the end of `prepare_with`, and only when the content contains `](` or `]:`.
  - It parses with the options `TextView` uses (GFM plus math, as `tests/markdown_prepare.rs::parse` does).
  - It walks `Link` nodes whose URL `is_external_link` and whose text (`node.to_string()`) isn't the URL (normalised as in assumption 2).
  - It replaces each node's source span with `[children source](stoat:link?url=…&text=…)`, building the query with `url::form_urlencoded::Serializer`. The children's source runs from the first child's start offset to the last child's end, so reference links work too.
  - `InternalLink::External { url, text }` in `parse_internal_link`.
- `src/safety.rs`: `classify` gets real text:
  - `Plain` when the text is the address
  - `Mismatch(text)` when the text parses as a URL, or as a dotted host with no spaces, whose host differs
  - `Text(text)` otherwise
- `src/ui/chat.rs`:
  - the link handler sends `InternalLink::External` to `open_external(url, Some(text))`
  - the dialog adds `You clicked on "{text}"` for `Text`
  - for `Mismatch`, it adds the "Be careful!…" block and an "I understand the risk" checkbox that Continue waits for
  - there's no trust checkbox for `Text` or `Mismatch`
**Depends on:** Chunk 2.
**Risks:**
- **Re-parsing costs.** `prepare` runs for every row on every `sync_pane`. The `](`/`]:` fast path keeps plain messages free; measure a channel full of links if it feels slow.
- **Span offsets.** `markdown` positions are byte offsets in the prepared string. Rewrite from the last link to the first so earlier offsets stay valid.
- **Nested constructs.** A link inside a spoiler is already flattened by chunk 4 of milestone 10, and links in code are code. Images (`![](…)`) are `Image` nodes, not `Link`, so they're untouched.
- **Selection copy** of a guarded link copies its text, which is fine. Copying as Markdown would give the `stoat:link?…` URL; that's acceptable.
- **The `url=` parameter must round-trip exactly,** including `#` fragments, `&` and non-ASCII. Test it.
**How to verify:** `cargo test`:
- `tests/markdown_prepare.rs`:
  - `[docs](https://a.example/x)` becomes a `stoat:link?…` link with the same text
  - `<https://a.example>` and bare URLs are unchanged
  - a reference-style link is guarded
  - a link whose text is its own URL is unchanged
  - links in code are untouched
  - a parse check that the URL round-trips through `parse_internal_link`, including `?`, `&`, `#` and `é`
- `src/safety.rs` `classify` cases: same address, `www.` vs `https://www.`, a trailing slash, a different domain as text, plain words, an email address as text
**Manual check:**
- From the web client, send `[google.com](https://example.com)`. Clicking it shows the "Be careful!" warning, and Continue only works after ticking.
- `[the docs](https://example.com)` shows "You clicked on "the docs"" with no trust box.
- A bare `https://example.com` behaves as in chunk 2.
**Shippable when:** deceptive links get the strong warning and the other levels behave as in assumption 2.

### Chunk 4: Age gate on mature channels
**Goal:** A mature channel shows an 18+ gate until you confirm and enter it.
**Files likely touched:**
- `src/store.rs`:
  - `is_mature(channel) -> bool` (`TextChannel` or `Group` with `nsfw`)
  - `ChannelEntry`/sidebar rows may carry it, for an optional marker; not required
- `src/safety.rs`: `SafetySettings::allows(channel)` (`adult_confirmed && mature_channels.contains(channel)`) and `enter(channel)`
- `src/ui/chat.rs`:
  - `render_main` draws `render_age_gate(channel)` in place of the timeline and the message box when `store.is_mature(ch) && !safety.allows(ch)`
  - the gate: a centred card with `IconName::TriangleAlert` (or the closest available), "#name" (or the group's name), "This channel is marked as mature.", a `Checkbox` ("I confirm that I am at least 18 years old.", ticked if `adult_confirmed`), and a primary "Enter channel" button disabled until ticked
  - entering calls `safety.enter`, saves, then loads the channel (`ensure_current_loaded`)
  - `ensure_current_loaded`, `maybe_ack_current`, and the search and pins panel toggles skip a gated channel
  - a `ChannelUpdate` setting `nsfw` re-renders, which happens already through `cx.notify()`; check it
- `src/assets.rs`: the gate's icon, if it isn't a default component icon
**Depends on:** Chunk 2 (`SafetyFile`).
**Risks:**
- **Several code paths load or acknowledge the open channel:** opening it, switching accounts, window activation and keyboard navigation. Gate them in `ensure_current_loaded` and `maybe_ack_current`, not at each caller.
- **The composer still holds that channel's draft.** It's hidden, not cleared; check that switching away and back keeps it.
- **The checkbox's state lives in the view.** Keep it on `ChatView` (`age_gate_ticked: bool`), reset when the open channel changes.
**How to verify:** `cargo test`:
- store tests: `is_mature` for a mature text channel, a group, a DM, and a channel turned mature by a `ChannelUpdate`
- `safety.rs` tests: `allows` needs both the confirmation and the channel, and `enter` saves both
**Manual check:**
- Mark a test channel mature in the web client. Opening it in RuSToat shows the gate with no messages, and it stays unread.
- "Enter channel" is disabled until you tick the box. Entering shows the messages and marks it read.
- Relaunch: it opens straight in.
- A second mature channel shows the gate with the box already ticked.
- Marking an open, never-entered channel mature puts the gate up live.
**Shippable when:** mature channels are gated as in assumption 3, and confirmations persist per account.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`. A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines hasn't grown (`src/ui/chat.rs` is 17).

### Unit Tests
- **`safety.rs`:** `origin`; trust matching; `classify` in each of the three levels, plus edge cases (missing scheme, `www.`, trailing slash, mailto, an email address as text); `allows`/`enter`; `SafetyFile` per-account round trip and loading an older file.
- **`markdown.rs`:** `guard_links` (inline, reference, autolink, bare URL, in code, round trip of tricky URLs); `InternalLink::External` parsing and malformed `stoat:link` URLs.
- **`store.rs`:** blocked-run folding, reveal, unread divider, reply previews; `is_mature`.

### Integration Tests
- **`tests/store_blocked.rs`:** a channel history with blocked, other and system authors, through `timeline()` and `with_unread_divider`; unblocking through a relationship event.
- **`tests/markdown_prepare.rs`:** guarded links parse as `Link` nodes with the `stoat:link?…` URL and the original text.

### Manual Tests
You need a second account in a server where you can mark a channel mature.
1. Run every chunk's manual check.
2. Block the second account mid-conversation: its messages fold at once. Unblock: they come back.
3. Every kind of link: bare, `<…>`, `[text](…)`, deceptive, mailto, an embed's title, a YouTube video card, an attachment. Each asks or doesn't as assumption 2 says.
4. With two accounts in RuSToat, trust a site and enter a mature channel on one. The other account still asks and still gates.
5. Light and dark themes: the blocked row, the dialogs and the gate are readable.

## Definition of Done
- [ ] Runs of messages from `Blocked` authors show as "1 blocked message" / "N blocked messages". Clicking shows them for the session. Replies to them preview "Blocked user".
- [ ] Every external link, from messages, embeds and bios, asks before opening, except own-instance URLs, attachments and trusted origins.
- [ ] "Don't ask me again" (only when the text is the address) trusts that origin for the account, across relaunches.
- [ ] A link whose text is a different address shows the "Be careful!" warning, and Continue waits for "I understand the risk".
- [ ] Mature text channels and groups show the 18+ gate. Nothing loads and nothing is marked read until you enter. The confirmation and each entered channel are saved per account.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/safety-implementation.md`.
