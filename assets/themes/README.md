# Bundled themes

The colour palettes offered in Settings › Appearance, besides gpui-component's own "Default Light" and "Default Dark". `src/themes.rs` compiles them in and registers them at launch.

- **Source:** <https://github.com/longbridge/gpui-component/tree/48ac25b50c81ca5e34ec1631e3efabe22f4af64a/themes> (gpui-component `main`, 2026-11-06), copied unchanged.
- **Files:** `ayu.json`, `catppuccin.json`, `everforest.json`, `flexoki.json`, `gruvbox.json`, `macos-classic.json`, `solarized.json`.
- **Licences:** the theme files are part of gpui-component, licensed under Apache-2.0 (Copyright 2024–2026 Longbridge). The palettes themselves (Ayu, Catppuccin, Everforest, Flexoki, Gruvbox, Solarized) are MIT-licensed by their authors, named in each file's `author` field.

They come from gpui-component `main`, not the pinned 0.9 release. `tests/themes.rs` checks that each still parses, so a gpui-kit upgrade that rejects a key fails a test, not a launch.

## Local changes

None. A colour a file leaves out is derived from the palette's own colours by gpui-component (for example the sidebar from `background` and `border`, `danger` from `red`, `link` from `primary`), not taken from the default theme.
