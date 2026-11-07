# Plan: Invites (RuSToat milestone 18)

## Summary
Milestone 18 of the [roadmap](roadmap.md) lets you join servers and bring people into them:
- **Join from a code or link:** a "Join a server" dialog (a "+" at the bottom of the server rail, and Stoat → Join Server…) looks the invite up, shows the server, and joins it, or opens it if you're already in.
- **Invite cards in messages:** invite links in a message show as cards under it (server icon, name, member count) with a Join or Open button. Clicking an invite link opens the same preview in RuSToat rather than the browser.
- **Create an invite:** for a channel, with a maximum number of uses and an expiry, then copy its link. From the channel's right-click menu, the server's right-click menu, and a button in the channel header, each shown only when you may invite.

Group invites join and show as cards too. Creating them waits for milestone 20 (Groups and DMs). Listing and revoking invites is milestone 23.

The references are Stoat's server (`stoatchat/stoatchat` v0.16.2: `crates/delta/src/routes/invites/{invite_fetch,invite_join,invite_delete}.rs`, `crates/delta/src/routes/channels/invite_create.rs`, `crates/core/database/src/util/reference.rs` `as_invite`, `crates/core/database/src/models/channel_invites/{model.rs,ops/mongodb.rs}`, `server_members/model.rs` `Member::create`, `channels/model.rs` `add_user_to_group`, `users/model.rs` `can_acquire_server`, `crates/delta/src/util/ratelimits.rs`, `crates/core/config/Revolt.toml`), the web client (`stoatchat/for-web` `stoat-for-web-v0.16.2`: `components/markdown/plugins/anchors.tsx`, `components/routing/index.tsx` `RE_INVITE_EXACT`, `components/ui/components/features/messaging/elements/Invite.tsx`, `components/modal/modals/{Invite,JoinServer,CreateInvite}.tsx`, `app/menus/{ChannelContextMenu,ServerContextMenu}.tsx`), and the old `revoltchat/revite` (`components/common/messaging/embed/EmbedInvite.tsx`, `controllers/modals/components/CreateInvite.tsx`). All were read on 2026-11-07. The server's `CHANGELOG.md` stops at 0.15.5, so the code at the tag is the source of truth.

## Approach
**What the API does** (from the server source):
- **Resolving a code** (`Reference::as_invite`, shared by every `/invites/{code}` route):
  - A string that parses as a ULID is a server ID. A `discoverable` server answers with a made-up `Invite::Server` (its first channel); any other server is `NotFound`. So a 26-character server ID works as a code for discoverable servers.
  - Anything else is looked up as an invite. The query leaves out expired and used-up invites, so **unknown, expired and used-up codes are all 404 `{"type":"NotFound"}`**. There's no "expired" error.
  - Real codes are **8 characters** (`nanoid!(8)` from digits and letters without I, L, O, U).
- **Fetch:** `GET /invites/{code}` returns `InviteResponse` (tagged `type`).
  - `Server`: `code`, `server_id`, `server_name`, `server_icon?`, `server_banner?`, `server_flags?`, `channel_id`, `channel_name`, `channel_description?`, `user_name` (the creator), `user_avatar?`, `member_count` (approximate: cached in Redis, 0 if Redis is down).
  - `Group`: `code`, `channel_id`, `channel_name`, `channel_description?`, `user_name`, `user_avatar?`. No member count and no icon.
  - It needs no login. It doesn't say how many uses are left or when the invite expires.
- **Join:** `POST /invites/{code}`, no body, returns `InviteJoinResponse` (tagged `type`). The checks run in this order:
  1. A bot account: 400 `IsBot`. RuSToat only signs in users.
  2. `TooManyServers { max }` (400), **for group invites too**. The limit is 100, or 50 for new accounts, on `api.stoat.chat`.
  3. `NotFound` (404).
  4. **One use is counted** (an atomic `$inc` with the same validity filter; `NotFound` if it lost a race).
  5. **Server:** `Banned` (403) or `AlreadyInServer` (409). **These come after the use is counted**, so joining a server you're already in, or are banned from, uses up one of the invite's uses. On success it returns `Server { channels, server }`. The channels are only the ones you can see. Stoat then sends `ServerMemberJoin` to the server and **`ServerCreate` to you**.
  6. **Group:** `AlreadyInGroup` (409) or `GroupTooLarge { max }` (403). On success it returns `Group { channel, users }`, sends a `UserAdded` system message, and sends `ChannelCreate` to you.
  - There's no MFA, email or `invite_only` check on joining. `invite_only` only gates sign-up.
