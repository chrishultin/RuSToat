//! Gifbox, Stoat's GIF service: categories, trending and search for the
//! message box's GIF picker. Its address is built into Stoat's web client,
//! not given by the instance, and it takes the account's session token, so
//! only accounts on the official instance use it (see
//! [`GifboxClient::for_account`]).
//!
//! The API isn't versioned or documented, so responses are read leniently:
//! every media format is optional, unknown fields are ignored, and a result
//! or category that doesn't parse is skipped rather than failing the page.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::ApiError;
use super::client::DEFAULT_API_URL;

/// Where Gifbox's API is (as in Stoat's web client).
pub const GIFBOX_URL: &str = "https://api.gifbox.me";
/// The language results are for.
const LOCALE: &str = "en_US";
/// How many results a trending list or search asks for.
pub const GIF_LIMIT: u32 = 50;

/// A category on the picker's first page: searching `title` finds its GIFs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GifCategory {
    pub title: String,
    /// An animated GIF for the tile (several MB: see [`category_poster`]).
    pub image: String,
}

/// One GIF in a trending list or search.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GifResult {
    pub id: String,
    /// Its page (`https://gifbox.me/view/{id}`): what's sent as the message.
    pub url: String,
    /// Its files by format name (`preview`, `gif`, `mediumgif`, `mp4`, …).
    #[serde(default, deserialize_with = "lenient_map")]
    pub media_formats: HashMap<String, GifMedia>,
}

/// One file of a GIF.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GifMedia {
    pub url: String,
    /// `[width, height]` in pixels, if given (read leniently by [`Self::size`]).
    #[serde(default, alias = "dims")]
    dimensions: Option<Value>,
}

impl GifMedia {
    /// Width and height, if given as two positive numbers.
    pub fn size(&self) -> Option<(u32, u32)> {
        let dims = self.dimensions.as_ref()?.as_array()?;
        let number = |ix: usize| {
            dims.get(ix)?
                .as_f64()
                .filter(|n| *n >= 1.)
                .map(|n| n as u32)
        };
        Some((number(0)?, number(1)?))
    }
}

/// A page of results. `next` would continue it; the picker doesn't page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GifPage {
    pub results: Vec<GifResult>,
    pub next: Option<String>,
}

impl GifResult {
    /// A still to show in the picker (`preview`, small), if there is one.
    pub fn still(&self) -> Option<&GifMedia> {
        self.media_formats.get("preview")
    }

    /// An animated GIF to play on hover (`gif`, else `mediumgif`), if there
    /// is one: some GIFs are only video, which can't be drawn.
    pub fn animated(&self) -> Option<&GifMedia> {
        self.media_formats
            .get("gif")
            .or_else(|| self.media_formats.get("mediumgif"))
    }

    /// The GIF's width and height, from its still or else any format.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.still()
            .and_then(GifMedia::size)
            .or_else(|| self.media_formats.values().find_map(GifMedia::size))
    }
}

/// The still of a category's GIF: its `image` with the last path segment
/// (`gif_high`, `gif_low`) replaced by `poster`, and no query. Left as it is
/// if it has no path to change.
pub fn category_poster(image: &str) -> String {
    let Ok(mut url) = url::Url::parse(image) else {
        return image.to_string();
    };
    let segments = url
        .path_segments()
        .map_or(0, |s| s.filter(|s| !s.is_empty()).count());
    if segments < 2 {
        return image.to_string();
    }
    if let Ok(mut path) = url.path_segments_mut() {
        path.pop().push("poster");
    }
    url.set_query(None);
    url.to_string()
}

/// A Gifbox client for one account (its session token).
#[derive(Clone, Debug)]
pub struct GifboxClient {
    http: Client,
    base_url: String,
    token: String,
}

