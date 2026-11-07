# Implementation Record: Security (milestone 16)

_Plan: docs/plans/security.md_
_Started: 2026-11-06_

Run in continuous mode on branch `milestone-16-security` (based on `milestone-15-your-profile`, which isn't merged to `main` yet). At the user's request ("Batch all the changes to the end") nothing is committed per chunk: commits were hanging on the SSH commit signer, so all four chunks are committed together at the end, and every chunk's manual check is batched for then too. `docs/plans/security.md` was staged but uncommitted at the start. Pre-existing dirty files kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked).

Starting rustfmt drift (`Diff in` count): `src/ui/chat.rs` 17, `src/store.rs` 15, `src/api/client.rs` 3, `tests/api_upload.rs` 10; 0 in `src/ui/settings.rs`, `src/ui/app_root.rs`, `src/ui/login.rs`, `src/accounts.rs`, `src/assets.rs`, `src/ui/thumbhash.rs`, `src/profile_edit.rs`, `tests/accounts.rs`.

## Chunk 1: Sessions page: list and rename

### Changes Made
- `src/accounts.rs`: `AccountEntry.session_id: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, so old files load and unchanged entries write as before); `AccountList::set_session(id, session_id) -> bool`. Existing `AccountEntry` literals gain `session_id: None`. Unit test `a_login_remembers_its_session`.
- `src/ui/app_root.rs`: both `LoginEvent::LoggedIn` handlers pass `session.id` to `logged_in`, which saves it with `set_session` after `add_or_update` (the "already signed in" early return ends the new session before that, so its ID isn't saved). New `AppRoot::shown_session_id()`.
- `src/api/client.rs`: `fetch_sessions() -> Vec<SessionInfo>` (`GET /auth/session/all`) and `rename_session(id, name) -> SessionInfo` (`PATCH /auth/session/{id}`, `DataEditSession`).
- `src/security.rs` (new, pure): `Platform { Phone, Desktop }` and `platform_of(name)` (android/ios/ipad/iphone, but not "macOS"/"Mac OS", is a phone); `SessionRow { id, name, platform, created_ms, current }`; `session_rows(list, current)` (current first, then newest first by ULID time); `created_label` ("Created 3 days ago" through `time::relative_time`); `SESSION_NAME_MAX = 64` and `check_session_name` (trimmed; blank is `Clear`, which can't be saved). 5 unit tests.
- `src/ui/sessions_page.rs` (new): the `SessionsView` entity per account (weak chat, this device's session ID, the list's load state, the rename dialog's input). Header "Sessions of @name#1234" with a Refresh button; "This device" (when known) then "Other sessions" — or just "Sessions" when this device isn't known — each row a phone or monitor icon, the name, "Created …" and Rename. "Rename session" dialog: one field (filled with the name, focused), a `n/64` count, Cancel/Save; the row updates from Stoat's answer. Failures toast `error_text(err, "Couldn't rename the session.")`.
- `src/ui/settings.rs`: "Sessions" page after Profile (`MonitorSmartphone` icon, search keywords); `SettingsView.sessions_views` cached per account like `profile_editors`, made in `render` and replaced when the chat or session ID changes (`SessionsView::is_for`); a shared `signed_out_item` for the signed-out message.
- `src/profile_edit.rs`: `error_text` is now `pub(crate)` (reused for the rate-limit wording).
- `src/assets.rs`: bundles `KeyRound`, `LogOut`, `Monitor`, `MonitorSmartphone`, `RefreshCw`, `Smartphone` (for all four chunks), with a `security_icons_load` test.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- Tests: `tests/api_sessions.rs` (new: the list parses; rename sends exactly `{"friendly_name":"Work Mac"}`); `tests/accounts.rs` (an old file without `session_id` loads, and the ID round-trips).

### Divergences
- `add_or_update` keeps its signature; a separate `set_session` stores the session ID (the plan said `add_or_update` "takes and keeps it"). Same outcome, no churn for its other callers.
- The page's header says "Sessions of @name#1234" rather than the plan's "@name#1234" alone, and when this device isn't known the single list is headed "Sessions" rather than "Other sessions".

### Verification Performed
- `cargo test --lib security`: 5 passed.
- Full suite `cargo test`: 813 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/api/client.rs` 3 (unchanged; the new import list hand-formatted); 0 in `settings.rs`, `app_root.rs`, `accounts.rs`, `assets.rs`, `profile_edit.rs`, `tests/accounts.rs`, and the new files.

### Manual Validation
Pending (batched for the end):
- Sign in to an account again; Settings → Sessions marks "This device".
- Your other account (not signed in again) lists every session with no marker.
- Rename this device and another session; the web client's Sessions page shows the new names.
- Switch accounts with Settings open; the page follows.

## Chunk 2: Confirm it's you, and logging sessions out

### Changes Made
- `src/api/client.rs`: `mfa_status()` (`GET /auth/mfa/`), `create_mfa_ticket(&MFAResponse) -> MFATicket` (`PUT /auth/mfa/ticket`, never with a ticket header), `revoke_session(ticket, id)` (`DELETE /auth/session/{id}`) and `revoke_other_sessions(ticket)` (`DELETE /auth/session/all?revoke_self=false`), through a private `send_with_ticket` that adds `X-MFA-Ticket`. Import list hand-formatted.
- `src/security.rs`: `ConfirmMethod { Password, Totp, Recovery }` with its prompt, placeholder and switch label; `confirm_methods(&MultiFactorStatus)` (Password without TOTP; Totp, plus Recovery if `recovery_active`, with it); `mfa_response(method, typed)` (password as typed, TOTP without whitespace, recovery code trimmed and lower-cased; blank is `None`); `ConfirmError { Retry, Methods, Fail }` and `ticket_error(&ApiError)` (`InvalidCredentials` → "That password isn't right.", `InvalidToken` → "That code isn't right.", a rate limit → retry with the wait, `DisallowedMFAMethod` → ask Stoat again, anything else → fail); `action_error_text(err, fallback)` reads `Unauthorized` or `InvalidToken` after a ticket as "That confirmation expired. Try again." 6 more unit tests.
- `src/ui/confirm_dialog.rs` (new): `confirm_then(client, ConfirmRequest { title, detail, confirm_label, failed_text }, action, done, window, cx)` opens a `ConfirmDialog<T>` entity in a dialog. It fetches the status ("Checking how to confirm it's you…", or the failure with Retry), asks for the first method with one field (masked for a password; Enter confirms), offers the others as links, makes the ticket, runs the action with it on the IO runtime, then closes and calls `done`. A wrong answer or a rate limit keeps the dialog with the reason under the field; other failures close it with a toast. The ticket's token is never logged.
- `src/ui/sessions_page.rs`: Log out on every session except this device (which logs out from the Stoat menu); "Log out other sessions" in the header whenever there's more than one session, even when this device isn't known (Stoat spares the current one). Both go through `confirm_then`, then fetch the list again.
- Tests: `tests/api_mfa.rs` (new: the status with omitted fields; ticket bodies for a password, TOTP code and recovery code, and no `x-mfa-ticket` header on that call); `tests/api_sessions.rs` (revoking one and the others send `x-mfa-ticket`, the `revoke_self=false` query, accept 204; a 401 `{"type":"InvalidToken"}` is an `Api` error, and reads as an expired confirmation).

### Divergences
- No separate "Are you sure?" dialog before "Log out other sessions": the confirmation dialog itself is that step (title "Log out other sessions?", "Every other session will be signed out. You'll stay signed in here.", button "Log out others", plus Cancel). One dialog instead of two, with the same chance to back out.
- Log out isn't offered on the row marked "This device" (the Stoat menu's Log Out does that, and also forgets the account here). On accounts where this device isn't known, every row has it, as planned.
- The risk about a bad ticket signing the account out: checked. Only the events connection and session restore sign an account out; a REST `Unauthorized` never does. A JSON 401 `InvalidToken` maps to `ApiError::Api` (tested), and both read as an expired confirmation rather than "Your session has expired".

