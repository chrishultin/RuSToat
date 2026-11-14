# Implementation Record: Server settings (milestone 23)

_Plan: docs/plans/server-settings.md_
_Started: 2026-11-09_

**Mode:** continuous. Manual checks are batched at the end, as in milestone 22 (the user didn't ask for them per chunk).

**Files dirty at the start:** the plan (`docs/plans/server-settings.md`, untracked) and the roadmap (`docs/plans/roadmap.md`, linking the plan). Both are this milestone's, so they go in with chunk 1.

## Chunk 1: Server Settings window with name and description

### Changes Made
- `src/server_settings.rs` (new, no GPUI; registered in `src/lib.rs` by hand):
  - `Access { overview, invites, bans, ownership }` with `any()`.
  - `access(store, server)`: the owner sees everything. Anyone else needs a known membership: Manage Server for Overview and Invites, Ban Members for Bans.
  - `OverviewForm { name, description }`:
    - `of(&Server)` fills it from the server;
    - `is_valid()`: name 1–32 and description ≤1024, both trimmed;
    - `changes(&Server) -> Option<DataEditServer>`: only what changed, trimmed. A blank description becomes `remove: [Description]`. `None` when nothing changed or the form is invalid.
  - `empty_server_edit()`, `DESCRIPTION_MAX`, and `NAME_MAX` re-exported from `server_edit`.
  - `save_error_text`: texts for `MissingPermission`, `FailedValidation` and `NotFound`, else `error_text`.
- `src/ui/server_settings.rs` (new; registered in `src/ui/mod.rs` by hand):
  - `ServerSettingsWindow` (library global):
    - `open(chat, server, cx)` is deferred. It brings the same server's window forward; otherwise it closes the old window and opens a new one, 760×560 and centred.
    - `close(cx)`.
  - `ServerSettingsView`:
    - pages are `ServerPageFn = fn(&ServerSettingsView, &mut App) -> Option<SettingPage>`, in `PAGES`;
    - `shown: Vec<bool>` records which pages were visible at the last render. The sidebar's keyed ID (`page_key`) is bumped only when that set changes;
    - the window title is "Server Settings — ‹name›", updated on rename;
    - it observes the `ChatView`, and closes the window when the server leaves the store or no page is left;
    - `observe_release_in` closes it when the `ChatView` goes (sign-out);
    - it follows macOS appearance, as `SettingsView` does.
  - `overview_page`, and `OverviewPage`:
    - a name input and a description textarea, with counters through `profile_editor::field`;
    - untouched fields follow live changes (`follow`, compared against `filled`);
    - Reset and Save. Save sends one `edit_server`, then `ChatView::server_edited`, refills the form, and toasts "Saved." A refusal toasts `save_error_text`.
- `src/api/client.rs`: `edit_server(server, &DataEditServer) -> Server` (`PATCH /servers/{id}` through `edit_body`). Imports `DataEditServer` and `Server`.
- `src/store.rs`: `apply_server_edit(&Server) -> bool`, built on `apply_server_update`. It takes name, owner, description, icon, banner and system messages from Stoat's answer, and clears those the answer lacks.
- `src/ui/chat.rs`:
  - "Server settings…" (`Settings` icon) in `server_menu`, after "Invite people…", when `access(..).any()`;
  - `ChatView::open_server_settings(server, cx)` and `ChatView::server_edited(&Server, cx)` (applies the edit, then `server_permissions_changed`, then notify);
  - `open_info_dialog` passes a "Server settings…" callback for servers the user may manage.
- `src/ui/server_dialogs.rs`: `render_info` takes `on_settings: Option<OnSettings>` (a new `Rc` alias), drawn as a ghost button beside Close.
- `src/assets.rs`: `server_settings_icons_load` (Settings, Link, Ban, Crown).
- Tests:
  - `tests/server_settings.rs` (new), 9 tests:
    - `access` for the owner, Manage Server, Ban Members only, a plain member, an unknown membership and an unknown server;
    - `OverviewForm`: unchanged gives `None`, including surrounding whitespace; a rename sends only the name; a description is trimmed; a blank description is removed; the 32 and 1024 limits.
  - `tests/api_servers.rs`: `edit_server` sends `{"name":"Lounge","remove":["Description"]}` once; `MissingPermission` gives "You don't have permission to change this server."
  - `tests/store_servers.rs`: a saved edit shows at once, and the matching `ServerUpdate` then leaves the server identical. An unknown server is ignored.

### Divergences
- **`open_server_settings` takes no `window`.** Opening is deferred to a new window, so the main window isn't needed. The plan's signature had one.
- **The window also closes when no page is left** (e.g. Manage Server taken away), not only when the server goes. Chunk 6's risk asked for this; it's done here because the same observer handles it.
- **`apply_server_edit(&Server)` takes no `removed` list.** The answer is the whole server, so a field it lacks is cleared. That gives the same result as passing the edit's `remove`.
- None of the plan's risks materialised:
  - `gpui_kit::open_window` works from the library;
  - `window.remove_window()` only sets a flag, so it's safe inside the observer;
  - no `cx.defer` was needed for closing.
- No look-ahead changes to later chunks.

### Verification Performed
- `cargo test --test server_settings --test api_servers --test store_servers`: 9 + 17 + 15 passed.
- `cargo test`: 1137 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after adding the `OnSettings` alias for `type_complexity`.
- rustfmt:
  - drift unchanged: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/ui/server_dialogs.rs` 0, `src/assets.rs` 0;
  - new files clean: `src/server_settings.rs`, `src/ui/server_settings.rs`, `tests/server_settings.rs`;
  - `tests/api_servers.rs` and `tests/store_servers.rs` are still clean.

### Manual Validation
Deferred to the end (rename, clear the description, switch servers, and a member without permissions).

## Chunk 2: Icon and banner

### Changes Made
- `src/api/client.rs`:
  - `FileSizeLimits` gains `icons` and `banners`;
  - `DEFAULT_ICON_SIZE_LIMIT` (2.5 MB) and `DEFAULT_BANNER_SIZE_LIMIT` (6 MB);
  - `RootConfig::icon_size_limit()` / `banner_size_limit()` take the stricter of the two account limits, else the default.
- `src/api/mod.rs`: re-exports the two defaults (hand-edited).
- `src/profile_edit.rs`: `ImageLimits` gains `server_icon` and `server_banner`, set by `Default` and `from_root`.
- `src/ui/profile_editor.rs`:
  - `upload_image(client, file_server, tag, path)` and `Preview::of(image, tag, media, fallback)` take the file server's tag instead of an `ImageKind`. Profile callers pass `ImageKind::…tag()`.
  - `banner_preview(id, preview, cx)` is `pub(crate)` and takes its element ID. It was already a 232:100 strip.
- `src/ui/identity_dialog.rs`: its two calls pass `ImageKind::Avatar.tag()`.
- `src/server_settings.rs`:
  - `ServerImage { Icon, Banner }` with `tag()` (`icons` / `banners`) and `limit(ImageLimits)`;
  - `OverviewForm` gains `icon` and `banner: ProfileImage`, filled by `of`;
  - `new_files()` and `has_changes(&Server)`;
  - `changes` sends a new uploaded ID as `icon` / `banner` (no remove), a removed image as `remove: [Icon/Banner]`, and leaves out a picked file until it's uploaded.
- `src/ui/server_settings.rs`, `OverviewPage`:
  - `icon` / `banner: ImageSlot` rows above the name, using milestone 15's `render_image_row`, `avatar_preview` (round) and `banner_preview`;
  - Change… goes through `prompt_for_image` with the server limits;
  - live-follow and Reset cover the images;
  - Save uploads new files first ("Uploading…"), then computes `changes` against the server as it is then, and sends one `PATCH`;
  - a failed upload or `PATCH` toasts and keeps the form (`save_failed`).
- Tests:
  - `tests/server_settings.rs`, 6 new: the images from the server; a new icon sends its ID without a remove; removed images go in `remove`; unchanged images are left out; a picked image waits for its upload; tags and limits.
  - `tests/api_upload.rs`: `root_config_gives_the_server_icon_and_banner_limits` (`icons` 2.5 MB from `root.json`, `banners` defaulting to 6 MB, the stricter account limit, and the defaults with no limits). `a_server_icon_is_uploaded_then_saved_by_its_id` uploads to `/icons`, then sends `PATCH /servers/srv` `{"icon":"f-icon"}`. The existing limits test gains the two new fields.
  - `tests/api_servers.rs`: the chunk 1 edit test builds its form with `..OverviewForm::of(..)`.

### Divergences
- **`ImageKind` wasn't widened.** A parallel `ServerImage` lives in `src/server_settings.rs`, as the plan allowed. Widening would have put meaningless server arms in three of the profile editor's matches. Instead, `upload_image` and `Preview::of` now take a tag. Profile banners still go to `backgrounds`; `avatars_and_banners_go_to_their_own_tags` still passes.
- **The limits tests are in `tests/api_upload.rs`**, where the avatar and banner limits already were. The plan said `tests/api_instance.rs`.
- **`tests/api_upload.rs` started with 10 rustfmt hunks**, not 0 as the plan's formatting note implied for unlisted files. I formatted it whole by mistake, then restored it from HEAD and reapplied only this chunk's changes. Its new code is hand-formatted, and the count stays at 10.
- No look-ahead changes. Chunk 3 adds system messages to the same `OverviewForm` and `changes`.

### Verification Performed
- `cargo test --test server_settings --test api_upload`: 15 + 14 passed.
- `cargo test`: 1145 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt:
  - drift unchanged: `src/api/client.rs` 3, `tests/api_upload.rs` 10;
  - clean files still clean: `src/profile_edit.rs`, `src/ui/profile_editor.rs`, `src/ui/identity_dialog.rs`, `tests/api_servers.rs`;
  - new files clean: `src/server_settings.rs`, `src/ui/server_settings.rs`, `tests/server_settings.rs`.

### Manual Validation
Deferred to the end (PNG and animated GIF icons, a banner, removing both, and a 3 MB icon refused).

## Chunk 3: System-message channels

### Changes Made
- `src/server_settings.rs`:
  - `SystemMessageKind { Joined, Left, Kicked, Banned }` with `ALL` and `label()` ("User joined", "User left", "User kicked", "User banned").
  - `SystemChannels { user_joined, user_left, user_kicked, user_banned: Option<String> }`:
    - `of(&Server)` reads the server's channels, with any channel not in `server.channels` shown as off;
    - `existing(&Server)` drops unknown channels;
    - `get` / `set`, and `From<SystemChannels> for SystemMessageChannels`.
  - `OverviewForm` gains `system`. `changes` compares the edited channels (unknown ones dropped) with the server's. A difference sends the whole object as `system_messages`, or `remove: [SystemMessages]` when every kind is off. It goes in the same `DataEditServer` as the other fields.
  - `system_channel_choices(store, server) -> Vec<(id, name)>`: the server's channels in channel-list order (`server_channels`), voice channels included. A chosen channel the list doesn't show the user is added at the end ("Hidden channel" if its name is unknown).
- `src/ui/server_settings.rs`, `OverviewPage`:
  - a `system: SystemChannels` field, covered by `edited`, Reset and live-follow (per kind);
  - a "System messages" section under the description: four rows, each an outline button "#channel ▾" or "Off ▾" with a dropdown (`DropdownMenu`, scrollable, max 320 px): Off, then every choice, the current one ticked;
  - saved by the same Save as the rest of the form.
- `tests/server_settings.rs`:
  - the `srv` fixture gains three channels (`ch-b`; `ch-a` and voice `ch-voice` in a category) and `system_messages` with a deleted `ch-gone`;
  - 6 new tests: a deleted channel shows as off and isn't sent; setting one sends the whole object without deleted channels (JSON `{"user_joined":"ch-a","user_banned":"ch-voice"}`); all off gives `remove: [SystemMessages]`; a channel picked then deleted isn't sent; choices follow channel order and include voice channels; the labels.

### Divergences
- **Voice channels are offered.** Stoat 0.16.2's `server_edit.rs` only checks that each ID is in `server.channels`, and voice channels there are text channels with chat. As the plan said, I kept them and noted it.
- **The plan's `SystemMessagesForm` / `edit()` is `SystemChannels`**, merged into `OverviewForm::changes`, so one `DataEditServer` carries every change.
- **Pickers are dropdown menus on outline buttons** (the `CreateInviteDialog` pattern), not gpui-component's `Select`. No select state is needed.
- No look-ahead changes.

### Verification Performed
- `cargo test --test server_settings`: 21 passed.
- `cargo test`: 1151 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: the new files (`src/server_settings.rs`, `src/ui/server_settings.rs`, `tests/server_settings.rs`) are clean. No other file was touched.

### Manual Validation
Deferred to the end (point "User joined" at a channel and have the second account join; turn "User left" off and have it leave).

## Chunk 4: Invites page

### Changes Made
- `src/api/client.rs`: `fetch_server_invites(server) -> Vec<Invite>` (`GET /servers/{id}/invites`) and `delete_invite(code)` (`DELETE /invites/{code}`).
- `src/server_settings.rs`:
  - `InviteRow { code, creator, creator_name, creator_avatar, channel, uses, expiry }`.
  - `invite_rows(store, invites, now)`:
    - keeps Stoat's order, since codes carry no creation time;
    - skips `Invite::Group`, and invites that have expired by `now`;
    - uses "Unknown user" (no avatar) for an uncached creator, and "Unknown channel" for an unknown channel;
    - expiry is "Never expires", or "Expires in 3 days" through `time::relative_time`.
  - `uses_label`: "1 use", "3 uses", "3 / 10 uses".
  - `timestamp_utc` (a Stoat `Timestamp` to `chrono`, through its JSON string, as `permissions::is_timed_out` does).
  - `invites_error_text` and `revoke_error_text`.
- `src/ui/moderation_dialogs.rs`: `confirm_member_action_then(request, action, after, …)` and `AfterAction`. It is the same confirmation, but Stoat's answer goes to `after` once the dialog has closed, instead of the usual refusal toast. `confirm_member_action` is unchanged for milestone 22's callers; both share `open_confirmation`.
- `src/ui/invites.rs`: `CreateInviteDialog` emits `InviteCreated` when Stoat makes the invite.
- `src/ui/chat.rs`: `first_invitable_channel` is now `pub`, and there's a new `invite_max_days()` accessor.
- `src/ui/server_settings.rs`:
  - a generic `Load<T> { Idle, Loading, Loaded, Failed(text) }`;
  - `invites_page` (Link icon, Manage Server);
  - `InvitesPage`:
    - fetched the first time the page may be shown (`load_once`, from `ServerSettingsView::render`);
    - a header with "Create invite…" (disabled with no channel to invite to) and Refresh;
    - rows: the creator's avatar and name, "#channel · code", uses and expiry, Copy link (a link from `web_app`, else the code, with a toast), and a red Revoke;
    - Revoke confirms with `confirm_member_action_then` ("Revoke invite {code}?"). The row is removed on success or `NotFound`; any other refusal toasts `revoke_error_text`;
    - "Create invite…" opens `CreateInviteDialog` in the settings window for `first_invitable_channel`, and refreshes the list on `InviteCreated`;
    - Loading, Failed with Retry, and "This server has no invites." states.
- Tests:
  - `tests/api_invites.rs`: a list with and without `max_uses` / `expires`; a revoke is one `DELETE /invites/abc`; a `MissingPermission` on the list becomes its text.
  - `tests/server_settings.rs`: rows name the creator and channel (unknown ones too), with uses and relative expiry; expired and group invites are left out; `uses_label`.

### Divergences
- **`confirm_member_action` couldn't report success**, but the page must drop the revoked row. I added `confirm_member_action_then` beside it rather than changing milestone 22's callers.
- **`CreateInviteDialog` needed only an event** (`InviteCreated`). It doesn't depend on the main window.
- **Uses read "3 uses" / "3 / 10 uses"** rather than the plan's bare "3" / "3 / 10".
- **Look-ahead:** chunk 5 (Bans) will reuse `Load<T>`, `load_once` and `confirm_member_action_then`. The plan's approach is unchanged; no amendment needed.

### Verification Performed
- `cargo test --test server_settings --test api_invites`: 24 + 12 passed.
- `cargo test`: 1157 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt:
  - drift unchanged: `src/ui/chat.rs` 17, `src/api/client.rs` 3;
  - still clean: `src/ui/invites.rs`, `src/ui/moderation_dialogs.rs`, `tests/api_invites.rs`;
  - new files clean.

### Manual Validation
Deferred to the end (create from the page, copy and open the link in the web client, revoke, and a Ban-Members-only account doesn't see Invites).

## Chunk 5: Bans page

### Changes Made
- `src/api/client.rs`: `fetch_bans(server) -> BanListResult` (`GET /servers/{id}/bans`) and `unban(server, user)` (`DELETE /servers/{id}/bans/{user}`). Imports `BanListResult`.
- `src/server_settings.rs`:
  - `BanRow { user, name, avatar, reason }`.
  - `ban_rows(store, &BanListResult)`:
    - joins each ban with its user on the ID, and sorts by name, case-insensitively;
    - a ban whose user is missing keeps its row under the ID;
    - avatars come from `store.media().file(..)`, since banned users aren't in the store;
    - a blank reason becomes `None`.
  - `filter_bans(rows, query)`: name or reason contains the query, ignoring case; a blank query keeps everything.
  - `bans_error_text` and `unban_error_text` (`NotFound` reads "They aren't banned any more.").
- `src/ui/server_settings.rs`:
  - `bans_page` (Ban icon, Ban Members);
  - `BansPage`:
    - fetched the first time the page may be shown;
    - a filter field ("Filter by name or reason") and Refresh;
    - rows: avatar, `username#1234`, the reason or "No reason given", and Unban;
    - Unban confirms with `confirm_member_action_then` ("Unban {name}?", "They can rejoin with an invite."). The row goes on success or `NotFound` (which also toasts); other refusals toast;
    - Loading, Failed with Retry, "Nobody is banned from this server." and "No bans match." states.
- Tests:
  - `tests/api_members.rs`: the list parses, including a ban without a reason; an unban is one `DELETE /servers/srv/bans/u-ada`; `NotFound` gives "They aren't banned any more."
  - `tests/server_settings.rs`: bans are joined and sorted, a missing user keeps its row, and a blank reason is none; the filter matches name or reason, ignoring case.

### Divergences
- **No automatic refresh after banning from the main window.** That would need the `ServerMemberLeave` reason routed to the settings window. The plan's fallback, a Refresh button, is there.
- **A single filter field** matches name or reason (the plan's assumption 9), rather than the web client's two column filters.
- No look-ahead changes.

### Verification Performed
- `cargo test --test server_settings --test api_members`: 26 + 18 passed.
- `cargo test`: 1162 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/api/client.rs` stays at 3; `tests/api_members.rs` is still clean; new files are clean.

### Manual Validation
Deferred to the end (ban the second account with a reason, filter, unban, rejoin).

## Chunk 6: Ownership: transfer and delete

### Changes Made
- `src/api/client.rs`:
  - `transfer_server(ticket, server, owner) -> Server`: `PATCH /servers/{id}` with only `{"owner": …}`, through `send_with_ticket`;
  - `delete_server(server)`: `DELETE /servers/{id}`, documented as owner-only, since for anyone else it's a leave.
- `src/server_settings.rs`:
  - `Candidate { id, name, handle, avatar }`;
  - `transfer_candidates(&AllMemberResponse, me, &MediaUrls)`: leaves out yourself, bots, and members without a user. The name is the nickname, else the display name. Sorted by name, case-insensitively;
  - `filter_candidates` (name or `@handle`, ignoring case);
  - `delete_confirmed(typed, name)`: an exact match, trimmed, case-sensitive;
  - `transfer_refusal(err) -> Option<String>` for `NotOwner`, `InvalidOperation` ("Bots can't own servers.") and `NotFound` ("They're not a member any more.");
  - `delete_error_text`.
- `src/ui/chat.rs`: `ChatView::server_deleted(server, cx)` drops the server at once (`leave_server`) and toasts "Deleted {name}." in the main window (deferred).
- `src/ui/server_settings.rs`:
  - `ownership_page` (Crown icon, owner only), and `OwnershipPage` with two sections: "Transfer ownership…" (outline) and "Delete server…" (danger).
  - `TransferDialog`:
    - fetches every member (`fetch_members(server, false)`), with a "Search members" filter;
    - lists at most 200 matches, scrollable, with "N more. Type to narrow the list.";
    - Loading and Failed with Retry states;
    - picking someone closes it and opens `confirm_then` ("Transfer {server} to {name}?" with the plan's detail, button "Transfer").
  - The transfer action returns Stoat's own refusals (`transfer_refusal`) as a result, which toasts after. Ticket problems stay the confirmation dialog's to explain.
  - On success, `ChatView::server_edited` applies the new owner, so Ownership disappears through chunk 1's page reset (or the window closes with no page left), and a toast says "{name} owns {server} now."
  - `DeleteServerDialog`: "Delete {name}?" and "…Type the server's name, {name}, to confirm.", a field, and a danger Delete enabled only on an exact match, busy while sending. On success it closes, and `ChatView::server_deleted` drops the server; the settings window then closes itself, as its server has left the store. A refusal toasts `delete_error_text`.
  - `open_focused_dialog` focuses each dialog's field once it's drawn (the Sessions rename pattern).
- Tests:
  - `tests/api_servers.rs`: a transfer sends `{"owner":"u-ada"}` with `x-mfa-ticket`; `InvalidOperation` reads "Bots can't own servers."; a delete is one `DELETE /servers/srv`.
  - `tests/server_settings.rs`: candidates leave out the owner and bots and sort by nickname or display name; the filter matches name or handle; `delete_confirmed` takes "My Server" and " My Server " but not "my server", "My Serve" or a blank name; refusal texts (an expired ticket is left to the dialog); after a `ServerUpdate` hands the server away, a former owner with no roles has no page left.

### Divergences
- **`confirm_then` would have turned every refusal into its generic `failed_text`.** Instead the transfer action returns `NotOwner`, `InvalidOperation` and `NotFound` as an `Ok(Err(text))` result, toasted after the dialog closes. The plan's `transfer_error_text` is `transfer_refusal`.
- **`server_deleted` was needed**, since `leave_server` is private and doesn't toast. The plan anticipated this.
- **Closing the window after a delete happens through chunk 1's observer** (the server leaves the store), not an explicit close.
- No look-ahead changes. Chunk 7 is independent.

### Verification Performed
- `cargo test --test server_settings --test api_servers`: 31 + 18 passed.
- `cargo test`: 1170 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/ui/chat.rs` stays at 17 and `src/api/client.rs` at 3; `tests/api_servers.rs` and the new files are clean.

### Manual Validation
Deferred to the end (transfer to the second account and back; delete a throwaway server).

## Chunk 7: Reports (milestone 22's chunk 5)

### Changes Made
- `src/reports.rs` (new, no GPUI; registered in `src/lib.rs` by hand):
  - `DETAIL_MAX` (1000);
  - `ReportTarget { Message { id, author }, User { id, message }, Server { id } }` with `title()` ("Tell us what's wrong with this message/user/server") and `reasons()`;
  - `Reason { Content(ContentReportReason), User(UserReportReason) }` with the web client's `label()`s and `is_other()`. The lists are in the web client's order, with Other (`NoneSpecified`) last;
  - `DataReportContent { content: ReportedContent, additional_context }`, the client's own body type;
  - `can_send` (a reason, detail ≤1000, and detail for Other);
  - `report_body` (detail trimmed; `None` for a reason of the wrong kind);
  - `can_report` (not yourself, not your own message, not a server you own or don't know);
  - `report_error_text` (`CannotReportYourself`, `NotFound`, `FailedValidation`);
  - unit tests.
- `src/api/client.rs`: `report(&DataReportContent)` (`POST /safety/report`, nulls stripped so a user report without a message leaves `message_id` out).
- `src/ui/report_dialog.rs` (new; registered in `src/ui/mod.rs` by hand): `open_report_dialog(client, target, subject, …)` and `ReportDialog`:
  - what's reported (a name, or "A message from {name}");
  - a "Reason for report" dropdown ("Please select a reason ▾", scrollable);
  - "Give us some detail", a textarea counted to 1000, which says "Tell us what's wrong." when Other has none;
  - a footnote that reports go to the instance's moderators;
  - Cancel and a danger Report, enabled by `can_send`. Success closes and toasts "Thanks, your report was sent."; a refusal toasts and keeps the dialog.
- `src/profile.rs`: `UserMenuItem::Report`, after Block / Unblock in the same group, for anyone but the user.
- `src/ui/chat.rs`:
  - `ChatView::open_report(target, window, cx)` (checks `can_report`, works out the subject);
  - **message menu:** "Report message…" (TriangleAlert) at the end, after a separator, for others' non-system messages (`RowAction::Report { id, author }`);
  - **person menu:** "Report user…". `People` gains an optional `message`, set by `about_message` on a message's avatar and name, so a user report from there carries `message_id`. Elsewhere (the member list, a card) it goes without;
  - **server menu:** "Report server…" before Copy server ID, unless the user owns the server.
- Tests:
  - `tests/api_safety.rs` (new): the exact message report JSON (`{"content":{"type":"Message","id":"m1","report_reason":"SpamAbuse"},"additional_context":""}`); a user report with `message_id` and trimmed detail; a user report without one leaves it out; a server report; `CannotReportYourself` reads "You can't report yourself."; `can_report` against the `ready.json` fixture (own message, self, own server, unknown server).
  - `tests/profile.rs`: the three exact menus gain `Report` after Block. New: Report is offered for anyone but yourself, and isn't destructive.
  - `src/reports.rs`: every reason has a label, Other is last, and the list sizes (14 / 7); titles; `can_send`; a mismatched reason makes no report.
- `docs/plans/member-moderation-implementation.md`: chunk 5, the summary and the remaining manual steps now point here.

### Divergences
- **`src/reports.rs`, not `src/safety.rs`**, as the plan allowed. `safety.rs` is about link safety.
- **`People` gained an optional message** so "Report user…" from a message's author carries `message_id`. That item is handled in `People::menu` directly, since `UserMenuItem` is `Copy` and can't carry the ID through `on_user_menu`. From other places, `on_user_menu`'s `Report` arm reports without a message.
- **The dialog shows a one-line subject** instead of the web client's rendered message or avatar.
- No later chunks.

### Verification Performed
- `cargo test --test api_safety --test profile` and `cargo test --lib reports`: 6 + 38 + 5 passed.
- `cargo test`: 1181 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt:
  - `src/ui/chat.rs` stays at 17 (three import-order hunks were fixed by hand), `src/api/client.rs` at 3;
  - `src/profile.rs` and `tests/profile.rs` are still clean;
  - new files clean: `src/reports.rs`, `src/ui/report_dialog.rs`, `tests/api_safety.rs`.

### Manual Validation
Deferred to the end (one report of each kind on a test server; Other refused without detail).

## Summary

_Completed: 2026-11-09_

### Chunks Completed
1. Server Settings window with name and description: `54ca0f8`.
2. Icon and banner: `eb3a0de`.
3. System-message channels: `bd5ed1b`.
4. Invites page: `8e2ae94`.
5. Bans page: `386b14b`.
6. Ownership (transfer and delete): `7974b2a`.
7. Reports (milestone 22's chunk 5): this commit.

### Divergences
- Chunk 1:
  - `open_server_settings` takes no window;
  - the window also closes when no page is left;
  - `apply_server_edit` takes the whole answer instead of a remove list.
- Chunk 2:
  - a parallel `ServerImage` instead of widening `ImageKind`, with `upload_image` / `Preview::of` now taking a tag;
  - the limits tests are in `tests/api_upload.rs`, which had 10 rustfmt hunks to begin with (kept at 10).
- Chunk 3:
  - voice channels are offered, since Stoat accepts any of the server's channels;
  - `SystemChannels` is merged into `OverviewForm::changes`;
  - pickers are dropdown menus, not a `Select`.
- Chunk 4:
  - `confirm_member_action_then` added beside `confirm_member_action`;
  - `CreateInviteDialog` emits `InviteCreated`;
  - uses read "3 uses" / "3 / 10 uses".
- Chunk 5: no automatic refresh after a ban from the main window (there's a Refresh button); one filter for name or reason.
- Chunk 6:
  - Stoat's transfer refusals are returned as a result so they toast in words;
  - `ChatView::server_deleted` was added;
  - the window closes through chunk 1's observer.
- Chunk 7:
  - `src/reports.rs` rather than `src/safety.rs`;
  - `People` carries the message for user reports;
  - a one-line subject in the dialog.

### Definition of Done
- [x] "Server settings…" in the server menu and the info dialog, shown with Manage Server, Ban Members, or ownership. It opens one window at a time, titled with the server's name. (Unit-tested `access`; window behaviour awaits the manual pass.)
- [x] Overview saves name (1–32), description (≤1024, blank removes it), icon and banner (set or remove, within the limits), and the four system-message channels. Only changed fields are sent, and a deleted channel can't make the save fail.
- [x] Invites lists every server invite with creator, channel, code, uses and expiry. Revoke removes it (confirmed), Copy link copies, and Create invite… adds one.
- [x] Bans lists bans with user and reason, filters them, and Unban (confirmed) lifts one.
- [x] Transfer ownership sends `owner` with an MFA ticket to a non-bot member chosen from a filtered list. Ownership pages disappear afterwards, through the keyed page reset. That it doesn't leave the sidebar on the wrong page awaits the manual pass.
- [x] Delete server is enabled only after typing the name exactly. It deletes, closes the window, and drops the server from the rail.
- [x] The window closes itself when its server leaves the store or its account signs out (an observer and `observe_release_in`; awaits the manual pass).
- [x] Report message / user / server from their menus, with Stoat's reasons, never offered for yourself, your own messages, or servers you own.
- [x] Stoat's refusals become plain toasts: `MissingPermission`, `NotOwner`, `NotFound`, `FailedValidation`, `InvalidOperation`, `CannotReportYourself`.
- [x] `cargo test` (1181 passed) and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [x] The roadmap links this plan, and milestone 22's record points to chunk 7 for Reports.

Every item is built and covered by automatic tests where it can be. The visual and live-server behaviour is unconfirmed until the manual pass below.

### Remaining Manual Steps
All batched at the user's request. Run with two accounts and the web client, on a test server owned by the first account:
1. **Overview:**
   - rename, describe, and set an icon (static, then animated) and a banner, then remove each;
   - check the rail, the header, the info dialog and the web client;
   - a 3 MB icon is refused before upload.
2. **System messages:** set and turn off each kind, and watch the second account join, leave, get kicked and get banned.
3. **Invites:** create from the page, copy the link and open it in the web client, then revoke it (the web client's join should fail).
4. **Bans:** ban with a reason from the member menu, Refresh, filter, unban, and rejoin.
5. **Transfer:** to the second account and back. The pages appear and disappear without showing the wrong page.
6. **Delete** a throwaway server: Delete stays disabled until the name matches exactly, then the window closes, the rail drops it, and the main window toasts.
7. **Permissions:** an account with only Ban Members sees only Bans; one with neither doesn't see "Server settings…" (in the menu or the info dialog).
8. **Self-closing:** the window closes when the server is deleted from the web client, and when you log out. Opening another server's settings closes the first.
9. **Reports:** one of each kind (message, user from a message's avatar, server). Other is refused without detail. Use a test server, and say so in the detail.
10. **Themes:** in both, the window, dialogs and destructive buttons are readable.