impl GifboxClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let http = Client::builder()
            .user_agent(concat!("stoat-gpui-poc/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    /// A client for an account on `api_url` with `token`, only if that's
    /// the official instance: Gifbox is Stoat's, and another instance's
    /// token shouldn't go to it (it couldn't use it anyway).
    pub fn for_account(api_url: &str, token: &str) -> Option<Self> {
        (api_url.trim_end_matches('/') == DEFAULT_API_URL).then(|| Self::new(GIFBOX_URL, token))
    }

    /// `GET /categories`: the picker's category tiles.
    pub async fn categories(&self) -> Result<Vec<GifCategory>, ApiError> {
        let value: Value = self.get("/categories", &[]).await?;
        // A list, as Stoat's web client expects; anything else is none.
        Ok(match value {
            Value::Array(items) => lenient_list(items, "category"),
            _ => Vec::new(),
        })
    }

    /// `GET /trending`: up to `limit` popular GIFs.
    pub async fn trending(&self, limit: u32) -> Result<GifPage, ApiError> {
        let limit = limit.to_string();
        self.page("/trending", &[("limit", &limit)]).await
    }

    /// `GET /search`: up to `limit` GIFs for `query`.
    pub async fn search(&self, query: &str, limit: u32) -> Result<GifPage, ApiError> {
        let limit = limit.to_string();
        self.page("/search", &[("query", query), ("limit", &limit)])
            .await
    }

    async fn page(&self, path: &str, params: &[(&str, &str)]) -> Result<GifPage, ApiError> {
        #[derive(Deserialize)]
        struct RawPage {
            #[serde(default)]
            results: Vec<Value>,
            #[serde(default)]
            next: Option<Value>,
        }
        let raw: RawPage = self.get(path, params).await?;
        Ok(GifPage {
            results: lenient_list(raw.results, "GIF"),
            next: raw.next.and_then(|next| match next {
                Value::String(s) if !s.is_empty() => Some(s),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            }),
        })
    }

    /// `GET path?locale=…&params`, with the session token.
    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T, ApiError> {
        let mut query = vec![("locale", LOCALE)];
        query.extend_from_slice(params);
        let query = serde_urlencoded::to_string(&query).expect("Gifbox query is always encodable");
        let url = format!("{}{path}?{query}", self.base_url);
        log::debug!("GET {}{path}", self.base_url);
        let response = self
            .http
            .get(url)
            .header("x-session-token", &self.token)
            .send()
            .await
            .map_err(ApiError::Network)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(ApiError::Network)?;
        if !status.is_success() {
            let err = ApiError::from_response(status, &bytes);
            log::debug!("Gifbox {path} failed: HTTP {status}: {err:?}");
            return Err(err);
        }
        serde_json::from_slice(&bytes).map_err(|e| ApiError::Unexpected {
            status: status.as_u16(),
            detail: format!("couldn't parse Gifbox response: {e}"),
        })
    }
}

/// The items that parse as `T`, skipping (and logging) the rest.
fn lenient_list<T: DeserializeOwned>(items: Vec<Value>, what: &str) -> Vec<T> {
    items
        .into_iter()
        .filter_map(|item| match serde_json::from_value(item) {
            Ok(parsed) => Some(parsed),
            Err(err) => {
                log::debug!("skipping a Gifbox {what} that didn't parse: {err}");
                None
            }
        })
        .collect()
}

/// A map of media formats, skipping entries that don't parse.
fn lenient_map<'de, D>(deserializer: D) -> Result<HashMap<String, GifMedia>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<HashMap<String, Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(raw
        .into_iter()
        .filter_map(|(name, value)| Some((name, serde_json::from_value(value).ok()?)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result(formats: Value) -> GifResult {
        serde_json::from_value(json!({
            "id": "abc",
            "url": "https://gifbox.me/view/abc",
            "media_formats": formats
        }))
        .unwrap()
    }

    #[test]
    fn the_still_is_the_preview() {
        let gif = result(json!({
            "preview": { "url": "https://media.gifbox.me/media/post/abc/poster", "dims": [220, 124] },
            "gif": { "url": "https://media.gifbox.me/media/post/abc/gif_high" }
        }));
        assert_eq!(
            gif.still().map(|m| m.url.as_str()),
            Some("https://media.gifbox.me/media/post/abc/poster")
        );
        assert_eq!(gif.size(), Some((220, 124)));
        assert!(result(json!({ "gif": { "url": "x" } })).still().is_none());
    }

    #[test]
    fn the_animation_is_the_gif_else_the_medium_one() {
        let both = result(json!({
            "gif": { "url": "big" }, "mediumgif": { "url": "medium" }
        }));
        assert_eq!(both.animated().map(|m| m.url.as_str()), Some("big"));
        let medium = result(json!({ "mediumgif": { "url": "medium" } }));
        assert_eq!(medium.animated().map(|m| m.url.as_str()), Some("medium"));
        // Video only: nothing to animate.
        let video = result(json!({
            "preview": { "url": "p" }, "mp4": { "url": "v.mp4" }, "webm": { "url": "v.webm" }
        }));
        assert!(video.animated().is_none());
    }

    #[test]
    fn formats_are_read_leniently() {
        // No formats at all, a format without a URL, odd dimensions.
        let bare: GifResult =
            serde_json::from_value(json!({ "id": "a", "url": "https://gifbox.me/view/a" }))
                .unwrap();
        assert!(bare.media_formats.is_empty());
        assert_eq!(bare.size(), None);
        let odd = result(json!({
            "preview": { "url": "p", "dimensions": [0, 10] },
            "gif": { "size": 12 },
            "mp4": { "url": "v", "dims": [320.0, 180.0], "duration": 2.5 }
        }));
        assert!(odd.animated().is_none());
        assert_eq!(odd.still().and_then(GifMedia::size), None);
        assert_eq!(odd.size(), Some((320, 180)));
    }

    #[test]
    fn a_categorys_poster_replaces_the_last_segment() {
        assert_eq!(
            category_poster("https://media.gifbox.me/media/post/abc/gif_high"),
            "https://media.gifbox.me/media/post/abc/poster"
        );
        assert_eq!(
            category_poster("https://media.gifbox.me/media/post/abc/gif_low?x=1"),
            "https://media.gifbox.me/media/post/abc/poster"
        );
        // Nothing to replace: left alone.
        assert_eq!(category_poster("https://gifbox.me"), "https://gifbox.me");
        assert_eq!(category_poster("not a url"), "not a url");
    }

    #[test]
    fn only_the_official_instance_gets_a_client() {
        assert!(GifboxClient::for_account(DEFAULT_API_URL, "tok").is_some());
        assert!(GifboxClient::for_account("https://api.stoat.chat/", "tok").is_some());
        assert!(GifboxClient::for_account("https://stoat.example.org/api", "tok").is_none());
        assert!(GifboxClient::for_account("https://api.stoat.chat.evil.com", "tok").is_none());
        assert!(GifboxClient::for_account("http://localhost:14702", "tok").is_none());
    }
}
