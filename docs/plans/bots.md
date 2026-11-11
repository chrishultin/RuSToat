# Plan: Bots (RuSToat milestone 29)

## Summary
Milestone 29 of the [roadmap](roadmap.md) adds:
- **"My Bots"**, a section in the Settings window ([`settings-and-themes.md`](settings-and-themes.md)): list, create, edit and delete your bots, with their token, profile and requested permissions;
- **adding a bot to a server or group**, from its page or from a `{app}/bot/{id}` link, granting the permissions it requests when you may;
- **bot marks:** a "Bot" tag on bots' messages, and "Owned by …" on their profile cards.

## Approach
**What Stoat allows.** Read from `/tmp/stoatchat-0.16.4` (tag `v0.16.4`, the live API since 2026-11-11) on 2026-11-11. Paths are under `crates/`.
- **Your bots** (`delta/src/routes/bots/`, mounted at `/bots`); every route is the bot's owner's only (anyone else gets `NotFound`):
  - **List:** `GET /bots/@me` gives `OwnedBotsResponse { bots, users }`, both sorted by ID.
  - **Create:** `POST /bots/create` with `DataCreateBot { name }`. The username is 2–32 characters matching `RE_USERNAME` (`^(\p{L}|[\d_.-])+$`), and some names are blocked (`InvalidUsername`). There's an account limit, `limits.{default,new_user}.bots` (5, or 2 for new accounts): `ReachedMaximumBots`. A bot can't make bots (`IsBot`). The answer is `BotWithUserResponse { ...bot, user }`.
  - **Edit:** `PATCH /bots/{id}` with `DataEditBot { name, public, default_permissions, analytics, interactions_url, remove: [Token | InteractionsURL] }`. **`remove: ["Token"]` makes a new token.** The answer is `BotWithUserResponse`.
  - **Delete:** `DELETE /bots/{id}`. It also leaves voice calls.
- **A bot's profile** (avatar, banner, display name, bio) is its user's: `PATCH /users/{bot_id}` with `DataEditUser`, allowed for the bot's owner (`delta/src/routes/users/edit_user.rs`: `is_bot_owner`). Uploads use the same tags as a user's (`avatars`, `backgrounds`).
- **Adding a bot** (`bots/fetch_public.rs`, `bots/invite.rs`):
  - `GET /bots/{id}/invite` gives `PublicBot { _id, username, avatar (file ID), description, privacy_policy_url, terms_of_service_url, default_permissions: Option<i64> }`, for a public bot or your own (else `NotFound`).
  - `POST /bots/{id}/invite?permissions=n` with `InviteBotDestination`, untagged: `{ "server": id }` or `{ "group": id }`. A private bot that isn't yours is `BotIsPrivate`.
  - **Server:** needs **Manage Server**. The bot joins **first**. Then, only if `permissions` is given, Stoat checks **Manage Roles** and the grant rule `throw_permission_override(Override::default(), { allow: n })` (the user must hold every bit), and makes a **managed role** named after the bot with those permissions and `owner: bot` (new in 0.16.4), given to the bot. **A refusal there comes after the bot has joined,** so the client must only send `permissions` it knows Stoat will take.
  - **Group:** needs **Invite Others** in it. `permissions` is ignored.
- **Bot users:** `User.bot: Option<BotInformation { owner }>`.

**What the web client does** (`/tmp/for-web-0.16.3`, `packages/client/`):
- **`components/app/interface/settings/user/bots/MyBots.tsx`:** "My Bots" in user settings. Create Bot (a username), then a row per bot opening `ViewBot.tsx`, which has:
  - the bot's profile through the normal `UserProfileEditor` (avatar, banner, display name, bio);
  - "Allow others to invite your bot" (public);
  - Reset Token ("Generate a new token if it gets lost or compromised", confirmed in a modal) and Change Username;
  - Invite Bot, Copy Invite URL (`{app}/bot/{id}`), Copy ID, Copy Token, and Delete Bot (confirmed).
