# Implementation Record: Server settings (milestone 23)

_Plan: docs/plans/server-settings.md_
_Started: 2026-11-09_

**Mode:** continuous. Manual checks are batched at the end, as in milestone 22 (the user didn't ask for them per chunk).

**Files dirty at the start:** the plan (`docs/plans/server-settings.md`, untracked) and the roadmap (`docs/plans/roadmap.md`, linking the plan). Both are this milestone's, so they go in with chunk 1.

## Chunk 1: Server Settings window with name and description

### Changes Made
- `src/server_settings.rs` (new, no GPUI; registered in `src/lib.rs` by hand):
  - `Access { overview, invites, bans, ownership }` with `any()`.
  - `access(store, server)`: the owner sees everything. Anyone else needs a known membership: Manage Server for Overview and Invites, Ban Members for Bans.
  - `OverviewForm { name, description }`:
    - `of(&Server)` fills it from the server;
    - `is_valid()`: name 1–32 and description ≤1024, both trimmed;
    - `changes(&Server) -> Option<DataEditServer>`: only what changed, trimmed. A blank description becomes `remove: [Description]`. `None` when nothing changed or the form is invalid.
  - `empty_server_edit()`, `DESCRIPTION_MAX`, and `NAME_MAX` re-exported from `server_edit`.
  - `save_error_text`: texts for `MissingPermission`, `FailedValidation` and `NotFound`, else `error_text`.
- `src/ui/server_settings.rs` (new; registered in `src/ui/mod.rs` by hand):
  - `ServerSettingsWindow` (library global):
    - `open(chat, server, cx)` is deferred. It brings the same server's window forward; otherwise it closes the old window and opens a new one, 760×560 and centred.
    - `close(cx)`.
  - `ServerSettingsView`:
    - pages are `ServerPageFn = fn(&ServerSettingsView, &mut App) -> Option<SettingPage>`, in `PAGES`;
    - `shown: Vec<bool>` records which pages were visible at the last render. The sidebar's keyed ID (`page_key`) is bumped only when that set changes;
    - the window title is "Server Settings — ‹name›", updated on rename;
    - it observes the `ChatView`, and closes the window when the server leaves the store or no page is left;
    - `observe_release_in` closes it when the `ChatView` goes (sign-out);
    - it follows macOS appearance, as `SettingsView` does.
  - `overview_page`, and `OverviewPage`:
    - a name input and a description textarea, with counters through `profile_editor::field`;
    - untouched fields follow live changes (`follow`, compared against `filled`);
    - Reset and Save. Save sends one `edit_server`, then `ChatView::server_edited`, refills the form, and toasts "Saved." A refusal toasts `save_error_text`.
- `src/api/client.rs`: `edit_server(server, &DataEditServer) -> Server` (`PATCH /servers/{id}` through `edit_body`). Imports `DataEditServer` and `Server`.
- `src/store.rs`: `apply_server_edit(&Server) -> bool`, built on `apply_server_update`. It takes name, owner, description, icon, banner and system messages from Stoat's answer, and clears those the answer lacks.
- `src/ui/chat.rs`:
  - "Server settings…" (`Settings` icon) in `server_menu`, after "Invite people…", when `access(..).any()`;
  - `ChatView::open_server_settings(server, cx)` and `ChatView::server_edited(&Server, cx)` (applies the edit, then `server_permissions_changed`, then notify);
  - `open_info_dialog` passes a "Server settings…" callback for servers the user may manage.
- `src/ui/server_dialogs.rs`: `render_info` takes `on_settings: Option<OnSettings>` (a new `Rc` alias), drawn as a ghost button beside Close.
- `src/assets.rs`: `server_settings_icons_load` (Settings, Link, Ban, Crown).
- Tests:
  - `tests/server_settings.rs` (new), 9 tests:
    - `access` for the owner, Manage Server, Ban Members only, a plain member, an unknown membership and an unknown server;
    - `OverviewForm`: unchanged gives `None`, including surrounding whitespace; a rename sends only the name; a description is trimmed; a blank description is removed; the 32 and 1024 limits.
  - `tests/api_servers.rs`: `edit_server` sends `{"name":"Lounge","remove":["Description"]}` once; `MissingPermission` gives "You don't have permission to change this server."
  - `tests/store_servers.rs`: a saved edit shows at once, and the matching `ServerUpdate` then leaves the server identical. An unknown server is ignored.

### Divergences
- **`open_server_settings` takes no `window`.** Opening is deferred to a new window, so the main window isn't needed. The plan's signature had one.
- **The window also closes when no page is left** (e.g. Manage Server taken away), not only when the server goes. Chunk 6's risk asked for this; it's done here because the same observer handles it.
- **`apply_server_edit(&Server)` takes no `removed` list.** The answer is the whole server, so a field it lacks is cleared. That gives the same result as passing the edit's `remove`.
- None of the plan's risks materialised:
  - `gpui_kit::open_window` works from the library;
  - `window.remove_window()` only sets a flag, so it's safe inside the observer;
  - no `cx.defer` was needed for closing.
- No look-ahead changes to later chunks.

### Verification Performed
- `cargo test --test server_settings --test api_servers --test store_servers`: 9 + 17 + 15 passed.
- `cargo test`: 1137 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean, after adding the `OnSettings` alias for `type_complexity`.
- rustfmt:
  - drift unchanged: `src/ui/chat.rs` 17, `src/store.rs` 14, `src/api/client.rs` 3, `src/ui/server_dialogs.rs` 0, `src/assets.rs` 0;
  - new files clean: `src/server_settings.rs`, `src/ui/server_settings.rs`, `tests/server_settings.rs`;
  - `tests/api_servers.rs` and `tests/store_servers.rs` are still clean.

### Manual Validation
Deferred to the end (rename, clear the description, switch servers, and a member without permissions).
