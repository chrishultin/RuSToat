//! Safeguards the user has settled, per account: sites whose links open
//! without asking, and mature channels they've confirmed their age for.
//!
//! Plain data with no GPUI, saved like [`crate::places`]: JSON in the app's
//! folder, one entry per account (API URL and user ID). Not synced, as in
//! Stoat's web client.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use url::Url;

/// What one account has settled.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetySettings {
    /// Origins (see [`origin`]) whose links open without asking.
    #[serde(default)]
    pub trusted_origins: BTreeSet<String>,
    /// The user confirmed they're at least 18.
    #[serde(default)]
    pub adult_confirmed: bool,
    /// Mature channels the user entered, by ID.
    #[serde(default)]
    pub mature_channels: BTreeSet<String>,
}

impl SafetySettings {
    /// Whether `url` opens without asking: its origin is trusted.
    pub fn trusts(&self, url: &str) -> bool {
        origin(url).is_some_and(|origin| self.trusted_origins.contains(&origin))
    }

    /// Open links to `url`'s origin without asking from now on. False if it
    /// has no origin (`mailto:`), or was already trusted.
    pub fn trust(&mut self, url: &str) -> bool {
        origin(url).is_some_and(|origin| self.trusted_origins.insert(origin))
    }
}

/// The whole file: account key → that account's settings.
type SavedFile = HashMap<String, SafetySettings>;

/// Where safety settings are saved, for one account.
pub struct SafetyFile {
    path: PathBuf,
    account: String,
}

impl SafetyFile {
    /// The real file, `~/Library/Application Support/com.chrishultin.rustoat/safety.json`.
    /// `None` if there's no home directory to put it in.
    pub fn for_account(instance: &str, user: &str) -> Option<Self> {
        Some(Self::at(
            crate::places::app_dir()?.join("safety.json"),
            instance,
            user,
        ))
    }

    /// A file somewhere else (tests).
    pub fn at(path: impl Into<PathBuf>, instance: &str, user: &str) -> Self {
        Self {
            path: path.into(),
            // URLs have no spaces, so this can't collide.
            account: format!("{instance} {user}"),
        }
    }

    /// This account's settings. Nothing saved, or a file that can't be read,
    /// means nothing settled yet.
    pub fn load(&self) -> SafetySettings {
        match self.read() {
            Ok(mut file) => file.remove(&self.account).unwrap_or_default(),
            Err(err) => {
                log::warn!("couldn't read safety settings: {err:#}");
                SafetySettings::default()
            }
        }
    }

    /// Save this account's settings, keeping other accounts' entries.
    pub fn save(&self, settings: &SafetySettings) -> anyhow::Result<()> {
        // An unreadable file is replaced rather than blocking saves forever.
        let mut file = self.read().unwrap_or_default();
        file.insert(self.account.clone(), settings.clone());
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

/// A web link's scheme, host and port (when it isn't the scheme's default),
/// e.g. `https://example.com:8443`. `None` for anything else (`mailto:`).
pub fn origin(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.host_str()?;
    Some(url.origin().ascii_serialization())
}

/// Whether `url` is on the user's own instance: the same origin as one of
/// `bases` (the API, its file server and proxy, its web client).
pub fn is_own_instance<'a>(url: &str, bases: impl IntoIterator<Item = &'a str>) -> bool {
    let Some(origin) = origin(url) else {
        return false;
    };
    bases
        .into_iter()
        .any(|base| self::origin(base).as_deref() == Some(origin.as_str()))
}

/// How hard to warn before opening a link, by what the link's text said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkCheck {
    /// The text is the address (or there's no text to go by).
    Plain,
    /// The text is something else (`[the docs](…)`).
    Text(String),
}

/// How hard to warn before opening `url`, whose text (if it had any other
/// than the address) was `text`.
pub fn classify(url: &str, text: Option<&str>) -> LinkCheck {
    match text {
        Some(text) if normalise(text) != normalise(url) => LinkCheck::Text(text.to_string()),
        _ => LinkCheck::Plain,
    }
}

