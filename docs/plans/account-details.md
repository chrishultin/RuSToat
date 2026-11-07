# Plan: Account details (RuSToat milestone 17)

## Summary
Milestone 17 of the [roadmap](roadmap.md) lets you change your account's login details on the Account page that milestone 16 added:
- **Username:** change it, confirmed with your current password. Stoat may give you a new `#tag`.
- **Email:** change it, confirmed with your current password (plus an authenticator or recovery code when two-factor is on). Stoat emails the new address a link, and the change happens when it's opened.
- **Password:** change it, confirmed the same way.

The references are Stoat's server (`stoatchat/stoatchat` v0.16.2: `crates/delta/src/routes/account/{change_email,change_password,fetch_account}.rs`, `crates/delta/src/routes/users/change_username.rs`; `revolt-database` 0.16.2: `models/users/model.rs` `update_username`/`validate_username`/`find_discriminator`, `models/accounts/model.rs` `start_email_move`, `util/password.rs` `assert_safe`, `util/email.rs` `validate_email`, `models/mfa_tickets/rocket.rs`), and the web client (`stoatchat/for-web` `stoat-for-web-v0.16.2`: `settings/user/Account.tsx` `EditAccount`, `modal/modals/{EditUsername,EditEmail,EditPassword}.tsx`). All were read on 2026-11-07.

## Approach
**What the API does** (from the server source):
- **Username:** `PATCH /users/@me/username` with `{"username", "password"}` returns the updated `User`.
  - It needs **no MFA ticket**, even with two-factor on.
  - The body is validated first: username 2–32 characters matching `^(\p{L}|[\d_.-])+$`, and password 8–1024 characters. A failure is `FailedValidation` (400). A current password under 8 characters therefore fails validation rather than `InvalidCredentials`.
  - A wrong password: `InvalidCredentials` (401, JSON).
  - The name is then "cured" of homoglyphs (`decancer`, keeping capitals) and padded with `_` to the minimum length, so **what Stoat stores can differ from what was typed**.
  - The cured name is checked again: `admin`, `revolt` and `stoat` (any case) and some blocked patterns are `InvalidUsername` (400).
  - **The `#tag`:** a change of case only keeps it. Otherwise Stoat keeps it if it's free under the new name, or picks a random free one; that pick is allowed **once per 24 h** (`DiscriminatorChangeRatelimited`, sent as **429** with a JSON body). No free tag at all: `UsernameTaken` (409).
  - Other clients see it through a `UserUpdate` event, which `Store::apply_user_update` already merges.
- **Email:** `PATCH /auth/account/change/email` with `{"email", "current_password"}` returns 204.
  - The order of checks: `validate_email` (`IncorrectData { with: "email" }`, or `Blacklisted` for a disposable domain); then **with TOTP on and no valid ticket, `InvalidCredentials`**; then the password (`InvalidCredentials`).
  - With SMTP set up (as on `api.stoat.chat`), it emails a link to the new address and the account's email **only changes once the link is opened**. There's no API to see a pending change. Without SMTP (some self-hosted instances), it changes at once.
- **Password:** `PATCH /auth/account/change/password` with `{"password", "current_password"}` returns 204.
  - The order of checks: with TOTP on and no valid ticket, `InvalidCredentials`; then `assert_safe` (under 8 **bytes**: `ShortPassword`; in the compromised list: `CompromisedPassword`); then the current password (`InvalidCredentials`).
  - It **doesn't end other sessions**.
- **The ticket for email and password** (`Option<ValidatedTicket>`, sent as `X-MFA-Ticket`):
  - It's only required when TOTP is on (`account.mfa.is_active()`). Without TOTP, RuSToat sends none.
  - With TOTP on, `PUT /auth/mfa/ticket` doesn't take a password (`DisallowedMFAMethod`), so it needs an authenticator or recovery code.
  - The guard **claims the ticket before the handler runs**, so a wrong current password, a too-short or compromised new password, or a bad email all use it up, and **a recovery code used for it is gone for good**.
  - The guard is optional, so a missing, expired or invalid ticket reads as `InvalidCredentials`, the same error as a wrong password. RuSToat makes the ticket just before the change, so in practice that error means the password.
  - The web client's password dialog sends no ticket, so it fails for accounts with TOTP on. RuSToat does it properly.
