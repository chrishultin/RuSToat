# Plan: Packaging and loose ends (RuSToat milestone 5)

## Summary
RuSToat gets an identity of its own:
- **A stable code signature.** It uses the user's Apple Development certificate, so the Keychain stops prompting after every rebuild.
- **Its own bundle ID**, `com.chrishultin.rustoat`, with saved data migrated to it.
- **A cyborg take on the Stoat icon.**
- **A DMG release script**, ready for notarization once a Developer ID certificate exists.

It also clears milestone 4's loose ends:
- live member and user updates
- the suggestion list following the cursor
- custom emoji `:name:` conversion
- a default skin tone
- uploads streamed from disk
- cleanup of pasted temp files

The deferred features (jump to message, spoilers, the image viewer, notification settings and sounds, moderation) are **milestone 6**, planned separately once this one is done.

## Approach
Keep the milestone 1–4 architecture:
- **Pure model, thin views:** rules live in GPUI-free modules (`ChatStore`, `draft.rs`, `complete.rs`, `emoji.rs`, plus a new `prefs.rs`). `ChatView` and `Composer` only wire them up.
- **Backend types:** wire types come from `revolt-models`. Partial updates use the backend's own `apply_options`, as `MessageUpdate` and `ChannelUpdate` already do.
- **Plain shell scripts for packaging,** extending the existing `scripts/bundle.sh` and `scripts/run-bundled.sh`, with no new build tools. Signing, DMG creation and notarization use Apple's own `codesign`, `hdiutil`, `notarytool` and `stapler`, which are all installed.

**Context from milestone 4** (read `docs/plans/composer-attachments-and-autocomplete.md` and its `-implementation.md` before starting):
- **`scripts/bundle.sh`:**
  - Builds `target/<profile>/RuSToat.app` with a heredoc `Info.plist` (`CFBundleIdentifier chat.stoat.rustoat`, executable `stoat-gpui`).
  - Signs it ad hoc: `codesign --force --sign - --identifier chat.stoat.rustoat`.
  - Prints the bundle path last.
- **`scripts/run-bundled.sh`** runs the bundle's executable directly, with logs in the terminal.
- **`src/banners.rs`** reads the bundle identifier from `NSBundle` and only posts banners when bundled.
- **The Keychain:** `src/session.rs` has `KEYCHAIN_SERVICE = "chat.stoat.gpui-poc"` (the account is the API URL), using the `keyring` crate.
- **Saved places:** `src/places.rs` saves `~/Library/Application Support/chat.stoat.rustoat/places.json`.
- **The session name:** `src/api/client.rs` has `SESSION_FRIENDLY_NAME = "Stoat GPUI POC"`.
- **Events:** `src/api/events.rs` decodes events into its own `ServerEvent` enum and has no member or user events yet.
- **Store data:** `ChatStore.members` is keyed by `(server, user)` and `users` by ID. `mention_candidates`, `name_in`, `author_name` and `user_avatar_url` read them when rows are built (`timeline()`). `ChatView::sync_pane(channel)` rebuilds a loaded channel's rows.
- **Composer:**
  - `update_completion` runs only on `InputEvent::Change`.
  - `replace_closed_shortcode` converts Unicode `:code:` only on a typed `:` (`typed_char`).
  - `ComposerEvent::Complete(Query)` and `set_suggestions` exchange suggestions with `ChatView`.
  - Custom emoji chips are `TokenRef::Emoji(id)` with the label `:name:`.
- **Emoji:** `src/emoji.rs` has `EmojiChoice::{Unicode { emoji, shortcode }, Custom(CustomEmoji)}`, `search`, `search_all`, `groups`, `RecentEmoji`, `picker_sections` and `picker_rows`. `EmojiBrowser` is in `src/ui/emoji_picker.rs`.
- **Uploads:**
  - `ChatView`'s `upload_file` reads the whole file (`std::fs::read` in `spawn_blocking`), then calls `StoatClient::upload_attachment(file_server, name, content_type, bytes: Vec<u8>, on_progress)`, which streams 64 KB chunks of the `Vec` through `Body::wrap_stream`.
  - Pasted images are saved by `draft::save_pasted_image` to `$TMPDIR/rustoat-pasted-<pid>/pasted-image-N.ext` and never deleted.
  - `ChatStore::apply_message` confirms pending messages through `confirm_pending`, and `discard_pending` drops them.

**Facts this plan relies on** (checked on 2026-11-05):
- **Signing identities** (`security find-identity -v -p codesigning`): "Apple Development: Christopher Hultin (87ATGCDMGT)" and an older "iPhone Developer" one.
  - **There's no "Developer ID Application" certificate.** That's needed to notarize and distribute. Only the team's Account Holder can create one (developer.apple.com → Certificates).
  - `xcrun notarytool` 1.1.4 and `stapler` are installed. No notary profile is stored yet.