- **Create:** `POST /channels/{id}/invites` with an optional `DataCreateInvite` body returns `Invite` (tagged `type`; `_id` is the code; `max_uses?`, `uses`, `expires?`).
  - `max_uses`: 1–100, else `FailedValidation` (400).
  - `expires`: an ISO-8601 **timestamp**, not seconds; the model's doc comment is wrong. It must be after now and no more than `max_invite_duration_days` ahead (default 30; sent in `GET /` as `features.limits.global.max_invite_duration_days`), else `InvalidOperation` (400). A missing or malformed body counts as "no limits".
  - Needs `ChannelPermission::InviteOthers` (in the default permissions), else `MissingPermission` (403).
  - Works for server text and voice channels (`Invite::Server`) and groups (`Invite::Group`). DMs and Saved Notes are `InvalidOperation`.
- **Rate limits:**
  - Every `/invites/{code}` call is in the catch-all bucket: 20 per 10 s per session.
  - Create is in the per-channel `channels` bucket: 15 per 10 s.
  - A 429 carries `retry_after`, which `ApiError::RateLimited` already reads.

**What the web client does:**
- **Links:** a link is an invite when its path is `/invite/{code}` on the instance or a known Stoat host (`stoat.chat`, `beta.stoat.chat`, `old.stoat.chat`, `revolt.chat`, `app.revolt.chat`). `https://stt.gg/{code}` and `https://rvlt.gg/{code}` are short forms. revite also scanned for `rvlt.gg` and skipped the code `discover`.
- **Cards:** for-web draws a card only when the link's text is its address. It shows a 42 px icon, the name, and "N members", with Join, or a disabled "Joined". Join navigates to the server. A group invite draws an empty card.
  - revite scanned the whole message text, deduplicated, kept up to 5 cards, and showed "Invalid invite!" for unknown codes.
- **Join dialog:** for-web joins straight away with no preview, and its group branch is a TODO. Its `/invite/{code}` route does show a preview: icon, name, banner, then Join, or Open when already a member.
- **Create:** the channel menu ("Create invite", with `InviteOthers`), the server menu ("Invite", using the first channel where you can invite), and a hover icon on channel rows.
  - It sends **no body**, so every invite is unlimited and never expires.
  - It shows the link with Copy Link. The link is `https://stt.gg/{code}` when the instance is `stoat.chat`, else `{origin}/invite/{code}`.

**What exists already.**
- **Models:** `revolt-models` 0.16.2 has `Invite`, `DataCreateInvite`, `InviteResponse` and `InviteJoinResponse` (`v0/channel_invites.rs`). `DataCreateInvite.expires` is an `iso8601_timestamp::Timestamp`, a crate RuSToat doesn't depend on directly. So create uses a small private `Serialize` struct with `expires: Option<String>`, formatted by chrono, like `ChangeUsername` in milestone 17.
- **Client** (`src/api/client.rs`): `send` / `send_raw` (~983–1040), and the `fetch_member` / `send_friend_request` pattern (~582, ~625). `RootConfig.app` (~41) gives the web client's address. `RootLimits` (~60) has `default` and `new_user` but no `global`.
- **Errors** (`src/api/error.rs`): `describe` (~114) says "Bots can't be friends." for `IsBot`, so invite errors get their own wording in a new module, as `account_edit.rs` did. `profile_edit::error_text(err, fallback)` (`pub(crate)`) covers rate limits.
- **The store:**
  - `ChatStore::add_server(server, channels, emojis)` (`store.rs` ~2953) replaces a known server, keeping history and members, so adding the join response and then the `ServerCreate` event is safe.
  - `add_channel` (~1932) and `add_user` (~2941) do the same for groups.
  - `ChatView` already handles `ServerCreate` (`chat.rs` ~1074): `add_server`, then `fetch_own_member`, since Stoat doesn't send your own `ServerMemberJoin`.
  - `MessageRow` (`store.rs` ~111) is built by `message_row` (~2315), with `embeds` beside where invite codes would go.
- **Navigating:**
  - `ChatView::open_channel(channel, cx)` (`chat.rs` ~4250) switches to the channel's server and loads it.
  - `select_place(Place::Server(id))` (~3986) opens a server.
- **Permissions** (`src/permissions.rs`): `channel_permissions`, `has`, `in_server` and wrappers like `can_pin` (~95), plus `ChatStore` wrappers on `self.me` (`store.rs` ~2476). `InviteOthers` isn't used yet.
- **Message rows** (`chat.rs`):
  - `render_message_row` (~7217) draws embeds at ~7310–7323 through `render_embed`. `render_embed_card` (~8392) is the visual model for an invite card.
  - The "bare" check (~7333) also looks at embeds.
  - `RowAction` (~6749) is handled in `on_row_action` (~1285).
  - The `Pickers` snapshot (~6877, built at ~5624) carries per-render state into rows.
