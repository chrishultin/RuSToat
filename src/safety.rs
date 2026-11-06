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
    /// The text is a different web address or domain
    /// (`[google.com](https://example.com)`).
    Mismatch(String),
}

/// How hard to warn before opening `url`, whose text was `text` (`None`:
/// it showed its address).
pub fn classify(url: &str, text: Option<&str>) -> LinkCheck {
    let Some(text) = text.filter(|text| !shows_address(url, text)) else {
        return LinkCheck::Plain;
    };
    let destination = Url::parse(url).ok();
    let destination = destination.as_ref().and_then(Url::host_str);
    match text_host(text) {
        Some(host) if Some(host.as_str()) != destination => LinkCheck::Mismatch(text.to_string()),
        _ => LinkCheck::Text(text.to_string()),
    }
}

/// Whether a link's `text` is its `url`: the same once percent-escapes are
/// decoded, without a trailing `/`, and ignoring case. Text without a scheme
/// is compared with the URL's after its scheme (`www.example.com` is
/// `http://www.example.com`, as GFM links it).
pub fn shows_address(url: &str, text: &str) -> bool {
    let (url, text) = (percent_decode(url), percent_decode(text.trim()));
    let (url, text) = (url.trim_end_matches('/'), text.trim_end_matches('/'));
    let url = if has_scheme(text) {
        url
    } else {
        ["https://", "http://", "mailto:"]
            .iter()
            .find_map(|scheme| strip_prefix_ignore_case(url, scheme))
            .unwrap_or(url)
    };
    url.eq_ignore_ascii_case(text)
}

/// The host a link's text names, if it reads as a web address: a URL, or a
/// host with a dot and no spaces (`example.com/docs`). Not an email address.
fn text_host(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let url = if has_scheme(text) {
        Url::parse(text).ok()?
    } else {
        if text.contains('@') {
            return None;
        }
        Url::parse(&format!("https://{text}")).ok()?
    };
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    host.contains('.').then(|| host.to_string())
}

fn has_scheme(text: &str) -> bool {
    text.contains("://") || strip_prefix_ignore_case(text, "mailto:").is_some()
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// `%XX` escapes decoded (invalid UTF-8 replaced), anything else as is.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let Some(hi) = bytes.get(i + 1).copied().and_then(hex)
            && let Some(lo) = bytes.get(i + 2).copied().and_then(hex)
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
    fn text_that_is_the_address_is_plain() {
        let plain = |url, text| classify(url, Some(text)) == LinkCheck::Plain;
        assert_eq!(classify("https://example.com", None), LinkCheck::Plain);
        assert!(plain("https://example.com/", "https://example.com"));
        assert!(plain("https://example.com", "example.com/"), "no scheme");
        assert!(
            plain("http://www.example.com", "www.example.com"),
            "GFM's www. links"
        );
        assert!(plain("https://www.example.com", "www.example.com"));
        assert!(plain(
            "https://example.com/caf%C3%A9",
            "https://example.com/café"
        ));
        assert!(plain("https://Example.com", "https://example.COM"));
        assert!(plain("mailto:a@example.com", "a@example.com"));
    }

    #[test]
    fn other_words_are_text() {
        assert_eq!(
            classify("https://example.com", Some("the docs")),
            LinkCheck::Text("the docs".into())
        );
        assert_eq!(
            classify("https://example.com", Some("docs")),
            LinkCheck::Text("docs".into()),
            "no dot: not a host"
        );
        // An email address isn't a web address.
        assert_eq!(
            classify("https://example.com", Some("someone@other.com")),
            LinkCheck::Text("someone@other.com".into())
        );
        // The same site, another page.
        assert_eq!(
            classify("https://example.com/b", Some("example.com/a")),
            LinkCheck::Text("example.com/a".into())
        );
    }

    #[test]
    fn another_address_is_a_mismatch() {
        let mismatch =
            |url, text: &str| classify(url, Some(text)) == LinkCheck::Mismatch(text.to_string());
        assert!(mismatch("https://example.com", "google.com"));
        assert!(mismatch("https://example.com", "https://google.com/search"));
        assert!(
            mismatch("https://example.com", "www.example.com"),
            "another host"
        );
        assert!(mismatch("https://example.com.evil.io", "example.com"));
        assert!(mismatch("mailto:a@evil.io", "example.com"));
        assert!(mismatch("https://example.com", "http://google.com"));
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