### Verification Performed
- `cargo test --lib security`: 11 passed.
- Full suite `cargo test`: 824 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/api/client.rs` 3 (unchanged); new files 0.

### Manual Validation
Pending (batched for the end):
- Sign in to the web client, then log that session out from RuSToat with your password; the web client is signed out.
- A wrong password shows "That password isn't right." and the dialog stays.
- "Log out other sessions" signs out the web client, and RuSToat stays signed in.

## Chunk 3: Account page: two-factor status and recovery codes

### Changes Made
- `src/api/client.rs`: `fetch_recovery_codes(ticket)` (`POST /auth/mfa/recovery`) and `generate_recovery_codes(ticket)` (`PATCH /auth/mfa/recovery`), both `Vec<String>`, through `send_with_ticket`.
- `src/security.rs`: `RecoveryAction { Generate, View, Regenerate }` and `recovery_actions(&status)` (Generate without codes; View and Regenerate with them); `recovery_codes_text` (one per line, for Copy all). 2 more unit tests.
- `src/ui/account_page.rs` (new): the `AccountSecurityView` entity per account (weak chat, the status's load state). Header "Signed in as @name#1234" with Refresh; an "Authenticator app" row (status only: "On: signing in asks for a code…" / "Off.") and a "Recovery codes" row with its buttons. Each recovery action goes through `confirm_then` with its own title, detail and button ("Generate recovery codes?", "View recovery codes?", "Regenerate recovery codes?" — "Your current recovery codes will stop working."), then opens "Your recovery codes": "Save these somewhere safe. Each works once.", the codes in a two-column Menlo grid, Copy all (toast "Copied your recovery codes."), Regenerate (closes, confirms again, reopens with the new set) and Done. Generating refreshes the status. Only the count of codes is logged.
- `src/ui/settings.rs`: "Account" page second, after Profile (`KeyRound` icon, group "Two-factor authentication", search keywords); `account_views` cached per account like the others.
- `src/ui/mod.rs`: the new module.
- Tests: `tests/api_mfa.rs` (`POST` and `PATCH /auth/mfa/recovery` send `x-mfa-ticket` and parse the codes).

### Divergences
- The header reads "Signed in as @name#1234" (the page's group already says "Two-factor authentication").
- With no codes left (all used), View shows "You have no recovery codes left. Regenerate them for a new set." — a case the plan didn't mention.

### Verification Performed
- `cargo test --lib security`: 13 passed.
- Full suite `cargo test`: 827 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/api/client.rs` 3 (unchanged); `src/ui/settings.rs` and new files 0.

