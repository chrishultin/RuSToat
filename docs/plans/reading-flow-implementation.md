# Implementation Record: Reading flow (milestone 9)

_Plan: [reading-flow.md](reading-flow.md)_
_Started: 2026-11-06_

Branch: `feature/reading-flow`, from `main` (a6f284d). Running in continuous mode.

Before chunk 1, the uncommitted window-reopen work in `src/main.rs` and `README.md` (from before this plan) was committed on its own as 9520c37, so chunks 5 and 6 can edit `main.rs` cleanly. Kept out of every commit: `.gitignore` (a duplicated Riptide entry added by tooling) and the untracked `.shadow/`.

Formatting: `src/ui/chat.rs`, `src/store.rs` and `src/api/client.rs` already differ from rustfmt's output (17, 15 and 3 hunks at the start). Running `rustfmt` on them would reformat unrelated code, so only the lines each chunk adds are formatted, and checked by confirming those counts don't grow. New files are formatted whole.

## Chunk 1: Copy link and IDs

### Changes Made
- `src/api/client.rs`: `RootConfig.app: Option<String>` (`#[serde(default)]`), the instance's web client.
- `src/links.rs` (new, in `lib.rs`): `message_link(app, server, channel, message)` builds `{app}/server/{server}/channel/{channel}/{message}`, or `{app}/channel/{channel}/{message}` for DMs and groups, dropping a trailing `/` from `app`.
- `src/store.rs`: `ChannelSection.id: Option<String>`, the category's ID (`None` for uncategorized).
- `src/ui/chat.rs`:
  - `ChatView.web_app`, set from `root.app` when `GET /` loads (logged with the media URLs).
  - `RowAction::CopyLink(id)` / `CopyId(id)`. The message menu has "Copy message link" (only when `Pickers.can_link`, i.e. `web_app` is known) and "Copy message ID", after "Copy text".
  - `sections()` returns `Vec<ChannelSection>` instead of tuples, so the channel list has category IDs.
  - Category headers and channel rows have a right-click menu ("Copy category ID", "Copy channel ID") via a shared `copy_id_item`. `render_channel_row` now returns `AnyElement`.
  - `rail_button`'s `menu` is now `Vec<(label, action)>`. Server icons have "Mark as read" and "Copy server ID".
- `src/ui/switcher.rs`: passes the account menu as a one-item list.
- `src/assets.rs`: embeds the Lucide `Link` icon; the icon test covers it.
- `tests/links.rs` (new): server link, DM link, trailing slash, and `app` present (fixture) and missing (older instance).
- `tests/store_ready.rs`: the section shape test also checks category IDs.

### Divergences
- **Minor:** "Copy channel ID" is on Home's DM and group rows too, since they're channel rows.
- **Minor:** the plan's risk about an `/app` prefix was checked before building. stoat.chat's web client serves its files from `/app/`, but its own "Copy link" builds `new URL("/server/…/channel/…/{message}", location.origin)`, and `https://stoat.chat/server/…` serves the client. So no prefix.

### Verification Performed
- `cargo test --test links`: 4 passed.
- `cargo test`: 581 passed, 0 failed, 5 ignored.
- `cargo clippy --all-targets`: no warnings.
- The app's debug log shows `web app: Some("https://stoat.chat")` after `GET /` loads.

### Manual Validation
The user ran the app, copied message links (server channel and DM) and opened them in the browser, and copied message, channel, category and server IDs. They confirmed all of it looks right.