- **Why a stable signature fixes the prompts:** Keychain access is tied to an app's designated requirement. With an Apple Development certificate and a fixed identifier, that requirement (`identifier "com.chrishultin.rustoat" and anchor apple generic and certificate leaf[subject.CN] = …`) stays the same across rebuilds, so "Always Allow" sticks. Ad-hoc signatures are tied to a hash of the binary, which changes on every build.
- **Member and user events** (`revolt-database` 0.16.2 `src/events/client.rs`, AGPL; read for reference only, not a dependency):
  - `ServerMemberJoin { id: server, user, member: Member }`
  - `ServerMemberLeave { id: server, user, reason }`
  - `ServerMemberUpdate { id: MemberCompositeKey, data: PartialMember, clear: Vec<FieldsMember> }`
  - `UserUpdate { id, data: PartialUser, clear: Vec<FieldsUser>, event_id }`
  - `Member` and `User` have `apply_options` (the `revolt_optional_struct` partials feature, already enabled).
- **The textarea's events:** `InputEvent` has only `Change`, `PressEnter`, `Focus` and `Blur`, with no cursor or selection event. `cx.observe(&input, …)` fires whenever the textarea's state notifies, including cursor moves.
- **Skin tones:** `emojis` 0.9.1 has `Emoji::skin_tones()`, `with_skin_tone(SkinTone)` and `skin_tone()`. `SkinTone` has `Default`, `Light`, `MediumLight`, `Medium`, `MediumDark` and `Dark`, plus mixed-pair tones for two-person emoji.
- **Icon tools:**
  - `resvg` 0.45.1 is already in the tree (through gpui-component).
  - `iconutil`, `sips` and `hdiutil` are built into macOS.
  - Stoat's icon is `https://stoat.chat/favicon-stoat.svg` (2 KB): a white stoat silhouette on a dark purple (`rgb(39,32,56)`) circle.
- **Streaming from disk:** tokio is built without its `fs` feature, so it's added for that.

**Rejected alternatives:**
- **`cargo-bundle` or `cargo-packager`:** another tool to install and configure, for a single-binary app the existing script already bundles. Shell keeps every signing step visible.
- **Notarizing now:** it needs a Developer ID certificate the user doesn't have yet (user decision: "not just yet"). The release script supports it behind a check, so it's ready when the certificate exists.
- **Keeping `chat.stoat.*` names:** that's Stoat's domain, and the user chose `com.chrishultin.rustoat`.
- **A long-press menu of tones per emoji:** more UI for little gain. The user chose one default tone.

## Assumptions
1. **The bundle ID is `com.chrishultin.rustoat` (user decision).** Everything moves to it, with migration:
   - **The Keychain service** becomes `com.chrishultin.rustoat`. On load, if the new item is missing and the old `chat.stoat.gpui-poc` one exists, it's copied to the new service and the old item is deleted. macOS may prompt once for the old item, since it was created by an ad-hoc binary.
   - **The Application Support folder** becomes `com.chrishultin.rustoat`. If the old `chat.stoat.rustoat` folder exists and the new one doesn't, the old folder is renamed.
   - **The session name** shown in Stoat's Settings → Sessions becomes "RuSToat" (from "Stoat GPUI POC"), for new logins only.
   - **macOS asks for notification permission again**, once, for the new bundle ID.
2. **Day-to-day builds are signed with "Apple Development"**, picked automatically from the keychain:
   - The `RUSTOAT_SIGN_IDENTITY` environment variable overrides the choice.
   - If there's no such identity, the build falls back to ad hoc with a warning, so a fresh machine still builds.
   - Plain `cargo run` is signed too, through a cargo `runner` that signs only the `stoat-gpui` binary, not test binaries.
3. **The DMG script is built now and notarizes only when it can (user decision: "not just yet").**
   - `scripts/release.sh` builds a release bundle with the hardened runtime and a secure timestamp, and puts it in a compressed DMG (`hdiutil`, UDZO) with an `/Applications` link.
   - It signs with "Developer ID Application" if present, and otherwise with Apple Development, warning that the DMG then only works on this Mac.
   - It notarizes and staples only when `RUSTOAT_NOTARY_PROFILE` names a `notarytool` keychain profile and a Developer ID identity exists.
4. **The release build is for the host architecture (Apple silicon).** A universal binary (`x86_64` through `lipo`) is deferred until there's someone to distribute to.
5. **The icon is a "cyborg respin" of Stoat's icon (user decision).**
   - It's drawn as an SVG by hand: the stoat silhouette on its purple circle, with a glowing cyan eye, a metal head plate with rivets, and circuit traces in Rust orange.
   - It's committed as `assets/icon/rustoat.svg`, with the rendered `assets/icon/RuSToat.icns` committed too (so bundling needs no render step), plus a script to regenerate it.
   - **It derives from Stoat's logo:** fine for personal use. Check Stoat's brand terms before any public distribution.
