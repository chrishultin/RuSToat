# Plan: Settings window and themes (RuSToat milestone 13)

## Summary
RuSToat gets a settings window, opened with ⌘, or from the Stoat menu, with a sidebar of sections. Later milestones (14–17, 23–28, 29) add their own sections to it. This milestone fills it with:
- **Appearance:** pick one of eight colour palettes, and System, Light or Dark.
- **App:** keep running when the window is closed, so accounts stay connected and notifications keep coming; and start RuSToat at login.
- **Keyboard Shortcuts:** every shortcut, listed from the keymap itself. This answers milestone 9's open question.

## Approach
Everything builds on what's already there. The only new dependency is Apple's ServiceManagement bindings.
- **The window is gpui-component's `Settings` component.** gpui-component 0.9 ships `gpui_kit::component::setting`: `Settings` → `SettingPage` → `SettingGroup` → `SettingItem`. It has a sidebar of pages, and switch and dropdown fields. Custom rows use `SettingItem::render`. Building our own sidebar and layout was the alternative, but it would only re-create this.
- **Sections are functions.** `src/ui/settings.rs` has a `SettingsView`, the window's root view, whose `pages()` is a list of functions, each returning a `SettingPage`: `appearance_page`, `app_page`, `shortcuts_page`. A later milestone adds a section by adding one function.
  - `SettingsView` holds a `WeakEntity<AppRoot>`, so that per-account sections (profile, sessions) can reach the account on screen later. This milestone doesn't use it.
  - Milestone 23's server settings reuse the same window code with a different list of pages. Keep the page list a field set when the view is created, not a constant.
- **The settings window is a second GPUI window,** managed like `MainWindow` in `src/main.rs`: a `SettingsWindow` global. ⌘, opens it, or brings it forward if it's already open.
- **App-level actions move into the library.** `Quit`, `LogOut`, `AddAccount` and `SwitchAccount1`–`9` are declared in `src/main.rs` today, which the library can't name. They move to `src/ui/actions.rs` with the new `OpenSettings`. Their key bindings become a function returning `Vec<KeyBinding>`, like a new `chat::key_bindings()`. That way:
  - the Keyboard Shortcuts page can look up each action's keys in the live keymap (`cx.key_bindings().borrow().bindings_for_action(..)`), instead of a hand-written copy that would drift;
  - a plain unit test can build a `Keymap` from those lists and check that every listed shortcut is bound. No GPUI test harness is needed; the repo has none.
- **Preferences become app-wide.** `AppRoot` owns `Prefs` (skin tone, member list) today. With two windows reading and writing them, `Prefs` moves into a GPUI global, `AppPrefs` in `src/ui/app_prefs.rs`, which loads `prefs.json` once, saves on every change, and is observed by whoever cares. `prefs.rs` stays plain data with no GPUI, and gains the new fields with `#[serde(default)]`.
- **Themes come from gpui-component's theme registry.** The pinned crate ships only "Default Light/Dark". The other seven palettes are upstream gpui-component theme files (Apache-2.0; the palettes themselves are MIT). They're vendored into `assets/themes/`, compiled in with `include_str!`, and registered with `ThemeRegistry::load_themes_from_str` at startup. All seven were checked on 2026-11-06 to parse with gpui-component 0.9.
  - Applying a choice sets `Theme::light_theme` and `Theme::dark_theme` to the palette's two configs, then calls `Theme::change(mode)`, where the mode is the chosen appearance or the system's.
  - `AppRoot`'s existing `observe_window_appearance` → `Theme::sync_system_appearance` becomes a call to that same `themes::apply`.
