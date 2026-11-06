//! Colour palettes: the theme files bundled in `assets/themes/`, and
//! applying the chosen [`Palette`] and [`Appearance`] to every window.
//!
//! gpui-component brings "Default Light" and "Default Dark"; the rest are
//! upstream gpui-component theme files (see `assets/themes/README.md`).

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};

pub use crate::prefs::{Appearance, Palette};
use crate::ui::app_prefs::AppPrefs;

/// Each bundled theme file, by file name.
pub const BUNDLED: [(&str, &str); 7] = [
    ("ayu.json", include_str!("../assets/themes/ayu.json")),
    (
        "catppuccin.json",
        include_str!("../assets/themes/catppuccin.json"),
    ),
    (
        "everforest.json",
        include_str!("../assets/themes/everforest.json"),
    ),
    (
        "flexoki.json",
        include_str!("../assets/themes/flexoki.json"),
    ),
    (
        "gruvbox.json",
        include_str!("../assets/themes/gruvbox.json"),
    ),
    (
        "macos-classic.json",
        include_str!("../assets/themes/macos-classic.json"),
    ),
    (
        "solarized.json",
        include_str!("../assets/themes/solarized.json"),
    ),
];

/// Add the bundled themes to `registry`.
pub fn load_bundled(registry: &mut ThemeRegistry) -> anyhow::Result<()> {
    for (file, json) in BUNDLED {
        registry
            .load_themes_from_str(json)
            .map_err(|err| err.context(format!("loading {file}")))?;
    }
    Ok(())
}

/// Register the bundled themes. Call after `gpui_kit::init`, which creates
/// the registry. A file that doesn't load is logged, and its palette falls
/// back to the default themes.
pub fn register(cx: &mut App) {
    if let Err(err) = load_bundled(ThemeRegistry::global_mut(cx)) {
        log::error!("couldn't load a bundled theme: {err:#}");
    }
}

/// Show `palette` in `appearance`, in every window. System follows macOS's
/// setting now; call it again when that changes.
pub fn apply(palette: Palette, appearance: Appearance, cx: &mut App) {
    let mode = match appearance {
        Appearance::System => ThemeMode::from(cx.window_appearance()),
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    };
    let registry = ThemeRegistry::global(cx);
    let find = |name: &str| registry.themes().get(name).cloned();
    let light = find(palette.light_name()).unwrap_or_else(|| {
        log::warn!("no theme {:?}; using the default", palette.light_name());
        registry.default_light_theme().clone()
    });
    let dark = find(palette.dark_name()).unwrap_or_else(|| {
        log::warn!("no theme {:?}; using the default", palette.dark_name());
        registry.default_dark_theme().clone()
    });
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
    // Loads the mode's theme even when the mode hasn't changed, and
    // refreshes every window.
    Theme::change(mode, None, cx);
}

/// Show the palette and appearance chosen in [`AppPrefs`].
pub fn apply_current(cx: &mut App) {
    let prefs = AppPrefs::get(cx);
    apply(prefs.palette, prefs.appearance, cx);
}
