# Implementation Record: Packaging and loose ends (RuSToat milestone 5)

_Plan: docs/plans/packaging-and-loose-ends.md_
_Started: 2026-11-05_

Supervision mode: continuous.

Pre-existing dirty files, kept out of every commit:
- `.gitignore`: a duplicated, uncommitted "Riptide artifacts (cloud-synced)" block (`.humanlayer/tasks/`), added by tooling.
- `.shadow/`: untracked tooling folder.

The repo already had `cargo fmt --check` drift in many milestone 4 files. Formatting isn't in the plan's checks, so only files written or substantially changed in this milestone are run through `rustfmt`.

## Chunk 1: Own bundle ID and a stable signature

### Changes Made
- `scripts/sign.sh` (new): picks the identity (`RUSTOAT_SIGN_IDENTITY`, else the first "Apple Development:" identity from `security find-identity -v -p codesigning`, else ad hoc `-` with two warnings to stderr). Signs `<path>` with `--force --identifier com.chrishultin.rustoat`, plus extra codesign options from the caller; with none, it adds `--timestamp=none`. Prints the identity used.
- `scripts/cargo-runner.sh` (new): signs the executable through `sign.sh` only when its basename is `stoat-gpui`, then `exec`s the command line.
- `.cargo/config.toml` (new): `runner = "scripts/cargo-runner.sh"` for `aarch64-apple-darwin` and `x86_64-apple-darwin`. Cargo resolves the relative path from the project root, so it works from subdirectories (checked from `src/`).
- `scripts/bundle.sh`: `CFBundleIdentifier` is `com.chrishultin.rustoat`. It signs through `scripts/sign.sh` and prints `signed as: <identity>` to stderr, so the bundle path stays the last stdout line.
- `src/session.rs`:
  - `KEYCHAIN_SERVICE = "com.chrishultin.rustoat"` and `LEGACY_KEYCHAIN_SERVICE = "chat.stoat.gpui-poc"`.
  - New `load_migrating(current, legacy, instance)` over `&dyn TokenStore`. A current token wins and the legacy store isn't touched. Otherwise a legacy token is saved to current and then cleared from legacy. If the save fails, the legacy item is kept; either way the token is returned. A legacy read error is logged and treated as "nothing saved".
  - New `MigratingStore<C, L>` (`TokenStore`): load migrates, while save and clear only touch the current store. `MigratingStore::keychain()` builds the real pair.
- `src/main.rs`: uses `MigratingStore::keychain()` instead of `KeychainStore::new()`.
- `src/places.rs`: `APP_ID = "com.chrishultin.rustoat"`, private `LEGACY_APP_ID = "chat.stoat.rustoat"`, and `support_dir()`. `app_dir()` runs the migration, logging any failure, then returns `support_dir()/APP_ID`; chunk 6's prefs file should use it too. `migrate_app_dir(base) -> Result<bool>` renames the old folder only if it's a directory and the new path doesn't exist. `PlacesFile::for_account` uses `app_dir()`. Two new unit tests.
- `src/api/client.rs`: `SESSION_FRIENDLY_NAME = "RuSToat"`.
- `tests/session_restore.rs`: four new tests: a legacy token moves, a new token wins with legacy untouched, nothing in either store, and `restore` through `MigratingStore` signs in with a migrated token.
- `README.md`:
  - A new "Code signing" section (identity choice, `RUSTOAT_SIGN_IDENTITY`, the ad-hoc fallback, the codesign key prompt).
  - New identifiers throughout.
  - The Keychain bullet now says there's one prompt and then none.
  - A new "Moving over from the old names" section (the legacy Keychain prompt, the folder rename, notification permission asked again).

### Divergences
- **Migration lives in a wrapper store, not inside `KeychainStore::load`.** The plan said `KeychainStore::load` would migrate through a function over `TokenStore`. Instead, `KeychainStore` stays a single-service store and `MigratingStore` wraps two stores and calls `load_migrating`. Same behaviour, and testable with two `MemoryStore`s. Minor.
- **The app folder migrates lazily,** in `places::app_dir()`, rather than at a fixed startup point. That way every caller (places now, prefs in chunk 6) gets the migrated folder. Minor.

