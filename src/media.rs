//! Where images come from: uploaded files, default avatars, custom emoji, and
//! external images (which always go through Stoat's proxy, January, so remote
//! sites never see the user's IP).
//!
//! Checked against the live service on 2026-11-04: the file server ignores
//! resize parameters (originals only), `GET /users/{id}/default_avatar` needs
//! no auth, and `{proxy}/proxy?url=<encoded>` serves external images.

use crate::api::RootConfig;
use crate::api::file_url;
use crate::api::models::File;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaUrls {
    /// API base, e.g. `https://api.stoat.chat`.
    api: String,
    /// File server base, e.g. `https://cdn.stoatusercontent.com`.
    files: Option<String>,
    /// Media proxy base, e.g. `https://proxy.stoatusercontent.com`.
    proxy: Option<String>,
}

impl MediaUrls {
    pub fn new(api: impl Into<String>, files: Option<String>, proxy: Option<String>) -> Self {
        let trim = |s: String| s.trim_end_matches('/').to_string();
        Self {
            api: trim(api.into()),
            files: files.map(trim),
            proxy: proxy.map(trim),
        }
    }

    /// From the API base URL and its `GET /` config.
    pub fn from_root(api: &str, root: &RootConfig) -> Self {
        Self::new(
            api,
            root.file_server().map(str::to_string),
            root.proxy_server().map(str::to_string),
        )
    }

    /// The API, file server and proxy bases that are known, for telling
    /// Stoat's own links from others (see [`crate::safety::is_own_instance`]).
    pub fn bases(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.api.as_str())
            .chain(self.files.as_deref())
            .chain(self.proxy.as_deref())
    }

    pub fn file_server(&self) -> Option<&str> {
        self.files.as_deref()
    }

    /// An uploaded file (`{files}/{tag}/{id}`). `None` without a file server.
    pub fn file(&self, file: &File) -> Option<String> {
        Some(file_url(self.files.as_deref()?, file))
    }

    /// An uploaded file known only by ID and tag, e.g. a webhook avatar.
    pub fn file_by_id(&self, tag: &str, id: &str) -> Option<String> {
        Some(format!("{}/{tag}/{id}", self.files.as_deref()?))
    }

    /// A custom emoji image.
    pub fn emoji(&self, id: &str) -> Option<String> {
        self.file_by_id("emojis", id)
    }

    /// The generated avatar for a user without one. Needs no auth.
    pub fn default_avatar(&self, user_id: &str) -> String {
        format!("{}/users/{user_id}/default_avatar", self.api)
    }

    /// An image URL from message content (masquerade avatars, embeds). Our own
    /// file server is used directly; anything else goes through the proxy.
    /// Without a proxy, external images aren't loaded at all.
    pub fn external(&self, url: &str) -> Option<String> {
        if let Some(files) = &self.files
            && url.starts_with(&format!("{files}/"))
        {
            return Some(url.to_string());
        }
        let proxy = self.proxy.as_deref()?;
        let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
        Some(format!("{proxy}/proxy?url={encoded}"))
    }
}
