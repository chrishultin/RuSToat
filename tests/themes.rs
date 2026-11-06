//! The bundled theme files load with the pinned gpui-component, and every
//! palette Settings offers names a light and a dark theme that exist.

use std::collections::HashSet;

use gpui_kit::component::{ThemeMode, ThemeRegistry};
use stoat_gpui::themes::{BUNDLED, Palette, load_bundled};

#[test]
fn every_bundled_file_parses() {
    for (file, json) in BUNDLED {
        let mut registry = ThemeRegistry::default();
        registry
            .load_themes_from_str(json)
            .unwrap_or_else(|err| panic!("{file}: {err:#}"));
        assert!(!registry.themes().is_empty(), "{file} has no themes");
    }
}

#[test]
fn every_palette_names_a_light_and_a_dark_theme() {
    let mut registry = ThemeRegistry::default();
    load_bundled(&mut registry).unwrap();
    for palette in Palette::ALL {
        // gpui-component's own themes are registered at launch, not from
        // these files.
        if palette == Palette::Default {
            assert_eq!(palette.light_name(), "Default Light");
            assert_eq!(palette.dark_name(), "Default Dark");
            continue;
        }
        for (name, mode) in [
            (palette.light_name(), ThemeMode::Light),
            (palette.dark_name(), ThemeMode::Dark),
        ] {
            let theme = registry
                .themes()
                .get(name)
                .unwrap_or_else(|| panic!("{palette:?}: no theme {name:?}"));
            assert_eq!(theme.mode, mode, "{name} is the wrong mode");
        }
    }
}

#[test]
fn there_are_eight_palettes_with_unique_labels() {
    assert_eq!(Palette::ALL.len(), 8);
    let labels: HashSet<_> = Palette::ALL.iter().map(|p| p.label()).collect();
    assert_eq!(labels.len(), 8);
}
