# Implementation Record: Account details (milestone 17)

_Plan: docs/plans/account-details.md_
_Started: 2026-11-07_

Run in continuous mode on branch `milestone-17-account-details-967a`, committing after each chunk. Every chunk's manual check is batched for the end, as in milestone 16, so the run isn't interrupted; each is listed as pending below. Pre-existing dirty files kept out of every commit: `.gitignore` (modified) and `.shadow/` (untracked). `docs/plans/account-details.md` was new and uncommitted at the start; it goes in with chunk 1.

Starting rustfmt drift (`Diff in` count): `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15; 0 in `src/api/error.rs`, `src/security.rs`, `src/profile_edit.rs`, `src/ui/account_page.rs`, `src/ui/settings.rs`, `src/ui/confirm_dialog.rs`, `tests/api_users.rs`, `tests/api_mfa.rs`.

## Chunk 1: Account details group, and changing your username

### Changes Made
- `src/api/error.rs`: `ApiError::from_response` keeps a 429 `{"type":"DiscriminatorChangeRatelimited"}` as `ApiError::Api`, like `InSlowmode` (doc comment updated). Unit test `a_new_tag_too_soon_isnt_a_rate_limit`.
- `src/api/client.rs`: `change_username(username, password) -> User` (`PATCH /users/@me/username`, a private `ChangeUsername` body struct, since `revolt-models` has none).
- `src/account_edit.rs` (new, pure):
  - `USERNAME_MIN`/`USERNAME_MAX` (2/32) and `PASSWORD_MIN_BYTES` (8)
  - `ChangeField { Value, Confirm, Current, Code, Form }` and `ChangeError { Field, Toast }`
  - `check_username(text, current) -> Result<&str, Option<&'static str>>`: trimmed; 2–32 characters; letters (`\p{L}` through `unicode-properties`' general category), decimal digits, `_`, `.` and `-`; not `admin`/`revolt`/`stoat` in any case. `Err(None)` for blank or unchanged, so the dialog says nothing while the prefilled name is untouched.
  - `username_error(err, short_password)` and `username_changed_text(before_tag, username, tag)`
  - 4 unit tests.
- `src/ui/account_details.rs` (new):
  - `AccountDetailsView` (weak chat, `is_for`): a Username row "@name#tag" with Change, read from the store so `UserUpdate`s show at once.
  - The `ChangeDialog` entity, "Change your username":
    - a line explaining the #number
    - "New username", prefilled, with the `#tag` as an input suffix, the check's message and a `n/32` count
    - "Current password", masked
    - a form-level error line (rate limits), then Cancel / Change
    - Enter moves from the name to the password, then submits.
  - On success it closes, passes the returned `User` to `ChatView::own_profile_saved(me, None, false, cx)`, and toasts the result. Refusals land under their field (focused); anything unexpected closes with an error toast.
- `src/ui/account_page.rs`: `row` is `pub(crate)` and takes `detail: impl Into<SharedString>`.
- `src/ui/settings.rs`:
  - `details_views` is cached per account (`sync_details_view`, `shown_details_view`), like `account_views`.
  - The Account page has an "Account details" group above "Two-factor authentication", each with its own keywords.
  - Signed out, the page shows one "Sign in to manage your account." row (with `ACCOUNT_KEYWORDS`) instead of one per group.
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- `tests/api_users.rs`:
  - the exact username-change body, and the parsed `User`
  - a `DiscriminatorChangeRatelimited` 429 is an `Api` error
  - a `{"retry_after":2500}` 429 is still `RateLimited`

### Divergences
- The Email and Password rows are left out until chunks 2 and 3 (the plan allowed either way).
- `check_username` returns `Err(None)` for blank or unchanged input rather than a message, so an untouched prefilled name isn't shown as an error.
- The `Form` field was added to `ChangeField` (not in the plan): a rate limit keeps the dialog open with the wait above the buttons, rather than closing it with a toast.
- The signed-out Account page shows a single message rather than one per group.

