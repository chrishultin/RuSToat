# Plan: Security (RuSToat milestone 16)

## Summary
Milestone 16 of the [roadmap](roadmap.md) lets you look after your account's security from RuSToat:
- **Sessions:** list where you're signed in, rename a session, log out one, or log out every other session.
- **Two-factor:** turn an authenticator app (TOTP) on or off, with a QR code, and view or regenerate recovery codes.

The references are Stoat's server (`stoatchat/stoatchat` v0.16.2: `crates/delta/src/routes/{session,mfa}/*.rs`, `crates/core/database/src/models/{mfa_tickets,accounts}`), the live spec (`https://api.stoat.chat/openapi.json`), and the web client (`stoatchat/for-web` `stoat-for-web-v0.16.2`: `settings/user/Sessions.tsx`, `settings/user/Account.tsx`, `modal/modals/{MFAFlow,MFAEnableTOTP,MFARecovery,RenameSession,SignOutSessions}.tsx`). All were read on 2026-11-06.

## Approach
**What the API does** (from the server source and live spec; Stoat's own `delta` handlers, not the upstream `authifier` crate, which differs):
- **Sessions:**
  - `GET /auth/session/all` returns `[SessionInfo {_id, name}]`, including the current session. There's no "current" flag, and no `last_seen` or `origin`. The creation time is in the `_id` ULID.
  - `PATCH /auth/session/{id}` with `{"friendly_name"}` renames a session and returns `SessionInfo`. It needs no ticket.
  - `DELETE /auth/session/{id}` revokes one session, and `DELETE /auth/session/all?revoke_self=false` revokes every other session. Both return 204, and both **need an MFA ticket**.
- **MFA tickets:**
  - `PUT /auth/mfa/ticket` takes one `MFAResponse` and returns an `MFATicket`; its `token` goes in an `X-MFA-Ticket` header. The response is `{"password"}` without TOTP, or `{"totp_code"}` / `{"recovery_code"}` with TOTP on (`Password` isn't allowed then). Don't send `X-MFA-Ticket` on this call.
  - A ticket is **single-use**: the guard deletes it before the handler runs, even if the handler then fails. It lasts 5 minutes.
  - Using a recovery code for a ticket burns that code.
  - Errors:
    - a wrong password: `InvalidCredentials`
    - a wrong code: `InvalidToken`
    - a method that isn't allowed: `DisallowedMFAMethod`
- **MFA status:** `GET /auth/mfa/` returns `MultiFactorStatus`. False fields are left out of the JSON, so default them to false. Only `totp_mfa` and `recovery_active` matter.
- **Recovery codes** (ticket, returns `[String]`):
  - `POST /auth/mfa/recovery` fetches the existing codes.
  - `PATCH /auth/mfa/recovery` replaces them with 10 new `xxxxx-xxxxx` codes.
- **TOTP:**
  - `POST /auth/mfa/totp` (ticket) returns `{secret}`, 16 base32 characters, saved on Stoat's side as pending.
  - `PUT /auth/mfa/totp` with `{"totp_code"}` enables TOTP. It needs **no ticket**.
    - Codes use SHA-1, 6 digits and a 30 s step.
    - Stoat accepts only the current step, so a fast clock or a slow typist fails. The error should say so.
  - `DELETE /auth/mfa/totp` (ticket) disables TOTP, and keeps the recovery codes.
  - Stoat doesn't require recovery codes before enabling TOTP.
- **Rate limits:** every non-DELETE call under `/auth` shares one bucket per session: 15 requests per 10 s. A 429 carries `retry_after` in ms, which `ApiError::RateLimited` already reads. A protected action costs three calls (status, ticket, action).

**What exists already.**
- **Models:** `revolt-models` 0.16.2 has `SessionInfo`, `DataEditSession`, `MFATicket`, `MFAResponse`, `MFAMethod`, `MultiFactorStatus` and `ResponseTotpSecret`.
- **Client:** `StoatClient` has `send` / `send_raw` / `send_raw_with(…, headers)` (`src/api/client.rs` ~786–830). Use `send_raw_with` for `X-MFA-Ticket`. `logout` already handles a 204.
- **Login:**
  - `LoginView` (`src/ui/login.rs`) already asks for a TOTP code, or a recovery code with a toggle. Reuse its wording and code clean-up (spaces stripped from TOTP codes).
  - Login returns the full `Session`, which `AppRoot::logged_in` (`src/ui/app_root.rs` ~522) receives as a token and user ID only. **The session's `_id` isn't saved anywhere.**
  - `AccountEntry` (`src/accounts.rs`) is saved as JSON with `#[serde(default)]` fields.
- **Settings window:**
  - Pages come from `SettingsView::app_pages()`.
  - Milestone 15 added the account-on-screen pattern: `AppRoot::shown_chat()`, and a per-account entity cached in `SettingsView` (`profile_editors`), created in `render` because inputs need the window.
  - `ProfileEditor::is_for` replaces a stale one.
- **Other pieces to reuse:**
  - Dialogs: `window.open_dialog` (the status editor in `chat.rs`, and `identity_dialog.rs`).
  - Toasts: `window.push_notification`.
  - Time: `crate::time::ulid_local_time` and `relative_time` ("3 days ago").
  - Images from pixels: `src/ui/thumbhash.rs`, which turns an `RgbaImage` into an `Arc<RenderImage>` and draws it with `img`.
  - Error wording: `profile_edit::save_error_text`, which says how long to wait after a rate limit. Its private `error_text(err, fallback)` can be made `pub(crate)` and reused.
- **A revoked current session already signs the account out.** The connection gets `Unauthorized`, and `AppRoot` shows "Your session has ended. Please log in again." (`app_root.rs` ~518).

**Two Settings pages, following the account on screen** (user decision: separate pages, as the web client splits them):
- **Account:**
  - It goes second, after Profile.
  - It has a "Two-factor authentication" group: the status, the Authenticator app row (Enable or Remove) and the Recovery codes row (Generate, or View and Regenerate).
  - Milestone 17 adds username, email and password to this page.
- **Sessions:** it goes third. It has "This device" (when known: its name, and Rename), then the other sessions, newest first. Each shows a name, a platform icon and "Created 3 days ago", with Rename and Log out. "Log out other sessions" sits above them, with a confirmation.
- Each page is an entity per account, cached in `SettingsView` like `profile_editors`. It says which account it's for ("@name#1234"). Without a signed-in account on screen it says "Sign in to manage security."

**Knowing which session is this device** (user decision: save it from now on, no note):
- `AccountEntry` gains `session_id: Option<String>`, saved at every login, from `ResponseLogin::Success`'s `Session.id`.
- Accounts restored from before this milestone have none. Their Sessions page lists every session with no "This device" marker.
- "Log out other sessions" still spares the current one, since Stoat excludes it with `revoke_self=false`.
- Logging out a single session that turns out to be this one simply ends this account's session, through the existing "Your session has ended" path.

**One "Confirm it's you" dialog for every protected action:**
- **Which actions:** log out a session, log out the others, view or regenerate recovery codes, enable or remove the authenticator.
- **What it does:**
  - It fetches the status, then asks for your **password** (TOTP off), or your **authenticator code** with a "Use a recovery code" toggle (TOTP on, recovery codes active).
  - It creates the ticket, then runs the action with it.
- **One ticket per action, fetched just before it's used.** Tickets are single-use and short-lived.
- **Errors:**
  - a wrong password or code keeps the dialog open with the reason under the field
  - anything else closes it with a toast
- **Rejected alternative: caching a ticket for several actions.** Stoat burns it on first use.

**The QR code is drawn locally** (user decision: add `qrcodegen`):
- **The code:** `qrcodegen` (Nayuki's, no dependencies) encodes the `otpauth://` URI at medium error correction, as the web client does.
- **The image:** RuSToat turns the module grid into an `RgbaImage`, black on white with a 4-module quiet zone, scaled up with whole pixels per module. It goes through the same `RenderImage` path as `thumbhash.rs`. It stays black on white in dark mode, so phone cameras can read it.
- **The URI:** `otpauth://totp/Stoat:{username}?secret={secret}&issuer=Stoat`, matching the web client. Percent-encode the label: usernames may hold non-ASCII letters, and this is identical for ASCII ones.
- **Typing it in:** the secret is shown beside the QR code, with Copy.
- **Rejected alternative: no QR code (secret only).** Scanning is what most people expect.

## Assumptions
1. **What "1 – B" covers.** It means building both pages now: Sessions, and an Account page holding Two-factor, which milestone 17 extends. Two-factor isn't deferred to milestone 17.
2. **Platform icons** come from the session name, as the web client's regexes do:
   - `/android|i(pad)?os|iphone/i` → a phone
   - anything else → a desktop
   - Check `IconName` and bundle any missing icon in `src/assets.rs`, as in milestones 14–15. Use Lucide's `smartphone` and `monitor`.
3. **RuSToat's session name stays "RuSToat".** Renaming is up to the user. Changing the name sent at login is out of scope.
4. **Session names:** Stoat doesn't validate them. RuSToat trims the name, refuses a blank one, and caps it at 64 characters (an assumption, so a slip doesn't store a huge name).
5. **After enabling TOTP without recovery codes,** the success toast suggests generating them ("Generate recovery codes too, in case you lose your device."). Nothing forces it, as in the web client.
6. **Recovery codes** show in a dialog: a two-column monospace grid, "Save these somewhere safe. Each works once.", and Copy all, Regenerate and Done. Regenerating asks to confirm it's you again.
7. **Removing the authenticator** needs no extra confirmation beyond the ticket, as in the web client.
8. **Status and lists refresh** when the page is first shown, after each change, and with a Refresh button. There's no live event for them.
9. **Error texts:**
   - `InvalidCredentials`: "That password isn't right."
   - `InvalidToken` on a ticket: "That code isn't right."
   - `InvalidToken` on enabling TOTP: "That code isn't right. Check your device's clock and try the newest code."
   - `DisallowedMFAMethod`: refresh the status and ask again.
   - A rate limit says how long to wait.
   - Anything else: "Couldn't do that. Try again."

## Plan

### Chunk 1: Sessions page: list and rename
**Goal:** Settings → Sessions lists the account's sessions, marks this device when RuSToat knows it, and renames any of them.
**Files likely touched:**
- `src/api/client.rs`:
  - `fetch_sessions() -> Vec<SessionInfo>` (`GET /auth/session/all`)
  - `rename_session(id, name) -> SessionInfo` (`PATCH /auth/session/{id}`)
- `src/accounts.rs`: `AccountEntry.session_id: Option<String>` (`#[serde(default)]`); `add_or_update` takes and keeps it.
- `src/ui/login.rs` / `src/ui/app_root.rs`:
  - pass `Session.id` through `LoginEvent::LoggedIn` into `logged_in`, and save it
  - `AppRoot::shown_session()` gives `Option<(account id, Entity<ChatView>, Option<session id>)>` (or extend `shown_chat`)
- `src/security.rs` (new, pure, no GPUI):
  - `SessionRow { id, name, created: Option<DateTime>, platform: Platform, current: bool }`
  - `session_rows(list, current_id)`: the current session first, then newest first by ULID
  - `platform_of(name)`
  - `check_session_name(text) -> FieldCheck`-like (trimmed, 1–64)
- `src/ui/sessions_page.rs` (new):
  - The `SessionsView` entity, per account: the rows, and its load and rename state.
  - "This device" group, "Other sessions" group, Rename dialog (`open_dialog` with one field) and Refresh.
- `src/ui/settings.rs`: `sessions_page` in `app_pages()` (after Profile; Account is inserted between them in chunk 3); `SettingsView.sessions_views` cached like `profile_editors`.
- `src/assets.rs`: `Monitor`, `Smartphone` (if not already bundled), plus the icon test.

**Depends on:** None.
**Risks:**
- **Threading the session ID.** `LoginView` already emits `LoggedIn { session: Box<Session>, instance }`, but `AppRoot` passes only the token and user ID on to `logged_in`. Keep the change narrow: one more argument, saved with the entry.
  - Re-login of an existing account must update `session_id`.
  - The "already signed in" path ends the new session, so it must not save its ID.
- **The page and its account.** Same pattern and risk as `ProfileEditor`: create in `SettingsView::render`, keyed by account ID, replaced when the chat entity changes. A re-login also changes the session ID, so the cached view must notice that (compare it in `is_for`).
- **Sessions from before this milestone:** no marker. That's expected (Approach); don't treat it as an error.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/security.rs` unit tests:
  - `session_rows` puts the current session first, then newest first, and with no current ID marks none
  - `platform_of` for "Stoat for Web (Chrome on Android)", "… on iOS", "… on Mac OS", "RuSToat" and "Unknown"
  - `check_session_name`: blank, spaces, 64 and 65 characters
- `tests/api_sessions.rs` (new, wiremock):
  - `GET /auth/session/all` parses
  - `PATCH /auth/session/s1` sends exactly `{"friendly_name":"Work Mac"}` and parses the answer
- `tests/accounts.rs`: an old accounts file without `session_id` still loads; `add_or_update` stores and replaces it

**Manual check:**
- Sign in to an account again; Settings → Sessions marks "This device".
- Your other account (not signed in again) lists every session with no marker.
- Rename this device and another session; the web client's Sessions page shows the new names.
- Switch accounts with Settings open; the page follows.

**Shippable when:** the Sessions page lists and renames sessions for the account on screen, new logins save their session ID, and the tests pass.

### Chunk 2: Confirm it's you, and logging sessions out
**Goal:** Log out a single session, or every other session, after confirming with a password or code.
**Files likely touched:**
- `src/api/client.rs`:
  - `mfa_status() -> MultiFactorStatus` (`GET /auth/mfa/`)
  - `create_mfa_ticket(MFAResponse) -> MFATicket` (`PUT /auth/mfa/ticket`, never with `X-MFA-Ticket`)
  - `revoke_session(ticket, id)` (`DELETE /auth/session/{id}`)
  - `revoke_other_sessions(ticket)` (`DELETE /auth/session/all?revoke_self=false`)
  - The last two use `send_raw_with(…, &[("X-MFA-Ticket", ticket)])` and accept 204.
- `src/security.rs`:
  - `ConfirmMethod { Password, Totp, Recovery }`
  - `confirm_methods(&MultiFactorStatus) -> Vec<ConfirmMethod>`: Password without TOTP; Totp, plus Recovery if `recovery_active`, with TOTP on
  - `mfa_response(method, typed) -> Option<MFAResponse>`: spaces stripped from TOTP codes, recovery codes trimmed and lower-cased, blank is `None`
  - `security_error_text(&ApiError, Step)`, reusing `profile_edit`'s rate-limit wording (make its `error_text` `pub(crate)`)
- `src/ui/confirm_dialog.rs` (new):
  - `confirm_then(window, cx, chat/client, title, action: impl FnOnce(ticket) -> Task<Result<…>>)`: fetch the status, open "Confirm it's you" with the right field (password, or code with the recovery toggle), create the ticket, run `action`, then close or show the error.
  - Reusable by chunks 3 and 4.
- `src/ui/sessions_page.rs`:
  - Log out on each other session.
  - "Log out other sessions" (only when there are others) behind a confirmation: "Log out of every other session? You'll stay signed in here." It then runs `confirm_then`.
  - The list refreshes afterwards.

**Depends on:** Chunk 1.
**Risks:**
- **Single-use tickets.** A failed action has still burned its ticket. Never retry an action with the same ticket; a retry runs the whole confirmation again.
- **Logging out this device when it isn't marked.** It ends this account's session. Check the existing "session ended" path handles it cleanly with Settings open: the Sessions page must cope with its account going away (show "Sign in to manage security.").
- **Rate limits.** Status, ticket and action are three `/auth` calls (two in the shared 15/10 s bucket). Repeated failed passwords could hit it; the error must say how long to wait.
- **`MissingHeaders` / `InvalidToken` status codes vary (400/401/403).** `ApiError::from_response` maps a JSON `InvalidToken` to `Api`. But an HTML 401 from a guard maps to `Unauthorized`, which signs the account out. Make sure a bad ticket isn't treated as a dead session: check what `send_raw_with` does with a 401 here, and test it.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/security.rs` unit tests:
  - `confirm_methods` for the three status shapes, including `{}` (all fields omitted)
  - `mfa_response`: TOTP "123 456" becomes "123456", a recovery code " ABCDE-12345 " becomes "abcde-12345", blank is `None`
  - `security_error_text` for `InvalidCredentials`, `InvalidToken`, `RateLimited { Some(2500) }` ("3 s") and others
- `tests/api_mfa.rs` (new, wiremock):
  - `GET /auth/mfa/` with `{}` and with `{"totp_mfa":true,"recovery_active":true}`
  - `PUT /auth/mfa/ticket` with exactly `{"password":"pw"}` and no `x-mfa-ticket` header
- `tests/api_sessions.rs`:
  - `DELETE /auth/session/s2` and `DELETE /auth/session/all?revoke_self=false` send `x-mfa-ticket: tok-ticket` and accept 204
  - a 401 `{"type":"InvalidToken"}` is `ApiError::Api`, not `Unauthorized`

**Manual check:**
- Sign in to the web client, then log that session out from RuSToat with your password; the web client is signed out.
- A wrong password shows "That password isn't right." and the dialog stays.
- "Log out other sessions" signs out the web client, and RuSToat stays signed in.

**Shippable when:** single and bulk logout work behind the confirmation, errors keep the dialog where they should, and the tests pass.

### Chunk 3: Account page: two-factor status and recovery codes
**Goal:** Settings → Account shows whether two-factor is on, and generates, views and regenerates recovery codes.
**Files likely touched:**
- `src/api/client.rs`:
  - `fetch_recovery_codes(ticket)` (`POST /auth/mfa/recovery`)
  - `generate_recovery_codes(ticket)` (`PATCH /auth/mfa/recovery`)
  - Both return `Vec<String>`.
- `src/ui/account_page.rs` (new):
  - The `AccountSecurityView` entity, per account: the status (loading, failed with Retry, or loaded).
  - A "Two-factor authentication" group with an Authenticator app row (status only in this chunk) and a Recovery codes row: "Generate recovery codes", or "View" and "Regenerate".
  - The recovery codes dialog (two-column monospace grid, Copy all, Regenerate, Done), opened after `confirm_then`.
- `src/ui/settings.rs`: `account_page` second in `app_pages()`; `account_views` cached like the others.

**Depends on:** Chunk 2 (`confirm_then`).
**Risks:**
- **Regenerating from inside the codes dialog** needs a fresh ticket. Close the codes dialog, confirm, then reopen it with the new codes, or swap its content in place. Don't stack two dialogs if gpui-component's dialog layer can't.
- **Showing codes in a toast or log would leak them.** Never log the codes or tickets; `log::debug!` the counts only.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `tests/api_mfa.rs`: `POST` and `PATCH /auth/mfa/recovery` send `x-mfa-ticket` and parse `["abcde-fghjk", …]`
- `src/security.rs`:
  - `recovery_codes_text(codes)` (for Copy all) gives one code per line
  - the Recovery row's buttons follow `recovery_active`

**Manual check:**
- With no codes, Generate asks for your password and shows 10 codes.
- Copy all pastes them one per line.
- Regenerate changes them, and the web client's "View Recovery Codes" shows the new ones.

**Shippable when:** recovery codes can be generated, viewed, copied and regenerated from the Account page, and the tests pass.

### Chunk 4: Authenticator app on and off, with a QR code
**Goal:** Turn TOTP on (QR code, secret, first code) and off from the Account page.
**Files likely touched:**
- `Cargo.toml`: `qrcodegen = "1"`, with a comment saying why.
- `src/api/client.rs`:
  - `create_totp_secret(ticket) -> String` (`POST /auth/mfa/totp`)
  - `enable_totp(code)` (`PUT /auth/mfa/totp`, `{"totp_code"}`, no ticket, 204)
  - `disable_totp(ticket)` (`DELETE /auth/mfa/totp`, 204)
- `src/security.rs`:
  - `otpauth_uri(username, secret)`
  - `qr_modules(text) -> (size, Vec<bool>)` with `qrcodegen` at `QrCodeEcc::Medium`
  - `qr_image(modules, scale) -> RgbaImage` (quiet zone 4)
  - `check_totp_code(text)`: six digits after stripping spaces
- `src/ui/account_page.rs`:
  - Enable: run `confirm_then` (password), then fetch the secret and open "Set up an authenticator app". The dialog shows the QR code (an `img` of the `RenderImage`, about 200 px, crisp: whole-pixel scale), the secret in monospace with Copy, a "6-digit code" field, and Cancel / Turn on.
  - Turn on calls `enable_totp`, then refreshes the status. The toast adds the recovery-codes hint when `!recovery_active`.
  - Remove: run `confirm_then` (authenticator or recovery code), then `disable_totp` and refresh.
- `src/ui/thumbhash.rs` or a small shared helper: building a `RenderImage` from an `RgbaImage`, if it isn't already reusable.

**Depends on:** Chunk 3 (the Account page) and chunk 2 (`confirm_then`).
**Risks:**
- **Clock and timing.** Stoat accepts only the current 30 s step. The error text must point at the clock and the newest code. The plan doesn't add tolerance; it can't, as the server decides.
- **Image drawing.** GPUI may blur a scaled image. Draw the `RenderImage` at its own pixel size (`scale` chosen so the image is about 200 px) rather than letting `img` stretch it. If it still blurs, draw the modules as a grid of `div`s instead (a 33×33 grid is about 1 100 small divs, acceptable for a dialog).
- **Colour order.** `RenderImage` expects BGRA in some GPUI versions. Follow whatever `thumbhash.rs` does, since that's known to draw correctly; black and white are the same either way, but keep it consistent.
- **Leaving setup half-done.** Cancelling after the secret is fetched leaves a pending secret on Stoat's side. That's harmless: TOTP stays off, and the next Enable replaces it.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/security.rs` unit tests:
  - `otpauth_uri("ada", "JBSWY3DPEHPK3PXP")` is exactly `otpauth://totp/Stoat:ada?secret=JBSWY3DPEHPK3PXP&issuer=Stoat`; a non-ASCII username is percent-encoded
  - `qr_modules` gives a square of odd size ≥ 21; `qr_image` is `(size + 8) × scale` pixels, white at the corners and black in the finder pattern's corner module
  - `check_totp_code`: "123456" and "123 456" pass; "12345", "1234567" and "abcdef" fail
- `tests/api_mfa.rs`:
  - `POST /auth/mfa/totp` with `x-mfa-ticket` returns the secret
  - `PUT /auth/mfa/totp` sends exactly `{"totp_code":"123456"}` without `x-mfa-ticket`
  - `DELETE /auth/mfa/totp` sends `x-mfa-ticket`

**Manual check:**
- Enable: scan the QR code with an authenticator app (1Password, Google Authenticator or similar). It shows "Stoat (username)".
- Enter its code; the Account page says it's on. A new login (Add Account… with this account, or the web client) asks for a code.
- Remove it with an authenticator code, then with a recovery code (once codes exist); the status goes off. The recovery code used is gone from View.
- In dark mode, the QR code is black on white and scans.

**Shippable when:** TOTP can be turned on with the QR code or the secret, and off with a code, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`. Never run it on a `mod.rs` or `lib.rs`, since it formats their child modules too.
- A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines (from `rustfmt --edition 2024 --check <file>`) hasn't grown. Record each touched file's starting count before chunk 1. At the end of milestone 15 they were `src/ui/chat.rs` 17, `src/store.rs` 15, `src/api/client.rs` 3, `tests/api_upload.rs` 10, and 0 in `src/ui/settings.rs`, `src/ui/app_root.rs`, `src/ui/login.rs`, `src/accounts.rs`, `src/assets.rs`, `src/ui/thumbhash.rs` and `tests/accounts.rs`.

### Unit Tests
- **`src/security.rs`:**
  - session rows (order, current, platform, creation time)
  - session name checks
  - confirm methods from the status (including omitted fields)
  - MFA response clean-up
  - error texts
  - recovery codes text
  - `otpauth_uri`
  - QR modules and image geometry
  - TOTP code check

### Integration Tests
- **`tests/api_sessions.rs`** (new): list, rename (exact body), revoke one and revoke others (ticket header, query, 204), and a bad ticket's 401 staying an `Api` error.
- **`tests/api_mfa.rs`** (new):
  - status (with omitted fields)
  - ticket (exact bodies, no ticket header)
  - recovery fetch and regenerate
  - TOTP secret, enable (no ticket) and disable
- **`tests/accounts.rs`:** `session_id` saved, replaced on re-login, and missing from old files.

### Manual Tests
You need an `api.stoat.chat` account you can safely turn 2FA on for, an authenticator app, the web client signed in to the same account, and a second RuSToat account.
1. Run every chunk's manual check.
2. With two accounts, both pages always show the account on screen, and acting on one never touches the other.
3. Log out this device from the Sessions page on an account without the marker: the account shows "Your session has ended", the other account is unaffected, and Settings doesn't break.
4. Hit the rate limit (several wrong passwords quickly): the message says how long to wait.
5. Light and dark themes: lists, dialogs, the QR code and the recovery code grid are readable.
6. Before finishing, turn 2FA off again on the test account (or keep the recovery codes safe).

## Definition of Done
- [ ] Settings → Sessions lists the account on screen's sessions (newest first, with a platform icon and "Created …"), marks this device when its session ID is known (saved at every login from now on), renames any session, logs out one, and logs out every other session after a confirmation.
- [ ] Every protected action asks to confirm it's you: password without TOTP; authenticator or recovery code with it. It uses a fresh `X-MFA-Ticket` each time, and wrong passwords or codes say so without closing the dialog.
- [ ] Settings → Account shows the two-factor status, generates, views, copies and regenerates recovery codes, and turns the authenticator app on (QR code of `otpauth://totp/Stoat:{username}?secret=…&issuer=Stoat`, plus the secret) and off.
- [ ] Rate limits say how long to wait; a bad ticket never signs the account out.
- [ ] Codes and tickets are never logged.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/security-implementation.md`.
