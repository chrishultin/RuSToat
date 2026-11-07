# Implementation Record: Invites (RuSToat milestone 18)

_Plan: docs/plans/invites.md_
_Started: 2026-11-07_

Pre-existing uncommitted changes, kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt `Diff in` counts (2026-11-07): `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15, `src/markdown.rs` 1. Zero in `src/api/error.rs`, `src/links.rs`, `src/permissions.rs`, `src/main.rs`, `src/ui/actions.rs`, `src/assets.rs`, `src/shortcuts.rs`, `tests/links.rs`, `tests/permissions.rs` and `tests/store_servers.rs`.

Supervision: continuous. The user asked for every manual check to be batched at the end.

## Chunk 1: Join a server from a code or link

### Changes Made
- `src/api/client.rs`:
  - `fetch_invite(code) -> InviteResponse` (`GET /invites/{code}`)
  - `join_invite(code) -> InviteJoinResponse` (`POST /invites/{code}`, no body)
  - The two models added to the hand-formatted import list.
- `src/invites.rs` (new, pure):
  - `InviteHosts` (`new(app)`, `is_official`, `placeholder`), `is_official_app`, `is_code`
  - `code_from_link`: the own host (port included) always, and `stt.gg` / `rvlt.gg` / the Stoat and Revolt hosts on the official instance only. It rejects user-info URLs, ports on the official hosts, and `discover`.
  - `parse_input`, with the `NOT_AN_INVITE` and `OTHER_INSTANCE` texts
  - `member_count_text`
  - `InvitePreview` / `InviteTarget` (from either `InviteResponse` shape), with `detail()` and `server_id()`
  - `already_joined(&ApiError)` and `join_error_text(&ApiError)`
  - 12 unit tests.
- `src/ui/invites.rs` (new): the `JoinDialog` entity (input, lookup, preview, Join/Open, errors under the field, "Joining as @name#tag"), `JOIN_DIALOG_WIDTH`, `render_preview`.
  - Editing the field drops a stale preview, and a lookup that returns for a code no longer typed is ignored.
  - Enter looks up, or joins once a preview is shown.
- `src/ui/chat.rs`:
  - `invite_hosts()`, `open_join_dialog(code, window, cx)`, `is_invite_member`, `open_invite` and `invite_joined`
    - A server join is added with `add_server(server, channels, [])`, then `fetch_own_member` if needed.
    - A group join adds only users not already known, then `add_channel`.
  - The rail gets a "+" `rail_button` ("Join a server") after the servers, behind a second divider when there are servers.
- `src/ui/actions.rs`: the `JoinServer` action.
- `src/main.rs`: "Join Server…" in the Stoat menu after Add Account…. Its handler opens the dialog for `root.shown_chat()`, and does nothing without one.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- `tests/api_invites.rs` (new): server and group lookups, `NotFound`, server join with an empty request body, group join, and `AlreadyInServer` as an `Api` error.
- `tests/store_servers.rs`: a join response then its `ServerCreate`, and the reverse. Both give one rail entry, with the emoji.

### Divergences
- **"Joining as @name#tag" always shows** (Assumption 12 said only with several accounts). `ChatView` doesn't know how many accounts are signed in, and the line is harmless with one.
- **The rail's second divider** only shows when there are servers, to avoid two dividers in a row.
- **The JoinDialog is its own entity** (like `IdentityEditor`) rather than fields on `ChatView`, which keeps `chat.rs` smaller. The plan allowed either.

### Verification Performed
- `cargo test --lib invites`: 12 passed.
- `cargo test --test api_invites --test store_servers`: 6 and 6 passed. The first run of `joining_a_server_sends_no_body` failed on an incomplete test fixture (`approximate_member_count` missing), which was fixed in the fixture.
- `cargo test`: 875 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after fixing `manual_is_multiple_of` and `let_and_return`.
- rustfmt `Diff in` counts:
  - `src/api/client.rs` 3 and `src/ui/chat.rs` 17: unchanged.
  - `src/main.rs` 0 and `src/ui/actions.rs` 0.
  - New files 0, and `tests/store_servers.rs` 0.

### Manual Validation
Deferred: the user asked for every manual check to be run together at the end.

## Chunk 2: Invite cards in messages, and invite links that open in RuSToat

### Changes Made
- `src/markdown.rs`: `pub fn visible_text_spans(content)`, the pieces of text outside code, math and spoilers (built on the private `segments` and `spoiler`). What `prepare` makes is unchanged.
- `src/invites.rs`:
  - `CARDS_MAX = 5` and `InviteHosts::has_links`
  - `invite_codes(content, &hosts)`: words split on whitespace and `<>()[]"'`, with Markdown `*_~` and trailing punctuation trimmed; scheme optional, deduplicated, capped at 5, with a cheap early exit when the text has no `/`
  - `is_invite_embed(&Embed, codes, &hosts)`
  - 2 new unit tests (14 in the module).
- `src/store.rs`:
  - `ChatStore.invite_hosts` with `set_invite_hosts`
  - `MessageRow.invites: Vec<String>`, filled in `message_row` for non-system messages
  - website embeds whose `url` or `original_url` is one of those invites are dropped (embed keys keep their original index)
- `src/ui/chat.rs`:
  - **The cache:** `invites: HashMap<code, InviteLookup>` (Loading / Found / Invalid / Failed), `invite_queue`, `invite_loading` and `invite_joins`.
  - **Fetching:** `fetch_invites(channel)` runs from both branches of `sync_pane`. `next_invite_lookup` makes one lookup at a time; on a 429 it waits `retry_after` and requeues the same code.
  - **Cards:** `invite_cards()` builds the per-render card states (icon URL, member, joining) and passes them through the `Pickers` snapshot (empty in search and pins).
  - **Actions:** `join_invite_card` (joins straight from the card, toasts success or error; already in → open), `retry_invite`, and `RowAction::JoinInvite` / `RetryInvite`.
  - **Drawing:** `render_invite_card` (a fixed 64 px height in every state, max 400 px wide), drawn after the embeds when not a preview.
  - **Links:** `open_external` sends invite links of this instance to `open_join_dialog(Some(code))` before any warning. This covers plain links, links whose text isn't their address, and embed links.
  - The invite hosts are set on the store before `set_media` when the root config loads, so the re-sync adds the cards.
- `tests/store_invites.rs` (new): codes on a row, none without hosts, none for system messages, and a duplicate website preview dropped while another embed is kept.
- `tests/store_live.rs`: `invites: Vec::new()` in its hand-built `MessageRow`.

### Divergences
- **The "bare" check (~7333) didn't need changing.** Invite codes only come from text, so a message with cards always has a body.
- **Clicked invite links are caught in `ChatView::open_external`**, not `People::link_handler`, so every link-opening path (Markdown links, masked links, embed links) is covered by one check.
- **`InviteLookup::Found` / `InviteCard::Found` hold `Rc<InvitePreview>`**, because clippy's `large_enum_variant` flagged the plain value.
- **No remeasure was needed:** every card state is the same height, as the plan's Risks suggested.

### Verification Performed
- `cargo test --lib invites`: 14 passed. `cargo test --test store_invites`: 4 passed. `store_live`: 37 passed.
- `cargo test`: 881 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after the `Rc` change.
- rustfmt `Diff in` counts:
  - `src/ui/chat.rs` 17, after splitting one long line that had broken an existing hunk in two; `src/store.rs` 15; `src/markdown.rs` 1. All unchanged.
  - `src/invites.rs`, `tests/store_invites.rs` and `tests/store_live.rs`: 0.

### Manual Validation
Deferred to the end, at the user's request.