- **Links:**
  - `People::link_handler` (`chat.rs` ~7092) sends web links to `open_link` → `ChatView::open_external` (~2440). It also sends links whose text isn't their address there (`InternalLink::External`, from `markdown::guard_links`).
  - `links::message_link(app, …)` (`src/links.rs`) builds message links; tests are in `tests/links.rs`.
  - `markdown.rs` already splits text from code and math (`segments`, ~480) and finds spoilers (`spoiler`, ~644). Both are private.
- **UI:**
  - The rail is `render_rail` (`chat.rs` ~5150): Home, a divider, then servers, with no add button. `rail_button(...)` is ~8737.
  - The menus are `channel_menu` (~3327) and `server_menu` (~3382). `copy_id_item` (~8654) copies with no toast.
  - The channel header is in `render_main` (~5529–5592): Pins and Members buttons.
  - "Add friend" (`open_add_friend` ~3180, `send_friend_request` ~3213, `src/ui/friends.rs` `render_add_friend` ~294) is the model for a one-field dialog with an async step.
  - Avatars use `avatar(id, url, name, size, bg, fg)` (~6833) and `MediaUrls::file`.
  - `DropdownMenu` (gpui-component, already imported in `chat.rs`) works for the option pickers.
  - Toasts use `window.push_notification(Notification::success(..))`.
- **App menu:** `set_menus` (`src/main.rs` ~27) and the actions in `src/ui/actions.rs` (~7). Commands reach the account on screen through `MainWindow::update` → `root.shown_chat()` (`app_root.rs` ~245), as `LogOut` does.

**Decisions** (user, 2026-11-07):
1. **The copied link** follows the web client: `https://stt.gg/{code}` when the account's instance is `https://stoat.chat`, else `{app}/invite/{code}`.
2. **Creating an invite has options:** maximum uses and expiry. The web client has neither.
3. **The Join dialog previews first:** paste, look it up, see the server, then Join, or Open if you're already in. This also avoids using up an invite's use on a server you're already in (Approach, join step 5).
4. **Cards come from any invite link in the message's text**, as revite did, up to 5 per message, not only bare links. Clicking an invite link opens the preview in RuSToat, not the browser.
5. **Group invites** join and show as cards. Creating them waits for milestone 20.
6. **Create invite** is in the channel's right-click menu, the server's right-click menu ("Invite people…"), and a header button. Each is shown only when you have `InviteOthers` there.

**Which links are invites** (`src/invites.rs`, pure):
- Always: `{app}/invite/{code}`, on the account's own `app` host.
- Only when the account's `app` is `https://stoat.chat`:
  - `stt.gg/{code}` and `rvlt.gg/{code}`, but not `discover`
  - `/invite/{code}` on `stoat.chat`, `beta.stoat.chat`, `old.stoat.chat`, `revolt.chat` or `app.revolt.chat`

  These all point at `api.stoat.chat`. A self-hosted account looking a `stt.gg` code up on its own instance would only get `NotFound`, so it treats them as ordinary links.
- **Matching:**
  - The scheme is optional, `http` or `https`, and hosts match in any case.
  - The host must match exactly, so `evilstt.gg` and `stt.gg.example.com` don't count.
  - A trailing `/`, a query or a fragment are ignored.
  - The code is 1–32 ASCII letters and digits.
- **A bare code** in the Join dialog is the same 1–32 letters and digits, trimmed.

**Layout.**
- **Join a server** dialog (420 px, like Add friend):
  - "Paste an invite link or code." over one field (placeholder `stt.gg/wVEJDGVs` on stoat.chat, else `{app host}/invite/wVEJDGVs`), with Look up.
  - The preview below:
    - the server icon (48 px) or the group's initials
    - the name
    - "1,234 members · #general" (a group shows "Group · #name"), and "Invited by ada"
  - Cancel and **Join** (or **Open**) at the bottom. Editing the field drops the preview.
- **Invite card** (under the message, beside embeds, max 400 px wide, one line high like `render_embed_card`): the icon, the name, "1,234 members", and Join / Joining… / Open.
  - Loading: a muted "Loading invite…" card the same height.
  - Unknown: "This invite is invalid or has expired." with no button.
  - Failed to load: "Couldn't load this invite." with Retry.
- **Create invite** dialog: "Invite people to {server}" and "#channel".
  - Two pickers: **Expire after** (30 minutes, 1 hour, 6 hours, 12 hours, 1 day, 7 days, Never; default Never) and **Max uses** (No limit, 1, 5, 10, 25, 50, 100; default No limit). The defaults match what the web client makes.
  - Then Create. After creating, the dialog shows the link in a read-only monospace field, a summary ("Expires in 7 days · 10 uses"; "Never expires · No limit"), **Copy link**, and Done.