### Verification Performed
- Full suite `cargo test`: 844 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/api/client.rs` 3 (unchanged); 0 in `src/api/error.rs`, `src/ui/account_page.rs`, `src/ui/settings.rs`, `tests/api_users.rs` and the new files.

### Manual Validation
Pending (batched for the end):
- Change your username to a new name; the toast shows the new name and tag, and the sidebar, the account switcher and your messages show it. The web client shows it too.
- Change only its case; the tag stays.
- A wrong password shows "That password isn't right." under the password field, and the dialog stays.
- "stoat" is refused before sending.

## Chunk 2: Changing your email, with the code field for two-factor

### Changes Made
- `src/api/client.rs`:
  - `fetch_account() -> AccountInfo` (`GET /auth/account/`)
  - `change_email(ticket: Option<&str>, email, current_password)` (`PATCH /auth/account/change/email`, `DataChangeEmail`, 204)
  - a private `send_maybe_ticket` that adds `X-MFA-Ticket` only when there's a ticket
  - The import list is hand-formatted.
- `src/account_edit.rs`:
  - `email_error(err)`: `IncorrectData` / `Blacklisted` on the value, `InvalidCredentials` on the current password, a rate limit on the form, else a toast.
  - A shared private `change_error(err, fallback, specific)`, which `username_error` now uses too.
  - `check_email(text)`: trimmed; one `@` with a local part; a domain with a dot that has text on both sides; no whitespace. `Err(None)` when blank.
  - `code_methods(&status)` (`confirm_methods` minus `Password`), `CODE_USED` and `MASKED_EMAIL`.
  - 3 more unit tests.
- `src/ui/account_details.rs`:
  - **The Email row:** `••••••••@••••••` with Reveal (fetches `GET /auth/account/`; a failure toasts and stays hidden), or the address with Hide, plus Change.
  - **After a change:** "Your email is now …" if Stoat changed it at once (the follow-up `fetch_account` already shows it), or "Check … for a link to confirm the change." and a "Waiting for you to open the link sent to …" note under the row.
  - **The dialog** is now one `ChangeDialog` for both kinds (`ChangeKind { Username, Email }`), with per-kind intro, label, checks and errors.
  - **The code step:** for an email change it fetches the MFA status ("Checking your account…", Retry on failure). With an authenticator on, it adds a "Two-factor code" field with the method's prompt, the "Each code works once, even if the change fails." note, and a link to switch to a recovery code.
  - **Submitting:** it makes the ticket from the code, then sends the change with it, all in one IO task. A refused code shows under the code field (`ticket_error`; `DisallowedMFAMethod` fetches the status again). A refused change that used a code empties the code field and shows "That code is used up. Enter a new one." under it, besides the reason at its own field.
  - Enter moves value → current password → code → submit.
  - The ticket token, code, password and email are never logged.
- `tests/api_account.rs` (new):
  - the account's email parses
  - an email change sends exactly `{"email":"new@example.com","current_password":"pw"}`, with no `x-mfa-ticket` for `None` and `x-mfa-ticket: t1` for `Some("t1")`, and accepts 204
  - a 401 `InvalidCredentials` is an `Api` error, not `Unauthorized`

### Divergences
- `Outcome::Username` boxes its `User` (clippy's `large_enum_variant`).
- Emptying the code field (as a used code is) leaves what's said about it, so the "used up" note and the error at another field survive the input's change event.
- The code field's label is "Two-factor code", with the method's own prompt under it ("Enter the 6-digit code from your authenticator app.").

### Verification Performed
- Full suite `cargo test`: 851 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/api/client.rs` 3 (unchanged); 0 in `src/account_edit.rs`, `src/ui/account_details.rs` and `tests/api_account.rs`.

### Manual Validation
Pending (batched for the end):
- Reveal shows your email, and Hide masks it.
- Without two-factor: change your email; the toast and the row say to check the new address; the link arrives. After opening it, Reveal shows the new address.
- With two-factor on: the dialog asks for an authenticator code too. A wrong password clears the code and says it's used up; a right one sends the link.
- Change it back afterwards.

## Chunk 3: Changing your password

