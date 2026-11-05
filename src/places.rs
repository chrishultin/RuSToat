//! Remembering where you were between launches: the open server (or Home),
//! and the last channel picked in each.
//!
//! Plain data with no GPUI. It's saved as JSON in Application Support, one
//! entry per account (API URL and user ID), so another account or instance
//! never gets IDs it can't open. The file is small and only written when the
//! user picks something, so reads and writes are synchronous.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

/// The app's folder name in Application Support (the bundle identifier).
pub const APP_ID: &str = "com.chrishultin.rustoat";

/// The folder name used before RuSToat had its own bundle identifier.
const LEGACY_APP_ID: &str = "chat.stoat.rustoat";

/// `~/Library/Application Support`, or `None` without a home directory.
pub fn support_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/Application Support"))
}

/// The app's own folder in Application Support, moved over from its legacy
/// name the first time it's asked for.
pub fn app_dir() -> Option<PathBuf> {
    let base = support_dir()?;
    if let Err(err) = migrate_app_dir(&base) {
        log::warn!("couldn't move the old app folder: {err:#}");
    }
    Some(base.join(APP_ID))
}

/// Rename `base/chat.stoat.rustoat` to `base/com.chrishultin.rustoat`, if
/// the old folder exists and the new one doesn't. Returns whether it moved.
pub fn migrate_app_dir(base: &Path) -> anyhow::Result<bool> {
    let old = base.join(LEGACY_APP_ID);
    let new = base.join(APP_ID);
    if !old.is_dir() || new.exists() {
        return Ok(false);
    }
    std::fs::rename(&old, &new)
        .with_context(|| format!("renaming {} to {}", old.display(), new.display()))?;
    log::info!("moved {} to {}", old.display(), new.display());
    Ok(true)
}

/// Which list the channel column shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    Home,
    Server(String),
}

/// The open place and the last channel picked in each place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    pub place: Place,
    pub selected: HashMap<Place, String>,
}

impl Default for Places {
    fn default() -> Self {
        Self {
            place: Place::Home,
            selected: HashMap::new(),
        }
    }
}

/// One account's entry on disk. JSON object keys must be strings, so Home
/// and servers are kept apart instead of keying by [`Place`].
#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// The open server; `None` is Home.
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    home_channel: Option<String>,
    /// Server ID → last channel picked in it.
    #[serde(default)]
    server_channels: HashMap<String, String>,
}

impl From<&Places> for Saved {
    fn from(places: &Places) -> Self {
        let mut saved = Saved {
            server: match &places.place {
                Place::Home => None,
                Place::Server(id) => Some(id.clone()),
            },
            ..Default::default()
        };
        for (place, channel) in &places.selected {
            match place {
                Place::Home => saved.home_channel = Some(channel.clone()),
                Place::Server(id) => {
                    saved.server_channels.insert(id.clone(), channel.clone());
                }
            }
        }
        saved
    }
}

impl From<Saved> for Places {
    fn from(saved: Saved) -> Self {
        let mut selected: HashMap<Place, String> = saved
            .server_channels
            .into_iter()
            .map(|(server, channel)| (Place::Server(server), channel))
            .collect();
        if let Some(channel) = saved.home_channel {
            selected.insert(Place::Home, channel);
        }
        Places {
            place: saved.server.map_or(Place::Home, Place::Server),
            selected,
        }
    }
}

/// The whole file: account key → that account's places.
type SavedFile = HashMap<String, Saved>;

/// Where places are saved, for one account.
pub struct PlacesFile {
    path: PathBuf,
    account: String,
}

impl PlacesFile {
    /// The real file, `~/Library/Application Support/com.chrishultin.rustoat/places.json`.
    /// `None` if there's no home directory to put it in.
    pub fn for_account(instance: &str, user: &str) -> Option<Self> {
        Some(Self::at(app_dir()?.join("places.json"), instance, user))
    }

    /// A file somewhere else (tests).
    pub fn at(path: impl Into<PathBuf>, instance: &str, user: &str) -> Self {
        Self {
            path: path.into(),
            // URLs have no spaces, so this can't collide.
            account: format!("{instance} {user}"),
        }
    }

