# Plan: Server settings (RuSToat milestone 23)

## Summary
Milestone 23 of the [roadmap](roadmap.md) gives a server's moderators and owner a **Server Settings window**:
- **Overview:** name, description, icon, banner, and the four system-message channels.
- **Invites:** list them, revoke one, create one, copy a link.
- **Bans:** list them, filter, unban.
- **Owner only:** transfer ownership, and delete the server, confirmed by typing its name.

It also finishes **milestone 22's deferred chunk 5, Reports**: report a message, a user or a server to Stoat (user's choice, 2026-11-09).

Milestones 24–25 (the roles editor and permission matrix) come straight after. They add pages to this window, so the window is built for that.

## Approach
**What Stoat allows.** Read from `/tmp/stoatchat-0.16.2` on 2026-11-09. Paths are under `crates/`.

**Edit:** `PATCH /servers/{id}` with `DataEditServer` (`delta/src/routes/servers/server_edit.rs`; type in `revolt_models::v0::servers`). It returns the full `Server`.
- **Fields:**
  - `name`: 1–32 characters.
  - `description`: 0–1024 characters.
  - `icon` / `banner`: Autumn file IDs, uploaded with the tags `icons` and `banners`.
  - `system_messages`: `SystemMessageChannels { user_joined, user_left, user_kicked, user_banned }`, each `Option<channel id>`. The object **replaces** the stored one; a missing key means that message is off. Every ID must be one of the server's channels, or the request fails with `NotFound`.
  - `remove: [FieldsServer]`: `Description`, `Icon`, `Banner`, `SystemMessages` (and `Categories`).
- **Permissions and refusals:**
  - All of the above need **Manage Server** (`MissingPermission`). Validation errors come back as `FailedValidation`.
  - An unknown or already-used file ID is `NotFound`.
- **Size limits:** icons 2.5 MB (`icons` in `GET /`'s limits); banners 6 MB (not in `GET /`, so it's a default).
- **Event:** `ServerUpdate { id, data, clear }` goes to every member. RuSToat already applies it (`store.rs` `apply_server_update`).

**Transfer ownership:** the same `PATCH` with `{ "owner": "<user>" }`.
- **Who:** only the owner (`NotOwner`).
- **It needs an `X-MFA-Ticket`.** The ticket is used up even if the edit then fails.
- **The new owner** must be a member (`NotFound`), and not a bot (`InvalidOperation`).
- **Event:** `ServerUpdate` with `data.owner`, which the store already follows (`tests/store_servers.rs` "ownership_follows_a_server_update").
- **The web client has no UI for it.**

**Delete:** `DELETE /servers/{id}`.
- For the owner it deletes the server, and everyone gets `ServerDelete`. For anyone else the same request is a leave, which `StoatClient::leave_server` already sends.
- No MFA ticket is needed.

**Invites:**
- **List:** `GET /servers/{id}/invites`. It needs Manage Server and returns `Vec<Invite>`, where `Invite::Server` is `{ _id: code, server, creator, channel, max_uses?, uses, expires? }`. It carries IDs only, no user or channel objects.
- **Revoke:** `DELETE /invites/{code}`. The creator, or anyone with Manage Server, may do it.
- **No websocket event** for either.

**Bans:**
- **List:** `GET /servers/{id}/bans`. It needs Ban Members and returns `BanListResult { users: [BannedUser { _id, username, discriminator, avatar? }], bans: [ServerBan { _id: { server, user }, reason? }] }`, joined on the user ID.
- **Unban:** `DELETE /servers/{id}/bans/{user}`. It needs Ban Members; a missing ban is `NotFound`.
- **No websocket event.**

**Reports:** see milestone 22's record, [`member-moderation-implementation.md`](member-moderation-implementation.md) "Chunk 5: Reports — not started".
- The request is `POST /safety/report` with `{ content: ReportedContent, additional_context }` (≤1000 characters).
- `ReportedContent`, `ContentReportReason` and `UserReportReason` are in `revolt_models::v0`. The body type is the client's own.
- You can't report yourself, your own message, or a server you own (`CannotReportYourself`).

**What the web client does** (`/tmp/for-web-0.16.3/packages/client/components`, `app/interface/settings/ServerSettings.tsx` and `settings/server/*`):
- **Pages, in order:**
  - Overview;
  - Discover (official instance, owner only; out of scope);
  - Emojis (milestone 28);
  - Roles (milestones 24–25), Invites (Manage Server), Bans (Ban Members);
  - a red "Delete Server" (owner only).