**Rejected alternatives.**
- **Joining straight from the dialog without a preview** (for-web). It can use up an invite on a server you're already in, and shows nothing before you commit.
- **Cards only for bare links** (for-web). The user chose revite's behaviour (Decision 4).
- **Waiting for `ServerCreate` before navigating.** The join response already has the server and the channels you can see. Adding them at once is safe, because `add_server` replaces, and it avoids a race with the event.
- **Depending on `iso8601-timestamp` to use `DataCreateInvite` directly.** That's one more direct dependency for one field. A private struct with a chrono-formatted string sends the same JSON.

## Assumptions
1. **Spoilers and code don't make cards.** Links inside `` `code` ``, code blocks, `$$math$$` or `||spoilers||` are skipped, so a card can't reveal a hidden link. Reuse `markdown.rs`'s `segments` and `spoiler` (made `pub(crate)`, or wrapped in a small `pub(crate) fn plain_text_spans`) rather than a second parser.
2. **Card fetching:**
   - Each `ChatView` caches lookups by code for the session: Loading, Found, Invalid (`NotFound`), or Failed.
   - Codes are fetched one at a time from a queue, after a pane's rows are built. Lookups share the 20-per-10 s bucket with joins, and a channel full of invites shouldn't trip it. On `RateLimited`, the queue waits `retry_after`, then goes on.
   - Previews (search results, pins, replies) don't draw cards, as with embeds.
   - Invalid stays cached. Failed retries on Retry.
3. **Join or Open** is decided when drawing:
   - **Open** if the server is in the store (server invites) or the channel is (group invites).
   - Open goes to the invite's channel if you can see it (`open_channel`), else to the server.
4. **After joining:**
   - **Server:** add the response's server and channels to the store with no emoji (`ServerCreate` brings them), and call `fetch_own_member`. Then open the invite's channel, or the server if you can't see that channel. Toast "Joined {server name}."
   - **Group:** `add_channel` and `add_user` for each user, open the group, and toast "Joined {group name}."
   - `AlreadyInServer` and `AlreadyInGroup` are treated as Open: the store hadn't caught up.
5. **A card's Join joins at once**, with no dialog, as both web clients do. The card already is the preview.
6. **Clicking an invite link** opens the Join dialog with the code filled in and looked up. This also applies to links whose text isn't their address: the preview shows where it leads and nothing happens without Join, so the "External links can be dangerous" warning isn't needed. Links to another instance's `/invite/` still open in the browser.
7. **Website embeds of invite links are hidden** when the same message has a card for that code. Stoat's embedder may make a plain website card for `stt.gg` links, and two cards for one invite is noise.
8. **Error texts:**
   - `NotFound`: "This invite is invalid or has expired."
   - `Banned`: "You're banned from this server."
   - `TooManyServers { max }`: "You're in {max} servers, the most Stoat allows. Leave one to join another."
   - `GroupTooLarge { max }`: "This group is full ({max} members at most)."
   - A link to another instance's `/invite/` in the dialog: "That invite is for a different Stoat instance."
   - Anything else that doesn't parse: "That isn't an invite link or code."
   - **Create:**
     - `MissingPermission`: "You can't create invites in this channel."
     - `InvalidOperation` with an expiry: "Stoat doesn't allow invites to last that long."
     - `FailedValidation`: "Pick between 1 and 100 uses."
   - **Anything else:** `error_text(err, "Couldn't join. Try again.")` or `"Couldn't create the invite. Try again."`, which already says how long to wait after a rate limit.