    /// This account's saved places. Nothing saved, or a file that can't be
    /// read, means a fresh start at Home.
    pub fn load(&self) -> Places {
        match self.read() {
            Ok(mut file) => file
                .remove(&self.account)
                .map(Places::from)
                .unwrap_or_default(),
            Err(err) => {
                log::warn!("couldn't read saved places: {err:#}");
                Places::default()
            }
        }
    }

    /// Save this account's places, keeping other accounts' entries.
    pub fn save(&self, places: &Places) -> anyhow::Result<()> {
        // An unreadable file is replaced rather than blocking saves forever.
        let mut file = self.read().unwrap_or_default();
        file.insert(self.account.clone(), Saved::from(places));
        let json = serde_json::to_vec_pretty(&file)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        // Write then rename, so a crash mid-write can't leave half a file.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))
    }

    fn read(&self) -> anyhow::Result<SavedFile> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing {}", self.path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SavedFile::new()),
            Err(err) => Err(err).with_context(|| format!("reading {}", self.path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("stoat-gpui-places-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> Places {
        Places {
            place: Place::Server("S1".into()),
            selected: HashMap::from([
                (Place::Home, "DM1".into()),
                (Place::Server("S1".into()), "C1".into()),
                (Place::Server("S2".into()), "C2".into()),
            ]),
        }
    }

    #[test]
    fn nothing_saved_starts_at_home() {
        let dir = temp_dir("empty");
        let file = PlacesFile::at(dir.join("places.json"), "https://a", "U1");
        assert_eq!(file.load(), Places::default());
    }

    #[test]
    fn round_trips_and_creates_the_directory() {
        let dir = temp_dir("round-trip");
        let path = dir.join("nested/places.json");
        PlacesFile::at(&path, "https://a", "U1")
            .save(&sample())
            .unwrap();
        assert_eq!(PlacesFile::at(&path, "https://a", "U1").load(), sample());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn accounts_are_kept_apart() {
        let dir = temp_dir("accounts");
        let path = dir.join("places.json");
        let other = Places {
            place: Place::Home,
            selected: HashMap::from([(Place::Home, "DM9".into())]),
        };
        PlacesFile::at(&path, "https://a", "U1")
            .save(&sample())
            .unwrap();
        PlacesFile::at(&path, "https://a", "U2")
            .save(&other)
            .unwrap();
        assert_eq!(PlacesFile::at(&path, "https://a", "U1").load(), sample());
        assert_eq!(PlacesFile::at(&path, "https://a", "U2").load(), other);
        // Same user ID on another instance has nothing saved.
        assert_eq!(
            PlacesFile::at(&path, "https://b", "U1").load(),
            Places::default()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_legacy_app_folder_is_renamed() {
        let base = temp_dir("migrate");
        std::fs::create_dir_all(base.join(LEGACY_APP_ID)).unwrap();
        std::fs::write(base.join(LEGACY_APP_ID).join("places.json"), b"{}").unwrap();
        assert!(migrate_app_dir(&base).unwrap());
        assert!(!base.join(LEGACY_APP_ID).exists());
        assert_eq!(
            std::fs::read(base.join(APP_ID).join("places.json")).unwrap(),
            b"{}"
        );
        // Nothing left to move.
        assert!(!migrate_app_dir(&base).unwrap());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn an_existing_app_folder_is_left_alone() {
        let base = temp_dir("migrate-both");
        std::fs::create_dir_all(base.join(LEGACY_APP_ID)).unwrap();
        std::fs::create_dir_all(base.join(APP_ID)).unwrap();
        std::fs::write(base.join(LEGACY_APP_ID).join("places.json"), b"old").unwrap();
        std::fs::write(base.join(APP_ID).join("places.json"), b"new").unwrap();
        assert!(!migrate_app_dir(&base).unwrap());
        assert_eq!(
            std::fs::read(base.join(APP_ID).join("places.json")).unwrap(),
            b"new"
        );
        assert!(base.join(LEGACY_APP_ID).exists());
        // And with no folders at all, nothing happens.
        let empty = temp_dir("migrate-none");
        assert!(!migrate_app_dir(&empty).unwrap());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_corrupt_file_starts_fresh_and_is_replaced() {
        let dir = temp_dir("corrupt");
        let path = dir.join("places.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{not json").unwrap();
        let file = PlacesFile::at(&path, "https://a", "U1");
        assert_eq!(file.load(), Places::default());
        file.save(&sample()).unwrap();
        assert_eq!(file.load(), sample());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
