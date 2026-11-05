//! App-wide preferences, kept between launches: the default skin tone for
//! emoji, and whether the member list is open. Like [`crate::places`], plain data with no GPUI, saved
//! as a small JSON file in the app's folder and read and written
//! synchronously.

use std::path::PathBuf;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

/// A skin tone for emoji that have them (👍, 👋, 🤝…). Our own type, so the
/// saved file doesn't depend on the `emojis` crate's names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tone {
    /// The emoji's own yellow.
    #[default]
    Default,
    Light,
    MediumLight,
    Medium,
    MediumDark,
    Dark,
}

impl Tone {
    /// Every tone, lightest first after the default (the picker's swatches).
    pub const ALL: [Tone; 6] = [
        Tone::Default,
        Tone::Light,
        Tone::MediumLight,
        Tone::Medium,
        Tone::MediumDark,
        Tone::Dark,
    ];

    /// The same tone for the `emojis` crate. A single tone applies to
    /// every person in an emoji.
    pub fn skin_tone(self) -> emojis::SkinTone {
        match self {
            Tone::Default => emojis::SkinTone::Default,
            Tone::Light => emojis::SkinTone::Light,
            Tone::MediumLight => emojis::SkinTone::MediumLight,
            Tone::Medium => emojis::SkinTone::Medium,
            Tone::MediumDark => emojis::SkinTone::MediumDark,
            Tone::Dark => emojis::SkinTone::Dark,
        }
    }

    /// The swatch's tooltip.
    pub fn label(self) -> &'static str {
        match self {
            Tone::Default => "Default skin tone",
            Tone::Light => "Light skin tone",
            Tone::MediumLight => "Medium-light skin tone",
            Tone::Medium => "Medium skin tone",
            Tone::MediumDark => "Medium-dark skin tone",
            Tone::Dark => "Dark skin tone",
        }
    }
}

/// Everything saved in `prefs.json`. Missing fields take their defaults,
/// so older and newer files both load.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub skin_tone: Tone,
    /// The right-hand panel shows the member list (as the web client's
    /// toggle does). On until the user turns it off.
    pub member_list_open: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            skin_tone: Tone::default(),
            member_list_open: true,
        }
    }
}

/// Where preferences are saved.
pub struct PrefsFile {
    path: PathBuf,
}

impl PrefsFile {
    /// The real file, `~/Library/Application Support/com.chrishultin.rustoat/prefs.json`.
    /// `None` if there's no home directory to put it in.
    pub fn in_app_dir() -> Option<Self> {
        Some(Self::at(crate::places::app_dir()?.join("prefs.json")))
    }

    /// A file somewhere else (tests).
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The saved preferences. A missing file gives the defaults, and so does
    /// one that can't be read (it's replaced on the next save).
    pub fn load(&self) -> Prefs {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                log::warn!("couldn't parse {}: {err}", self.path.display());
                Prefs::default()
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Prefs::default(),
            Err(err) => {
                log::warn!("couldn't read {}: {err}", self.path.display());
                Prefs::default()
            }
        }
    }

    pub fn save(&self, prefs: &Prefs) -> anyhow::Result<()> {
        let json = serde_json::to_vec_pretty(prefs)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        // Write then rename, so a crash mid-write can't leave half a file.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("stoat-gpui-prefs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = temp_dir("missing");
        let file = PrefsFile::at(dir.join("prefs.json"));
        assert_eq!(file.load(), Prefs::default());
        assert_eq!(file.load().skin_tone, Tone::Default);
    }

    #[test]
    fn round_trips_and_creates_the_directory() {
        let dir = temp_dir("round-trip");
        let file = PrefsFile::at(dir.join("nested/prefs.json"));
        let prefs = Prefs {
            skin_tone: Tone::MediumDark,
            member_list_open: false,
        };
        file.save(&prefs).unwrap();
        assert_eq!(file.load(), prefs);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_corrupt_file_gives_the_defaults_and_is_replaced() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prefs.json");
        std::fs::write(&path, b"{not json").unwrap();
        let file = PrefsFile::at(&path);
        assert_eq!(file.load(), Prefs::default());
        // An unknown tone is corrupt too.
        std::fs::write(&path, br#"{"skin_tone":"Purple"}"#).unwrap();
        assert_eq!(file.load(), Prefs::default());
        let prefs = Prefs {
            skin_tone: Tone::Light,
            ..Prefs::default()
        };
        file.save(&prefs).unwrap();
        assert_eq!(file.load(), prefs);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_fields_are_ignored_and_missing_ones_default() {
        let dir = temp_dir("fields");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prefs.json");
        std::fs::write(&path, br#"{"future_setting":true}"#).unwrap();
        assert_eq!(PrefsFile::at(&path).load(), Prefs::default());
        std::fs::write(&path, br#"{"skin_tone":"Dark","future_setting":1}"#).unwrap();
        assert_eq!(PrefsFile::at(&path).load().skin_tone, Tone::Dark);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_member_list_is_open_unless_a_file_says_otherwise() {
        let dir = temp_dir("member-list");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prefs.json");
        assert!(Prefs::default().member_list_open);
        // A file from before the setting existed.
        std::fs::write(&path, br#"{"skin_tone":"Light"}"#).unwrap();
        let old = PrefsFile::at(&path).load();
        assert!(old.member_list_open);
        assert_eq!(old.skin_tone, Tone::Light);
        std::fs::write(&path, br#"{"member_list_open":false}"#).unwrap();
        assert!(!PrefsFile::at(&path).load().member_list_open);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