### Manual Validation
Pending (batched for the end):
- With no codes, Generate asks for your password and shows 10 codes.
- Copy all pastes them one per line.
- Regenerate changes them, and the web client's "View Recovery Codes" shows the new ones.

## Chunk 4: Authenticator app on and off, with a QR code

### Changes Made
- `Cargo.toml` / `Cargo.lock`: `qrcodegen = "1.8"` (Nayuki's encoder, no dependencies), with a comment saying why.
- `src/api/client.rs`: `create_totp_secret(ticket) -> String` (`POST /auth/mfa/totp`, `ResponseTotpSecret`), `enable_totp(code)` (`PUT /auth/mfa/totp` with `{"totp_code"}`, no ticket, 204) and `disable_totp(ticket)` (`DELETE /auth/mfa/totp`, 204).
- `src/security.rs`: `otpauth_uri(username, secret)` (`otpauth://totp/Stoat:{username}?secret={secret}&issuer=Stoat`, percent-encoded); `QrModules`, `QR_QUIET_ZONE = 4`, `qr_modules(text)` (`QrCodeEcc::Medium`) and `qr_image(modules, scale) -> RgbaImage` (black on white, quiet zone included); `secret_groups` ("JBSW Y3DP EHPK 3PXP"); `check_totp_code` (six digits, spaces removed); `enable_totp_error_text` (`InvalidToken` → "That code isn't right. Check your device's clock and try the newest code."). 5 more unit tests.
- `src/ui/account_page.rs`: the Authenticator app row gets "Set up" (off) or "Remove" (on). Set up confirms with the password ("Set up an authenticator app?"), fetches the secret, and opens "Set up an authenticator app" — a `TotpSetup` entity: instructions, the QR code (`img(ImageSource::Render(..))`, 10 px per module in the image, drawn at 5 pt per module so it's 1:1 on Retina; black on white in both themes), "Can't scan it? Enter this key instead:" with the key in groups of four and a Copy button, a 6-digit code field (Enter submits), and Cancel / Turn on. A wrong code keeps the dialog with the reason under the field. Success closes it, refreshes the status, and toasts "The authenticator app is on." (plus "Generate recovery codes too, in case you lose your device." when there are none). Remove confirms with an authenticator or recovery code ("Remove the authenticator app?" — "Signing in will only ask for your password. Your recovery codes stay."), then refreshes and toasts.
- Tests: `tests/api_mfa.rs` (the secret with `x-mfa-ticket`; enabling sends exactly `{"totp_code":"123456"}` and no ticket; a wrong first code is an `Api` `InvalidToken`; removing sends `x-mfa-ticket`).

### Divergences
- The setup button is labelled "Set up" (the plan said "Enable"), and the setup dialog's confirm button "Turn on".
- The QR code is built directly as a `RenderImage` (`ImageSource::Render`) rather than through a shared helper with `thumbhash.rs`: black and white are the same in RGBA and BGRA, so no channel swap is needed, and there's nothing to cache.
- Whether it scans and stays crisp couldn't be checked automatically (no QR decoder here); that's in the manual checks.

### Verification Performed
- `cargo test --lib security`: 23 passed.
- Full suite `cargo test`: 836 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean (after replacing an `and_then(|x| Some(..))` with `map`).
- rustfmt drift: `src/ui/chat.rs` 17, `src/store.rs` 15, `src/api/client.rs` 3 (all unchanged); 0 in every other touched file.

### Manual Validation
Pending (batched for the end):
- Set up: scan the QR code with an authenticator app (1Password, Google Authenticator or similar); it shows "Stoat (username)".
- Enter its code; the Account page says it's on. A new login (Add Account… with this account, or the web client) asks for a code.
- Remove it with an authenticator code, then (after setting it up again) with a recovery code; the status goes off. The recovery code used is gone from View.
- In dark mode, the QR code is black on white and scans.

## Summary

_Completed: 2026-11-06 (code); manual checks pending_

### Chunks Completed
1. Sessions page: list and rename (session ID saved at login from now on)
2. Confirm it's you, and logging sessions out (one or all others)
3. Account page: two-factor status and recovery codes
4. Authenticator app on and off, with a QR code

All four are committed together at the end, at the user's request (the SSH commit signer was hanging on per-chunk commits).

### Divergences
- Chunk 1: `AccountList::set_session` instead of changing `add_or_update`; slightly different headings ("Sessions of @…", "Sessions" when this device isn't known).
- Chunk 2: the confirmation dialog doubles as the "Are you sure?" step for "Log out other sessions"; Log out isn't offered on the row marked "This device".
- Chunk 3: header "Signed in as @…"; a message when no recovery codes are left.
- Chunk 4: "Set up" / "Turn on" labels; the QR image is built directly as a `RenderImage`.

### Definition of Done
- Settings → Sessions lists sessions (newest first, platform icon, "Created …"), marks this device when known (saved at every login from now on), renames, logs out one, and logs out every other session after a confirmation — implemented and API-tested; manual checks pending.
- Every protected action confirms with the password, or an authenticator or recovery code with TOTP on, using a fresh `X-MFA-Ticket`; wrong answers keep the dialog open — implemented and tested.
- Settings → Account shows the status; generates, views, copies and regenerates recovery codes; turns the authenticator on (QR of `otpauth://totp/Stoat:{username}?secret=…&issuer=Stoat`, plus the key) and off — implemented and tested; scanning is a manual check.
- Rate limits say how long to wait; a bad ticket never signs the account out — met (only the events connection signs accounts out; refused tickets read as an expired confirmation).
- Codes and tickets are never logged — met (only counts are logged).
- `cargo test` and `cargo clippy --all-targets -- -D warnings` pass; no touched file's drift grew — met.
- Every chunk's manual check recorded here — pending the walkthrough.

### Remaining Manual Steps
Every chunk's manual check above, plus the plan's Manual Tests 2–6 (two accounts never cross; logging out an unmarked session that is this one shows "Your session has ended" without breaking Settings; rapid wrong passwords say how long to wait; light and dark themes; turn 2FA off on the test account afterwards).