- **`components/modal/modals/AddBot.tsx`:** the bot's avatar, name and "Description provided by {name}", then a server you have Manage Server in (and the bot isn't in) or a group, and Add. "The bot will not be granted any permissions."
- **`src/index.tsx`:** the `/bot/:code` route fetches the public bot and opens AddBot.

**What exists in RuSToat** (as of 2026-11-11, after milestone 28, `899092d`):
- **Settings window** (`src/ui/settings.rs`, 576 lines): `SettingsView` with per-account page entities in maps (`profile_editors`, `sessions_views`, `account_views`, `details_views`), each made for the account on screen (`sync_*`, `shown_*`), and pages as `PageFn`s (Profile, Account, Sessions, Appearance, Notifications, App, Keyboard Shortcuts).
- **`src/ui/profile_editor.rs` (1,087 lines):** `ProfileEditor` (the Profile page) is bound to the account through its `me` field. It fetches the profile with `fetch_profile(&me)` and saves with `edit_user` (`PATCH /users/@me`). There are also `ImageSlot`, `render_image_row`, `field`, `upload_image` and the previews.
- **`src/profile_edit.rs`:** `ProfileForm`, `ProfileForm::changes`, `check_image`, `ImageLimits`, and `save_error_text`.
- **Profile cards:** `src/profile.rs` `ProfileCard.bot` and the "Bot" badge (`src/ui/profile_card.rs`), but no owner. Message headers have no bot mark.
- **Links:** `ChatView::open_external` (`src/ui/chat.rs`) turns this instance's invite links into the Join dialog (`invites::code_from_link`) and channel links into navigation (`links::channel_from_link`), using `invite_hosts()`.
- **Permissions:** `server_permissions`, `channel_permissions`, `has`; `permission_matrix::{entries, MatrixContext, value_enabled, missing_from_default}`; the `PermissionMatrix` view (Values mode).
- **Store:** `ChatStore::add_user`, `user`, `member(server, user)`, `servers()`, `channels()`.

**The chosen shape** (user's choices on 2026-11-11):
- **"My Bots" is a Settings window section**, per account like Sessions: a list, then a bot's page on the same page.
- **A bot's profile reuses the Profile editor:** `ProfileEditor` gains a target user, so the same form edits a bot's avatar, banner, display name, pronouns and bio, saved with `PATCH /users/{bot_id}`.
- **`/bot/{id}` links open the Add bot dialog** when clicked, as in the web client. There's no card in the message.
- **Adding a bot offers its requested permissions** (`default_permissions`):
  - they're shown as checkboxes in the dialog, each ticked and enabled only if the user may grant it: Manage Roles, and holding that permission (Stoat checks every bit, as for Everyone in milestone 25);
  - what's left ticked is sent as `?permissions=n`, and nothing is sent when none are;
  - a bot's page lets its owner set what it requests, with milestone 25's matrix in Values mode (the web client has no editor for this, but `DataEditBot.default_permissions` takes it).
- **Logic and views are split:**
  - `src/bots.rs` (new, no GPUI): rows, checks, forms, URLs, the grantable permissions and texts;
  - `src/ui/bots_page.rs` (new): the My Bots section and a bot's page;
  - `src/ui/add_bot.rs` (new): the Add bot dialog, opened in the main window.

## Assumptions
1. **"My Bots"** sits after Sessions in the Settings sidebar (icon `Bot` if bundled, else `Cpu`). It fetches `GET /bots/@me` the first time it shows for an account, with "Loading…", or the reason and Try again.
   - The list is "Create bot…", then a row per bot (avatar, display name, `@username`, "Public" or "Private", chevron), by name.
   - "{n} of {max} bots" uses root config's account limit (`default`, the established accounts' limit, else `new_user`, else 5; the stricter one would wrongly block established accounts at 2, and Stoat refuses new accounts itself). Create bot… is disabled at the limit, with the reason.
   - The bots' users go into the store (`add_user`), so avatars and the profile editor can read them.
