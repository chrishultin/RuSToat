# Implementation Record: Settings window and themes (milestone 13)

_Plan: docs/plans/settings-and-themes.md_
_Started: 2026-11-06_

Branch `feature/settings-and-themes`, cut from `feature/reading-flow` at f5314ca (milestone 9's shortcuts are on that branch, not yet on `main`). Supervision: continuous.

Pre-existing uncommitted changes kept out of every commit: `.gitignore` (a duplicated Riptide `.humanlayer/tasks/` entry) and `.shadow/`. The plan file and its link in `docs/plans/roadmap.md` go in with chunk 1.

## Chunk 1: Settings window with Keyboard Shortcuts

### Changes Made
- `src/ui/actions.rs` (new): `Quit`, `LogOut`, `AddAccount`, `OpenSettings`, `SwitchAccount1`–`9` (namespace `stoat`), `switch_account_action(index)`, and `app_key_bindings()` (⌘Q, ⌘,, ⌘1–⌘9, all without a key context). Moved out of `src/main.rs`.
- `src/ui/chat.rs`: `key_bindings()` returns the chat shortcuts; `init` binds them. Same bindings and contexts as before.
- `src/shortcuts.rs` (new): `Group` (Navigation, Reading, Accounts, App, with `ALL` and `label()`), `Keys` (`One(MakeAction)` or `Range(first, last)`), `Shortcut { group, description, keys }` with `actions()`, the `SHORTCUTS` table and `in_group(group)`.
- `src/ui/settings.rs` (new): `SettingsView { root: WeakEntity<AppRoot>, pages: Vec<PageFn> }` where `PageFn = fn(&SettingsView, &mut App) -> SettingPage`; `SettingsView::app_pages()` gives the app's sections (just `shortcuts_page` for now); `root()` upgrades the weak handle for later per-account sections. `shortcuts_page` renders one titled `SettingGroup` per `Group`, each row a `SettingItem::render` with the description and the group label as search keywords. Keys come from `cx.key_bindings()` (`bindings_for_action(..).next_back()`, the binding that wins) and draw as `Kbd`; a range shows "first – last"; an unbound action shows "Not bound".
- `src/main.rs`: imports the actions from `ui::actions`; binds `app_key_bindings()`; handles `OpenSettings`; the Stoat menu is Add Account…, Log Out, separator, Settings…, separator, Quit Stoat. New `SettingsWindow(Option<WindowHandle<Root>>)` global with `open_or_focus` (deferred via `cx.defer`, so ⌘, pressed inside the settings window doesn't re-enter it; activates the existing window if its handle still updates, else opens a new 760×560 centred window titled "Settings").
- `src/ui/mod.rs`, `src/lib.rs`: the new modules.
- `src/assets.rs`: Lucide `Keyboard` added to `ExtraIcons` (the default set lacks it), with a `settings_icons_load` test.
- `tests/shortcuts.rs` (new): every listed action is bound in a `Keymap` built from `chat::key_bindings()` + `actions::app_key_bindings()`; descriptions are unique; every group is non-empty and every shortcut is in a group; the account range is ⌘1 to ⌘9 and Settings is ⌘,.

### Divergences
- Minor: added the `Keyboard` icon to `src/assets.rs` (not listed in the plan) so the sidebar icon isn't blank.
- Minor: each direction is its own row ("Previous channel ⌥↑", "Next channel ⌥↓") rather than one "⌥↑/↓" row, so each is searchable.
- Minor: gpui-component's `Kbd` formats Esc as ⎋ on macOS, not "Esc".
- Risk checks: `rust_i18n` falls back to `en` and gpui-component never sets a locale, so `Settings`' built-in strings ("Search...") are English with no change. `MainWindow::window_id` and `on_window_closed` are untouched and only react to the main window.
- Observed: closing the Settings window logs `ERROR handling active status change for window WindowId(2v1): window not found`. That's GPUI's own `on_active_status_change` handler (gpui-pre 0.3.11 `window.rs:2178`) firing after the window is gone; it logs and does nothing else. Harmless; not ours to fix.

### Verification Performed
- `cargo test --test shortcuts`: 4 passed.
- `cargo test`: 603 passed, 0 failed.
- `cargo clippy --all-targets`: no warnings (a single-element loop in the new icon test was rewritten).
- Formatting: `rustfmt --edition 2024` on the new files and `src/main.rs` (clean before); `src/ui/chat.rs` drift count unchanged at 17.

### Manual Validation
Run with `scripts/run-bundled.sh` (two accounts signed in). The user confirmed:
- ⌘, opens Settings; ⌘, again brings the same window forward.
- The shortcut list shows the right keys for every row.
- The Stoat menu shows "Settings… ⌘,"; the Accounts menu still shows ⌘1, ⌘2.
- Closing Settings leaves the main window and accounts alone.

## Chunk 2: Themes

### Changes Made
- `assets/themes/{ayu,catppuccin,everforest,flexoki,gruvbox,macos-classic,solarized}.json` (new): vendored unchanged from gpui-component commit `48ac25b50c81ca5e34ec1631e3efabe22f4af64a` (`main`, 2026-11-06). `assets/themes/README.md` names the source, commit and licences (Apache-2.0 files, MIT palettes).
- `src/prefs.rs`: new `Palette` (8 variants; `ALL`, `label()`, `from_label()`, `light_name()`, `dark_name()`) and `Appearance` (`System`, `Light`, `Dark`; `ALL`, `label()`, `from_label()`) serde enums; `Prefs.palette` and `Prefs.appearance`, defaulting to Default / System through the struct's `#[serde(default)]`. Two new tests: a pre-themes file loads with Default / System and keeps its other fields; labels round-trip.
- `src/themes.rs` (new): `BUNDLED` (file name, `include_str!` contents), `load_bundled(&mut ThemeRegistry)`, `register(cx)` (logs a file that fails), `apply(palette, appearance, cx)` (sets `Theme::light_theme` / `dark_theme` to the palette's configs, falling back to the defaults with a warning if a name is missing, then `Theme::change(mode)`, where System reads `cx.window_appearance()`), and `apply_current(cx)` (from `AppPrefs`). Re-exports `Palette` and `Appearance`.
- `src/ui/app_prefs.rs` (new): `AppPrefs` global with `init(cx)` (loads `prefs.json`), `get(cx)`, and `update(cx, |prefs| ..)` (saves and notifies `observe_global` observers only when something changed).
- `src/ui/app_root.rs`: dropped `prefs` / `prefs_file`; skin tone and member-list state are read and written through `AppPrefs`; the window-appearance observer (and the call in `new`) is `themes::apply_current` instead of `Theme::sync_system_appearance`.
- `src/ui/settings.rs`: `appearance_page` (first in the sidebar, `Palette` icon) with Palette and Appearance dropdowns (`SettingField::dropdown`, labels as values, defaults Default / System so the page's reset button works). `SettingsView::new` now takes the window and context: it re-renders when `AppPrefs` changes and re-applies the theme when macOS's appearance changes, so System works with the main window closed.
- `src/main.rs`: after `gpui_kit::init`, `themes::register`, `AppPrefs::init`, `themes::apply_current` (before any window opens), and an `observe_global::<AppPrefs>` that re-applies the theme only when palette or appearance changed.
- `src/assets.rs`: `settings_icons_load` also checks `Palette` (a default icon).
- `src/lib.rs`, `src/ui/mod.rs`: the new modules.
- `tests/themes.rs` (new): each bundled file parses into a `ThemeRegistry::default()`; each non-Default palette's light and dark names exist with the right mode (Default's are gpui-component's built-ins, which aren't reachable without an `App`, so the test pins their names); 8 palettes with unique labels.

### Divergences
- Minor: `Palette` and `Appearance` live in `src/prefs.rs` (plain data next to `Tone`) rather than `src/themes.rs`; `themes` re-exports them.
- Minor: the Settings window also observes the window appearance (not in the plan), so Appearance System follows macOS with the main window closed.
- Risk resolved: every palette file omits the sidebar colours and most omit `danger` / `link`, but gpui-component 0.9 derives missing colours from the palette's own (`apply_config`'s fallbacks), not from the default theme. No vendored file was edited.
- Risk resolved: the `Theme` global already exists after `gpui_kit::init` (its `theme::init` calls `Theme::change`), so `apply` sets the configs then calls `Theme::change` once.

### Verification Performed
- `cargo test --test themes`: 3 passed.
- `cargo test`: 608 passed, 0 failed.
- `cargo clippy --all-targets`: no warnings.
- `rustfmt --check` drift unchanged (0) in `src/ui/app_root.rs`, `src/prefs.rs`, `src/assets.rs`, `src/main.rs`; new files formatted.
- Launch log shows no "couldn't load a bundled theme" error.

### Manual Validation
Run with `scripts/run-bundled.sh`. The user checked the palettes and appearances in Settings and reported it looks good. The relaunch checks (palette and appearance kept with no flash of Default; skin tone and member list remembered) were waived by the user; persistence is covered only by the `prefs.rs` unit tests and the unchanged `PrefsFile` save path.

## Chunk 3: Keep running when the window closes

### Changes Made
- `src/prefs.rs`: `Prefs.keep_running: bool`, default `true` (through `Default` and the struct's `#[serde(default)]`). The pre-milestone-13 file test (renamed `a_file_from_before_milestone_13_gets_its_defaults`) checks it loads `true` and that `false` round-trips.
- `src/window_hide.rs` (new): `hide(window) -> bool`. On macOS it takes the window's `raw_window_handle` (`HasWindowHandle::window_handle`, called through the trait because GPUI's `Window` has its own `window_handle()`), casts the AppKit handle's `ns_view` to `NSView`, gets its `NSWindow` and calls `orderOut:`. Returns `false` (with a warning) if any step fails; elsewhere it always returns `false`.
- `Cargo.toml`: `objc2-app-kit` gains the `NSView` and `NSWindow` features; new `raw-window-handle = "0.6"` (already in the tree via GPUI), both macOS-only.
- `src/main.rs`:
  - `MainWindow::open` registers `window.on_window_should_close`: with `keep_running` on it hides the window (falling back to `cx.hide()` if `window_hide::hide` fails), logs "main window hidden; still running", and returns `false`; off, it returns `true` and the window closes as before.
  - New `MainWindow::show`: activates the window (`activate_window`, which is `makeKeyAndOrderFront:`) and the app if it's open (hidden or not), otherwise opens a new one. `app.on_reopen` is now `MainWindow::show`.
  - Add Account… activates the window before showing the login screen.
  - The `on_window_closed` comment notes it only runs for a real close.
- `src/ui/settings.rs`: `app_page` (between Appearance and Keyboard Shortcuts, `Settings` icon) with the "Keep running when the window is closed" switch and its description, default `true`.
- `src/lib.rs`: the new module. `src/assets.rs`: `settings_icons_load` also checks `Settings`.

### Divergences
- None to the approach: `orderOut:` through the raw handle works, so the `cx.hide()` fallback (assumption 5) isn't used in practice.
- Known limitation, not addressed: macOS sends `applicationShouldHandleReopen` (GPUI's `on_reopen`) only when the app has no visible windows. With the main window hidden and Settings open, clicking the Dock icon doesn't show the main window (Add Account… or a banner still do). Likewise ⌘1–⌘9 pressed in Settings while the main window is hidden switches the account without showing it.
- Risk checks: GPUI's macOS key-status callback is spawned, not run inline, so `orderOut:` inside `windowShouldClose:` doesn't re-enter the window. A hidden window resigns key, so `is_window_active()` is false and nothing is marked read.
- The open question about banner clicks reopening a *closed* window is left as it was: with Keep running on (the default) the window is only hidden, so the click already brings it back.

### Verification Performed
- `cargo build`: clean.
- `cargo test`: 608 passed, 0 failed.
- `cargo clippy --all-targets`: no warnings.
- `rustfmt --check` drift 0 in `src/main.rs`, `src/prefs.rs`, `src/assets.rs`; new file formatted.

### Manual Validation
Run with `scripts/run-bundled.sh`, two accounts signed in. The log shows the red button hiding the window ("main window hidden; still running") with no errors. The user reported that the keep-running behaviour looks good. The log doesn't show a real close with the switch off ("main window closed" never appears), so the "switch off restores the old close" check wasn't exercised in this run.

## Chunk 4: Start at login

### Changes Made
- `Cargo.toml`: `objc2-service-management = "0.3"` (macOS only), default features off, with `std`, `objc2`, `objc2-foundation` and `SMAppService`.
- `src/login_item.rs` (new): `LoginItem` (`On`, `Off`, `NeedsApproval`, `Unavailable(Unavailable)`), where `Unavailable` is `NotBundled`, `OldMacOs` or `UnknownStatus`, each with a `note()` for the greyed-out switch. On macOS:
  - `from_status(SMAppServiceStatus)` is the pure mapping;
  - `status()`, `set(on)` (register / unregister, with the `NSError`'s localized description as the error) and `open_system_settings()` all check the bundle identifier (`banners::bundle_identifier`) and `objc2::available!(macos = 13.0)` first.
  - Other platforms get stubs that report `NotBundled`.
  - Two unit tests: every status maps (plus an unknown raw value), and outside the bundle `status()` is `NotBundled` and `set` fails.
- `src/ui/settings.rs`: the App page gains "Open RuSToat at login". It reads `login_item::status()` on every render (nothing in `prefs.json`), with a note that changes with the state. While it waits for approval, an extra row has an "Open System Settings" button. It's disabled with the reason when unavailable, and a failed `set` shows an error toast in the front window (deferred). The App page also re-adds `default_value(true)` to the Keep running switch.
- `src/lib.rs`: the new module.

### Divergences
- Minor: `SMAppServiceStatus::NotFound` maps to `Off`, not `Unavailable`. macOS reports it for an app that has never been registered, and registering it works.
- Minor: `Unavailable` carries a reason (`NotBundled`, `OldMacOs`, `UnknownStatus`), so the note can say why. The bundle's `LSMinimumSystemVersion` is 11.0, so macOS 11–12 get a greyed-out switch instead of a crash.

### Verification Performed
- `cargo test --lib login_item`: 2 passed.
- `cargo test`: 610 passed, 0 failed, 5 ignored (all pre-existing).
- `cargo clippy --all-targets`: no warnings.
- New files formatted; `src/ui/chat.rs` drift unchanged at 17.

### Manual Validation
None. The user waived the login-item checks (System Settings › Login Items both ways, a login launch, and the greyed-out switch under `cargo run`). The greyed-out state outside the bundle is covered by the unit test.

## Summary

_Completed: 2026-11-06_

### Chunks Completed
1. Settings window with Keyboard Shortcuts (36004f5)
2. Themes (4e25fec)
3. Keep running when the window closes (1b8d2b0)
4. Start at login (this commit)

### Divergences
- Chunk 1: the Lucide `Keyboard` icon was added to `src/assets.rs`; each shortcut direction is its own row; Esc shows as ⎋. Closing Settings logs a harmless GPUI "window not found" error.
- Chunk 2: `Palette` and `Appearance` live in `src/prefs.rs`; the Settings window also follows macOS's appearance; no vendored theme needed edits, because gpui-component derives missing colours from each palette's own.
- Chunk 3: no change to the approach. Known limitation: with the main window hidden and Settings open, clicking the Dock icon doesn't show the main window, and ⌘1–⌘9 pressed in Settings switches the account without showing it.
- Chunk 4: `NotFound` counts as off; `Unavailable` carries a reason.

### Definition of Done
- [x] ⌘, and Stoat › Settings… open a single settings window with a sidebar of Appearance, App and Keyboard Shortcuts. (Manually confirmed.)
- [x] The Keyboard Shortcuts page lists every shortcut, with keys read from the keymap; `tests/shortcuts.rs` fails if a listed shortcut isn't bound.
- [~] Eight palettes × System / Light / Dark apply app-wide at once (manually confirmed); `tests/themes.rs` covers every palette loading. Persistence across relaunch is unit-tested only; the user waived the relaunch check.
- [~] Skin tone and member-list preferences go through `AppPrefs` and the unchanged `PrefsFile`; not re-checked across a relaunch (waived).
- [~] Keep running: hiding on close confirmed by the user and the log. The switch-off path (real close, badge cleared) wasn't exercised.
- [~] "Open RuSToat at login": implemented, and the greyed-out case outside the bundle is unit-tested; the System Settings checks were waived.
- [x] `cargo test` (610 passed) and `cargo clippy --all-targets` pass with no new warnings.
- [x] Every chunk's manual check is recorded here, including the ones waived.

### Remaining Manual Steps
- Optional: relaunch to confirm the palette, appearance, skin tone and member list are kept, with no flash of Default.
- Optional: with Keep running off, close the window and check that the badge clears and the Dock icon opens a fresh window.
- Optional: in the bundle, check that the login-item switch matches System Settings › Login Items both ways.