- **Overview:**
  - fields: Server Icon, Server Banner, Server Name (counter), Server Description (counter, placeholder "This server is about...");
  - a heading "System message channels" with four pickers: "User Joined", "User Left", "User Kicked", "User Banned". Each offers "Disabled" plus every channel;
  - Save sends only what changed: a blank description becomes `remove: ["Description"]`, and a cleared image becomes `remove: ["Icon"/"Banner"]`. Images are uploaded first.
  - **Don't copy its bug:** it can send `"none"` as a channel ID.
- **Invites:**
  - columns: inviter and `#channel`, the code with a copy-link button, and a red delete button;
  - "Create invite" at the top, disabled with no channels;
  - rows are removed locally after a delete.
- **Bans:**
  - User and Reason columns, each with a filter field;
  - each row shows an avatar and `username#discriminator`;
  - a "Pardon User" button removes the row locally.
- **Delete server:** "Delete {name}?" / "Once it's deleted, there's no going back.". It runs a password/2FA prompt, but never sends the ticket. RuSToat types the name instead (user's choice).
- **Settings entry:** "Open server settings" is in the server menu.

**What exists in RuSToat** (line numbers as of 2026-11-09):
- **Settings window:**
  - `src/ui/settings.rs` `SettingsView` (milestone 13) draws gpui-component's `Settings` from `pages: Vec<PageFn>`.
  - It is account-shaped: four per-account entity caches, synced in `render`, and `PageFn = fn(&SettingsView, &mut App) -> SettingPage`.
  - `src/main.rs` (~340–407) manages it as a single-slot `SettingsWindow` global. That global is private to the binary, so the library can't open it.
- **The async page pattern:** `src/ui/sessions_page.rs`:
  - `enum Load { Loading, Loaded, Failed }`, `spawn_io` for the request, then `cx.spawn_in(window, …)`;
  - a "Couldn't load… Retry" row;
  - toasts through `profile_edit::error_text`.
- **Images:** milestone 15's pieces in `src/ui/profile_editor.rs` are all `pub(crate)`:
  - `ImageKind { Avatar, Banner }` with `tag()`;
  - `ImageSlot`, `prompt_for_image`, `upload_image`, `Preview`, `render_image_row`;
  - `ProfileImage` and `check_image` in `src/profile_edit.rs`.

  Previews centre-crop with `ObjectFit::Cover`; uploads aren't cropped. `FileSizeLimits` (`src/api/client.rs` ~103) parses only `attachments`, `avatars` and `backgrounds`.
- **Confirmations:**
  - `src/ui/moderation_dialogs.rs` `confirm_member_action(MemberActionRequest, action, window, cx)` asks, runs one request with a busy button, and toasts a refusal.
  - `src/ui/confirm_dialog.rs` `confirm_then` is the MFA "Confirm it's you" dialog. It hands the action a ticket; `StoatClient::send_with_ticket` sends it.
  - There is no "type the name" dialog yet. `SessionsView`'s rename dialog shows input-driven enabling (~124–153).
- **Server UI:**
  - `server_menu` (`src/ui/chat.rs` ~4777) is the rail icon's menu: Mark as read, Edit identity…, Invite people…, folder items, mute, notifications, Leave server…, Copy server ID.
  - The server heading opens the info dialog (`open_server_info` ~7259, `server_dialogs::render_info`).
  - `ChatView::leave_server` (~1421) drops a server locally, and runs on `ServerDelete` and on your own `ServerMemberLeave`.
- **Invites:** `src/invites.rs` has `invite_code`, `max_uses_label`, `expires_at`. `src/ui/invites.rs` has `CreateInviteDialog`. `links::invite_link` builds the link.
- **Permissions:** `src/permissions.rs` has `server_permissions`, `has`, `member_rank`, `acting_rank`, `moderation`. Nothing checks Manage Server yet.

**The chosen shape:**
- **A separate Server Settings window, one at a time** (user's choice).
  - It's a new library view, `ServerSettingsView` in `src/ui/server_settings.rs`, which draws the same gpui-component `Settings` component with its own pages. Each page is a function, `fn(&ServerSettingsView, &mut App) -> Option<SettingPage>`, returning `None` when the user may not see it. Milestones 24–25 add a page by adding a function.
  - The window is managed by a library global, `ServerSettingsWindow`, in the same file, so `chat.rs` can open it directly with `ServerSettingsWindow::open(chat, server, cx)`.
  - Opening it for the server already shown brings it forward. Opening it for another server (or another account's) closes the old one first.
  - It closes itself when its server leaves the store (deleted, left, kicked or banned), or its `ChatView` is released (log out, or the main window really closing). This is done by observing the `ChatView`.
- **Rejected: adding a server context to `SettingsView`.** Its render is built around per-account caches and the shown account, and it is a singleton managed in `main.rs`. Bending it to serve both would tangle two lifecycles. The shared part is gpui-component's `Settings`, and that is what's reused.
- **Rejected: routing through `main.rs` with an action.** An action with a field needs GPUI's derive under the crate name `gpui` (`src/ui/actions.rs` comment). Nothing app-level needs to open server settings, so a library global is simpler.
- **Plain logic lives in `src/server_settings.rs`, with no GPUI**, so it can be unit-tested:
  - who sees which page;
  - the Overview form's diff to a `DataEditServer`;
  - the system-message edit;
  - ban rows and their filter;
  - invite rows;
  - the "type the name" match;
  - transfer candidates;
  - error texts.

  This is the same split as `profile_edit.rs` / `profile_editor.rs` and `moderation.rs` / `moderation_dialogs.rs`.
- **The pages:** Overview, Invites, Bans, Ownership.
  - **Ownership** is owner only, and holds Transfer ownership and Delete server.
  - Pages are recomputed every render. If the set of visible pages changes (an owner transfers ownership, or a role edit takes Manage Server away), the sidebar is reset to its first page, through a keyed ID as `SettingsView::page_reset` does. gpui-component selects pages by index, so a shrinking list would otherwise show the wrong page.
- **Saving:**
  - The Overview sends one `PATCH`. On success it applies the saved fields to the store at once, through the existing `apply_server_update` path, so the rail and header update before the event arrives.
  - Invites and Bans update their lists locally after a revoke or unban, since Stoat sends no event.

## Assumptions
1. **"Server settings…" appears** in the server menu (after "Invite people…") and as a button in the server info dialog. It shows when you have Manage Server, Ban Members, or own the server. Milestone 24 widens this to Manage Role / Manage Permissions.
2. **Page access:**
   - Overview and Invites need Manage Server.
   - Bans needs Ban Members.
   - Ownership needs you to be the owner.

   Each is computed by `server_settings::access(store, server) -> Access { overview, invites, bans, ownership }`, which can be tested with fixture roles.
3. **The window** is titled "Server Settings — ‹name›", follows renames, and opens at 760×560, centred, like Settings.
4. **Unsaved edits are dropped** when opening another server's settings closes the window. They're also dropped when the window closes itself because the server went away. This is a small loss for a rare action. If it's missed later, ask before closing.
5. **The Overview form follows live changes** while a field is untouched, and keeps your edit once it's touched. A Reset button returns every field to the server's values. Save is enabled only with a change and valid values (name 1–32, description ≤1024).
6. **System-message pickers:**
   - they list the server's text channels in channel-list order, plus "Off";
   - a saved channel that no longer exists shows as "Off";
   - saving sends the whole `SystemMessageChannels` object, without missing channels, so a stale ID can't make Stoat refuse the edit (`servers-and-channels.md`'s note on deleted channels);
   - all four "Off" sends `remove: ["SystemMessages"]`.
7. **Icon and banner** work like milestone 15's profile images: Change… / Remove, a preview, centre-cropped previews, and no crop editor.
   - Icon limit: `GET /`'s `icons` (default 2.5 MB). Banner limit: 6 MB (a new default; `banners` is parsed if `GET /` ever sends it).
   - The icon preview is round and the banner a 232:100 strip, as the web client's.
8. **Invites page:**
   - a row per invite: the creator's name and avatar ("Unknown user" if not cached), `#channel`, the code, uses (`3` or `3 / 10`), expiry ("Never" or relative), a Copy link button and a Revoke button;
   - Revoke confirms with `confirm_member_action`;
   - "Create invite…" opens the existing `CreateInviteDialog` in the settings window, for the first channel you may invite to, then refreshes the list. It is disabled with no such channel.
   - Unknown creators aren't fetched.
9. **Bans page:**
   - a row per ban: avatar, `username#discriminator`, the reason or "No reason given", and Unban;
   - Unban confirms ("Unban ‹user›? They can rejoin with an invite.");
   - one filter field matches the username or reason, case-insensitively;
   - no pagination: the list is drawn in a scrollable list.
10. **Transfer ownership:**
    - **Picking:** a picker lists the server's members (`fetch_members`, the whole list as Stoat 0.16.2 has no search), without yourself and bots, filtered as you type, by display name and username.
    - **Confirming:** choosing one opens `confirm_then`, "Transfer ‹server› to ‹name›?", detail "They'll own the server. You'll keep your roles, but lose the owner's powers. Only they can give it back.", button "Transfer".
11. **Delete server:**
    - a dialog "Delete ‹name›?", "Once it's deleted, there's no going back. Type the server's name to confirm.";
    - a text field, and a red Delete button enabled only when the trimmed text equals the name exactly (case-sensitive, as GitHub does);
    - on success: the window closes, the server leaves the store at once (`ChatView::leave_server`), and the main window toasts "Deleted ‹name›.".
12. **Reports** follow milestone 22's recorded research and the web client:
    - **Menu items:**
      - "Report message…" in the message menu, but not for your own messages;
      - "Report user…" in the person menu, next to Block, but not for yourself;
      - "Report server…" in the server menu, but not for a server you own.
    - **Dialog:**
      - title "Tell us what's wrong with this message/user/server";
      - a "Reason for report" picker with Stoat's reasons in the web client's order, "Other" last;
      - "Give us some detail" (≤1000 characters, required for Other);
      - Report sends; success toasts "Thanks, your report was sent.".
    - A user report opened from a message's author carries `message_id`.

## Plan

### Chunk 1: Server Settings window with name and description
**Goal:** a moderator opens "Server settings…" and renames the server or edits its description, in a window that later pages slot into.
**Files likely touched:**
- `src/server_settings.rs` (new, no GPUI):
  - `Access` and `access(store, server)`;
  - `OverviewForm { name, description }` with `changes(&Server) -> Option<DataEditServer>` (trimmed; a blank description becomes `remove: [Description]`; `None` when nothing changed) and `valid()`;
  - `save_error_text`.
- `src/ui/server_settings.rs` (new):
  - `ServerSettingsWindow` global with `open(chat: Entity<ChatView>, server, cx)` and `close`;
  - `ServerSettingsView { chat: WeakEntity<ChatView>, server, pages, page_key, overview: Entity<OverviewPage> }`;
  - `overview_page`;
  - `OverviewPage`, the form entity with inputs, busy state, Reset and Save.
- `src/api/client.rs`: `edit_server(server, &DataEditServer) -> Server`, through `edit_body`.
- `src/store.rs`: helper(s) to apply a saved edit, e.g. `apply_server_edit(id, &Server, &[FieldsServer])` built on `apply_server_update`, if the existing path can't be reused directly.
- `src/ui/chat.rs`:
  - "Server settings…" in `server_menu`, and a button in the server info dialog (`render_info` / `open_server_info`);
  - `ChatView::open_server_settings(server, window, cx)`.
- `src/permissions.rs`: `has` for `ManageServer` is already possible; add a store wrapper only if it reads better.
- `src/lib.rs`, `src/ui/mod.rs`: the modules.
- `tests/server_settings.rs` (new); `tests/api_servers.rs`.

**Depends on:** None
**Risks:**
- **First window opened from the library.** `gpui_kit::open_window` must work there as in `main.rs`, with the `Root` wrapper for dialogs and toasts. If it can't, fall back to an app-level callback global that `main.rs` installs, and note it.
- **Closing itself on removal.** The view observes the `ChatView` and checks `store.server(id)`. Closing a window from inside its own observer may need `cx.defer` or `window.remove_window()`. Check it doesn't panic when the server is deleted while the window has focus.
- **Page-index shifting.** The page set changes live, and gpui-component selects by index. The keyed reset must trigger only when the set of visible pages changes, not on every render, or the sidebar jumps back on each keystroke.
- **The form following live updates.** Set an input's value only when that field is untouched, or typing gets clobbered by its own echo.
- **`chat.rs` drift is 17 hunks** at the start. Hand-format the new code there, and check the count doesn't grow.

**How to verify:**
- `cargo test --test server_settings`:
  - `access` for the owner, someone with Manage Server, someone with only Ban Members, and a plain member;
  - `OverviewForm::changes`: unchanged gives `None`; a rename sends `name` only; a blank description gives `remove: [Description]`; a 33-character name is invalid.
- `cargo test --test api_servers`: `edit_server` sends `PATCH /servers/srv` with only the set fields, and a `MissingPermission` refusal becomes its text.
- Then the full suite.

**Manual check:**
- As the owner, the server menu has "Server settings…". The window opens with Overview. Rename the server: the rail tooltip and header change at once, and the web client shows the new name.
- Clear the description and save: the info dialog shows none.
- Open settings for a second server: the first window closes and the second opens.
- With a second account and no permissions, there's no "Server settings…".

**Shippable when:** moderators can rename and describe their server from a Server Settings window, which closes itself if the server goes away.

### Chunk 2: Icon and banner
**Goal:** change or remove the server's icon and banner from the Overview.
**Files likely touched:**
- `src/ui/profile_editor.rs`:
  - widen `ImageKind` with `ServerIcon` / `ServerBanner` (tags `icons`, `banners`, and their limits), or add a parallel kind in `server_settings.rs` if widening tangles the profile code;
  - make `banner_preview` usable with a 232:100 aspect.
- `src/profile_edit.rs`: `ImageLimits` gains `server_icon` and `server_banner`.
- `src/api/client.rs`:
  - `FileSizeLimits` gains `icons` and `banners`;
  - new defaults `DEFAULT_ICON_SIZE_LIMIT` (2.5 MB) and `DEFAULT_BANNER_SIZE_LIMIT` (6 MB).
- `src/server_settings.rs`: `OverviewForm` gains `icon` and `banner: ProfileImage`. `changes` takes the uploaded IDs, or a remove.
- `src/ui/server_settings.rs`: two `render_image_row`s above the name. Save uploads first (`Saving::Uploading`, then `Sending`), as `ProfileEditor::save` does.
- `tests/server_settings.rs`, `tests/api_instance.rs` (limits parsing), `tests/fixtures/root.json` if needed.

**Depends on:** Chunk 1
**Risks:**
- **Widening `ImageKind`** touches milestone 15's profile and identity editors. Run their tests, and check a profile banner still uploads to `backgrounds`.
- **Uploading and removing in one request.** Don't send `remove: [Icon]` with a new `icon`: a replacement is just the new ID.
- **A picked file that fails to upload** must not save the rest silently. Follow the profile editor: toast and keep the form dirty.

**How to verify:**
- Unit tests:
  - a new icon gives `icon: <id>` and no remove;
  - a removed banner gives `remove: [Banner]`;
  - an unchanged image is left out;
  - limits parse `icons` from `root.json` and fall back to the defaults.
- A wiremock test: upload to `/icons`, then a `PATCH` with the returned ID.
- Then the full suite.

**Manual check:**
- Set an icon (PNG, then an animated GIF) and a banner. The rail icon and the info dialog's banner change at once, and in the web client.
- Remove both: the rail falls back to initials.
- A 3 MB icon is refused with the size note before upload.

**Shippable when:** a server's icon and banner can be set and removed from RuSToat.

### Chunk 3: System-message channels
**Goal:** choose where "joined", "left", "kicked" and "banned" messages go, or turn each off.
**Files likely touched:**
- `src/server_settings.rs`:
  - `SystemMessagesForm` (four `Option<channel>`), built from the server with missing channels shown as off;
  - `edit(&Server) -> Option<(Option<SystemMessageChannels>, remove)>`, which sends the whole object (or `remove: [SystemMessages]` when all are off) and drops unknown channels;
  - `system_channel_choices(store, server)`: text channels in channel-list order.
- `src/ui/server_settings.rs`: a "System messages" group on the Overview with four dropdowns ("User joined", "User left", "User kicked", "User banned"), saved by the same Save as the rest of the form (one `PATCH`).
- `tests/server_settings.rs`.

**Depends on:** Chunk 1
**Risks:**
- **gpui-component's dropdown inside a custom `SettingItem::render`.** If its select needs state that can't live there, keep `SelectState`s on `OverviewPage`, as the inputs are.
- **One Save for both groups.** Everything goes in one `DataEditServer`; check `changes` merges them, and a refusal of one part refuses all.

**How to verify:**
- Unit tests:
  - all off gives `remove: [SystemMessages]`;
  - setting one keeps the other three as they were;
  - a stored ID for a deleted channel shows "Off" and isn't sent;
  - choices exclude voice channels (if Stoat allows those, keep them and note it) and follow channel order.
- Then the full suite.

**Manual check:**
- Point "User joined" at a channel and have the second account join: the message appears there.
- Turn "User left" off and have it leave: no message.

**Shippable when:** all four system-message channels can be set or turned off without Stoat refusing a stale channel.

### Chunk 4: Invites page
**Goal:** see the server's invites, revoke one, create one, and copy a link.
**Files likely touched:**
- `src/api/client.rs`: `fetch_server_invites(server) -> Vec<Invite>`, `delete_invite(code)`.
- `src/server_settings.rs`:
  - `InviteRow { code, creator, channel, uses_label, expiry }` from `Invite` and the store (names when cached);
  - `invite_rows(store, invites, now)` sorted with the newest first if it's known (else as Stoat sends them), with expired invites left out;
  - `invites_error_text`.
- `src/ui/server_settings.rs`: `invites_page` and `InvitesPage` (Load / Loaded / Failed with Retry), with rows, Copy link (`links::invite_link`), Revoke (via `confirm_member_action`, then the row is removed locally), and "Create invite…".
- `src/ui/invites.rs`: let `CreateInviteDialog` open in any window and report the created invite back, if it doesn't already.
- `tests/api_invites.rs`, `tests/server_settings.rs`.

**Depends on:** Chunk 1
**Risks:**
- **`CreateInviteDialog` may assume the main window** (its chat entity, its toasts). If it does, adapt it minimally, or on success open nothing more than a refresh.
- **Group invites appear in the same `Invite` enum.** The list only expects `Invite::Server`; skip any other variant.
- **Uses and expiry** use `invites.rs` helpers. Expiry is shown relative to now (`time.rs`), and the pure function takes `now` so it can be tested.

**How to verify:**
- wiremock: `GET /servers/srv/invites` parses server invites (with and without `max_uses` / `expires`); `DELETE /invites/abc` sends once.
- Unit tests: rows show "Unknown user" for an uncached creator, `3 / 10` and `3`, and leave out an expired invite.
- Then the full suite.

**Manual check:**
- Create an invite from the page: it appears.
- Copy its link and open it in the web client.
- Revoke it: the row goes, and the web client's join fails.
- A Ban-Members-only account doesn't see Invites.

**Shippable when:** moderators can list, create, copy and revoke invites from Server Settings.

### Chunk 5: Bans page
**Goal:** see who is banned and why, and unban someone.
**Files likely touched:**
- `src/api/client.rs`: `fetch_bans(server) -> BanListResult`, `unban(server, user)`.
- `src/server_settings.rs`:
  - `BanRow { user, name, avatar, reason }`, joined from `BanListResult` and sorted by name;
  - `filter_bans(rows, query)`;
  - `bans_error_text`.
- `src/ui/server_settings.rs`: `bans_page` and `BansPage` (Load states, a filter input, rows), with Unban through `confirm_member_action` and the row removed locally on success. Banning from milestone 22 while the page is open refreshes it, if it's cheap to notice (the `ServerMemberLeave` with reason Ban); otherwise there's a Refresh button.
- `tests/api_members.rs` (or a new `tests/api_bans.rs`), `tests/server_settings.rs`.

**Depends on:** Chunk 1
**Risks:**
- **A ban whose user is missing from `users`.** Show the ID rather than dropping the row, so it can still be unbanned.
- **Avatars of banned users** are `File`s without a user in the store. Build their URLs with `media.file(..)` directly.

**How to verify:**
- wiremock: the list parses and joins.
- Unit tests: `filter_bans` matches the username or reason case-insensitively; a missing user keeps its row.
- `DELETE /servers/srv/bans/u-ada` sends once, and `NotFound` reads "They aren't banned any more." (and removes the row).
- Then the full suite.

**Manual check:**
- Ban the second account from the member menu with a reason. Open Bans: it's there with the reason. Filter by part of its name.
- Unban: it can rejoin from an invite.

**Shippable when:** moderators can review and lift bans from Server Settings.

### Chunk 6: Ownership: transfer and delete
**Goal:** the owner can hand the server to another member, or delete it after typing its name.
**Files likely touched:**
- `src/api/client.rs`:
  - `transfer_server(ticket, server, owner) -> Server` (the `PATCH` with `X-MFA-Ticket`, via `send_with_ticket`);
  - `delete_server(server)` (`DELETE /servers/{id}`; documented as owner-only, as `leave_server` is the same route).
- `src/server_settings.rs`:
  - `transfer_candidates(members, me) -> Vec<Candidate>`, without yourself and bots, sorted by name;
  - `filter_candidates`;
  - `delete_confirmed(typed, name) -> bool`;
  - `transfer_error_text` / `delete_error_text` (`NotOwner`, `InvalidOperation` "Bots can't own servers.", `NotFound` "They're not a member any more.").
- `src/ui/server_settings.rs`:
  - `ownership_page`, with two groups;
  - "Transfer ownership…" opens a picker dialog (fetch members, filter input, list), then `confirm_then`;
  - "Delete server…" opens a `DeleteServerDialog` (text field, Delete enabled on a match, busy while sending). On success it closes the window and calls `ChatView::leave_server` plus a toast in the main window.
- `src/ui/chat.rs`: a public way to drop the server and toast after a delete, if `leave_server` isn't enough.
- `tests/api_servers.rs`, `tests/server_settings.rs`.

**Depends on:** Chunk 1
**Risks:**
- **The ticket is used up even when the transfer fails.** A retry asks again, which `confirm_then` already does per attempt.
- **The page set changes after a transfer:** Ownership disappears. This is the page-reset path from chunk 1; check it lands on Overview (if you still have Manage Server) or that the window closes if no page is left.
- **Large servers.** `fetch_members` returns everyone; for thousands of members the picker could be slow to filter. Filter on a plain `Vec` (it's fine at that size), and don't render more than, say, the first 200 matches.
- **Delete runs while the main window is hidden** (Keep running). The toast goes to the main window and shows when it's next shown; that's acceptable.

**How to verify:**
- wiremock: transfer sends `PATCH /servers/srv` with `{"owner":"u-ada"}` and the `x-mfa-ticket` header; delete sends `DELETE /servers/srv` once.
- Unit tests:
  - candidates exclude yourself and bots;
  - `delete_confirmed` is true only on an exact trimmed match ("My Server " yes, "my server" no);
  - error texts.
- Then the full suite.

**Manual check:**
- Transfer a test server to the second account. Ownership disappears from your window, and the second account's server menu now has "Leave server…" hidden for it and shown for you.
- Transfer it back from the second account.
- Delete the test server: Delete stays disabled until the name is typed exactly. Then the window closes, the server leaves the rail, the main window toasts, and the web client loses it too.

**Shippable when:** the owner can transfer or delete a server from Server Settings, with the confirmations above.

### Chunk 7: Reports (milestone 22's chunk 5)
**Goal:** report a message, a user or a server to Stoat's moderators.
**Files likely touched:**
- `src/safety.rs` (or a new `src/reports.rs`, no GPUI):
  - `ReportTarget { Message { id, author }, User { id, message }, Server { id } }`;
  - the reason lists and labels in the web client's order;
  - `report_body(target, reason, detail) -> DataReportContent` (our own serialize type);
  - `can_report(store, target)`: not yourself, not your own message, not a server you own;
  - `report_error_text` (`CannotReportYourself` text already exists from milestone 22).
- `src/api/client.rs`: `report(&DataReportContent)` (`POST /safety/report`).
- `src/ui/report_dialog.rs` (new): `ReportDialog` (reason dropdown, detail input with counter, Report enabled when a reason is picked, and detail present for Other).
- `src/profile.rs`: `UserMenuItem::Report` in `user_menu`.
- `src/ui/chat.rs`: the message menu, `on_user_menu` and `server_menu` items.
- `tests/api_safety.rs` (new) or `tests/api_users.rs`; `tests/profile.rs`; unit tests for the reasons and body.

**Depends on:** None (independent of chunks 1–6)
**Risks:**
- **`ReportedContent`'s serde shape** (internally tagged `type`, field names). Write the wiremock test against the exact JSON from Stoat's route before wiring the UI.
- **Menu growth.** The message menu is already long; put Report at the end, after a separator, as the web client does.

**How to verify:**
- wiremock: a message report sends `{"content":{"type":"Message","id":"m1","report_reason":"SpamAbuse"},"additional_context":""}`; a user report with `message_id`; a server report.
- Unit tests:
  - `can_report` hides own messages, yourself, and owned servers;
  - every reason has a label, and Other comes last.
- `cargo test --test profile`: the user menu has Report for others, not yourself.
- Then the full suite.

**Manual check:** report a message, a user and a server on a test server. Each toasts success, and the dialog refuses Other without detail. Don't spam the official instance's moderators: use a self-hosted instance if one's handy, or file one report of each kind on a test server and say so in the detail.

**Shippable when:** messages, users and servers can be reported from their menus, as in the web client.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–22):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3; every other file this plan touches is 0.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/server_settings.rs`:
  - `access` for owner, Manage Server, Ban Members only, nobody;
  - the Overview diff (name, description, images, system messages; unchanged is `None`; validation);
  - system messages with stale channels;
  - invite rows (uses, expiry, unknown creator, expired left out);
  - ban join and filter;
  - transfer candidates;
  - the delete name match.
- Reports: body building, the reason lists, and `can_report`.
- `FileSizeLimits`: parsing `icons` / `banners`, and the defaults.

### Integration Tests
wiremock tests against the exact routes, headers and bodies:
- `PATCH /servers/{id}` (edit; transfer with `X-MFA-Ticket`);
- `DELETE /servers/{id}`;
- `GET /servers/{id}/invites` and `DELETE /invites/{code}`;
- `GET /servers/{id}/bans` and `DELETE /servers/{id}/bans/{user}`;
- `POST /safety/report`;
- the upload to `/icons` and `/banners` followed by the edit.

Failure cases: `MissingPermission`, `NotOwner`, `NotFound`, `FailedValidation`, `CannotReportYourself` become plain toasts.

Store: an edit applied locally, then the same `ServerUpdate` arriving, leaves the server in one consistent state (`tests/store_servers.rs`).

### Manual Tests
Run with two accounts and the web client, on a test server owned by the first account. Batch them at the end if the user asks, as in recent milestones:
1. Overview: rename, describe, icon (static and animated), banner, removing each. Check the rail, the header, the info dialog and the web client.
2. System messages: set and turn off each kind, and watch the second account join, leave, get kicked and get banned.
3. Invites: create, copy, open in the web client, revoke.
4. Bans: ban with a reason, filter, unban, rejoin.
5. Transfer to the second account and back. The pages appear and disappear without showing the wrong page.
6. Delete a throwaway server. The window closes and a toast shows.
7. Permissions: an account with only Ban Members sees only Bans; one with neither doesn't see "Server settings…".
8. The window closes when the server is deleted from the web client, and when you log out.
9. Reports: one of each kind.
10. Both themes: the window, dialogs and destructive buttons are readable.

## Definition of Done
- [ ] "Server settings…" in the server menu and the info dialog, shown with Manage Server, Ban Members, or ownership. It opens one window at a time, titled with the server's name.
- [ ] Overview saves name (1–32), description (≤1024, blank removes it), icon and banner (set or remove, within the limits), and the four system-message channels. Only changed fields are sent, and a deleted channel can't make the save fail.
- [ ] Invites lists every server invite with creator, channel, code, uses and expiry. Revoke removes it (confirmed), Copy link copies, and Create invite… adds one.
- [ ] Bans lists bans with user and reason, filters them, and Unban (confirmed) lifts one.
- [ ] Transfer ownership sends `owner` with an MFA ticket to a non-bot member chosen from a filtered list. Ownership pages disappear afterwards without leaving the sidebar on the wrong page.
- [ ] Delete server is enabled only after typing the name exactly. It deletes, closes the window, and drops the server from the rail.
- [ ] The window closes itself when its server leaves the store or its account signs out.
- [ ] Report message / user / server from their menus, with Stoat's reasons, never offered for yourself, your own messages, or servers you own.
- [ ] Stoat's refusals become plain toasts: `MissingPermission`, `NotOwner`, `NotFound`, `FailedValidation`, `InvalidOperation`, `CannotReportYourself`.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan, and milestone 22's record points to chunk 7 for Reports.
