# Implementation Record: Multiple accounts and instances (RuSToat)

_Plan: docs/plans/multiple-accounts.md_
_Started: 2026-11-05_

_Pre-existing uncommitted changes at start: `.gitignore` (Riptide artifacts lines) and the untracked `.shadow/`. Neither belongs to this work, and both are kept out of every commit. `docs/plans/multiple-accounts.md` (the plan, new) is committed with chunk 1._

## Chunk 1: Account list and per-account tokens

### Changes Made
- `Cargo.toml`: `dirs = "6"` (the account list's location) and dev-dependency `tempfile = "3"`, both already in the tree.
- New `src/accounts.rs`:
  - `keychain_name(instance, user_id)` gives `"{instance}#{user_id}"`.
  - `AccountEntry { id, instance, user_id: Option, name: Option }`. `id` is the Keychain name.
  - `AccountList { accounts, active }`, with `get`, `find`, `active_entry` (the active account, else the first), `add_or_update` (dedupes on `(instance, user_id)`), `set_user` (fills in a migrated entry), `set_active`, and `remove` (the active account moves to the next account, else the previous one).
  - `AccountsFile` trait:
    - `FileAccounts` writes JSON at `dirs::config_dir()/chat.stoat.rustoat/accounts.json` through a temp file and a rename. `load` returns `Ok(None)` when the file is missing.
    - `MemoryAccounts` is for tests.
  - `migrate_legacy(tokens, instance)`: if a token exists under the bare API URL, returns a one-entry list with `id = instance` and `user_id: None`. Keychain errors count as no saved session.
  - Unit tests for the list operations.
- `src/session.rs`:
  - `TokenStore` methods take a `key` (the Keychain name).
  - `restore(store, client, key)`, `remember(store, key, token)` and `log_out(store, client, key)` take the key explicitly.
  - `blocking` is `pub(crate)` so `accounts` can use it.
- `src/ui/app_root.rs`:
  - Holds the `AccountsFile`, the `AccountList` and `current` (the ID of the account on screen).
  - At startup it loads the list synchronously:
    - missing file: first launch, run `migrate_legacy` and save the result
    - corrupt file: log it, start with an empty list, and don't migrate or overwrite the file
  - Then it restores `active_entry()`.
  - A successful restore runs `set_user` with the user's display name, and saves if that changed anything.
  - `NeedsLogin` for an entry with no `user_id` removes it, because there's nothing to show for an account we never identified.
  - Login runs `add_or_update` with the default instance, sets it active, saves, and remembers the token under the account's ID.
  - Log out removes the account and clears its Keychain item.
  - The list is saved synchronously on the main thread: it's small, and this keeps writes in order.
- `src/main.rs`: passes `FileAccounts::in_config_dir()` to `AppRoot`, falling back to `MemoryAccounts` with a warning.
- `tests/session_restore.rs`: uses explicit keys. New tests:
  - `two_accounts_on_one_instance_keep_their_own_tokens`
  - `legacy_session_is_adopted_and_restores`
  - `nothing_to_adopt_gives_an_empty_list`
- New `tests/accounts.rs`:
  - a file round trip, including a config directory that doesn't exist yet and no leftover temp file
  - a missing file loads as `None`
  - an emptied list is not treated as a first launch
  - a corrupt file is an error and is left untouched
  - unknown and missing fields are tolerated
- `README.md`: the self-hosted and Keychain sections now describe the `{API URL}#{user ID}` naming, adopting an old session, and the account list file.

### Divergences
- **Corrupt account file.** The plan said "treated as empty, not overwritten until the next save". The implementation also skips the legacy migration in that case, so a corrupt file isn't immediately replaced by a migration save.
- **`STOAT_API_URL` with a saved account.** Until chunk 2 adds the instance field, a saved account is restored whatever `STOAT_API_URL` says. The variable only picks the instance for a fresh login. Documented in the README.
- **Found while doing the manual check:** `main` is 10 commits ahead of this branch. Its changes include `src/places.rs`, which remembers the open server and channels per account in the same Application Support folder, plus about +1200 lines in `src/ui/chat.rs`. A parallel branch, `let-s-continue-the-implementation-from-where-we-4ae9` (commit 9a63302, not yet on `main`), renames the Keychain service to `com.chrishultin.rustoat` and moves the Application Support folder. Its token migration is per key name, and it moves the folder as a whole, so both compose with this chunk. Once it lands, `FileAccounts::in_config_dir` should use `places::app_dir()`, and the `dirs` dependency can go.

### Verification Performed
- `cargo test`: 179 passed, 0 failed (ignored: the live and Keychain opt-in tests).
- `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Run by Claude, with the user's approval, against the user's real saved session. `RUST_LOG=stoat_gpui=debug cargo run` logged:
1. `adopting the session saved for https://api.stoat.chat as the first account`
2. `restoring https://api.stoat.chat`
3. `restored saved session for spedione`

The chat opened with no login prompt. `~/Library/Application Support/chat.stoat.rustoat/accounts.json` then contained one entry with `id` and `instance` set to `https://api.stoat.chat`, `user_id` `01KH4Q74NX640MPRYJF506Y71W`, `name` `spedione`, and `active` set to that entry.

The optional log-out-and-relaunch step was not run, to keep the user's session.