- **Current email:** `GET /auth/account/` returns `AccountInfo {_id, email}`.
- **Rate limits:** everything under `/auth` shares the 15-per-10-s bucket from milestone 16. With TOTP on, a change costs three calls (status, ticket, change). `/users/@me/username` is in the users bucket.

**What exists already.**
- **Models:** `revolt-models` 0.16.2 has `DataChangeEmail`, `DataChangePassword` and `AccountInfo`. It has **no `DataChangeUsername`** (that's defined in `delta`), so RuSToat adds a small private `Serialize` struct in `client.rs`.
- **Client** (`src/api/client.rs`):
  - `send`, `send_raw` and a private `send_with_ticket(method, path, ticket, body)` (~870).
  - Milestone 16's `mfa_status()` and `create_mfa_ticket(&MFAResponse)`.
- **Errors** (`src/api/error.rs`): `ApiError::from_response` turns every 429 into `RateLimited`, except `InSlowmode`. **`DiscriminatorChangeRatelimited` needs the same exception**, or it reads as "Stoat is limiting changes. Try again in a moment.", which is wrong (it's a day).
- **Confirming** (`src/security.rs`):
  - `ConfirmMethod { Password, Totp, Recovery }` with `prompt` / `placeholder` / `switch_label`
  - `confirm_methods(&status)`
  - `mfa_response(method, typed)`
  - `ticket_error(&ApiError) -> ConfirmError { Retry, Methods, Fail }`
  - `action_error_text`
  - `profile_edit::error_text(err, fallback)` (`pub(crate)`) for rate limits and `FailedValidation`.
- **The Account page** (`src/ui/account_page.rs`):
  - `AccountSecurityView`, one per account, cached in `SettingsView.account_views`, `is_for(&chat)`.
  - The `row(title, detail, actions, cx)` helper.
  - `settings.rs::account_page` has one "Two-factor authentication" group.
- **Your own user after a change:** `ChatView::own_profile_saved(me, None, false, cx)` (`chat.rs` ~4187) puts the returned `User` in the store with `apply_own_user` and redraws. `AppRoot::sync_outside` keeps the saved account name in step.
- **Unicode letters:** `unicode-properties` is already a dependency. Its general category gives `\p{L}` exactly, for the local username check.

**Decisions** (user, 2026-11-07):
1. **The Email row hides the address** behind a "Reveal" link (`GET /auth/account/`), as the web client does, since Settings may be on screen while you share it. "Hide" masks it again.
2. **One dialog per change.** It has the new value, your current password and, with TOTP on, an authenticator code field with a "Use a recovery code" link, all in one step.
   - A wrong password keeps everything typed except the code (used up).
   - It reuses `ConfirmMethod`, `mfa_response` and `ticket_error`, not `confirm_then`. Chaining into "Confirm it's you" would close the form and lose what was typed. It would also ask for the password twice without TOTP.
3. **The password dialog** asks for the new password twice, then the current one.
4. **After a password change**, the success toast points to Settings → Sessions to log other sessions out. There's no checkbox: with TOTP on that would need a second code, since each ticket works once.

**Layout:** the Account page gets an **"Account details"** group above "Two-factor authentication":
- **Username:** `@ada#1234`, with "Change".
- **Email:** `••••••••@••••••` with Reveal, or the address with Hide. It has "Change", and a note under it after a change that's waiting for its link ("Check new@example.com for a link to confirm the change.").
- **Password:** "Change your login password.", with "Change".

It's a **new entity**, `AccountDetailsView` (`src/ui/account_details.rs`), cached per account in `SettingsView.details_views` like the others. It's not part of `AccountSecurityView`, because each settings group renders one entity, and the two groups don't share state. The pure checks and error texts go in a new **`src/account_edit.rs`**, matching the `profile_edit.rs` / `security.rs` split.

**The dialog** (`ChangeDialog`, one entity, `kind: ChangeKind { Username, Email, Password }`):
- **When it opens:**
  - Username opens ready, since it never needs a code.
  - Email and password fetch the MFA status first ("Checking your account…", then Retry on failure), because the status may have changed since the page loaded. With `totp_mfa` they add the code field: the methods from `confirm_methods`, minus `Password`.
- **The fields:**
  - Username: the name, prefilled, with `#1234` beside it and `n/32`; then the current password.
  - Email: the new email; then the current password.
  - Password: the new password, the new password again, then the current password. All are masked.
  - Then, with TOTP on, the code field.
  - The submit button is disabled until the local checks pass. Enter in the last field submits.
- **The steps:**
  1. Check locally.
  2. With TOTP on, make the ticket. A wrong code shows under the code field and keeps the dialog.
  3. Send the change.
  4. On success, close and toast.
  5. On failure, show the error under the field it's about and keep the dialog, or close with a toast for anything unexpected.
  - After any failed change with TOTP on, the code field is cleared, with "That code is used up. Enter a new one." when the error was elsewhere.
- **The warning:** with TOTP on, the text above the code field says "Each code works once, even if the change fails." This matters most for recovery codes.

**Rejected alternatives.**
- **Asking for the code only after `InvalidCredentials`.** Without a ticket Stoat answers `InvalidCredentials` for a missing ticket as well as for a wrong password, so the two can't be told apart.
- **Making the ticket only after checking the password some other way.** No endpoint checks a password without a ticket or logging in again.
- **Putting the details in `AccountSecurityView`.** See Layout.

## Assumptions
1. **Local checks mirror Stoat's**, to save a round trip and, with TOTP on, a code. Stoat is still the judge.
   - **Username:** trimmed; 2–32 characters; only letters (`\p{L}`), digits, `_`, `.` and `-`; not `admin`, `revolt` or `stoat` in any case; and different from the current name (exactly; a change of case is allowed). The blocked patterns and the homoglyph cure stay server-side.
   - **Email:** trimmed; one `@` with something on both sides, a `.` after the `@`, and no spaces. Not compared with the current address: it may not have been revealed, and Stoat doesn't mind.
   - **New password:** at least 8 bytes (Stoat counts bytes), and the two entries must match. The current password can't be blank.
2. **Username result:** the toast shows what Stoat stored, from the returned `User`.
   - "Your username is now ada#1234."
   - When the tag changed: "Your username is now ada#5678. #1234 was taken under that name, so Stoat gave you a new number."
   - When the cure changed the spelling, the toast's name simply shows it. No extra text.
3. **Email result:** after a 204, RuSToat fetches `GET /auth/account/`.
   - If the email already equals the new one (no SMTP), it toasts "Your email is now new@example.com." (and updates the revealed address).
   - Otherwise it toasts "Check new@example.com for a link to confirm the change." and keeps that note on the Email row until the page is rebuilt.
   - If the fetch fails, it's treated as pending.
4. **Password result:** the toast says "Password changed. Your other sessions stay signed in; you can log them out in Settings → Sessions." RuSToat saves no passwords and its session token stays valid, so nothing else changes locally.
5. **Error texts** (under the field when it's about one):
   - **Current password** `InvalidCredentials`: "That password isn't right."
   - **Username:**
     - `FailedValidation`: if the typed current password is under 8 characters, it's the password's length that failed, so "That password isn't right." under the password; otherwise "Use 2–32 letters, numbers, `_`, `.` or `-`." under the username.
     - `InvalidUsername`: "Stoat doesn't allow that username."
     - `UsernameTaken`: "That username is taken. Try another."
     - `DiscriminatorChangeRatelimited`: "Your #number can only change once a day, and that name needs a new one. Try again tomorrow, or pick another name."
   - **Email:** `IncorrectData`: "That isn't a valid email address."; `Blacklisted`: "Stoat doesn't accept addresses from that provider."
   - **Password:** `ShortPassword`: "Use at least 8 characters."; `CompromisedPassword`: "That password has appeared in a data breach. Pick another."
   - **Code** (from `ticket_error`): "That code isn't right." `DisallowedMFAMethod` fetches the status again.
   - **Anything else:** `error_text(err, "Couldn't change your username/email/password. Try again.")`, which already says how long to wait after a rate limit.
6. **The username row and dialog read the store**, so a change from another client (a `UserUpdate`) shows at once. The dialog's prefill is the name when it opened.
7. **No icons are needed:** the rows follow the existing `row` helper, which has none.
8. **Passwords, codes and tickets are never logged.** Only the kind of change and the error type are.

## Plan

### Chunk 1: Account details group, and changing your username
**Goal:** Settings → Account shows an "Account details" group, and changes your username with your current password.
**Files likely touched:**
- `src/api/error.rs`: `from_response` keeps a 429 `{"type":"DiscriminatorChangeRatelimited"}` as `ApiError::Api`, like `InSlowmode`. Update its doc comment.
- `src/api/client.rs`: `change_username(username, password) -> User` (`PATCH /users/@me/username`, a private `#[derive(Serialize)] struct ChangeUsername<'a> { username, password }`).
- `src/account_edit.rs` (new, pure, no GPUI):
  - `USERNAME_MIN = 2`, `USERNAME_MAX = 32`
  - `check_username(text, current) -> Result<&str, &'static str>` (Assumption 1)
  - `ChangeError { Field(ChangeField, String), Toast(String) }` with `ChangeField { Value, Confirm, Current, Code }`
  - `username_error(&ApiError, short_password: bool) -> ChangeError`
  - `username_changed_text(before_tag, &User) -> String`
- `src/ui/account_details.rs` (new):
  - `AccountDetailsView`, one per account: weak chat, and `is_for`. It has the three rows; Email and Password are just the row text with Change disabled until chunks 2–3, or leave them out until then.
  - `ChangeDialog` with `ChangeKind::Username`: the username field with the `#tag` and count, the current password (masked), errors under fields, Cancel / Change.
  - On success: `chat.own_profile_saved(me, None, false, cx)`, close, then toast.
- `src/ui/settings.rs`:
  - `details_views` cached like `account_views`: `sync_details_view`, `shown_details_view`, both called where the others are.
  - `account_page` gains the "Account details" group first, with keywords `account`, `username`, `email`, `password`, `change`, `login`.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- `src/ui/account_page.rs`: make `row` `pub(crate)` (or move it to a shared spot) so both entities use it.

**Depends on:** None.
**Risks:**
- **Two groups, two entities.** Settings search only matches custom rows by keywords, so give each group its own. Both must say "Sign in to manage your account." when there's no account on screen.
- **The returned `User`.** It's the full user (`user.into(db, None)`), like `edit_user`'s answer. `apply_own_user` keeps status, presence and relationship from before, so passing it to `own_profile_saved` is safe. Check that `own_profile_saved` with `bio: None, banner_changed: false` doesn't clear the cached profile.
- **The 429 exception:** keep a rate-limiter 429 (`{"retry_after": …}`, no `type`) as `RateLimited`. Test both.
- **`\p{L}` versus `char::is_alphabetic`:** they differ for marks and letter numbers. Use `unicode_properties::UnicodeGeneralCategory` (`general_category_group() == GeneralCategoryGroup::Letter`). Check that the crate's `general-category` feature is on in `Cargo.toml`.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/account_edit.rs` unit tests:
  - `check_username`:
    - "ada" passes; " ada " is trimmed
    - "a" fails; 33 characters fail
    - "ada lovelace" and "ada!" fail
    - "Ådå" and "名前" pass
    - "Stoat" and "ADMIN" fail
    - the current name fails; the current name in another case passes
  - `username_error` for `InvalidCredentials`, `UsernameTaken`, `InvalidUsername`, `DiscriminatorChangeRatelimited`, `FailedValidation` (both cases) and a rate limit
  - `username_changed_text` with the same and with a new tag
- `tests/api_users.rs`:
  - `PATCH /users/@me/username` sends exactly `{"username":"ada","password":"pw123456"}` and parses the `User`
  - a 429 `{"type":"DiscriminatorChangeRatelimited"}` is `ApiError::Api` with that type
  - a 429 `{"retry_after":2500}` is still `RateLimited { Some(2500) }`

**Manual check:**
- Change your username to a new name; the toast shows the new name and tag, and the sidebar, the account switcher and your messages show it. The web client shows it too.
- Change only its case; the tag stays.
- A wrong password shows "That password isn't right." under the password field, and the dialog stays.
- "stoat" is refused before sending.

**Shippable when:** the Account page has an "Account details" group, the username changes behind the current password with clear errors, and the tests pass.

### Chunk 2: Changing your email, with the code field for two-factor
**Goal:** Reveal your email, and change it with your current password, plus an authenticator or recovery code when two-factor is on.
**Files likely touched:**
- `src/api/client.rs`:
  - `fetch_account() -> AccountInfo` (`GET /auth/account/`)
  - `change_email(ticket: Option<&str>, email, current_password)` (`PATCH /auth/account/change/email`, `DataChangeEmail`, 204). With `Some`, it goes through `send_with_ticket`; with `None`, through `send_raw`.
- `src/account_edit.rs`:
  - `MASKED_EMAIL`
  - `check_email(text) -> Result<&str, &'static str>`
  - `email_error(&ApiError, with_ticket: bool) -> ChangeError`: `IncorrectData` / `Blacklisted` go on the value; `InvalidCredentials` on the current password; the rest is a toast.
  - `code_methods(&MultiFactorStatus) -> Vec<ConfirmMethod>`: empty without TOTP; otherwise `confirm_methods` minus `Password`.
- `src/ui/account_details.rs`:
  - **Email row:** masked with Reveal, or the address with Hide. Reveal calls `fetch_account`; a failure toasts. The row also shows the pending note.
  - **`ChangeDialog`:**
    - The status load for `Email` ("Checking your account…", Retry on failure).
    - The code field with method links and the "Each code works once, even if the change fails." note.
    - The ticket step with `ticket_error` mapped to `ChangeField::Code`.
    - After a failed change with a ticket, clear the code and say it's used up.
    - After a 204, `fetch_account` to tell "changed" from "pending" (Assumption 3).

**Depends on:** Chunk 1.
**Risks:**
- **A code used up by a failed change.** It's unavoidable (Approach). Make sure the dialog clears it and says so, rather than letting the user resubmit a used code: a TOTP code may be refused as reused within its 30 s step, and a recovery code is gone.
- **`InvalidCredentials` with a ticket we sent.** Treat it as the password. Only an expired ticket (over 5 minutes, impossible here since it's made just before) would also give it.
- **The status went stale** (TOTP turned on in the web client while the dialog was open). The change then returns `InvalidCredentials` without a ticket, which reads as "That password isn't right.". Accept that for this rare case; reopening the dialog fetches the status again.
- **The email in the toast and row is the user's own address.** It's fine on screen, but don't log it.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/account_edit.rs` unit tests:
  - `check_email`: "a@b.co" passes; " a@b.co " is trimmed; "ab.co", "a@", "@b.co", "a@b" and "a b@c.de" fail
  - `email_error` for `IncorrectData`, `Blacklisted`, `InvalidCredentials` and a rate limit
  - `code_methods` for `{}`, TOTP only, and TOTP with recovery
- `tests/api_account.rs` (new, wiremock):
  - `GET /auth/account/` parses `{"_id":"a1","email":"ada@example.com"}`
  - `PATCH /auth/account/change/email` sends exactly `{"email":"new@example.com","current_password":"pw"}`: without an `x-mfa-ticket` header for `None`, with `x-mfa-ticket: tok` for `Some("tok")`; and accepts 204
  - a 401 `{"type":"InvalidCredentials"}` is `ApiError::Api`, not `Unauthorized`

**Manual check:**
- Reveal shows your email, and Hide masks it.
- Without two-factor: change your email; the toast and the row say to check the new address; the link arrives. After opening it, Reveal shows the new address.
- With two-factor on: the dialog asks for an authenticator code too. A wrong password clears the code and says it's used up; a right one sends the link.
- Change it back afterwards.

**Shippable when:** the email can be revealed and changed with or without two-factor, used codes are never resubmitted, and the tests pass.

### Chunk 3: Changing your password
**Goal:** Change your password with your current password, plus a code when two-factor is on.
**Files likely touched:**
- `src/api/client.rs`: `change_password(ticket: Option<&str>, password, current_password)` (`PATCH /auth/account/change/password`, `DataChangePassword`, 204), the same way as `change_email`.
- `src/account_edit.rs`:
  - `PASSWORD_MIN_BYTES = 8`
  - `check_new_password(new, again) -> Result<(), (ChangeField, &'static str)>`: "Use at least 8 characters." on the value; "The passwords don't match." on the confirm field, shown only once the confirm field isn't blank
  - `password_error(&ApiError) -> ChangeError`: `ShortPassword` / `CompromisedPassword` on the value; `InvalidCredentials` on the current password; the rest is a toast
- `src/ui/account_details.rs`: `ChangeKind::Password` with three masked fields (new, again, current), the code field as in chunk 2, the Password row's Change, and the success toast (Assumption 4).

**Depends on:** Chunk 2 (the code field and ticket step).
**Risks:**
- **Bytes, not characters.** Stoat checks `password.len()` (bytes), so "Use at least 8 characters." is slightly generous for non-ASCII. Check bytes locally so RuSToat never sends a password Stoat will refuse for length.
- **A compromised password with TOTP on uses up the code**, and it can't be checked locally (the list is on the server). The used-code message from chunk 2 covers it.
- **Masked fields and Enter:** Enter moves from the new password to the confirm field, then to the current password, then submits. That's the login form's pattern (`login.rs` ~101).

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `src/account_edit.rs` unit tests:
  - `check_new_password`: "short" fails; 8 ASCII characters pass; "pässwörd" is 10 bytes and passes; a mismatch fails on the confirm field; a blank confirm isn't reported yet
  - `password_error` for `ShortPassword`, `CompromisedPassword`, `InvalidCredentials` and a rate limit
- `tests/api_account.rs`: `PATCH /auth/account/change/password` sends exactly `{"password":"newpass123","current_password":"pw"}`, with and without `x-mfa-ticket`, and accepts 204.

**Manual check:**
- Without two-factor: change your password; the toast points to Sessions. Log in from the web client with the new password, and RuSToat stays signed in.
- "password" is refused as compromised.
- With two-factor on: the change asks for a code and works (the web client can't do this).
- Change it back afterwards.

**Shippable when:** the password changes with or without two-factor, the errors land on the right field, and the tests pass.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`. Never run it on a `mod.rs` or `lib.rs`, since it formats their child modules too.
- A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines (from `rustfmt --edition 2024 --check <file>`) hasn't grown. Starting counts on 2026-11-07: `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15; 0 in `src/api/error.rs`, `src/security.rs`, `src/profile_edit.rs`, `src/ui/account_page.rs`, `src/ui/settings.rs`, `src/ui/confirm_dialog.rs`, `tests/api_users.rs` and `tests/api_mfa.rs`.

### Unit Tests
- **`src/account_edit.rs`:**
  - username, email and new-password checks
  - each error mapping (which field, which text)
  - the username result text (same and new tag)
  - code methods from the status

### Integration Tests
- **`tests/api_users.rs`:** change username (exact body, parsed `User`); the `DiscriminatorChangeRatelimited` 429 stays an `Api` error while a plain 429 stays `RateLimited`.
- **`tests/api_account.rs`** (new):
  - fetch the account
  - change email and change password, each with exact bodies, the ticket header only when given, and 204
  - `InvalidCredentials` stays an `Api` error

### Manual Tests
You need an `api.stoat.chat` account whose username, email and password you can change and change back, access to a second inbox, an authenticator app, the web client, and a second RuSToat account.
1. Run every chunk's manual check.
2. With two accounts, the Account details group always shows the account on screen, and changing one never touches the other.
3. Change your username from the web client with Settings open in RuSToat; the Username row follows.
4. Several wrong passwords quickly: the message says how long to wait.
5. Light and dark themes: the rows, the dialogs and the errors under fields are readable.
6. Before finishing, put the username, email and password back, and turn two-factor off again if you turned it on.

## Definition of Done
- [ ] Settings → Account has an "Account details" group above "Two-factor authentication", for the account on screen: Username (`@name#tag`), Email (hidden, with Reveal/Hide), and Password, each with Change.
- [ ] Username changes with the current password only; the toast shows what Stoat stored, and says when the `#tag` changed; the once-a-day tag limit is explained, not shown as a generic rate limit.
- [ ] Email and password change with the current password, plus an authenticator or recovery code (in the same dialog) when two-factor is on; without it no `X-MFA-Ticket` is sent.
- [ ] A pending email change says to check the new address; on an instance without email it says it's changed.
- [ ] The password dialog asks for the new password twice; success points to Settings → Sessions.
- [ ] Wrong passwords, invalid values and wrong codes show under their field without closing the dialog; a code used up by a failed change is cleared and said to be used.
- [ ] Passwords, codes, tickets and emails are never logged.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
- [ ] Every chunk's manual check is recorded in `docs/plans/account-details-implementation.md`.