6. **Live updates:**
   - **Joins and leaves:** `ServerMemberJoin` adds to the member cache and `ServerMemberLeave` removes from it.
   - **Edits:** `ServerMemberUpdate` and `UserUpdate` apply partial edits and clear fields.
   - **Leaving or being removed from a server:** when it's the user, the server and its channels leave the sidebar, and the view falls back to Home if it was open.
   - Loaded channels whose rows show an updated user (name, avatar, nickname) are rebuilt.
7. **The default skin tone (user decision)** is chosen in the emoji picker and kept across launches in `~/Library/Application Support/com.chrishultin.rustoat/prefs.json`.
   - It applies to every Unicode emoji that supports tones, wherever one is inserted: the picker, `:` completion, closing-colon conversion and recently used.
   - **Two-person emoji** (🤝, 💏…) use the same tone for both people when the crate has that variant, and the default otherwise.
   - **Reactions keep the default tone.** The reaction picker isn't part of the message box.
8. **Custom emoji `:name:` conversion** happens on a typed closing colon when the name exactly matches one of the channel's custom emoji (`custom_emoji_for`). Unicode shortcodes win if both match, as in completion ranking.
9. **The suggestion list follows the cursor:**
   - Moving the cursor (arrows, clicks, Home and End) re-evaluates the query. Moving into another `@word` reopens the list, and moving out closes it.
   - Up and Down still move the list's selection while it's open.
10. **Uploads stream from disk.**
    - The file is opened and read in 64 KB chunks during the upload, never held whole.
    - The size sent is the size when the upload starts. If the file shrinks or vanishes mid-upload, the upload fails with "couldn't read it", and Retry re-reads it.
11. **Pasted temp files** are deleted when their image leaves the tray (× or Discard), when its message is confirmed, or when a failed message is discarded.
    - At launch, leftover `rustoat-pasted-*` folders from other processes are deleted if they're more than a day old.
    - A draft with a pasted image that's still sitting in another channel's box keeps its file.

## Open Questions
- *(Deferred, not blocking)* **The Developer ID certificate and notary credentials**, for notarized distribution. When ready:
  - Create a "Developer ID Application" certificate (Account Holder only).
  - Run `xcrun notarytool store-credentials rustoat-notary --apple-id … --team-id 87ATGCDMGT`.
  - Run `RUSTOAT_NOTARY_PROFILE=rustoat-notary scripts/release.sh`.
- *(Deferred)* **A universal (Intel + Apple silicon) build.**
- *(Deferred)* **The icon's provenance**, if the app is ever distributed publicly (Assumption 5).

## Plan

### Chunk 1: Own bundle ID and a stable signature
**Goal:** RuSToat is `com.chrishultin.rustoat` everywhere, signed with the user's Apple Development identity in both `cargo run` and the bundle, so the Keychain prompt stops after one "Always Allow". The saved session and places carry over.

**Files likely touched:**
- `scripts/sign.sh` (new):
  - Picks the identity: `RUSTOAT_SIGN_IDENTITY`, else the first "Apple Development:" line of `security find-identity -v -p codesigning`, else `-` (ad hoc, with a warning to stderr).
  - Signs a given path with `--identifier com.chrishultin.rustoat --force --timestamp=none`.
  - Prints the identity used.
- `scripts/cargo-runner.sh` (new) and `.cargo/config.toml` (new):
  - `[target.aarch64-apple-darwin] runner = "scripts/cargo-runner.sh"` (and the same for `x86_64-apple-darwin`).
  - The runner signs the executable only if its file name is `stoat-gpui`, then `exec`s it with its arguments. Test binaries run unsigned, as now.
- `scripts/bundle.sh`:
  - Uses `com.chrishultin.rustoat` in the plist.
  - Signs through `scripts/sign.sh` instead of `--sign -`.
- `src/session.rs`:
  - `KEYCHAIN_SERVICE = "com.chrishultin.rustoat"`, plus `LEGACY_KEYCHAIN_SERVICE = "chat.stoat.gpui-poc"`.
  - `KeychainStore::load` migrates: if the new item is missing, it reads the old one, writes the new one, then deletes the old one. The migration rule is a function over the `TokenStore` trait, so it's testable with the in-memory store.
- `src/places.rs`: the app folder name becomes a constant `APP_ID = "com.chrishultin.rustoat"`, with `migrate_app_dir(base)`, which renames `chat.stoat.rustoat` when the new folder doesn't exist.
- `src/api/client.rs`: `SESSION_FRIENDLY_NAME = "RuSToat"`.
- `README.md`: the new IDs, signing, how to override the identity, and the one-time migration prompts.
- Tests: `tests/session_restore.rs` (migration with the in-memory store), and `src/places.rs` unit tests (folder migration in a temp dir).

**Depends on:** None.