2. **Create bot** is a dialog with a username, checked locally like Stoat (2–32 characters, letters in any script, digits, `_`, `.` and `-`). "Create" adds the bot and opens its page.
3. **A bot's page** has "‹ My Bots", then:
   - **Profile:** the Profile editor for the bot (chunk 3);
   - **Settings:**
     - the username (with its own Save, since it's a rename);
     - "Public" ("Anyone can add this bot to their servers and groups.");
     - "Interactions URL" (optional, up to 2048; blank removes it);
     - **Requested permissions:** the matrix in Values mode, server context, with every option enabled (the owner states what it asks for; nothing is granted here). These save together with the bar's Save;
   - **Token:** shown as `••••••` with Copy token, and "Reset token…", which asks "Reset {name}'s token?" / "The old token stops working at once. Anything using it needs the new one." and toasts "Token reset." The new token is in the answer;
   - **Share:** Copy ID, Copy invite URL (`{app}/bot/{id}`, when the instance's web client is known), and "Add to a server or group…";
   - **Delete bot…**, which asks "Delete {name}?" / "It leaves every server and group it's in. This can't be undone."
4. **Tokens** are never logged and never shown, only copied.
5. **The Add bot dialog** shows the bot's avatar, name and description ("Description provided by the bot's owner"), then a choice of destination:
   - **servers** where the user has Manage Server and the bot isn't a known member;
   - **groups** where the user has Invite Others and the bot isn't a member.

   Each is listed with its icon and name, servers first, with a filter field when there are more than 8.
   - **Requested permissions** (servers only, and only when the bot asks for any) are checkboxes with the matrix's titles. Each is ticked and enabled if the user has Manage Roles and holds it; otherwise it's unticked and disabled, with "You need Manage Roles to give it permissions." or "You don't have this permission."
   - "Add" sends the invite, toasts "Added {bot} to {place}.", and closes.
   - **Without a destination** (no servers or groups qualify), the dialog says so and offers only Close.
6. **`/bot/{id}` links:** `links::bot_from_link(url, hosts)` recognises `{app}/bot/{id}` on the account's own web client, and the official hosts on the official instance, like invite links (the ID must be a ULID). `open_external` opens the Add bot dialog for it, which fetches the public bot first. `NotFound` reads "That bot doesn't exist, or it's private."
7. **Bot marks:**
   - in a message header, a small "Bot" tag after the author's name, when the author is a known bot user (not for webhooks or masquerades, which show their own names);
   - on a profile card, "Owned by {name}" under the username, the owner's display name from the store, else their ID's start. Clicking it opens the owner's card if they're cached.
8. **Errors (`bot_error_text`):**
   - `InvalidUsername`: "That username can't be used. Try another.";
   - `UsernameTaken`: "That username is taken.";
   - `ReachedMaximumBots` (which carries no number): "You have {n} bots, the most Stoat allows.", with how many bots the user has (`bot_error_text(err, owned)`), else "You have as many bots as Stoat allows.";
   - `IsBot`: "Bots can't manage bots.";
   - `BotIsPrivate`: "That bot is private.";
   - `NotFound`: "That bot doesn't exist, or it's private.";
   - `MissingPermission`: "You don't have permission to add bots there.";
   - `AlreadyInServer` / `AlreadyInGroup`: "The bot is already there.";
   - `CannotGiveMissingPermissions`: "You can't grant permissions you don't have.";
   - `FailedValidation`: "Check the bot's name and interactions URL.";
   - else `error_text`.

## Plan

### Chunk 1: My Bots: list, create, delete
**Goal:** see your bots in Settings, create one, copy its ID, and delete one.
**Files likely touched:**
- `src/bots.rs` (new, no GPUI):
  - `BotRow { id, username, display_name, avatar_url, public }` and `bot_rows(&OwnedBotsResponse, media)`;
  - `check_bot_username`;
  - `bot_limit(root)` and `at_limit(count, max)`;
  - `delete_prompt` and `bot_error_text` (Assumption 8).
- `src/api/client.rs`: `fetch_owned_bots()`, `create_bot(name)` and `delete_bot(id)`; root limits gain `bots`.
- `src/ui/bots_page.rs` (new): `BotsPage` (fetch once per account, list, `CreateBotDialog`, delete with confirmation), and a minimal bot page (heading, Copy ID, Delete) to grow in chunks 2–3.
- `src/ui/settings.rs`: `bots_views` map, `sync_bots_view` / `shown_bots_view`, and `bots_page` after Sessions.
- `src/ui/chat.rs`: `ChatView::bot_users_loaded(users)` (store `add_user`), and the bot limit from root config.
- `src/assets.rs` (the icon), `src/lib.rs`, `src/ui/mod.rs`.
- `tests/bots.rs` (new), `tests/api_bots.rs` (new).

**Depends on:** None
**Risks:**
- **`OwnedBotsResponse` pairs bots and users by ID order,** not by index. Match them by ID.
- **A bot's user may also be cached** from a shared server. `add_user` must not lose fields the store has (check it replaces sensibly, or merge).

**How to verify:**
- `cargo test --test bots`: rows (matched by ID, a missing user, public/private, order); the username check (valid, too short, too long, spaces, letters in other scripts, `.`, `-` and `_`); the limit; the texts.
- `cargo test --test api_bots`: `GET /bots/@me` parses; `POST /bots/create` sends exactly `{"name"}`; `DELETE /bots/{id}`; `ReachedMaximumBots` and `InvalidUsername` read plainly.
- Then the full suite.

**Manual check:** My Bots lists your bots like the web client; create one, copy its ID, delete it.

**Shippable when:** your bots can be listed, created and deleted from Settings.

### Chunk 2: A bot's settings and token
**Goal:** rename a bot, make it public or private, set its interactions URL and requested permissions, and copy or reset its token.
**Files likely touched:**
- `src/bots.rs`: `BotForm { public, interactions_url, default_permissions }` with `of` and `changes -> Option<DataEditBot>` (a blank URL becomes `remove: [InteractionsURL]`), `username_change`, `reset_prompt`, `token_reset_edit()` (`remove: [Token]`), and `invite_url(app, id)`.
- `src/api/client.rs`: `edit_bot(id, &DataEditBot) -> BotWithUserResponse`.
- `src/ui/bots_page.rs`: the bot page's Settings (username with its own Save; Public; Interactions URL; the requested-permissions matrix; Reset and Save in the bar), Token (Copy token, Reset token…), and Share (Copy ID, Copy invite URL).
- `tests/bots.rs`, `tests/api_bots.rs`.

**Depends on:** Chunk 1
**Risks:**
- **The token is in every bot answer.** Keep it in the page's state only, never in logs, and never drawn.
- **A rename is a username change** (`user.update_username`): it can fail on its own (`UsernameTaken`), so it has its own Save, separate from the rest.

**How to verify:**
- `cargo test --test bots`: `BotForm::changes` (public, URL set, blank → remove, permissions, nothing → `None`); `token_reset_edit` is exactly `remove: [Token]`; `invite_url`.
- `cargo test --test api_bots`: the `PATCH` bodies (`{"public":true}`, `{"remove":["Token"]}`, `{"default_permissions":n}`, `{"name"}`) and the parsed answers.
- Then the full suite.

**Manual check:** make a bot public, reset its token (the old one stops working with a bot client), copy the new one, and rename the bot.

**Shippable when:** a bot's settings and token can be managed from its page.

### Chunk 3: A bot's profile
**Goal:** edit a bot's avatar, banner, display name, pronouns and bio with the Profile editor.
**Files likely touched:**
- `src/ui/profile_editor.rs`:
  - `ProfileEditor::new` keeps editing the account; `ProfileEditor::for_bot(chat, bot)` edits a bot's user;
  - the `me` field becomes `user`, plus whether it's the account's own;
  - fetching the profile and saving use that user. "Server identities" stays the account's only (it's a separate group in the Settings page).
- `src/api/client.rs`: `edit_bot_user(id, &DataEditUser) -> User` (`PATCH /users/{id}`, through `edit_body`).
- `src/ui/bots_page.rs`: the bot page's Profile section.
- `tests/api_bots.rs`.

**Depends on:** Chunk 1
**Risks:**
- **`ProfileEditor` assumes the account in places:** the store's `me`, the toast wording, and refreshing the main window's own user. Grep every use of `me` and of `edit_user` in the editor and keep the account's behaviour identical (the existing profile tests must pass unchanged).
- **The answer is the bot's `User`.** Apply it to the store with `add_user` (or `apply_user_update`), so avatars elsewhere follow.

**How to verify:**
- `cargo test --test profile` (unchanged) and the full suite.
- `cargo test --test api_bots`: `PATCH /users/{id}` with only what changed, and the parsed `User`.

**Manual check:** give a bot an avatar and a bio; its messages and profile card show them.

**Shippable when:** a bot's profile is edited with the same form as your own.

### Chunk 4: Add a bot to a server or group
**Goal:** add a bot from its page or a `/bot/{id}` link, granting the permissions it requests that you may give.
**Files likely touched:**
- `src/bots.rs`:
  - `destinations(store, bot) -> Vec<Destination { Server | Group, id, name, icon }>` (Assumption 5);
  - `requested(bot_permissions) -> Vec<&PermissionEntry>` (in the matrix's order);
  - `grantable(store, server, bits) -> u64` (Manage Roles, and held bits);
  - `invite_permissions(ticked, grantable) -> Option<u64>` (`None` when nothing is ticked).
- `src/links.rs`: `bot_from_link(url, hosts) -> Option<String>`.
- `src/api/client.rs`: `fetch_public_bot(id) -> PublicBot` and `invite_bot(id, &InviteBotDestination, permissions: Option<u64>)`.
- `src/ui/add_bot.rs` (new): `AddBotDialog` (loading the public bot, description, destinations with a filter, requested permissions, Add).
- `src/ui/chat.rs`:
  - `open_add_bot(id, window, cx)`;
  - `open_external` handles bot links;
  - after a server invite, a `ServerMemberJoin` will follow (no extra handling).
- `src/ui/bots_page.rs`: "Add to a server or group…" opens it in the main window.
- `tests/bots.rs`, `tests/links.rs` (or `tests/nav.rs`), `tests/api_bots.rs`.

**Depends on:** Chunk 1
**Risks:**
- **Stoat adds the bot before checking the permissions.** Only ever send bits that `grantable` allows; if the user lacks Manage Roles, send none. Test that `invite_permissions` can't produce a bit outside `grantable`.
- **"Already a member"** is only known for servers whose member list RuSToat has. A server whose list isn't loaded still shows. Stoat's `AlreadyInServer` reads plainly.
- **The dialog opens in the main window** (from Settings too), so it must work while the Settings window is in front: open it with the main window's handle.

**How to verify:**
- `cargo test --test bots`:
  - `destinations` (servers with and without Manage Server, the bot already a member, groups with and without Invite Others, the bot already in one, order);
  - `requested` order;
  - `grantable` (owner, without Manage Roles, unheld bits);
  - `invite_permissions` never exceeds `grantable` (exhaustive over a few bits), and is `None` when nothing is ticked.
- Links: `bot_from_link` for the own web client, the official hosts on the official instance only, a non-ULID, and other paths.
- `cargo test --test api_bots`: `GET /bots/{id}/invite` parses `PublicBot`; `POST /bots/{id}/invite` sends `{"server"}` with `?permissions=n`, and `{"group"}` without; `BotIsPrivate` and `AlreadyInServer` read plainly.
- Then the full suite.

**Manual check:**
- From a bot's page, add it to a server with some of its requested permissions; it joins with a managed role holding exactly those. Add it to a group.
- Paste `{app}/bot/{id}` in a message and click it: the dialog opens.

**Shippable when:** bots can be added to servers (with grantable requested permissions) and groups, from their page or a link.

### Chunk 5: Bot marks
**Goal:** bots' messages carry a "Bot" tag, and their profile cards say who owns them.
**Files likely touched:**
- `src/profile.rs`: `ProfileCard.owner: Option<(String id, String name)>`.
- `src/ui/profile_card.rs`: "Owned by {name}" under the username, clickable when cached.
- `src/store.rs` / `src/ui/chat.rs`: the message header's author is a bot (`user.bot.is_some()`, not for webhooks or masquerades), and the tag after the name.
- `tests/profile.rs`, a store or chat-row test.

**Depends on:** None
**Risks:**
- **Message rows are cached and measured** (`TimelineRow`). Adding the tag must be part of the row's data, so a bot whose user loads later still gets it (rows re-sync on user updates).

**How to verify:**
- `cargo test --test profile`: a bot's card has its owner (cached and not); a person's has none.
- A row test: a bot author's header row says bot; a webhook's and a masquerade's don't.
- Then the full suite.

**Manual check:** a bot's message shows "Bot"; its card shows "Owned by …", which opens the owner's card.

**Shippable when:** bots are marked in messages and on their cards.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–28):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3. Check any other file's count with `git show HEAD:<file> | rustfmt --edition 2024 --check | grep -c '^Diff in'` before formatting it whole.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/bots.rs` (new): rows, the username check, limits, forms and their changes, the token reset, invite URLs, destinations, requested and grantable permissions (and that what's sent never exceeds what's grantable), prompts and error texts.
- Link recognition for `/bot/{id}`.
- `tests/profile.rs`: a bot's owner on its card.

### Integration Tests
wiremock tests (`tests/api_bots.rs`, new) against the exact routes, headers and bodies:
- `GET /bots/@me`, `POST /bots/create`, `PATCH /bots/{id}`, `DELETE /bots/{id}`;
- `PATCH /users/{id}`;
- `GET /bots/{id}/invite`, and `POST /bots/{id}/invite` with and without `?permissions=`.

Failure cases that become plain toasts: `InvalidUsername`, `UsernameTaken`, `ReachedMaximumBots`, `BotIsPrivate`, `NotFound`, `AlreadyInServer`, `CannotGiveMissingPermissions`.

### Manual Tests
Run with the web client and a bot client (any library, to check the token), on a test server you own, plus a group. Batch them at the end if the user asks:
1. **My Bots:** matches the web client; create, copy ID, delete.
2. **Settings and token:** public/private, interactions URL, requested permissions (the web client's Add Bot shows nothing, but RuSToat's does); reset the token, and the old one stops working; copy the new one; rename.
3. **Profile:** avatar, banner, display name, bio; they show on the bot's messages and card.
4. **Add:**
   - from the page to a server, with some requested permissions ticked; the managed role has exactly those;
   - as a moderator without Manage Roles, nothing is ticked and the bot joins with no role;
   - to a group;
   - from a pasted `/bot/{id}` link.
5. **Marks:** "Bot" on its messages, and "Owned by …" on its card.
6. **Themes:** the section, the bot page and the dialog read well in light and dark.

## Definition of Done
- [ ] Settings has a My Bots section per account: list (with a count against the account's limit), create (username checked as Stoat does), and delete (confirmed).
- [ ] A bot's page edits its username, public flag, interactions URL and requested permissions; copies its ID, token and invite URL; and resets its token (confirmed). Tokens are never shown or logged.
- [ ] A bot's profile is edited with the Profile editor, saved with `PATCH /users/{bot_id}`. Your own Profile page behaves exactly as before.
- [ ] The Add bot dialog adds a bot to a server (Manage Server) or a group (Invite Others), from the bot's page or a `/bot/{id}` link. Requested permissions are offered, and only bits Stoat will grant are ever sent.
- [ ] Bots' messages show "Bot", and their profile cards show their owner.
- [ ] Refusals read plainly.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan.