/// An address as it's compared with a link's text: trimmed, with a scheme,
/// and without a trailing `/`.
fn normalise(address: &str) -> String {
    let address = address.trim();
    let address = if address.contains("://") || address.starts_with("mailto:") {
        address.to_string()
    } else {
        format!("https://{address}")
    };
    address.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("stoat-gpui-safety-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> SafetySettings {
        SafetySettings {
            trusted_origins: BTreeSet::from(["https://example.com".into()]),
            adult_confirmed: true,
            mature_channels: BTreeSet::from(["C1".into()]),
        }
    }

    #[test]
    fn origins() {
        assert_eq!(
            origin("https://Example.com/a/b?c#d").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            origin("http://example.com:8080/x").as_deref(),
            Some("http://example.com:8080")
        );
        // The scheme's own port is left out.
        assert_eq!(
            origin("https://example.com:443/").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(origin("mailto:someone@example.com"), None);
        assert_eq!(origin("not a url"), None);
    }

    #[test]
    fn trusting_an_origin_trusts_its_other_links_only() {
        let mut settings = SafetySettings::default();
        assert!(!settings.trusts("https://example.com/a"));
        assert!(settings.trust("https://example.com/a"));
        assert!(!settings.trust("https://example.com/b"), "already trusted");
        assert!(settings.trusts("https://example.com/b?c"));
        assert!(!settings.trusts("http://example.com/a"), "another scheme");
        assert!(
            !settings.trusts("https://example.com:8443/a"),
            "another port"
        );
        assert!(
            !settings.trusts("https://sub.example.com/a"),
            "another host"
        );
        assert!(!settings.trust("mailto:someone@example.com"));
        assert!(!settings.trusts("mailto:someone@example.com"));
    }

    #[test]
    fn own_instance() {
        let bases = [
            "https://api.stoat.chat",
            "https://cdn.stoatusercontent.com",
            "https://stoat.chat/app",
        ];
        assert!(is_own_instance(
            "https://stoat.chat/server/1/channel/2",
            bases
        ));
        assert!(is_own_instance(
            "https://cdn.stoatusercontent.com/attachments/x",
            bases
        ));
        assert!(!is_own_instance("https://stoat.chat.evil.com/", bases));
        assert!(
            !is_own_instance("http://stoat.chat/", bases),
            "another scheme"
        );
        assert!(!is_own_instance("mailto:a@stoat.chat", bases));
        assert!(!is_own_instance("https://stoat.chat/", []));
    }

    #[test]
    fn plain_without_other_text() {
        assert_eq!(classify("https://example.com", None), LinkCheck::Plain);
        assert_eq!(
            classify("https://example.com/", Some("example.com")),
            LinkCheck::Plain
        );
        assert_eq!(
            classify("https://example.com", Some("the docs")),
            LinkCheck::Text("the docs".into())
        );
    }

    #[test]
    fn nothing_saved_is_nothing_settled() {
        let dir = temp_dir("empty");
        let file = SafetyFile::at(dir.join("safety.json"), "https://a", "U1");
        assert_eq!(file.load(), SafetySettings::default());
    }

    #[test]
    fn round_trips_and_creates_the_directory() {
        let dir = temp_dir("round-trip");
        let path = dir.join("nested/safety.json");
        SafetyFile::at(&path, "https://a", "U1")
            .save(&sample())
            .unwrap();
        assert_eq!(SafetyFile::at(&path, "https://a", "U1").load(), sample());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn accounts_are_kept_apart() {
        let dir = temp_dir("accounts");
        let path = dir.join("safety.json");
        let other = SafetySettings {
            adult_confirmed: true,
            ..SafetySettings::default()
        };
        SafetyFile::at(&path, "https://a", "U1")
            .save(&sample())
            .unwrap();
        SafetyFile::at(&path, "https://b", "U1")
            .save(&other)
            .unwrap();
        assert_eq!(SafetyFile::at(&path, "https://a", "U1").load(), sample());
        assert_eq!(SafetyFile::at(&path, "https://b", "U1").load(), other);
        assert_eq!(
            SafetyFile::at(&path, "https://a", "U2").load(),
            SafetySettings::default()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_older_file_without_a_field_loads() {
        let dir = temp_dir("older");
        let path = dir.join("safety.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            r#"{ "https://a U1": { "trusted_origins": ["https://example.com"] } }"#,
        )
        .unwrap();
        assert_eq!(
            SafetyFile::at(&path, "https://a", "U1").load(),
            SafetySettings {
                trusted_origins: BTreeSet::from(["https://example.com".into()]),
                ..SafetySettings::default()
            }
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