**Risks:**
- **`codesign` may ask for the private key the first time** ("codesign wants to access key"). The user picks "Always Allow" once. Document it.
- **The legacy Keychain item belongs to an ad-hoc identity,** so the migration read prompts once more. Expected; say so in the README.
- **A cargo runner applies to every binary of that target triple,** including `cargo test` binaries. The name check keeps tests unsigned. Verify `cargo test` doesn't invoke `codesign` (time it, and check the log).
- **The Keychain ACL is copied with the item?** No: the migrated item is newly created by the signed app, so it gets the new app's ACL. That's the point.

**How to verify:**
- `bash -n scripts/*.sh`, and `shellcheck scripts/*.sh` if installed.
- `scripts/bundle.sh`, then `codesign -dv --verbose=2 target/debug/RuSToat.app 2>&1 | grep -E "Identifier|Authority|TeamIdentifier"`. Expect `Identifier=com.chrishultin.rustoat`, `Authority=Apple Development: Christopher Hultin (87ATGCDMGT)` and `TeamIdentifier=…`.
- `codesign -d -r- target/debug/RuSToat.app` shows a designated requirement with no `cdhash`.
- `cargo build && cargo run` signs `target/debug/stoat-gpui` (check with `codesign -dv target/debug/stoat-gpui`).
- `cargo test --test session_restore` covers: the legacy item migrates; an existing new item wins and the legacy one is left alone; no items gives nothing.
- `cargo test --lib places` covers: the old folder is renamed; an existing new folder isn't touched.

**Manual check:**
- Run `scripts/run-bundled.sh`. Allow the one-time prompts (the key, the legacy item, notifications). You should be signed in at the same place.
- Rebuild after a trivial code change and run again: **no Keychain prompt**.
- Also run `cargo run`: no prompt.
- Stoat's Settings → Sessions shows "RuSToat" after a fresh login.

**Shippable when:** Rebuilding and running, bundled or not, no longer prompts for the Keychain, and existing sessions, places and banners carry over to the new ID.

### Chunk 2: App icon (a cyborg Stoat)
**Goal:** RuSToat has its own icon in the Dock, Finder and banners: Stoat's stoat-on-purple, rebuilt as a cyborg.

**Files likely touched:**
- `assets/icon/rustoat.svg` (new), hand-written on a 1024×1024 canvas. macOS icons fill a rounded square, so the circle becomes a rounded square:
  - Stoat's silhouette path, from `favicon-stoat.svg`, on the purple background `rgb(39,32,56)`.
  - Cyborg parts: a brushed-metal plate over the head with rivets, a glowing cyan eye with a soft glow (an SVG `feGaussianBlur` filter), and thin circuit traces with nodes in Rust orange (`#CE422B`) running from the plate.
  - Margins as in Apple's icon grid (content inside about 824 px).
- `examples/render_icon.rs` (new): renders the SVG with `resvg` and `tiny-skia` (both already in the tree; added as dev-dependencies at the versions in `Cargo.lock`) into the standard `.iconset` PNG sizes (16 to 1024, with @2x).
- `scripts/make-icon.sh` (new): `cargo run --example render_icon -- assets/icon/rustoat.svg <tmp>/RuSToat.iconset`, then `iconutil -c icns -o assets/icon/RuSToat.icns`.
- `assets/icon/RuSToat.icns` (new, generated, committed).
- `scripts/bundle.sh`: copies the `.icns` to `Contents/Resources/` and adds `CFBundleIconFile RuSToat` to the plist.
- `Cargo.toml`: the `resvg` and `tiny-skia` dev-dependencies.

**Depends on:** Chunk 1 (the bundle script and the new ID).

**Risks:**
- **resvg's filter support:** if `feGaussianBlur` doesn't render as intended, fake the glow with stacked translucent circles.
- **Small sizes:** the 16 and 32 px icons may turn to mush. Check them with `qlmanage -p` or Finder, and simplify the 16/32 renders if needed (for example, no circuit traces). The example can take a "small" variant.
- **The Dock caches icons:** after bundling, `touch` the app, or run `killall Dock`, to refresh. Document it.
- **Licensing:** a derivative of Stoat's logo (Assumption 5).

**How to verify:**
- `scripts/make-icon.sh && file assets/icon/RuSToat.icns` reports a Mac OS X icon.
- `iconutil -c iconset assets/icon/RuSToat.icns -o /tmp/check.iconset && ls /tmp/check.iconset` lists all 10 sizes.
- `scripts/bundle.sh` gives a plist with `CFBundleIconFile` (`plutil -p target/debug/RuSToat.app/Contents/Info.plist`).

**Manual check:** Open `target/debug/RuSToat.app`. The Dock shows the cyborg Stoat, and a banner shows it too. Look at the 1024 render (`open assets/icon/rustoat.svg` in a browser) and say whether it needs another pass.

**Shippable when:** The bundle shows the new icon at every size, and the icon can be regenerated from its SVG with one script.

### Chunk 3: Release DMG script
**Goal:** `scripts/release.sh` produces a signed release bundle in a DMG, and notarizes and staples it when a Developer ID identity and a notary profile are available.