### Changes Made
- `src/api/client.rs`: `change_password(ticket: Option<&str>, password, current_password)` (`PATCH /auth/account/change/password`, `DataChangePassword`, 204) through `send_maybe_ticket`. Import list and call hand-formatted.
- `src/account_edit.rs`:
  - `check_new_password(new, again) -> Result<(), Option<(ChangeField, &str)>>`: at least `PASSWORD_MIN_BYTES` (8) bytes, as Stoat counts, else "Use at least 8 characters." on the value. Then the same twice, else "The passwords don't match." on the confirm field. Nothing is said while either is blank.
  - `password_error(err)`: `ShortPassword` / `CompromisedPassword` ("That password has appeared in a data breach. Pick another.") on the value, `InvalidCredentials` on the current password, a rate limit on the form, else a toast.
  - 2 more unit tests.
- `src/ui/account_details.rs`:
  - A Password row ("Change the password you sign in with.", Change).
  - `ChangeKind::Password`: "Use at least 8 characters. Your other sessions stay signed in.", masked "New password" and "Confirm new password" fields, the current password, and the two-factor code field from chunk 2.
  - `checked_value` now names the field its message is about, and `message_at` places check messages and Stoat's refusals together.
  - Enter moves new → confirm → current → code → submit.
  - Success toasts "Password changed. Your other sessions stay signed in; you can log them out in Settings → Sessions."
- `tests/api_account.rs`: a password change sends exactly `{"password":"newpass123","current_password":"pw"}`, without `x-mfa-ticket` for `None` and with it for `Some`; `CompromisedPassword` decodes.

### Divergences
None.

### Verification Performed
- Full suite `cargo test`: 855 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/api/client.rs` 3 (unchanged); 0 in every other touched file.
- `log::` calls in `src/ui/account_details.rs` print only error values (types and response snippets) and the change kind. Never passwords, codes, tickets or emails.

### Manual Validation
Pending (batched for the end):
- Without two-factor: change your password; the toast points to Sessions. Log in from the web client with the new password, and RuSToat stays signed in.
- "password" is refused as compromised.
- With two-factor on: the change asks for a code and works (the web client can't do this).
- Change it back afterwards.

## Summary

_Completed: 2026-11-07 (code); manual checks pending_

### Chunks Completed
1. Account details group, and changing your username
2. Changing your email, with the code field for two-factor
3. Changing your password

### Divergences
- Chunk 1:
  - The Email and Password rows came with their chunks.
  - `check_username` returns `Err(None)` for blank or unchanged input.
  - `ChangeField::Form` was added for rate limits, which keep the dialog open.
  - The signed-out Account page shows one message.
- Chunk 2:
  - `Outcome::Username` is boxed (clippy).
  - Emptying the code field keeps its "used up" note.
  - The code field is labelled "Two-factor code", with the method's prompt under it.
- Chunk 3: none.

### Definition of Done
- **The "Account details" group:** Settings → Account has it above "Two-factor authentication", for the account on screen: Username (`@name#tag`), Email (hidden, with Reveal/Hide) and Password, each with Change. Implemented.
- **Username:** changes with the current password only. The toast shows what Stoat stored and says when the `#tag` changed. The once-a-day tag limit is explained, not shown as a generic rate limit. Implemented and tested.
- **Email and password:** change with the current password, plus an authenticator or recovery code in the same dialog when two-factor is on. Without two-factor, no `X-MFA-Ticket` is sent. Implemented and tested.
- **A pending email change:** says to check the new address. On an instance without email, it says the email has changed. Implemented; the real email flow is a manual check.
- **The password dialog:** asks for the new password twice, and success points to Settings → Sessions. Implemented.
- **Errors:** wrong passwords, invalid values and wrong codes show under their field without closing the dialog, and a code used up by a failed change is cleared and said to be used. Implemented, with the error mappings unit-tested.
- **Logging:** passwords, codes, tickets and emails are never logged. Met.
- **Tests and formatting:** `cargo test` and `cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grew. Met.
- **Manual checks recorded here:** pending the walkthrough.

### Remaining Manual Steps
Every chunk's manual check above, plus the plan's Manual Tests 2–6:
2. Two accounts never cross.
3. A username change from the web client shows in the Username row.
4. Rapid wrong passwords say how long to wait.
5. Light and dark themes are readable.
6. Put the username, email and password back afterwards, and turn two-factor off if you turned it on.