- **Keep running means hiding the window, not closing it.** Each account's event loop runs on the window's tasks (`cx.spawn_in(window, …)` in `ChatView::new`), so a closed window takes the connections with it. Moving them out of the window would be a large refactor for no visible gain. Instead:
  - `window.on_window_should_close` hides the main window (AppKit `orderOut:` on its `NSWindow`, reached through the window's raw handle) and refuses the close.
  - Hidden, the window isn't active. So the existing rules already apply: nothing is marked read, and banners fire as for any background window.
  - The Dock icon (`on_reopen`), a banner click (which already calls `window.activate_window()`) and Add Account… bring it back. ⌘Q still quits.
  - With the switch off, closing behaves as today (commit 9520c37): the window closes and every account disconnects.
- **Start at login uses `SMAppService.mainAppService`** (macOS 13+), through `objc2-service-management` 0.3, which matches the objc2 0.3 crates already in use. It needs the signed app bundle, so under a plain `cargo run` the switch is shown greyed out with a note saying so. `src/login_item.rs` wraps it: status, register and unregister. The UI state comes from a pure mapping that can be tested.

## Assumptions
1. **"Keep running when the window is closed" is a switch, on by default.** Off means today's behaviour. ⌘Q always quits. (Confirmed.)
2. **Starting at login opens the window as usual.** There's no "start hidden" option. (Confirmed: option A.)
3. **There are eight palettes,** each with Appearance System / Light / Dark (confirmed):

   | Palette | Light | Dark |
   |---|---|---|
   | Default | Default Light | Default Dark |
   | Ayu | Ayu Light | Ayu Dark |
   | Catppuccin | Catppuccin Latte | Catppuccin Mocha |
   | Everforest | Everforest Light | Everforest Dark |
   | Flexoki | Flexoki Light | Flexoki Dark |
   | Gruvbox | Gruvbox Light | Gruvbox Dark |
   | macOS Classic | macOS Classic Light | macOS Classic Dark |
   | Solarized | Solarized Light | Solarized Dark |

   Catppuccin's file also has Frappé and Macchiato; they're registered but not offered. The defaults are Default and System, which is what RuSToat does today.
4. **Settings are app-wide, not per account.** One theme and one close behaviour for every account.
5. **Hiding hides only the main window.** RuSToat stays in the Dock and the menu bar, with no menu-bar icon. If `orderOut:` through the raw handle doesn't work, the fallback is `cx.hide()`, which hides the whole app, as ⌘H does. Note it in the implementation record if that's needed.
6. **The shortcut list is grouped as Navigation, Reading, Accounts and App.** Its descriptions are written by hand in one table, but the keys always come from the keymap. A shortcut whose action somehow isn't bound shows "Not bound" rather than vanishing.
7. **The settings window opens about 760×560, centred,** titled "Settings". It is the only window besides the main one.

## Open Questions
- *(Can wait)* Should a Help menu also have "Keyboard Shortcuts…", opening the settings window on that page? `Settings::default_selected_index` makes it easy, but it isn't needed.
- *(Can wait)* Launching hidden at login was declined for now. If it's wanted later, it needs a way to tell a login launch apart from a normal one.
- *(Can wait)* Roadmap leftover: "clicking an old banner while the window is closed doesn't reopen it". With keep-running on (the default), the window is only hidden, so the click already brings it back. With it off, the leftover remains. Chunk 3 may fix it too if the banner handler moves to the app level cheaply; otherwise leave it.

## Plan

### Chunk 1: Settings window with Keyboard Shortcuts
**Goal:** ⌘, (or Stoat › Settings…) opens a settings window with a sidebar and one page, listing every keyboard shortcut.
**Files likely touched:**
- `src/ui/actions.rs` (new): `actions!(stoat, [Quit, LogOut, AddAccount, OpenSettings, SwitchAccount1, …, SwitchAccount9])`, plus `switch_account_action(index)` and `app_key_bindings() -> Vec<KeyBinding>` (⌘Q, ⌘,, ⌘1–⌘9), moved from `src/main.rs`.
- `src/ui/chat.rs`: `chat::init` binds `key_bindings()`, a new function returning its `Vec<KeyBinding>`.
- `src/shortcuts.rs` (new, no GPUI): `SHORTCUTS`, the list the page shows. Each entry has a group, a description and a function building its action (`fn() -> Box<dyn Action>`). ⌘1–⌘9 is one row showing ⌘1 and ⌘9.
- `src/ui/settings.rs` (new): `SettingsView` rendering `Settings::new("settings").pages(..)`. `shortcuts_page` renders each row with its keys from `cx.key_bindings()`, drawn with gpui-component's `Kbd`.
- `src/main.rs`:
  - uses `ui::actions`;
  - a `SettingsWindow` global with `open_or_focus(cx)`, which opens it or activates it;
  - "Settings…" (⌘,) in the Stoat menu, after a separator below Log Out.
- `src/ui/mod.rs`, `src/lib.rs`: the new modules.
- `tests/shortcuts.rs` (new).
**Depends on:** None
**Risks:**
- **`Settings` uses `rust_i18n` for its built-in strings** (for example search and empty states). Check they come out in English with no locale set. If not, set the locale to `en` at startup.
- **`Settings`' sidebar search** filters items by `keywords`. Give shortcut rows keywords (the description), or the search finds nothing.
- **Moving the actions renames them** in GPUI's registry (the namespace stays `stoat`, but the module path changes). Nothing persists action names, so this should be harmless. Still check the menu items show their shortcuts afterwards.
- **A second window and `MainWindow::update_root`:** the settings window must not be mistaken for the main one. `MainWindow::window_id` stays the main window's ID, and `on_window_closed` only reacts to that ID.
**How to verify:** `cargo test --test shortcuts`, with:
- every entry in `SHORTCUTS` resolves to at least one binding in a `Keymap` built from `app_key_bindings()` and `chat::key_bindings()`;
- descriptions are unique;
- every group is non-empty.

Then `cargo test && cargo clippy --all-targets`.
**Manual check:**
- ⌘, opens Settings. ⌘, again brings the same window forward rather than opening a second.
- The page lists ⌥↑/↓, ⌥⇧↑/↓, ⌘⌥↑/↓, Esc, ⇧Esc, ⌘1–⌘9, ⌘, and ⌘Q with the right keys.
- The Stoat menu shows "Settings… ⌘,". The Accounts menu still shows ⌘1, ⌘2.
- Closing Settings leaves the main window and accounts alone.
**Shippable when:** the window opens from both the keyboard and the menu, and its shortcut list matches what the keys actually do.

### Chunk 2: Themes
**Goal:** Settings › Appearance picks a palette and System / Light / Dark. The whole app recolours at once and remembers the choice.
**Files likely touched:**
- `assets/themes/{ayu,catppuccin,everforest,flexoki,gruvbox,macos-classic,solarized}.json` (vendored from `https://github.com/longbridge/gpui-component/tree/main/themes`), and `assets/themes/README.md` naming the source, the commit and the licences.
- `src/themes.rs` (new):
  - `Palette` (8 variants, with `ALL`, `label()`, and `light_name()` / `dark_name()` giving the theme names);
  - `register(cx)`, which loads each file into `ThemeRegistry`;
  - `apply(palette, appearance, cx)`.
- `src/prefs.rs`: `Prefs.palette: Palette` and `Prefs.appearance: Appearance` (`System`, `Light`, `Dark`), both `#[serde(default)]`. `Palette`/`Appearance` are our own serde types (as `Tone` is), so the file doesn't depend on theme names.
- `src/ui/app_prefs.rs` (new): the `AppPrefs` global, with `get(cx)`, `update(cx, |prefs| ..)` (which saves and notifies observers) and `observe`.
- `src/ui/app_root.rs`: drop its own `prefs` / `prefs_file`. Read and write skin tone and member-list state through `AppPrefs`. The window-appearance observer calls `themes::apply`.
- `src/ui/settings.rs`: `appearance_page`, with a Palette dropdown (or a row of swatches using each palette's `background` / `primary` colours) and an Appearance choice of three.
- `src/main.rs`: `themes::register(cx)` and `AppPrefs::init(cx)` after `gpui_kit::init`, and the first `themes::apply` before the main window opens, so there's no flash of the default theme.
- `tests/themes.rs` (new); prefs tests in `src/prefs.rs`.
**Depends on:** Chunk 1
**Risks:**
- **Palettes may lack colours RuSToat uses.** `chat.rs` draws with `sidebar`, `sidebar_accent`, `primary`, `danger`, `accent`, `link`, `secondary` and `muted_foreground`. A file without a key falls back to the default theme's colour, which may clash. Check each palette on the main screen (manual check), and override a colour in the vendored file if one is unreadable. Note any such edit in `assets/themes/README.md`.
- **Moving prefs to a global touches every reader.** Skin tone and the member list must still persist. Their existing behaviour is covered only by manual use, so re-check both.
- **The `Theme` global might not exist yet** when `apply` first runs. `Theme::change` creates it if missing, but `light_theme` / `dark_theme` must be set after it exists. Order: `register`, `Theme::change(mode)` once, set the configs, then `Theme::change` again (or `Theme::update`).
- **Upstream files are from gpui-component `main`, not 0.9.** They parse today. A future gpui-kit bump could reject a key, and the `tests/themes.rs` parse test will catch that.
**How to verify:** `cargo test --test themes`:
- every bundled file parses into a `ThemeRegistry::default()`;
- every `Palette` names one light and one dark theme that exist after registering, with the right `mode`;
- `Palette::ALL` has 8 entries with unique labels.

Prefs unit tests: a file without `palette` / `appearance` loads with Default / System, and new values round-trip. Then `cargo test && cargo clippy --all-targets`.
**Manual check:**
- Pick each palette with Appearance Light, then Dark. The chat, sidebar, menus and Settings window all recolour at once and stay readable: unread names, mention badges, the "New" divider, links and code blocks.
- Appearance System follows a change in macOS's own setting while RuSToat is open.
- Quit and relaunch: the palette and appearance are kept, with no flash of Default.
- The skin tone and member-list toggle are still remembered across a relaunch.
**Shippable when:** all eight palettes work in both modes and survive a relaunch, and the existing prefs still persist.

### Chunk 3: Keep running when the window closes
**Goal:** By default, closing the main window hides it. Accounts stay connected, banners and the Dock badge keep working, and the Dock icon brings the window back.
**Files likely touched:**
- `src/prefs.rs`: `Prefs.keep_running: bool`, default `true`.
- `src/ui/settings.rs`: `app_page`, with "Keep running when the window is closed" and a description ("Stay connected and keep notifying. Quit with ⌘Q.").
- `src/main.rs`:
  - `MainWindow::open` registers `window.on_window_should_close`: if `AppPrefs` has `keep_running`, hide the window and return `false`.
  - `on_reopen` shows a hidden window (`MainWindow::update(.., window.activate_window())`), not only a closed one.
  - Add Account… does the same.
  - `on_window_closed` keeps clearing the badge only when the window really closes.
- `src/window_hide.rs` (new, or a function in `dock.rs`): `hide(window)`, which reaches the `NSWindow` through the window's raw window handle and calls `orderOut:`. `objc2-app-kit` gains the `NSWindow` and `NSView` features.
**Depends on:** Chunk 2 (`AppPrefs`, and the settings page pattern)
**Risks:**
- **Reaching the `NSWindow`:** GPUI's `Window` may not expose its raw handle in this snapshot. If it doesn't, fall back to `cx.hide()` (assumption 5) and say so in the record.
- **A hidden window still re-renders and runs `maybe_ack_current`.** It must not mark anything read. `window.is_window_active()` should be false while hidden. Confirm with a hidden window and a new message: it stays unread and a banner shows.
- **The badge:** today `on_window_closed` clears it. With the window hidden it must keep counting, and the close handler must not run at all.
- **Banner clicks** call `window.activate_window()`. Confirm that brings an ordered-out window back. If not, `orderFront:` explicitly.
**How to verify:** `cargo build`, `cargo test && cargo clippy --all-targets`. This chunk is mostly platform glue. If a small pure helper emerges (for example `CloseBehaviour::for(prefs)`), test it.
**Manual check** (run the signed bundle, `scripts/run-bundled.sh`, for banners):
- Close the window with the red button. RuSToat stays in the Dock.
- Have the second account send a DM. A banner shows and the Dock badge counts it.
- Click the banner: the window comes back on that DM.
- Close the window again, then click the Dock icon: it comes back where it was, and nothing was marked read while hidden.
- Turn the switch off and close the window. The badge clears and the accounts disconnect, as before. The Dock icon opens a fresh window.
- ⌘Q quits in both cases.
**Shippable when:** with the default setting, closing the window never drops a connection or a notification, and the old behaviour is one switch away.

### Chunk 4: Start at login
**Goal:** Settings › App has "Open RuSToat at login", backed by macOS's own login items.
**Files likely touched:**
- `Cargo.toml`: `objc2-service-management = { version = "0.3", features = ["SMAppService"] }`.
- `src/login_item.rs` (new):
  - `status() -> LoginItem` (`On`, `Off`, `NeedsApproval`, `Unavailable`), mapped from `SMAppServiceStatus` by a pure `from_status` function;
  - `set(on: bool) -> anyhow::Result<()>` (register or unregister);
  - `open_system_settings()` (`SMAppService::openSystemSettingsLoginItems`);
  - `Unavailable` when not running from a bundle (no `NSBundle.mainBundle.bundleIdentifier`, as `banners::available` checks).
- `src/ui/settings.rs`: the switch on `app_page`.
  - It reads `status()` when the page renders: the OS is the source of truth, so nothing goes in `prefs.json`.
  - `NeedsApproval` shows "Approve RuSToat in System Settings › Login Items" with a button that opens it.
  - `Unavailable` greys the switch out with "Only available in the RuSToat app bundle".
  - Errors from `set` show as a toast.
**Depends on:** Chunk 3 (the App page)
**Risks:**
- **`SMAppService` needs macOS 13.** Check the bundle's `LSMinimumSystemVersion` in `scripts/bundle.sh`. If it's lower, `status()` returns `Unavailable` on older systems instead of crashing.
- **Registering from `target/debug/RuSToat.app`** records that path. Rebuilding keeps the path, but a release DMG in /Applications is a different app as far as login items go. That's harmless, but say so in the manual check.
- **`objc2-service-management` 0.3.2's API** may name things differently than assumed. Read its docs before writing the wrapper.
**How to verify:** `cargo test`, with unit tests for `from_status` (each `SMAppServiceStatus` maps to the right `LoginItem`). Then `cargo clippy --all-targets`.
**Manual check** (signed bundle):
- Turn the switch on. RuSToat appears in System Settings › General › Login Items.
- Log out and back in: RuSToat opens with its window.
- Turn it off: it's gone from Login Items.
- Under plain `cargo run`, the switch is greyed out with its note.
**Shippable when:** the switch matches System Settings both ways, and a login launch opens RuSToat.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets`. As in milestone 9, don't run `cargo fmt` across the crate: `chat.rs`, `store.rs`, `client.rs`, `composer.rs` and some test files have formatting drift from before milestone 7. Run `rustfmt --edition 2024 <file>` on new or already-clean files. In drifted files, format only the lines you add, and check the file's drift count (`rustfmt --edition 2024 --check <file> | grep -c '^Diff in'`) doesn't grow.

For manual checks, run the signed app with `scripts/run-bundled.sh` (or `cargo run`). Don't run `target/debug/stoat-gpui` directly: it's unsigned, so the Keychain asks after every rebuild.

### Unit Tests
- **Shortcuts:** every `SHORTCUTS` entry is bound in a `Keymap` built from `app_key_bindings()` + `chat::key_bindings()`; descriptions are unique.
- **Themes:** each vendored file parses. Each `Palette`'s light and dark names exist in the registry with the right mode. There are 8 palettes with unique labels.
- **Prefs:** older `prefs.json` files (without `palette`, `appearance` or `keep_running`) load with Default / System / `true`, and new values round-trip.
- **Login item:** `from_status` maps every `SMAppServiceStatus`.

### Integration Tests
None. This milestone adds no API calls, and the window and OS behaviour (hiding, login items, banners) can only be checked by hand.

### Manual Tests
You need the signed bundle and a second account for the banner checks.
1. ⌘, opens Settings, and pressing it again focuses the same window. The Shortcuts page matches the real keys.
2. Every palette in Light and Dark looks right in the chat and in Settings. System follows macOS. The choice survives a relaunch. The skin tone and member list are still remembered.
3. Close the window. A DM from the second account still shows a banner and the badge, and clicking the banner reopens the window on that DM. With the switch off, the old close behaviour returns.
4. "Open RuSToat at login" shows in System Settings › Login Items, and a log out and back in starts RuSToat.

## Definition of Done
- [ ] ⌘, and Stoat › Settings… open a single settings window with a sidebar of Appearance, App and Keyboard Shortcuts.
- [ ] The Keyboard Shortcuts page lists every shortcut from milestones 9 and 13, with keys read from the keymap. A unit test fails if a listed shortcut isn't bound.
- [ ] Eight palettes × System / Light / Dark apply app-wide at once and persist across relaunches. A unit test covers that every palette's themes load.
- [ ] Skin tone and member-list preferences still persist after the move to `AppPrefs`.
- [ ] With "Keep running" on (the default), closing the window keeps accounts connected, banners and the badge keep working, and the Dock icon or a banner brings the window back. Off, closing behaves as before.
- [ ] "Open RuSToat at login" matches System Settings › Login Items, and is greyed out outside the app bundle.
- [ ] `cargo test` and `cargo clippy --all-targets` pass, with no new warnings.
- [ ] Every chunk's manual check is recorded in `docs/plans/settings-and-themes-implementation.md`.