**Files likely touched:**
- `scripts/release.sh` (new):
  - `cargo build --release`, then `scripts/bundle.sh release` (or shared functions).
  - Signing: with "Developer ID Application" if present, else Apple Development (warning that the DMG only runs on this Mac). Always `--options runtime --timestamp` (hardened runtime) and the entitlements file.
  - `hdiutil create -volname RuSToat -srcfolder <staging with the .app and an Applications symlink> -format UDZO dist/RuSToat-<version>.dmg`, then sign the DMG.
  - If `RUSTOAT_NOTARY_PROFILE` is set and a Developer ID identity was used: `xcrun notarytool submit dist/…dmg --keychain-profile "$RUSTOAT_NOTARY_PROFILE" --wait`, then `xcrun stapler staple`. Otherwise it prints how to enable notarization.
- `scripts/entitlements.plist` (new): empty, since the app needs no hardened-runtime exceptions: network clients, the Keychain and user notifications don't need any.
- `scripts/bundle.sh`: `CFBundleVersion` becomes `<cargo version>.<git commit count>`, so every release has a unique build number. It also takes an optional `--release-signing` to use the hardened runtime.
- `.gitignore`: `dist/`.
- `README.md`: a "Releasing" section (prerequisites, environment variables, what works without Developer ID).

**Depends on:** Chunks 1 and 2.