9. **Expiry options** longer than the instance's `max_invite_duration_days` are left out when the root config sends it; if it doesn't, all are offered. The expiry is computed from the local clock (`Utc::now() + duration`) and sent as RFC 3339 with milliseconds and `Z`.
10. **"Invite people…" in the server menu** uses the first channel, in sidebar order, where you have `InviteOthers`. It's hidden when there's none. The header button shows only on a server channel where you have `InviteOthers`.
11. **No keyboard shortcut** for Join Server. The menu item and the rail button are enough, and `SHORTCUTS` only lists bound actions.
12. **With several accounts,** the Join dialog says "Joining as @name#1234" under the title, since the rail button acts for the account on screen.
13. **Logging:** codes are fine to log at debug level (they're shared in public). Nothing else here is sensitive.

## Plan

### Chunk 1: Join a server from a code or link
**Goal:** A "+" at the bottom of the server rail, and Stoat → Join Server…, open a dialog that looks up an invite code or link, previews it, and joins or opens the server (or group).
**Files likely touched:**
- `src/api/client.rs`:
  - `fetch_invite(code) -> InviteResponse` (`GET /invites/{code}`)
  - `join_invite(code) -> InviteJoinResponse` (`POST /invites/{code}`, no body)
  - Add the imports to the hand-formatted `revolt_models::v0` list.
- `src/invites.rs` (new, pure, no GPUI):
  - `InviteHosts::new(app: Option<&str>)`: the account's own host, plus whether it's the official instance
  - `code_from_link(url, &hosts) -> Option<String>`
  - `parse_input(text, &hosts) -> Result<String, &'static str>`: a code, a link, or the error texts in Assumption 8
  - `join_error_text(&ApiError) -> String`
  - `member_count_text(n)`: "1 member" / "1,234 members"
  - `InvitePreview` (the fields the dialog and cards draw, from either `InviteResponse` variant: `server_id: Option`, `channel_id`, `name`, `icon: Option<File>`, `members: Option<i64>`, `channel_name`, `inviter`)
- `src/ui/invites.rs` (new):
  - `render_join_dialog(input, state, on_look_up, on_join, close, cx)`
  - `JoinState { Idle, LookingUp, Found(InvitePreview), Joining(InvitePreview), Error(String) }`, which could live in `src/invites.rs`
- `src/ui/chat.rs`:
  - `join_input` and `join_state` fields
  - `open_join_dialog(code: Option<String>, window, cx)`, `look_up_invite`, `join_invite` and `open_invite(&InvitePreview)`, which reuse `open_channel` / `select_place`
  - the "+" `rail_button` after the servers (behind a divider; tooltip "Join a server")
  - `join_succeeded(InviteJoinResponse, channel_id)` (Assumption 4), shared with chunk 2
- `src/ui/actions.rs`, `src/main.rs`: a `JoinServer` action, and "Join Server…" in the Stoat menu after Add Account…, routed to `root.shown_chat()`.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.

**Depends on:** None.
**Risks:**
- **Adding the server before `ServerCreate`.** Check that `add_server` with no emoji, followed by the event's `add_server` with emoji, leaves the server's emoji, members and order correct. The rail must show the server once, at the end, and `fetch_own_member` must run once. Write a store test for "response then event" and "event then response".
- **Channels you can't see.** The response lists only visible channels. If the invite's channel isn't among them (a private channel), open the server instead of failing.
- **Group joins.** `InviteJoinResponse::Group.users` may include people already in the store; `add_user` must not wipe their presence. Check what the `ChannelCreate` handler (`chat.rs` ~938) already does for groups, and do the same.
- **The menu-bar item with no account on screen** (the login view): disable it, or make it do nothing, as `LogOut` does.
- **Look up while editing.** Editing the field must drop a preview that's loading or shown, so Join can't act on a stale code. Keep the code that was looked up beside the preview and compare.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/invites.rs` unit tests:
  - `code_from_link` with `app = https://stoat.chat`:
    - `https://stt.gg/wVEJDGVs`, `stt.gg/wVEJDGVs`, `http://rvlt.gg/abc`, `https://stoat.chat/invite/abc/`, `https://app.revolt.chat/invite/abc?x=1` and `HTTPS://STT.GG/abc` give the code
    - `https://stt.gg/discover`, `https://evilstt.gg/abc`, `https://stt.gg.example.com/abc`, `https://stoat.chat/server/abc`, `https://stt.gg/` and `https://stt.gg/a-b` don't
  - `code_from_link` with `app = https://chat.example.org`: its `/invite/abc` gives the code; `stt.gg/abc` and `stoat.chat/invite/abc` don't
  - `parse_input`:
    - " wVEJDGVs " gives "wVEJDGVs"
    - a 26-character ULID passes
    - "two words" and "" fail with "That isn't an invite link or code."
    - `https://other.example/invite/abc` fails with "That invite is for a different Stoat instance."
  - `join_error_text` for `NotFound`, `Banned`, `TooManyServers { max: 100 }`, `GroupTooLarge { max: 50 }` and `RateLimited { Some(2500) }`
  - `member_count_text` for 0, 1 and 1234
  - `InvitePreview` from a server response and from a group response
- `tests/api_invites.rs` (new, wiremock):
  - `GET /invites/abc` parses a `Server` and a `Group` response, and an absent `server_icon`
  - `POST /invites/abc` sends no body and parses both `InviteJoinResponse` shapes
  - a 404 `{"type":"NotFound"}` and a 409 `{"type":"AlreadyInServer"}` are `ApiError::Api` with those types
- `tests/store_servers.rs`: `add_server` from a join response, then the `ServerCreate` event (and the other order), gives one rail entry, with the event's emoji.

**Manual check:**
- From a second account (or the web client), make an invite. In RuSToat, click "+", paste the `stt.gg` link, and Look up: the preview shows the icon, name, member count and channel. Join: the server appears at the end of the rail, its channel opens, and the toast says "Joined …".
- Look the same invite up again: the button says Open, and opens the server.
- Paste the bare code, and a `https://stoat.chat/invite/…` link: both work.
- Paste nonsense, and a deleted invite: the right message shows under the field, and the dialog stays.
- Join a group from a group invite (made in the web client): the group opens under Home.
- Stoat → Join Server… opens the dialog for the account on screen.

**Shippable when:** you can join a server or group from a code or link, with a preview and clear errors, from the rail or the menu bar, and the tests pass.

### Chunk 2: Invite cards in messages, and invite links that open in RuSToat
**Goal:** Invite links in a message's text show as cards under it with Join or Open, and clicking an invite link opens the Join dialog instead of the browser.
**Files likely touched:**
- `src/invites.rs`: `invite_codes(content, &hosts) -> Vec<String>`. It scans text outside code, math and spoilers (Assumption 1), splits words on whitespace and `<>()[]"'`, trims trailing `.,!?;:`, runs each through `code_from_link`, deduplicates, and keeps at most 5.
- `src/markdown.rs`: expose the text-only spans (`pub(crate)`), with no change to what `prepare` produces.
- `src/store.rs`:
  - `ChatStore` gets the `InviteHosts` (set by `ChatView::new` with `web_app`)
  - `MessageRow.invites: Vec<String>`, filled in `message_row` for non-system messages
  - embeds whose URL is one of those invites are dropped (Assumption 7)
- `src/ui/chat.rs`:
  - an `invites: HashMap<String, InviteLookup>` cache and the one-at-a-time fetch queue (Assumption 2), fed when a pane's rows are rebuilt
  - `Pickers` (or a sibling snapshot) carries `Rc<HashMap<code, CardState>>`, with Join/Open already decided from the store
  - `render_invite_card` beside `render_embed_card`, drawn after the embeds (~7323), and added to the "bare" check (~7333)
  - `RowAction::JoinInvite(code)` / `OpenInvite(code)` / `RetryInvite(code)` in `on_row_action`, with joining reusing chunk 1's `join_succeeded` (errors as toasts)
  - `People::link_handler`: for a web link, or an `InternalLink::External`, that `code_from_link` accepts, call `open_join_dialog(Some(code))` instead of `open_link`
- `src/ui/invites.rs`: the card's layout, if it's cleaner outside `chat.rs`.

**Depends on:** Chunk 1 (the API calls, `InviteHosts`, `InvitePreview`, `join_succeeded` and the dialog).
**Risks:**
- **Row heights.** A card changes height from Loading to Found or Invalid, or not at all if they're the same height, so make them the same. If the list caches row heights, a card that changes height needs the same remeasure that loading images use. Find what embeds do and follow it.
- **The store doesn't know about fetches.** Codes in `MessageRow` are pure. The lookups live in `ChatView`, so they must not be in `render_key`, or rows would rebuild for nothing. Pass them through the render snapshot.
- **Rate limits.** The 20-per-10 s bucket is shared with joins and other uncategorised calls. The queue must back off on `RateLimited` and never fetch the same code twice at once.
- **Scanning cost.** `invite_codes` runs for every message row on every rebuild. Keep it a single linear pass that skips messages containing no `/` (a cheap early exit).
- **Edited messages.** A message edited to add or remove a link rebuilds its row, so its codes update. New codes join the queue.
- **Blocked messages** are collapsed with their cards. Check the collapsed row draws no card.
- **`guard_links` and autolinks.** A bare `stt.gg/abc` with no scheme isn't a link in TextView (GFM needs a scheme or `www.`), so it shows as text with a card under it, as in revite. That's expected.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/invites.rs` unit tests for `invite_codes`:
  - "join https://stt.gg/abc!" gives ["abc"]
  - "(stt.gg/abc), <https://stt.gg/def>" gives ["abc", "def"]
  - the same link twice gives one
  - 7 links give the first 5
  - "`https://stt.gg/abc`", "```\nstt.gg/abc\n```" and "||stt.gg/abc||" give none
  - "https://stt.gg/discover" gives none
  - with a self-hosted `app`, "stt.gg/abc" gives none and "{app}/invite/abc" gives ["abc"]
- `tests/store_invites.rs` (new):
  - a message with an invite link gets `invites`
  - a system message gets none
  - a website embed whose URL is the same invite is dropped, and another embed is kept
- `tests/markdown_prepare.rs` still passes unchanged: `prepare` didn't change.

**Manual check:**
- Post a message with two invite links and some text: two cards appear under it, with icons, names and member counts.
- Join from a card: the server opens, and the card now says Open.
- An invalid code shows "This invite is invalid or has expired."
- A link in a spoiler or code block makes no card.
- Click an invite link in a message: the Join dialog opens, already looked up. A masked link (`[click](https://stt.gg/abc)`) does the same, with no "External links" warning.
- Scroll a channel with many invites: no 429 errors, and the cards fill in.

**Shippable when:** invite links in messages show as cards that join or open, clicking an invite link opens the dialog, and the tests pass.

### Chunk 3: Create an invite and copy its link
**Goal:** Create an invite for a channel, with optional maximum uses and expiry, from the channel menu, the server menu or the channel header, and copy its link.
**Files likely touched:**
- `src/api/client.rs`:
  - `RootLimits.global: GlobalLimits { max_invite_duration_days: Option<u32> }` (`#[serde(default)]`)
  - `create_invite(channel, max_uses: Option<u64>, expires: Option<String>) -> Invite` (`POST /channels/{id}/invites`), sending a private `#[derive(Serialize)] struct CreateInvite { #[serde(skip_serializing_if = "Option::is_none")] max_uses, …expires }`
- `src/links.rs`: `invite_link(app, code) -> String`: `https://stt.gg/{code}` when `app` is `https://stoat.chat` (any trailing `/`), else `{app}/invite/{code}`.
- `src/invites.rs`:
  - `Expiry { Minutes30, Hour1, Hours6, Hours12, Day1, Days7, Never }` with `label`, `duration` and `options(max_days: Option<u32>)`
  - `MAX_USES_OPTIONS: [Option<u64>]`
  - `expires_at(expiry, now: DateTime<Utc>) -> Option<String>`
  - `invite_summary(&Invite, now) -> String`
  - `create_error_text(&ApiError, had_expiry) -> String`
- `src/permissions.rs`: `can_invite(store, user, channel)`: `in_server && has(…, InviteOthers)`. Plus `ChatStore::can_invite(channel)` in `store.rs`.
- `src/ui/invites.rs`: the create dialog (pickers via `DropdownMenu`, Create, then the link, summary, Copy link and Done). Copy writes to the clipboard and toasts "Copied the invite link."
- `src/ui/chat.rs`:
  - `open_create_invite(channel, window, cx)`
  - "Create invite…" in `channel_menu`, before the separator, for server channels where `can_invite`
  - "Invite people…" in `server_menu`, using the first channel in sidebar order where `can_invite` (Assumption 10)
  - a `Button::new("create-invite").ghost().icon(IconName::UserPlus)` in the header, before Pins, with the tooltip "Create invite", when `can_invite` on the current channel and `web_app` is known
- `tests/api_login.rs` (~158, where `RootConfig` parsing is tested): the new `max_invite_duration_days` field.

**Depends on:** Chunk 1 (`src/invites.rs`, the client's invite imports). It doesn't need chunk 2.
**Risks:**
- **No `app` URL** (old instances). There's no link to copy, so show the code alone with "Copy code", or hide Create. Prefer showing the code: it works in the Join dialog.
- **Clock skew.** "7 days" is far inside the 30-day limit, so a fast local clock can't push it over. If an instance's limit is under 7 days, `options` drops the longer choices. An `InvalidOperation` still gets a clear message.
- **The `expires` format.** `iso8601_timestamp` parses RFC 3339. Send `2026-11-14T12:00:00.000Z` (`SecondsFormat::Millis`, `use_z = true`), and test the exact string.
- **Permissions can change while the menu is open** (a role edit). Stoat is the judge: `MissingPermission` shows its message.
- **`DropdownMenu` inside a dialog.** Check the popup layers above the dialog. If it doesn't, fall back to a row of small toggle buttons per option.
- **Voice channels** are `TextChannel` with `voice` set, and Stoat allows invites to them. Check `in_server` and `channel_menu` treat them alike.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/invites.rs` unit tests:
  - `expires_at(Days7, 2026-11-07T12:00:00Z)` is exactly `"2026-11-14T12:00:00.000Z"`; `Never` is `None`
  - `Expiry::options(Some(1))` stops at 1 day and keeps Never; `options(None)` has all seven
  - `invite_summary` for no limits ("Never expires · No limit"), 1 use ("· 1 use"), and an expiry
  - `create_error_text` for `MissingPermission`, `InvalidOperation` (with and without expiry), `FailedValidation` and a rate limit
- `tests/links.rs`: `invite_link("https://stoat.chat/", "abc")` is `https://stt.gg/abc`; `invite_link("https://chat.example.org", "abc")` is `https://chat.example.org/invite/abc`.
- `tests/api_invites.rs`:
  - `POST /channels/c1/invites` sends exactly `{}` with no options, and exactly `{"max_uses":10,"expires":"2026-11-14T12:00:00.000Z"}` with both
  - parses `{"type":"Server","_id":"abcdEFGH","server":"s1","creator":"u1","channel":"c1","uses":0}`
  - a 403 `{"type":"MissingPermission","permission":"InviteOthers"}` is `ApiError::Api`
- `tests/permissions.rs`: `can_invite` is true with the default permissions, false when a channel override denies `InviteOthers`, and false in a DM.
- `tests/api_login.rs` (or wherever `RootConfig` is parsed): `max_invite_duration_days` is read from `features.limits.global` and defaults to `None` without it.

**Manual check:**
- Right-click a channel → Create invite…: create with the defaults, then Copy link. The toast confirms, and the pasted link is `https://stt.gg/…`. The web client's Server Settings → Invites lists it.
- Create one with 1 use and 30 minutes: the summary says so. Join it from another account, then try it from a third: "This invite is invalid or has expired."
- The server menu's "Invite people…" and the header button create for the right channel.
- On an account with no `InviteOthers` (a role without it): none of the three entry points shows.
- Paste the copied link into a message: chunk 2's card appears (if chunk 2 has landed).

**Shippable when:** invites can be created with or without limits from all three places, shown only when allowed, with the right link copied, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`. Never run it on a `mod.rs`, `lib.rs` or `main.rs`, since it formats their child modules too. For `src/main.rs`, use `--check` and hand-format.
- A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines (from `rustfmt --edition 2024 --check <file>`) hasn't grown. Starting counts on 2026-11-07:
  - with drift: `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15, `src/markdown.rs` 1
  - clean: `src/api/error.rs`, `src/links.rs`, `src/permissions.rs`, `src/main.rs`, `src/ui/actions.rs`, `src/assets.rs`, `src/shortcuts.rs`, `tests/links.rs`, `tests/permissions.rs` and `tests/store_servers.rs`
  - New files must be clean.

### Unit Tests
- **`src/invites.rs`:**
  - link and input parsing (official and self-hosted instances, lookalike hosts, `discover`, ULIDs)
  - scanning message text (punctuation, brackets, duplicates, the cap of 5, code, math and spoilers)
  - preview from both response shapes
  - member count text
  - expiry options, timestamps and summaries
  - join and create error texts

### Integration Tests
- **`tests/api_invites.rs`** (new):
  - fetch (server, group)
  - join (no body; server and group responses)
  - create (exact bodies with and without options; the `Invite` parse)
  - `NotFound`, `AlreadyInServer` and `MissingPermission` stay `Api` errors
- **`tests/store_servers.rs`:** a join response and `ServerCreate` in either order.
- **`tests/store_invites.rs`** (new): `MessageRow.invites` and dropping duplicate website embeds.
- **`tests/links.rs`:** `invite_link` on stoat.chat and on another instance.
- **`tests/permissions.rs`:** `can_invite`.
- **Root config:** `max_invite_duration_days`.

### Manual Tests
You need two `api.stoat.chat` accounts (one in RuSToat, one in the web client or a second RuSToat account), a server where you can make a role without `InviteOthers`, and a group.
1. Run every chunk's manual check.
2. With two accounts in RuSToat, the "+" button, the menu bar item and the cards act for the account on screen, and the dialog says which one.
3. Joining while the other account is on screen doesn't touch it.
4. Light and dark themes: the dialog, the preview, the cards (all states) and the create dialog are readable.
5. Leave the servers and groups you joined for testing, and delete the test invites from the web client's Server Settings → Invites (milestone 23 brings that to RuSToat).

## Definition of Done
- [ ] A "+" at the bottom of the server rail and Stoat → Join Server… open "Join a server". It accepts a bare code, a ULID, `{app}/invite/{code}`, and (on stoat.chat) `stt.gg`, `rvlt.gg` and the known Stoat and Revolt hosts. It previews the server or group before joining, says Open when you're already in, and never calls join for a server already in the store.
- [ ] After joining, the server (or group) is in the sidebar at once, its invite channel (or the server) is open, and the later `ServerCreate` doesn't duplicate it.
- [ ] `NotFound`, `Banned`, `TooManyServers`, `GroupTooLarge` and rate limits show their own messages; bad input and other instances' links are refused before sending.
- [ ] Invite links anywhere in a message's text (not in code, math or spoilers), up to 5, show as cards with the icon, name and member count, and Join / Open; invalid ones say so; lookups are cached and queued.
- [ ] Clicking an invite link opens the Join dialog, looked up, instead of the browser.
- [ ] "Create invite…" (channel menu), "Invite people…" (server menu) and the header button appear only with `InviteOthers`, and create an invite with optional max uses (1–100) and expiry (up to 7 days, within the instance's limit), sending `{}` when neither is set.
- [ ] Copy link copies `https://stt.gg/{code}` on stoat.chat and `{app}/invite/{code}` elsewhere, and toasts.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/invites-implementation.md`.
