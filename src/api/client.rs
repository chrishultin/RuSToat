use std::time::Duration;

use reqwest::{Client, Method};
use revolt_models::v0::{
    BulkMessageResponse, DataLogin, MFAResponse, MessageSort, OptionsQueryMessages, ResponseLogin,
    User,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::ApiError;

/// The official Stoat instance. Override with the `STOAT_API_URL` env var.
pub const DEFAULT_API_URL: &str = "https://api.stoat.chat";

/// Session name shown in Stoat's Settings → Sessions, so it's easy to revoke.
pub const SESSION_FRIENDLY_NAME: &str = "Stoat GPUI POC";

/// The part of `GET /` this client uses.
///
/// `revolt-models` has no type for this: the server builds it from its own
/// `revolt-config`, so we read just the two fields we need.
#[derive(Debug, Clone, Deserialize)]
pub struct RootConfig {
    /// API version, e.g. `"0.16.2"`. Stoat still sends it as `revolt`.
    #[serde(rename = "revolt")]
    pub version: String,
    /// Events websocket URL, e.g. `wss://events.stoat.chat`.
    pub ws: String,
    #[serde(default)]
    pub features: RootFeatures,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RootFeatures {
    /// The file server ("Autumn"): icons, avatars, attachments.
    #[serde(default)]
    pub autumn: Option<ServiceFeature>,
    /// The media proxy ("January"): external images, fetched on the user's behalf.
    #[serde(default)]
    pub january: Option<ServiceFeature>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceFeature {
    pub enabled: bool,
    pub url: String,
}

impl RootConfig {
    /// Base URL of the file server, e.g. `https://cdn.stoatusercontent.com`.
    pub fn file_server(&self) -> Option<&str> {
        enabled_url(&self.features.autumn)
    }

    /// Base URL of the media proxy, e.g. `https://proxy.stoatusercontent.com`.
    pub fn proxy_server(&self) -> Option<&str> {
        enabled_url(&self.features.january)
    }
}

fn enabled_url(feature: &Option<ServiceFeature>) -> Option<&str> {
    feature
        .as_ref()
        .filter(|f| f.enabled)
        .map(|f| f.url.as_str())
}

/// Where a file (server icon, avatar, attachment) is served: `{server}/{tag}/{id}`.
pub fn file_url(file_server: &str, file: &revolt_models::v0::File) -> String {
    format!(
        "{}/{}/{}",
        file_server.trim_end_matches('/'),
        file.tag,
        file.id
    )
}

/// A small, cloneable Stoat REST client.
///
/// The methods are async and use `reqwest`, so they must run on the Tokio IO
/// runtime. From UI code, wrap the calls in [`crate::runtime::spawn_io`].
#[derive(Clone, Debug)]
pub struct StoatClient {
    http: Client,
    base_url: String,
    token: Option<String>,
}

impl StoatClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let http = Client::builder()
            .user_agent(concat!("stoat-gpui-poc/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: None,
        }
    }

    /// Client for `STOAT_API_URL`, or the official instance if that isn't set.
    pub fn from_env() -> Self {
        Self::new(std::env::var("STOAT_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.into()))
    }

    /// A copy of this client that sends `token` as `x-session-token`.
    pub fn with_token(&self, token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            ..self.clone()
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// `GET /`: API version and events websocket URL.
    pub async fn root_config(&self) -> Result<RootConfig, ApiError> {
        self.send(Method::GET, "/", None::<&()>).await
    }

    /// `POST /auth/session/login` with email and password.
    ///
    /// May return `ResponseLogin::MFA`. Finish that with [`Self::login_mfa`].
    pub async fn login(&self, email: &str, password: &str) -> Result<ResponseLogin, ApiError> {
        let body = DataLogin::Email {
            email: email.to_string(),
            password: password.to_string(),
            friendly_name: Some(SESSION_FRIENDLY_NAME.into()),
        };
        self.send(Method::POST, "/auth/session/login", Some(&body))
            .await
    }

    /// `POST /auth/session/login`: second step, using the MFA ticket.
    pub async fn login_mfa(
        &self,
        ticket: &str,
        response: MFAResponse,
    ) -> Result<ResponseLogin, ApiError> {
        let body = DataLogin::MFA {
            mfa_ticket: ticket.to_string(),
            mfa_response: Some(response),
            friendly_name: Some(SESSION_FRIENDLY_NAME.into()),
        };
        self.send(Method::POST, "/auth/session/login", Some(&body))
            .await
    }

    /// `GET /users/@me`: the logged-in user. Also confirms the token is valid.
    pub async fn fetch_self(&self) -> Result<User, ApiError> {
        if self.token.is_none() {
            return Err(ApiError::Unauthorized);
        }
        self.send(Method::GET, "/users/@me", None::<&()>).await
    }

    /// `GET /channels/{id}/messages`: up to `limit` messages, newest first,
    /// optionally only those older than `before`. Includes the authors
    /// (`users`, and `members` in server channels).
    pub async fn fetch_messages(
        &self,
        channel: &str,
        before: Option<&str>,
        limit: i64,
    ) -> Result<BulkMessageResponse, ApiError> {
        let path = format!(
            "/channels/{channel}/messages?{}",
            messages_query(before, limit)
        );
        self.send(Method::GET, &path, None::<&()>).await
    }

    /// `POST /auth/session/logout`: end this client's session on the server.
    ///
    /// A token the server already rejects counts as logged out.
    pub async fn logout(&self) -> Result<(), ApiError> {
        if self.token.is_none() {
            return Ok(());
        }
        match self
            .send_raw(Method::POST, "/auth/session/logout", None::<&()>)
            .await
        {
            Ok(_) | Err(ApiError::Unauthorized) => Ok(()),
            Err(err) => Err(err),
        }
    }

    async fn send<B, T>(&self, method: Method, path: &str, body: Option<&B>) -> Result<T, ApiError>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let (status, bytes) = self.send_raw(method, path, body).await?;
        serde_json::from_slice(&bytes).map_err(|e| ApiError::Unexpected {
            status: status.as_u16(),
            detail: format!("couldn't parse response: {e}"),
        })
    }

    /// Send a request and return the raw success body. Error statuses become [`ApiError`].
    async fn send_raw<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<(reqwest::StatusCode, Vec<u8>), ApiError>
    where
        B: Serialize + ?Sized,
    {
        let url = format!("{}{}", self.base_url, path);
        log::debug!("{method} {url}");

        let mut request = self.http.request(method, url);
        if let Some(token) = &self.token {
            request = request.header("x-session-token", token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await.map_err(ApiError::Network)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(ApiError::Network)?;

        if !status.is_success() {
            let err = ApiError::from_response(status, &bytes);
            log::debug!("{path} failed: HTTP {status}: {err:?}");
            return Err(err);
        }
        Ok((status, bytes.to_vec()))
    }
}

/// Query string for message history, built from the backend's own
/// `OptionsQueryMessages`. `sort` must be explicit: the server defaults to
/// `Relevance`, which is for search.
fn messages_query(before: Option<&str>, limit: i64) -> String {
    let options = OptionsQueryMessages {
        limit: Some(limit),
        before: before.map(str::to_string),
        after: None,
        sort: Some(MessageSort::Latest),
        nearby: None,
        include_users: Some(true),
    };
    serde_urlencoded::to_string(&options).expect("message query is always encodable")
}

#[cfg(test)]
mod tests {
    use super::messages_query;

    #[test]
    fn messages_query_is_latest_first_with_users() {
        assert_eq!(
            messages_query(None, 50),
            "limit=50&sort=Latest&include_users=true"
        );
        assert_eq!(
            messages_query(Some("01JC0000000000000000000009"), 50),
            "limit=50&before=01JC0000000000000000000009&sort=Latest&include_users=true"
        );
    }
}