**Risks:**
- **The hardened runtime may break something at run time** (GPUI's Metal shader loading, or `objc2` notification APIs). Run the hardened release bundle before calling the chunk done. If it fails, find the narrowest entitlement and document it.
- **`hdiutil` in a sandbox or CI** can fail with "resource busy". The script retries `hdiutil detach` and works from a temp staging folder.
- **The notarization path can't be fully tested** without a Developer ID certificate. Check its argument handling with a dry-run flag (`--dry-run` prints the `notarytool` and `stapler` commands).

**How to verify:**
- `scripts/release.sh` (no notary profile) creates `dist/RuSToat-0.1.0.dmg`.
- `hdiutil verify dist/*.dmg` passes.
- `codesign --verify --deep --strict --verbose=2` on the mounted app passes, and `codesign -dv` shows `flags=0x10000(runtime)`.
- `scripts/release.sh --dry-run` with `RUSTOAT_NOTARY_PROFILE=x` prints the notarize and staple commands, or explains that Developer ID is missing.
- `spctl -a -t exec -vv` on the mounted app is **expected to reject** it without Developer ID. Record the output.

**Manual check:** Mount the DMG, drag RuSToat to Applications, launch it from there: it should sign in with no Keychain prompt and show the icon. Then delete the copy in Applications.

**Shippable when:** One command produces a signed, hardened DMG that installs and runs on this Mac, and notarization switches on with an environment variable once a Developer ID certificate exists.

### Chunk 4: Live member and user updates
**Goal:** The member cache, names, nicknames and avatars stay current during a session. Joins and leaves update `@` suggestions, and leaving a server removes it from the sidebar.

**Files likely touched:**
- `src/api/events.rs`:
  - `ServerEvent::{ServerMemberJoin { server, member: Box<Member> }, ServerMemberLeave { server, user }, ServerMemberUpdate { id: MemberCompositeKey, data: Box<PartialMember>, clear: Vec<FieldsMember> }, UserUpdate { id, data: Box<PartialUser>, clear: Vec<FieldsUser> }}`, decoded with `decode_as` like the other events.
  - Unknown `reason` values in `ServerMemberLeave` are ignored.
- `src/store.rs`:
  - `apply_member_join`, `apply_member_leave` (returns whether it was the user) and `apply_member_update` (`apply_options` plus `clear`: `Nickname`, `Avatar`, `Roles`, `Timeout`, as `FieldsMember` lists them).
  - `apply_user_update` (`apply_options` plus `clear` for `FieldsUser`: `Avatar`, `StatusText`, `StatusPresence`, `ProfileContent`, `ProfileBackground`, `DisplayName`, `Internal`).
  - `remove_server(id)` drops the server, its channels and its members, and returns the removed channel IDs.
  - `channels_showing(user) -> Vec<String>`: loaded channels with a message by that user, or that are DMs or groups with them.
- `src/ui/chat.rs`:
  - Handles the four events.
  - Rebuilds the affected panes (`sync_pane`).
  - On `ServerMemberLeave` for the user: `remove_server`, closes its panes, and moves the place to Home if it was open (`places.rs`).
  - Refreshes the open suggestion list (`refresh_suggestions`).
- Tests: `src/api/events.rs` unit tests (decoding, with fixtures shaped like `revolt-database`'s), and `tests/store_live.rs` (applying each event).

**Depends on:** None (independent of chunks 1–3).

**Risks:**
- **`ServerMemberJoin` in 0.16.2 carries `member`** (`user` is deprecated). Older servers may send only `user`. Decode `member` as optional and fall back to a member with only an ID. Verify against a live join if possible.
- **`UserUpdate` arrives for every user the client "knows"**, including presence changes, which can be frequent. Only rebuild panes when a displayed field changed (display name, username, avatar). Presence and status changes update the cache without rebuilding.
- **The user leaving the open server** must not leave a dangling selection. Reuse the stale-selection logic from `drop_stale_selection`.

**How to verify:**
- `cargo test --lib events` covers decoding all four events, including `clear` lists and a `ServerMemberJoin` without `member`.
- `cargo test --test store_live` covers:
  - A join makes a member mentionable, and a leave removes them.
  - A nickname set or cleared changes `name_in` and the `@` labels.
  - A `UserUpdate` with a display name or avatar changes `author_name` and `user_avatar_url`; clearing `Avatar` goes back to the default avatar.
  - The user leaving removes the server and its channels from `servers()` and `server_channels`.
  - A presence-only update doesn't list channels to rebuild.

**Manual check:**
- In a server, change your nickname in the web app. RuSToat's messages and `@` show the new name without a restart.
- Have someone join, or join with a second account: they appear in `@`.
- Leave a test server from the web app: it disappears from the rail.

**Shippable when:** Names, avatars, nicknames and membership changes show live, and leaving a server removes it cleanly.

### Chunk 5: The suggestion list follows the cursor, and custom `:name:` converts
**Goal:** Moving the cursor with keys or the mouse updates the suggestion list. Typing the closing colon of a custom emoji's exact name inserts its chip.

**Files likely touched:**
- `src/ui/composer.rs`:
  - `cx.observe(&input, …)`: when `cursor()` differs from the last known cursor, and the text didn't change (text changes are already handled), call `update_completion`. Store `last_cursor`.
  - `replace_closed_shortcode`: when the code isn't a Unicode shortcode, emit `ComposerEvent::ClosedShortcode { range, code }`.
  - New `replace_with_custom_emoji(range, CustomEmoji)`: if the text at `range` is still `:code:`, replace it with the emoji chip (`replace_range_with_token`).
- `src/ui/chat.rs`: handles `ClosedShortcode` by finding the channel's custom emoji with exactly that name (`emoji::custom_by_name`) and calling `replace_with_custom_emoji`.
- `src/emoji.rs`: `custom_by_name(custom, name) -> Option<&CustomEmoji>`, exact and case-sensitive, as Stoat names are.
- Tests: `tests/emoji.rs` (`custom_by_name`).

**Depends on:** None.

**Risks:**
- **Observe frequency:** the textarea notifies on cursor blink and selection drags. Comparing cursors keeps this cheap, and the work is skipped when nothing moved. Check there's no flicker or redraw storm (watch for constant `Complete` events in debug logs).
- **Stale replacement:** the `ClosedShortcode` round trip runs a frame later. If the user keeps typing meanwhile, the range may no longer hold `:code:`, so check the text before replacing.
- **Up and Down while open:** cursor observation must not reopen or reset the selection when the list's own keys didn't move the cursor. Since those keys stop propagation, the cursor doesn't move.

**How to verify:**
- `cargo test --test emoji` covers `custom_by_name` (exact match, no case folding, missing).
- `cargo test --test complete` still passes, since the query rules are unchanged.

**Manual check:**
- Type `@ad`, press ← twice, then → twice: the list closes and reopens.
- Click into the middle of an earlier `@bo` word: the list opens for it.
- In a server with a custom emoji `party`, type `:party:`: it becomes the emoji chip.
- Typing `:smile:` still gives 😄.

**Shippable when:** The suggestion list always matches the word at the cursor, and exact custom emoji names convert like Unicode shortcodes.

### Chunk 6: Default skin tone
**Goal:** The user picks a skin tone once in the emoji picker. Every toneable emoji they insert uses it, and it's remembered across launches.

**Files likely touched:**
- `src/prefs.rs` (new, pure plus file IO, like `places.rs`):
  - `Prefs { skin_tone: Tone }`, with serde, saved as `prefs.json` in the app folder.
  - `Tone` is our own enum (`Default`, `Light`, `MediumLight`, `Medium`, `MediumDark`, `Dark`), mapped to `emojis::SkinTone`.
  - `PrefsFile::{load, save}` is tolerant of a missing or corrupt file (it gives the defaults).
- `src/emoji.rs`:
  - `EmojiChoice::Unicode` keeps the base emoji. `toned(tone) -> &'static str` uses `with_skin_tone` when `skin_tones()` exists, else the base.
  - `search`, `groups` and `picker_sections` stay tone-free. Callers apply the tone when drawing and inserting.
  - `RecentEmoji` stores base emoji, so changing the tone recolours recent ones too.
  - `by_shortcode(code, tone)`.
- `src/ui/emoji_picker.rs`:
  - A row of 6 tone swatches (✋ in each tone) beside the search field, with the chosen one highlighted.
  - Cells draw `toned(tone)`.
  - Choosing a tone calls back to the composer.
- `src/ui/composer.rs`: holds the tone, uses it when inserting (picker, suggestions, closing colon), and passes it to the picker. It's saved through `ChatView`, which owns the app folder paths.
- `src/ui/chat.rs`: loads prefs at start, gives the tone to the composer, and saves on change.
- `src/main.rs` or `src/ui/app_root.rs`: where prefs are loaded, next to the places file.
- Tests: `tests/emoji.rs` (tones), and `src/prefs.rs` unit tests (round trip, corrupt file).

**Depends on:** Chunk 1 (the app folder ID; otherwise it's independent).

**Risks:**
- **Two-person and multi-tone emoji:** `with_skin_tone` may return `None` for a tone on emoji with mixed-pair variants. Fall back to the base emoji, and test 🤝 and 👫.
- **Matching tone variants:** completion results show the toned glyph, but matching stays on the shortcodes, so `:thumbsup` still matches. Recently used and confirmed reactions store the base, so a toned emoji typed by hand (😀 isn't toneable; 👍🏽 pasted) stays as typed.
- **Font coverage:** toned emoji render through the system emoji font; nothing to do.

**How to verify:**
- `cargo test --test emoji` covers:
  - 👍 with `Medium` gives 👍🏽, and 😄 with any tone is unchanged.
  - 🤝 with `Dark` gives the single-tone handshake or the base (no panic).
  - `by_shortcode("wave", Light)` gives 👋🏻.
  - Recently used keeps the base.
- `cargo test --lib prefs` covers a round trip, a missing file and a corrupt file, each giving the defaults.

**Manual check:**
- Open the picker, choose the medium tone, and pick 👍: 👍🏽 is inserted.
- `:wave` in completion shows and inserts 👋🏽, and `:thumbsup:` converts to 👍🏽.
- Quit and relaunch: the tone is still selected.

**Shippable when:** One tone choice in the picker applies everywhere emoji are inserted, and survives restarts.

### Chunk 7: Stream uploads from disk; clean up pasted files
**Goal:** Uploads read files in chunks as they're sent instead of loading them whole. Pasted screenshots don't pile up in the temp folder.

**Files likely touched:**
- `Cargo.toml`: the tokio `fs` feature.
- `src/api/client.rs`:
  - `upload_attachment` takes `path: &Path`.
  - It opens the file with `tokio::fs::File` and reads its length from metadata at the start.
  - It streams 64 KB reads (`futures::stream::try_unfold` over `AsyncReadExt::read`) with the same progress callback.
  - It fails with an error mapped to "couldn't read it" if the file ends early.
- `src/ui/chat.rs`:
  - `upload_file` passes the path (no `spawn_blocking` read).
  - After confirmation or discard, it deletes owned temp files.
  - At startup, it calls the sweep of stale paste folders.
- `src/draft.rs`:
  - `DraftAttachment.temporary: bool`, set by `save_pasted_image`.
  - `remove_temporary(&DraftAttachment)`, which ignores missing files.
  - `sweep_paste_dirs(tmp, own_pid, now, max_age)` returns and removes stale `rustoat-pasted-*` folders. It's pure apart from the file system, so it's testable in a temp dir.
- `src/store.rs`:
  - `apply_message` returns or records the confirmed pending message's attachments: `take_confirmed_files()`, drained by `ChatView` after each event.
  - `discard_pending` returns the discarded message.
- `src/ui/composer.rs`: removing a temporary file from the tray deletes it. Discarded drafts and edits never delete files still referenced elsewhere.
- Tests: `tests/api_upload.rs` (upload from a temp file, progress up to the file size, a file that's missing at start or truncated), and `tests/store_pending.rs` (confirmed and discarded attachments are handed back; temp files only).

**Depends on:** None (it builds on milestone 4).

**Risks:**
- **`Part::stream_with_length` needs the exact length.** If the file changes size mid-upload, the request breaks. Read exactly `len` bytes, and stop with an error at early EOF. Treat that as a failed upload; Retry re-reads the file.
- **Deleting too early:** a retry after an answered failure re-uploads, so keep files until the message is confirmed or discarded, never on upload success alone.
- **Confirmation by echo or history:** confirmation can come from the POST response, the live echo or a history load (`confirm_pending_in_history`). All three must hand back the files.
- **The sweep must not delete another running RuSToat's folder.** Only folders older than 24 h are removed, and never the current pid's.

**How to verify:**
- `cargo test --test api_upload` covers:
  - A 200 KB temp file uploads with the right body bytes, and progress ends at `(len, len)`.
  - A missing file is an error before any request (the mock expects 0 calls).
  - A file truncated after the length was read is an error.
- `cargo test --test store_pending`:
  - Confirming a pending message (by the POST response, the echo and a history load) hands back its temporary files exactly once.
  - Discarding hands them back.
  - Non-temporary files are never handed back.
- `cargo test --lib draft`: the sweep removes old foreign folders and keeps new ones and its own.

**Manual check:**
- Paste a screenshot and send it. `ls $TMPDIR/rustoat-pasted-*` no longer lists it once the message is confirmed.
- Paste one and remove it with ×: its file is gone.
- Upload a large (about 15 MB) video. Activity Monitor shows memory doesn't jump by the file size.

**Shippable when:** Uploads use constant memory whatever the file size, and pasted files are deleted once they're no longer needed.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`. For scripts, also run `bash -n scripts/*.sh`, and `shellcheck scripts/*.sh` if installed. Optional live checks: `cargo test -- --ignored`.

### Unit Tests
- **Migration:** the legacy Keychain item moves to the new service once, and the new one wins if both exist (in-memory `TokenStore`). The Application Support folder is renamed only when the new one is missing.
- **Events:** `ServerMemberJoin` (with and without `member`), `ServerMemberLeave`, `ServerMemberUpdate` and `UserUpdate` decode, `clear` lists included.
- **Live store updates:** nickname and display-name changes reach `name_in`, `author_name` and labels. Avatar clears fall back to the default. The user leaving removes the server. Presence-only updates rebuild nothing.
- **Emoji:** `custom_by_name`, `toned` (single and two-person emoji, and non-toneable ones), `by_shortcode` with a tone, and recently used keeping bases.
- **Prefs:** round trip, missing file and corrupt file.
- **Temp files:** files handed back on confirm and discard, temporary files only, and the sweep's age and pid rules.

### Integration Tests
- **`wiremock`:** a streamed upload from disk (the exact body, progress), a missing file, and a file truncated mid-upload.
- **Store pipelines:**
  - A pending message with a pasted image, confirmed by echo, hands back the temp file once.
  - A member join, then `@`, includes them.
  - The user leaving, then the sidebar, drops the server.
- **Scripts:** `scripts/bundle.sh` then `codesign -dv` shows the identity and ID. `scripts/release.sh` gives a DMG that passes `hdiutil verify`, with `codesign --verify --deep --strict` passing on its app.

### Manual Tests
1. **Signing:** run bundled and with `cargo run`, rebuild, run again: no Keychain prompt after the first "Always Allow". Your session, places and notification permission (asked again once) carry over.
2. **Icon:** the Dock, Finder, banners and the DMG window show the cyborg Stoat, and it reads at small sizes.
3. **Release:** `scripts/release.sh`, then mount the DMG, install to /Applications, launch, sign in, quit, and delete the copy.
4. **Live updates:** change your nickname and avatar in the web app, have someone join, leave a test server. RuSToat follows each without a restart.
5. **Cursor:** move the cursor in and out of `@ad` with the arrows and the mouse. The list follows.
6. **Custom emoji:** `:party:` becomes the custom emoji, and `:smile:` still becomes 😄.
7. **Skin tone:** set a tone in the picker. 👍, 👋 and `:wave:` use it, and it survives a relaunch.
8. **Uploads:** a 15 MB upload keeps memory flat. Pasted screenshots disappear from `$TMPDIR` after sending or removing.
9. **Light and dark mode:** the tone swatches.

## Definition of Done
- [ ] **ID and migration:** the bundle and `cargo run` binary are signed with "Apple Development: Christopher Hultin" and the identifier `com.chrishultin.rustoat` (`codesign -dv`). Rebuilds don't trigger Keychain prompts. The session and places migrate from the `chat.stoat.*` names (tested), and the session name is "RuSToat".
- [ ] **Icon:** a cyborg-Stoat `RuSToat.icns` with all 10 iconset sizes, generated from `assets/icon/rustoat.svg` by `scripts/make-icon.sh`, and shown by the bundle.
- [ ] **Release:** `scripts/release.sh` produces `dist/RuSToat-<version>.dmg`, hardened runtime, passing `hdiutil verify` and `codesign --verify --deep --strict`. It installs and runs from /Applications. Notarization runs when `RUSTOAT_NOTARY_PROFILE` and a Developer ID identity exist (the dry run is checked), and the README documents how to enable it.
- [ ] **Live updates:** `ServerMemberJoin`, `ServerMemberLeave`, `ServerMemberUpdate` and `UserUpdate` are decoded and applied (tested). Names, nicknames and avatars update live, joins and leaves update `@`, and leaving a server removes it.
- [ ] **Cursor:** the suggestion list re-evaluates on cursor moves, by keyboard and mouse.
- [ ] **Custom `:name:`:** an exact custom emoji name converts to its chip on a typed closing colon.
- [ ] **Skin tone:** a default tone chosen in the picker applies to the picker, completion and conversion, and persists in `prefs.json` (tested).
- [ ] **Uploads and temp files:** uploads stream from disk with progress, and missing or truncated files fail cleanly (tested). Pasted temp files are deleted on confirm, discard or removal, and stale folders are swept at launch (tested).
- [ ] **Checks:** `cargo test && cargo clippy --all-targets -- -D warnings` passes, and `cargo test -- --ignored` passes.