### Verification Performed
- `bash -n scripts/*.sh` and `shellcheck scripts/*.sh`: clean.
- `scripts/bundle.sh`, then `codesign -dv --verbose=2 target/debug/RuSToat.app`: `Identifier=com.chrishultin.rustoat`, `Authority=Apple Development: Christopher Hultin (87ATGCDMGT)`, `TeamIdentifier=G55L4BQ44C`.
- `codesign -d -r-`: `designated => identifier "com.chrishultin.rustoat" and anchor apple generic and certificate leaf[subject.CN] = "Apple Development: Christopher Hultin (87ATGCDMGT)" and certificate 1[field.1.2.840.113635.100.6.2.1] /* exists */`, with no cdhash.
- `cargo run` from `src/` (with `STOAT_API_URL=http://127.0.0.1:9`, so the real Keychain item wasn't read): the runner ran, and `codesign -dv target/debug/stoat-gpui` shows `Identifier=com.chrishultin.rustoat`.
- `cargo test`: 326 passed, 0 failed. No "signature" lines in the test log, so test binaries weren't signed.
- `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
The user was given the steps: a bundled run with the one-time prompts, a rebuild and rerun with no prompt, and `cargo run` with no prompt. They replied "OK, keep going" without itemising results. The "RuSToat" session name after a fresh login was optional and wasn't reported.

## Chunk 2: App icon (a cyborg Stoat)

### Changes Made
- `assets/icon/rustoat.svg` (new, hand-written, 1024×1024):
  - Apple's 824 px rounded square at (100,100), `rx=185`, with a purple gradient (`#3b3156` to `#1d1730`) and a drop shadow (`feDropShadow`).
  - Stoat's silhouette path, copied from `favicon-stoat.svg`. Stoat's circle bounds (303..643, 382..722) map onto the rounded square, so the path sits in `matrix(0.761575,0,0,0.761575,229.94,298.32)` and the cyborg parts use the path's own coordinates (the eye hole is at about (415,322)).
  - Cyborg parts:
    - A brushed-metal plate (gradient plus a striped pattern), clipped to the silhouette, wrapping the crown and the eye, with a panel seam.
    - 8 rivets.
    - A glowing cyan eye: a metal socket, a blurred `feGaussianBlur` glow, a radial-gradient lens and highlights.
    - Rust-orange (`#ce422b`) circuit traces with hollow nodes, clipped to the silhouette.
  - Traces and rivets are in groups with `class="detail"`.
- `examples/render_icon.rs` (new): `render_icon <svg> <out.iconset>` renders the 10 `iconutil` PNGs with resvg. For images at 32 px or smaller, it swaps `class="detail"` for `display="none"` before parsing, so the small icons drop the traces and rivets.
- `scripts/make-icon.sh` (new): renders into a `mktemp` iconset (removed on exit), then `iconutil -c icns -o assets/icon/RuSToat.icns`.
- `assets/icon/RuSToat.icns` (new, generated, committed; 398 KB).
- `scripts/bundle.sh`: creates `Contents/Resources`, copies the `.icns` there, adds `CFBundleIconFile RuSToat` to the plist, and `touch`es the app after signing so Finder and the Dock look again.
- `Cargo.toml` and `Cargo.lock`: dev-dependency `resvg = { version = "0.46", default-features = false }`.
- `README.md`: an icon bullet in the bundle section (regenerating it, `killall Dock`, the brand-terms caveat).

### Divergences
- **resvg 0.46, not 0.45.1, and no separate tiny-skia dependency.** On this host the tree uses resvg 0.46.0 (through gpui-pre); 0.45.1 only appears for other targets. resvg re-exports `tiny_skia` (with PNG support) and `usvg`, so one dev-dependency covers both. Minor.
- **Small-size variant by class.** Detail elements are marked `class="detail"` (duplicate `id`s would be invalid) and hidden by a string substitution in the example. Minor; this is what the plan's risk note anticipated.

### Verification Performed
- `scripts/make-icon.sh && file assets/icon/RuSToat.icns`: `Mac OS X icon, 397654 bytes, "ic12" type`.
- `iconutil -c iconset assets/icon/RuSToat.icns -o /tmp/check.iconset`: all 10 sizes present, 16x16 through 512x512@2x.
- Visual check of the 1024 render against the original favicon: the round hole is the eye and the almond shape is the far eye. The 16 and 32 px renders keep the silhouette, the plate and the cyan eye.
- `scripts/bundle.sh`: `plutil -p` shows `CFBundleIconFile => RuSToat` and `CFBundleIdentifier => com.chrishultin.rustoat`. `Contents/Resources/RuSToat.icns` is present, and `codesign --verify --strict` is valid and satisfies its designated requirement.
- `bash -n` and `shellcheck` on `scripts/*.sh`: clean.
- `cargo test`: 326 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean (includes the example).

### Manual Validation
- The bundled app was launched for the user (`scripts/run-bundled.sh`). The log confirmed chunk 1's migration live:
  - "moved the saved session to the new Keychain service".
  - "restored saved session for spedione".
  - "moved …/chat.stoat.rustoat to …/com.chrishultin.rustoat".
  - Events reached Ready.
- The log also showed `banners: permission request failed (Notifications are not allowed for this application)`. The new bundle ID needs notification permission granted again. The user was pointed to System Settings → Notifications → RuSToat.
- The user confirmed: the Dock shows the icon, the notification prompt appeared, there was just one Keychain prompt (the legacy item), and the design is fine for now.
- **The user asked for the icon to be flipped horizontally** "for added distinction" (follow-up commit after `2fbc066`):
  - The stoat group's transform became `matrix(-0.761575,0,0,0.761575,794.06,298.32)`, mirrored about x = 512, so the stoat faces left.
  - Gradients and highlights inside the mirrored group were mirrored back (the metal gradient's direction, the rivet and lens `cx`, the eye highlight at `cx=432`), so the light still falls from the top left.
  - `RuSToat.icns` was regenerated, still with 10 sizes.
- After the flip, the bundle was relaunched and the Dock restarted:
  - It signed in from the new Keychain item with no migration and no prompt.
  - It logged `banners: allowed`, so notification permission had been re-granted for the new bundle ID.

## Chunk 3: Release DMG script

### Changes Made
- `scripts/release.sh` (new):
  - **Identity:** `RUSTOAT_SIGN_IDENTITY`, else the first "Developer ID Application:" identity, else the first "Apple Development:" one. Without any, it exits with an error, since a release isn't signed ad hoc. A non-Developer ID identity gets a warning that the DMG only runs on the team's Macs.
  - **Bundle:** `RUSTOAT_SIGN_IDENTITY=<identity> scripts/bundle.sh release --release-signing`, then `codesign --verify --deep --strict`.
  - **DMG:** `ditto`s the app into a `mktemp -d` staging folder (removed on exit) with an `Applications` symlink. `hdiutil create -volname RuSToat -srcfolder <staging> -format UDZO -ov dist/RuSToat-<version>.dmg`, retried 3 times for "resource busy". It then signs the DMG with `--timestamp`.
  - **Notarization:** runs only when the identity is Developer ID and `RUSTOAT_NOTARY_PROFILE` is set (`xcrun notarytool submit … --keychain-profile … --wait`, then `xcrun stapler staple`). Otherwise it prints why it's skipping and how to enable it.
  - **Last steps:** `hdiutil verify`, then the DMG path as the last line.
  - **`--dry-run`:** builds nothing. Every step goes through `run()`, which prints the command (`%q`-quoted) instead of running it.
- `scripts/entitlements.plist` (new): an empty `<dict/>`, with a comment explaining why.
- `scripts/bundle.sh`:
  - Arguments are now `[debug|release] [--release-signing]`, in any order.
  - `CFBundleVersion` is `<version>.<git rev-list --count HEAD>`, falling back to `.0` outside git.
  - `--release-signing` passes `--options runtime --timestamp --entitlements scripts/entitlements.plist` through `sign.sh`.
- `.gitignore`: `/dist`. Only this line is committed: the index blob was built from `HEAD:.gitignore` plus `/dist`, so the pre-existing uncommitted Riptide lines stay unstaged.
- `README.md`: a new "Releasing" section covering what `release.sh` does, how the identity is chosen, what works without Developer ID, the 3-step notarization setup, `--dry-run` and the empty entitlements.

### Divergences
- **The dry run builds nothing.** The plan only said `--dry-run` "prints the notarytool and stapler commands". Here it prints every step and skips building too, so it's instant and safe. Minor.
- **No ad-hoc fallback for releases.** `release.sh` exits with an error when there's no certificate identity, since a secure timestamp and the hardened runtime need a real one. Minor.
- **The DMG's own code-signing identifier** is codesign's default (`RuSToat-0`). Only the app inside carries `com.chrishultin.rustoat`. Harmless.

### Verification Performed
- `bash -n scripts/*.sh` and `shellcheck scripts/*.sh`: clean. `plutil -lint scripts/entitlements.plist`: OK.
- `RUSTOAT_NOTARY_PROFILE=x scripts/release.sh --dry-run`: identity Apple Development, `dist/RuSToat-0.1.0.dmg`. It prints the bundle, verify, staging, `hdiutil create`, DMG signing and verify commands, plus the note that notarization needs Developer ID.
- The same with `RUSTOAT_SIGN_IDENTITY="Developer ID Application: Test Person (TEAM123456)" RUSTOAT_NOTARY_PROFILE=rustoat-notary`: prints `xcrun notarytool submit dist/RuSToat-0.1.0.dmg --keychain-profile rustoat-notary --wait` and `xcrun stapler staple dist/RuSToat-0.1.0.dmg`.
- `scripts/release.sh` for real (1 min 47 s, including a cold release build): exit 0, a 15.3 MB `dist/RuSToat-0.1.0.dmg`, and `hdiutil verify`: checksum VALID.
- Mounted the DMG; it contains `RuSToat.app` and `Applications -> /Applications`. On the app:
  - `codesign --verify --deep --strict`: valid, and satisfies its designated requirement.
  - `codesign -dv`: `flags=0x10000(runtime)`, a secure `Timestamp`, and `Identifier=com.chrishultin.rustoat`.
  - Entitlements: an empty `[Dict]`. `CFBundleVersion`: `0.1.0.48`.
- `spctl -a -t exec -vv` on the mounted app: `rejected`, `origin=Apple Development: Christopher Hultin (87ATGCDMGT)`. Expected without Developer ID.
- The DMG itself is signed (Apple Development, timestamped).
- **The hardened-runtime risk didn't materialise.** `target/release/RuSToat.app` ran with the window open, the session restored with no Keychain prompt, `banners: allowed` and events Ready. No entitlements were needed.
- `cargo test`: 326 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
- With the user's go-ahead from the earlier "run it for me", I installed the app by `ditto`ing it from the mounted DMG into `/Applications/RuSToat.app` (nothing was there before), checked it with `codesign --verify --deep --strict`, and launched it with `open`.
- The user confirmed it signed in with no Keychain prompt and the Dock showed the icon.
- I then quit it (`osascript … quit`) and deleted `/Applications/RuSToat.app`.

## Chunk 4: Live member and user updates

### Changes Made
- `src/api/events.rs`:
  - New `ServerEvent::{ServerMemberJoin { server, member: Box<Member> }, ServerMemberLeave { server, user }, ServerMemberUpdate { id: MemberCompositeKey, data: Box<PartialMember>, clear }, UserUpdate { id, data: Box<PartialUser>, clear }}`, decoded with `decode_as`.
  - `ServerMemberJoin` decodes `member` as optional. Without it, it builds a bare member from `user`, with `joined_at` set to the epoch.
  - `ServerMemberLeave` ignores `reason` entirely, so unknown reasons decode.
  - The connection loop forwards the four events.
  - Five new unit tests. The existing `unknown_event_types_are_kept_as_unknown` test now uses `ServerRoleUpdate`, since `ServerMemberJoin` is handled.
- `src/store.rs`:
  - `apply_member_join` and `apply_member_leave` (returns `user == me`).
  - `apply_member_update`: `apply_options` plus every `FieldsMember` clear. Returns whether the nickname or avatar changed; members that aren't cached are ignored.
  - `apply_user_update`: `apply_options` plus `FieldsUser` clears (`Avatar`, `DisplayName`, `Pronouns`, `StatusText`, `StatusPresence`; profile fields and `Internal` are no-ops). Returns whether the username, display name or avatar changed, so presence-only updates return false.
  - `add_user` and `knows_user`.
  - `remove_server`: drops the server, its listed channels plus any `TextChannel` pointing at it, their history, read state and pending messages, and its members. Returns the removed channel IDs that were actually known.
  - `channels_showing(user)`: loaded channels where the user wrote or is mentioned, or DMs and groups they're in. Sorted.
  - New private `author_member(message)`: the member cache first, then the message's `member` snapshot. `author_name` and `avatar_url` now use it.
- `src/api/client.rs`: `StoatClient::fetch_user(id)` (`GET /users/{id}`).
- `src/ui/chat.rs`:
  - **Join:** cache the member. If the user is unknown and the server's member list was already fetched for `@`, `fetch_user` runs, which adds the user and refreshes suggestions. Then refresh suggestions.
  - **Leave:** if it's the user, `leave_server`. Then refresh suggestions.
  - **Member update:** if something shown changed, `sync_pane` each channel from `channels_showing` that's in that server, then refresh suggestions.
  - **User update:** if something shown changed, `sync_pane` each channel from `channels_showing` and refresh suggestions. Other users' presence-only updates return early without a redraw; the user's own still redraws.
  - New `leave_server`: removes the server from the store; drops panes, loading tasks and errors, and banners for its channels; cancels a pending ack into them; forgets the member fetch; removes `selected[Place::Server]`; then `drop_stale_selection` (falls back to Home), `remember_places` and `ensure_current_loaded`.
- `tests/store_live.rs`: a new `members_and_users` module with 9 tests (join and leave, an unknown joiner, nickname set and clear, member cache beating the snapshot, uncached member update, user update with name and avatar plus clears, presence-only update, `channels_showing`, the user leaving).
- `tests/api_send.rs`: `fetch_user_gets_one_user` (wiremock).

### Divergences
- **`author_name` and `avatar_url` now prefer the cached member over the message's snapshot.** Before, a message's own `member` snapshot won, so a live nickname change wouldn't have shown on messages that carry one (live messages do). The cache always holds at least the snapshot's info (`apply_message` inserts snapshots, history pages cache members), and it's now kept current by events. Minor, and needed for the chunk's goal.
- **New `StoatClient::fetch_user`.** `ServerMemberJoin` doesn't carry the `User`, and `@` only suggests known users. A joiner is fetched only when that server's full member list was already loaded; otherwise the first `@` loads everyone anyway. Minor addition, not in the plan.
- **No separate `refresh` of the sidebar for member or user events.** `cx.notify()` redraws it from the store. Same outcome.

### Verification Performed
- `cargo test --lib events`: 20 passed, including the 5 new decoding tests (join with and without `member`, leave with any reason, member update clears, user update clears).
- `cargo test --test store_live`: 22 passed (the 9 new ones included).
- `cargo test --test api_send fetch_`: `fetch_user_gets_one_user` passed.
- `cargo test`: 341 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.
- The bundled debug build launched and reached Ready, with log `/tmp/rustoat-m5-4ae9.log`.
- **Incident:** another worktree (`i-d-like-to-look-into-adding-support-for-being-l-6b03`, multi-account work on the old `chat.stoat.rustoat` IDs) was launching its own bundle at the same time, into the same `/tmp/rustoat-bundled.log`. Its build has recreated `~/Library/Application Support/chat.stoat.rustoat` (with an `accounts.json`) since chunk 1's migration renamed the old folder. That branch needs to move to the new IDs (`places::app_dir()`, `com.chrishultin.rustoat`) when the two are merged. From now on, this task's runs log to `/tmp/rustoat-m5-4ae9.log`.

### Manual Validation
Deferred to the end at the user's request ("we'll test at the very end"): a nickname change in the web app, someone joining (shows in `@`), and leaving a test server.

## Chunk 5: The suggestion list follows the cursor, and custom `:name:` converts

### Changes Made
- `src/ui/composer.rs`:
  - New field `last_cursor`.
  - In `Composer::new`, `cx.observe_in(&input, …)`: if `cursor()` differs from `last_cursor`, it stores it. If the text also equals `last_text` (a move, not an edit), it calls `update_completion` and notifies. A notify with the same cursor (blink, redraws) returns immediately.
  - `replace_closed_shortcode`: when `closed_shortcode` finds `:code:` but `by_shortcode(code)` is `None`, it emits the new `ComposerEvent::ClosedShortcode { range, code }`.
  - New `pub fn replace_with_custom_emoji(range, &CustomEmoji, …)`: only if the text at `range` is still exactly `:{name}:`, it records the chip label, calls `replace_range_with_token` with the emoji chip, and updates `last_text`.
- `src/ui/chat.rs`: handles `ClosedShortcode` by looking up `emoji::custom_by_name(&store.custom_emoji_for(channel), code)` and calling `replace_with_custom_emoji`.
- `src/emoji.rs`: `custom_by_name(custom, name) -> Option<&CustomEmoji>`, exact and case-sensitive.
- `tests/emoji.rs`: `custom_emoji_are_found_by_exact_name` (exact match, no case folding, no prefix, missing, empty list).

### Divergences
None. Unicode shortcodes still win, because the event is only emitted when `by_shortcode` finds nothing.

### Verification Performed
- `cargo test --test emoji`: 20 passed. `cargo test --test complete`: 12 passed (the query rules are unchanged).
- `cargo test`: 342 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Deferred to the end at the user's request:
- Moving in and out of `@ad` with the arrows reopens and closes the list.
- Clicking into an earlier `@bo` opens the list for it.
- `:party:` (an existing custom emoji) becomes its chip.
- `:smile:` still becomes 😄.
- No redraw storm: no stream of `Complete` events in the debug log while idle.

## Merge: multiple accounts (between chunks 5 and 6)

The session on `i-d-like-to-look-into-adding-support-for-being-l-6b03` (multiple accounts, finished at `ad283cd`; plan `docs/plans/multiple-accounts.md`, with its own implementation record) asked, on the user's behalf, for its branch to be merged here. It's merged with `--no-ff` after chunk 5's commit.

**History.** That branch was based on `main` at `4b71851`, without milestones 3–4, so this branch had 21 commits it lacked. `git merge-tree` showed 4 textual conflicts: `Cargo.lock`, `README.md`, `src/main.rs` and `src/ui/chat.rs`.

**Resolutions:**
- `src/ui/chat.rs`, render: their branch moved the Dock badge and banner dismissal into `AppRoot` (`ChatView::sync_outside`, per account), so only this branch's `self.sync_composer(window, cx)` stays in `render`.
- `src/ui/chat.rs`, chunk 4's `leave_server`: banner tags now include the account, so it dismisses `notify::banner_tag(&self.account_id, channel)` instead of the bare channel. This one wasn't a textual conflict but would have left banners showing.
- `src/main.rs`: `MigratingStore::keychain()` together with their `FileAccounts`/`MemoryAccounts` setup, the `AddAccount` action and menu item, and `AppRoot::new(store, accounts, …)`.
- `src/accounts.rs`: `FileAccounts::in_config_dir()` (`dirs::config_dir()/chat.stoat.rustoat`) became `in_app_dir()` = `places::app_dir()?.join("accounts.json")`, so it sits beside `places.json` and moves with chunk 1's folder migration. `CONFIG_DIR_NAME` and the module doc path were updated, `main.rs` follows, and the warning now says "no home directory".
- `Cargo.toml` and `Cargo.lock`: removed the `dirs = "6"` dependency (no longer used). `dirs` stays in the lock for other crates. Their `tempfile` dev-dependency comes along.
- `src/session.rs` (auto-merged): their API is keyed by account (`"{api_url}#{user_id}"`; `restore`, `remember` and `log_out` take the key, plus a new `forget`). `MigratingStore` and `load_migrating` keep their logic, with the parameter renamed `instance` → `key`. The two migrations compose: this one moves items between Keychain services under the same key, and theirs adopts a bare-URL key as the first account.
- `tests/session_restore.rs`: this branch's four migration tests now use their `KEY` constant, plus a new `OTHER_KEY`, and `restore(…, KEY.into())`.
- `README.md`, "Your session and the Keychain": service `com.chrishultin.rustoat`, account `{API URL}#{user ID}`, `accounts.json` in `com.chrishultin.rustoat/`, session name "RuSToat", and one Keychain prompt per account and then none. Their "Multiple accounts" and rewritten "Self-hosted instances" sections are kept.

**Verification:** `cargo test`: 377 passed, 0 failed (their branch alone had 245 passing). `cargo clippy --all-targets -- -D warnings`: clean. No stale `chat.stoat.*` references in code, apart from the deliberate legacy constants.

**State on this Mac, to fix before the final manual tests:** after chunk 1's folder migration, the other branch wrote a newer `~/Library/Application Support/chat.stoat.rustoat/accounts.json` with two per-user entries. `com.chrishultin.rustoat/accounts.json` only has the older one-entry list, and the folder migration won't run again because the new folder exists. Plan: back up the new folder's file, then copy the old folder's `accounts.json` over it. `MigratingStore` moves each token from `chat.stoat.gpui-poc` on first load.

## Chunk 6: Default skin tone

### Changes Made
- `src/prefs.rs` (new):
  - `Tone` (`Default`, `Light`, `MediumLight`, `Medium`, `MediumDark`, `Dark`; serde), with `ALL`, `skin_tone() -> emojis::SkinTone` and `label()` (for tooltips).
  - `Prefs { skin_tone }` with `#[serde(default)]`, so missing or unknown fields load fine.
  - `PrefsFile::{in_app_dir, at, load, save}`. It uses `places::app_dir()/prefs.json`; `load` gives the defaults for a missing, unreadable or corrupt file, and `save` writes then renames. Four unit tests.
- `src/lib.rs`: `pub mod prefs`.
- `src/emoji.rs`:
  - `toned(emoji, tone) -> &'static str`: `emojis::get(emoji)?.with_skin_tone(tone)`, falling back to the base.
  - `by_shortcode(code, tone)`.
  - `EmojiChoice::suggestion(tone)`: the glyph and the inserted text are toned, but the row key stays base, so the selection survives a tone change.
  - Search, groups and picker sections stay tone-free. The module doc explains this.
- `src/ui/emoji_picker.rs`:
  - `EmojiBrowser` takes `tone` and an `OnTone` callback, and has `set_tone`.
  - Beside the search field (now `flex_1`), a row of 6 ✋ swatches (22 px each) in each tone, with a tooltip per tone. The chosen one has a primary tint and ring.
  - `browser_row` draws Unicode cells with `toned`. The choices passed back stay base.
- `src/ui/composer.rs`:
  - A `skin_tone` field and `pub set_skin_tone` (which also updates an open picker).
  - `insert_emoji` inserts `toned`, and recently used keeps the base. Closing-colon conversion uses `by_shortcode(code, skin_tone)`.
  - The picker's `on_tone` calls `set_skin_tone` and emits the new `ComposerEvent::SkinTone(tone)`.
- `src/ui/chat.rs`:
  - A `skin_tone` field. `:` suggestions use `choice.suggestion(self.skin_tone)`.
  - `ComposerEvent::SkinTone` sets the field and emits the new `ChatEvent::SkinTone`.
  - `pub set_skin_tone` updates the composer and refreshes open suggestions.
- `src/ui/app_root.rs`:
  - `prefs` and `prefs_file` fields, loaded in `new`.
  - `start_chat` calls `view.set_skin_tone(prefs.skin_tone)`.
  - On `ChatEvent::SkinTone`, `set_skin_tone` saves `prefs.json` (if the tone changed) and pushes the tone to every account's `ChatView`.
- Tests:
  - `tests/emoji.rs`: four new tests (toned and untoned emoji; two-person emoji 🤝 👫 💏 🧑‍🤝‍🧑 get one tone or stay as they are, never panicking; toned shortcodes and suggestions with a stable key; recently used keeps the base).
  - `tests/emoji.rs` and `tests/complete.rs`: existing call sites pass `Tone::Default`.

### Divergences
- **Prefs are owned by `AppRoot`, not `ChatView`.** After the multiple-accounts merge there's one `ChatView` per account, so the tone goes `AppRoot` → every view. The plan file was amended before this chunk.
- **The swatches are a compact row of 22 px squares beside the search field.** The picker is only 272 px wide, so the search field shrinks to fit. As planned, but worth seeing in light and dark mode.
- **No GUI check of the swatches yet.** Manual testing is deferred to the end.

### Verification Performed
- `cargo test --lib prefs`: 4 passed.
- `cargo test --test emoji`: 16 passed. `cargo test --test complete`: 20 passed.
- `cargo test`: 385 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Deferred to the end:
- Choosing the medium tone in the picker, then 👍, inserts 👍🏽.
- `:wave` shows and inserts 👋🏽, and `:thumbsup:` converts to 👍🏽.
- The tone survives a relaunch.
- Swatches look right in light and dark mode.
- With two accounts signed in, a tone chosen in one applies in the other.

## Chunk 7: Stream uploads from disk; clean up pasted files

### Changes Made
- `Cargo.toml`: tokio features `fs` and `io-util`.
- `src/api/error.rs`: a new `ApiError::File(io::Error)`, shown as "Couldn't read the file (…)."
- `src/api/client.rs`, `upload_attachment(file_server, name, content_type, path: &Path, on_progress)`:
  - It opens the file with `tokio::fs::File` and takes the length from metadata. Failing either returns `ApiError::File` before any request.
  - The body is a `futures::stream::try_unfold` that reads at most `min(remaining, 64 KB)` per step (so exactly `len` bytes, even if the file grew), calls `on_progress` after each read, and errors with `UnexpectedEof` ("it got shorter while sending") on an early EOF.
  - A read failure is recorded in a shared slot, so a failed `send()` (or a response after a broken body) is reported as `ApiError::File` rather than `Network`.
  - `on_progress` is shared through an `Arc`. The unused `futures::StreamExt` import was removed.
- `src/draft.rs`:
  - A `DraftAttachment.temporary` flag: `from_path` sets false, `save_pasted_image` sets true.
  - New `PASTE_DIR_PREFIX` and `paste_dir(tmp, pid)`.
  - `remove_temporary(&DraftAttachment)`: only temporary files, and a file that's already gone is fine.
  - `sweep_paste_dirs(tmp, own_pid, now, max_age) -> Vec<PathBuf>`: removes `rustoat-pasted-<numeric pid>` folders that aren't this process's and whose modification time is at least `max_age` before `now`.
  - A new unit test module with 3 tests.
- `src/store.rs`:
  - A `released_files` field, filled by `release_files` with temporary attachments of pending messages that leave the list. That's `confirm_pending` (the POST response or echo), `confirm_pending_in_history` (a history load), `discard_pending`, and `remove_server` (chunk 4, leaving a server).
  - `take_released_files()` drains it.
  - `discard_pending` now returns `Option<PendingMessage>`.
  - Removed a stray doc line (`/// Forget an unconfirmed message…`) left above `set_upload_progress` from milestone 4.
- `src/ui/chat.rs`:
  - `upload_file` passes `&file.path`; there's no more `spawn_blocking` read.
  - `upload_error_text` maps `ApiError::File(err)` to "couldn't read it ({err}).".
  - `sync_outside`, called by `AppRoot` on every notify, including for hidden accounts, deletes every `take_released_files()` entry.
  - The two `discard_pending` callers use `.is_some()`.
- `src/ui/composer.rs`: the tray's × deletes the file if it's temporary. `pasted_dir()` uses `draft::paste_dir`.
- `src/main.rs`: at launch, a background thread runs `sweep_paste_dirs(temp_dir, pid, now, 24 h)` and logs how many folders it removed.
- Tests:
  - `tests/api_upload.rs`: the existing upload tests now use `tempfile` files. New: `a_missing_file_fails_before_any_request` (the mock expects 0 calls; `ApiError::File(NotFound)`), and `a_file_that_shrinks_mid_upload_is_a_file_error` (the progress callback truncates the file to 100 KB after the first 64 KB chunk; `ApiError::File(UnexpectedEof)`). The 200 KB test still checks the exact body bytes and that progress ends at `(200000, 200000)`.
  - `tests/store_pending.rs`: four new tests. Confirmation by the POST response, then the echo, hands back once. A history load hands back. A failure keeps the files and Discard hands them back. The user's own files are never handed back. The existing discard assertions were updated for the `Option` return.

### Divergences
- **`take_released_files`, not `take_confirmed_files`.** One drain covers confirmation, discard and server removal. Because discard also releases through it, `ChatView` deletes files in a single place (`sync_outside`) and doesn't use `discard_pending`'s return value for that. Minor.
- **The sweep runs on a background thread from `main.rs`** (the plan amendment after the merge), not from `ChatView`.
- **New `ApiError::File` variant** so file failures read "couldn't read it" rather than "couldn't reach the file server". Minor.
- **`remove_server` doesn't have its own test for releasing files.** It uses the same `release_files` helper as the tested paths.

### Verification Performed
- `cargo test --lib draft`: 3 passed.
- `cargo test --test api_upload`: 10 passed. The truncation test passed 10 out of 10 consecutive runs.
- `cargo test --test store_pending`: 34 passed.
- `cargo test`: 394 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Deferred to the end:
- Paste a screenshot and send it: its file disappears from `$TMPDIR/rustoat-pasted-*` once confirmed.
- Paste one and remove it with ×: its file is gone.
- A roughly 15 MB upload keeps memory flat in Activity Monitor.
