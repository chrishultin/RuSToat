use std::time::Duration;

use reqwest::{Client, Method};
use revolt_models::v0::{
    AllMemberResponse, BulkMessageResponse, Channel, DataEditMessage, DataLogin, DataMessageSearch,
    DataMessageSend, DataSendFriendRequest, Emoji, MFAResponse, Member, Message, MessageSort,
    OptionsQueryMessages, ReplyIntent, ResponseLogin, User, UserProfile,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::ApiError;

/// Uploads are handed to the connection in pieces this big, for progress.
const UPLOAD_CHUNK: usize = 64 * 1024;
/// A whole upload may take this long (the client's usual 30 s is too short
/// for 20 MB on a slow link).
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The official Stoat instance. Override with the `STOAT_API_URL` env var.
pub const DEFAULT_API_URL: &str = "https://api.stoat.chat";

/// Session name shown in Stoat's Settings → Sessions, so it's easy to revoke.
pub const SESSION_FRIENDLY_NAME: &str = "RuSToat";

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
    /// What accounts may do: established ones, and new ones (their first days).
    #[serde(default)]
    pub limits: RootLimits,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RootLimits {
    #[serde(default)]
    pub default: AccountLimits,
    #[serde(default)]
    pub new_user: AccountLimits,
}

/// The limits this client uses, out of many.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AccountLimits {
    /// Most files one message can carry.
    #[serde(default)]
    pub message_attachments: Option<usize>,
    #[serde(default)]
    pub file_upload_size_limits: FileSizeLimits,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FileSizeLimits {
    /// Largest attachment, in bytes.
    #[serde(default)]
    pub attachments: Option<u64>,
}

/// Used when `GET /` doesn't say: Stoat's own defaults (20 MB, 5 files).
pub const DEFAULT_ATTACHMENT_SIZE_LIMIT: u64 = 20_000_000;
pub const DEFAULT_MAX_ATTACHMENTS: usize = 5;

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

    /// Largest attachment, in bytes. The stricter of the two account limits
    /// (this client doesn't know which applies), else
    /// [`DEFAULT_ATTACHMENT_SIZE_LIMIT`].
    pub fn attachment_size_limit(&self) -> u64 {
        let limits = &self.features.limits;
        [&limits.default, &limits.new_user]
            .iter()
            .filter_map(|l| l.file_upload_size_limits.attachments)
            .min()
            .unwrap_or(DEFAULT_ATTACHMENT_SIZE_LIMIT)
    }

    /// Most files one message can carry, chosen the same way, else
    /// [`DEFAULT_MAX_ATTACHMENTS`].
    pub fn max_attachments(&self) -> usize {
        let limits = &self.features.limits;
        [&limits.default, &limits.new_user]
            .iter()
            .filter_map(|l| l.message_attachments)
            .min()
            .unwrap_or(DEFAULT_MAX_ATTACHMENTS)
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
            messages_query(before, None, MessageSort::Latest, limit)
        );
        self.send(Method::GET, &path, None::<&()>).await
    }

    /// `GET /channels/{id}/messages?after={message}&sort=Oldest`: up to
    /// `limit` messages newer than `after`, oldest first, with their authors.
    /// This is how a window that isn't at the present catches up with it.
    pub async fn fetch_messages_after(
        &self,
        channel: &str,
        after: &str,
        limit: i64,
    ) -> Result<BulkMessageResponse, ApiError> {
        let path = format!(
            "/channels/{channel}/messages?{}",
            messages_query(None, Some(after), MessageSort::Oldest, limit)
        );
        self.send(Method::GET, &path, None::<&()>).await
    }

    /// `POST /channels/{channel}/messages`: send `content` with the uploaded
    /// files `attachments` (their IDs, see [`Self::upload_attachment`]),
    /// replying to `replies` (if any). `content` may be empty if there are
    /// attachments; it's left out then. `idempotency_key` (at most 64 bytes)
    /// makes a retry of the same send harmless: Stoat refuses a key it has
    /// already seen with `DuplicateNonce`, and copies it into the new
    /// message's `nonce`.
    pub async fn send_message(
        &self,
        channel: &str,
        content: &str,
        attachments: &[String],
        replies: &[ReplyIntent],
        idempotency_key: &str,
    ) -> Result<Message, ApiError> {
        let body = DataMessageSend {
            nonce: None,
            content: (!content.is_empty()).then(|| content.to_string()),
            attachments: (!attachments.is_empty()).then(|| attachments.to_vec()),
            replies: (!replies.is_empty()).then(|| replies.to_vec()),
            embeds: None,
            masquerade: None,
            interactions: None,
            flags: None,
        };
        // `DataMessageSend` writes unset fields as `null`; send only what's set.
        let body = without_nulls(serde_json::to_value(&body).expect("message body serialises"));
        let (status, bytes) = self
            .send_raw_with(
                Method::POST,
                &format!("/channels/{channel}/messages"),
                Some(&body),
                &[("Idempotency-Key", idempotency_key)],
            )
            .await?;
        parse(status, &bytes)
    }

    /// `POST {file_server}/attachments`: upload the file at `path` for a
    /// message, as the multipart field `file`. Returns its ID, to send in
    /// [`Self::send_message`]'s `attachments` (each ID can be used once).
    ///
    /// The file is read from disk in chunks as it's sent, never held whole.
    /// Its size is taken when the upload starts; if it can't be opened, or
    /// gets shorter mid-upload, that's [`ApiError::File`].
    ///
    /// `on_progress(sent, total)` is called as the body is handed to the
    /// connection, ending with `(total, total)`.
    pub async fn upload_attachment(
        &self,
        file_server: &str,
        name: &str,
        content_type: &str,
        path: &std::path::Path,
        on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Result<String, ApiError> {
        use std::io::{Error, ErrorKind};
        use std::sync::{Arc, Mutex};

        use reqwest::multipart::{Form, Part};
        use tokio::io::AsyncReadExt as _;

        #[derive(Deserialize)]
        struct Uploaded {
            id: String,
        }

        let file = tokio::fs::File::open(path).await.map_err(ApiError::File)?;
        let total = file.metadata().await.map_err(ApiError::File)?.len();
        // A read failure only reaches reqwest as a broken body; keep what
        // it was, to report it as the file's fault rather than the network's.
        let read_failure: Arc<Mutex<Option<(ErrorKind, String)>>> = Arc::default();
        let (failure, on_progress) = (read_failure.clone(), Arc::new(on_progress));
        let chunks = futures::stream::try_unfold((file, 0u64), move |(mut file, sent)| {
            let (on_progress, failure) = (on_progress.clone(), failure.clone());
            async move {
                if sent >= total {
                    return Ok(None);
                }
                // Exactly `total` bytes, even if the file has grown since.
                let mut chunk = vec![0; (total - sent).min(UPLOAD_CHUNK as u64) as usize];
                let read = match file.read(&mut chunk).await {
                    Ok(0) => Err(Error::new(ErrorKind::UnexpectedEof, "it got shorter while sending")),
                    other => other,
                };
                let read = read.inspect_err(|err| {
                    *failure.lock().unwrap() = Some((err.kind(), err.to_string()));
                })?;
                chunk.truncate(read);
                let sent = sent + read as u64;
                on_progress(sent, total);
                Ok::<_, Error>(Some((chunk, (file, sent))))
            }
        });
        let part = Part::stream_with_length(reqwest::Body::wrap_stream(chunks), total)
            .file_name(name.to_string())
            .mime_str(content_type)
            .map_err(|e| ApiError::Unexpected {
                status: 0,
                detail: format!("bad content type {content_type:?}: {e}"),
            })?;
        let form = Form::new().part("file", part);

        let url = format!("{}/attachments", file_server.trim_end_matches('/'));
        log::debug!("POST {url} ({name}, {content_type}, {total} bytes)");
        let mut request = self.http.post(&url).multipart(form).timeout(UPLOAD_TIMEOUT);
        if let Some(token) = &self.token {
            request = request.header("x-session-token", token);
        }
        let file_error = || {
            let failure = read_failure.lock().unwrap().take();
            failure.map(|(kind, text)| ApiError::File(Error::new(kind, text)))
        };
        let response = match request.send().await {
            Ok(response) => response,
            Err(err) => return Err(file_error().unwrap_or(ApiError::Network(err))),
        };
        if let Some(err) = file_error() {
            return Err(err);
        }
        let status = response.status();
        let body = response.bytes().await.map_err(ApiError::Network)?;
        log::debug!("upload {name}: HTTP {status}: {}", String::from_utf8_lossy(&body));
        if !status.is_success() {
            return Err(ApiError::from_response(status, &body));
        }
        parse::<Uploaded>(status, &body).map(|u| u.id)
    }

    /// `PATCH /channels/{channel}/messages/{message}`: replace the text of
    /// one of the user's messages. `content` must not be empty.
    pub async fn edit_message(
        &self,
        channel: &str,
        message: &str,
        content: &str,
    ) -> Result<Message, ApiError> {
        let body = DataEditMessage {
            content: Some(content.to_string()),
            embeds: None,
        };
        let body = without_nulls(serde_json::to_value(&body).expect("edit body serialises"));
        self.send(
            Method::PATCH,
            &format!("/channels/{channel}/messages/{message}"),
            Some(&body),
        )
        .await
    }

    /// `DELETE /channels/{channel}/messages/{message}`: delete one of the
    /// user's messages.
    pub async fn delete_message(&self, channel: &str, message: &str) -> Result<(), ApiError> {
        self.send_raw(
            Method::DELETE,
            &format!("/channels/{channel}/messages/{message}"),
            None::<&()>,
        )
        .await
        .map(|_| ())
    }

    /// `GET /channels/{id}/messages?nearby={message}`: `message` itself (if
    /// it still exists) and up to `limit / 2 + 1` messages on each side,
    /// with their authors. Unlike `GET /channels/{id}/messages/{message}`,
    /// this includes the authors' `users` and `members`. A deleted message is
    /// simply missing from the result.
    pub async fn fetch_messages_around(
        &self,
        channel: &str,
        message: &str,
        limit: i64,
    ) -> Result<BulkMessageResponse, ApiError> {
        let options = OptionsQueryMessages {
            limit: Some(limit),
            before: None,
            after: None,
            sort: None,
            nearby: Some(message.to_string()),
            include_users: Some(true),
        };
        let query = serde_urlencoded::to_string(&options).expect("message query is always encodable");
        self.send(
            Method::GET,
            &format!("/channels/{channel}/messages?{query}"),
            None::<&()>,
        )
        .await
    }

    /// `POST /channels/{channel}/search`: up to `limit` (1 to 100) messages
    /// in `channel` that match `query` (1 to 64 characters), in `sort` order,
    /// with their authors. Stoat matches whole words, stemmed, not substrings,
    /// and searches one channel only.
    pub async fn search_messages(
        &self,
        channel: &str,
        query: &str,
        sort: MessageSort,
        limit: i64,
    ) -> Result<BulkMessageResponse, ApiError> {
        self.send(
            Method::POST,
            &format!("/channels/{channel}/search"),
            Some(&search_body(query, sort, limit)),
        )
        .await
    }

    /// `POST /channels/{channel}/search` with `pinned: true` and no query:
    /// up to `limit` (1 to 100) of the channel's pinned messages, newest
    /// first, with their authors. There's no endpoint just for pins.
    pub async fn fetch_pins(
        &self,
        channel: &str,
        limit: i64,
    ) -> Result<BulkMessageResponse, ApiError> {
        self.send(
            Method::POST,
            &format!("/channels/{channel}/search"),
            Some(&pins_body(limit)),
        )
        .await
    }

    /// `POST /channels/{channel}/messages/{message}/pin`: pin a message.
    /// Needs Manage Messages (not in a DM). Stoat also posts a "pinned a
    /// message" system message. An already pinned one is `AlreadyPinned`.
    pub async fn pin_message(&self, channel: &str, message: &str) -> Result<(), ApiError> {
        self.send_raw(Method::POST, &pin_path(channel, message), None::<&()>)
            .await
            .map(|_| ())
    }

    /// `DELETE` on the same path: unpin it. One that isn't pinned is `NotPinned`.
    pub async fn unpin_message(&self, channel: &str, message: &str) -> Result<(), ApiError> {
        self.send_raw(Method::DELETE, &pin_path(channel, message), None::<&()>)
            .await
            .map(|_| ())
    }

    /// `GET /servers/{server}/members`: every member of `server`, with
    /// their users. Stoat 0.16.2 has no member search, so it's the whole list.
    /// `exclude_offline` asks for the online members only (the web client
    /// does that for servers of more than 1000).
    pub async fn fetch_members(
        &self,
        server: &str,
        exclude_offline: bool,
    ) -> Result<AllMemberResponse, ApiError> {
        self.send(
            Method::GET,
            &members_path(server, exclude_offline),
            None::<&()>,
        )
        .await
    }

    /// `GET /servers/{server}/members/{user}`: one membership. For the
    /// user's own, after joining a server: Stoat doesn't send that session
    /// its own `ServerMemberJoin`. Someone who isn't a member is `NotFound`.
    pub async fn fetch_member(&self, server: &str, user: &str) -> Result<Member, ApiError> {
        self.send(
            Method::GET,
            &format!("/servers/{server}/members/{user}"),
            None::<&()>,
        )
        .await
    }

    /// `GET /users/{id}`: one user (someone who just joined a server, so
    /// they can be mentioned).
    pub async fn fetch_user(&self, id: &str) -> Result<User, ApiError> {
        self.send(Method::GET, &format!("/users/{id}"), None::<&()>)
            .await
    }

    /// `GET /users/{id}/profile`: the bio and banner. Needs a connection to
    /// them (a friend, a shared server, group or DM, or a bot); otherwise
    /// it's `MissingPermission`.
    pub async fn fetch_profile(&self, id: &str) -> Result<UserProfile, ApiError> {
        self.send(Method::GET, &format!("/users/{id}/profile"), None::<&()>)
            .await
    }

    /// `GET /users/{id}/dm`: the user's DM with `id`, created (open) if there
    /// isn't one yet. An existing one comes back as it is, even if closed.
    /// Needs Send Message on them: they're a friend, or one of the two is a
    /// bot sharing a server or group; otherwise it's `MissingPermission`.
    /// For the user themselves, it's their Saved Notes.
    pub async fn open_dm(&self, id: &str) -> Result<Channel, ApiError> {
        self.send(Method::GET, &format!("/users/{id}/dm"), None::<&()>)
            .await
    }

    /// `PUT /users/{id}/friend`: send them a friend request, or accept
    /// theirs. Stoat answers with them, as they now relate to the user.
    pub async fn add_friend(&self, id: &str) -> Result<User, ApiError> {
        self.send(Method::PUT, &format!("/users/{id}/friend"), None::<&()>)
            .await
    }

    /// `POST /users/friend`: send a friend request by `username#1234`.
    /// Stoat answers with them, as they now relate to the user.
    pub async fn send_friend_request(&self, username: &str) -> Result<User, ApiError> {
        let body = DataSendFriendRequest {
            username: username.to_string(),
        };
        self.send(Method::POST, "/users/friend", Some(&body)).await
    }

    /// `DELETE /users/{id}/friend`: reject or cancel a friend request, or
    /// stop being friends.
    pub async fn remove_friend(&self, id: &str) -> Result<User, ApiError> {
        self.send(Method::DELETE, &format!("/users/{id}/friend"), None::<&()>)
            .await
    }

    /// `PUT /users/{id}/block`: block them (bots too).
    pub async fn block_user(&self, id: &str) -> Result<User, ApiError> {
        self.send(Method::PUT, &format!("/users/{id}/block"), None::<&()>)
            .await
    }

    /// `DELETE /users/{id}/block`: unblock them.
    pub async fn unblock_user(&self, id: &str) -> Result<User, ApiError> {
        self.send(Method::DELETE, &format!("/users/{id}/block"), None::<&()>)
            .await
    }

    /// `GET /custom/emoji/{id}`: one custom emoji, from any server (one the
    /// user isn't in, seen in a reaction or message, so it can be named).
    pub async fn fetch_emoji(&self, id: &str) -> Result<Emoji, ApiError> {
        self.send(Method::GET, &format!("/custom/emoji/{id}"), None::<&()>)
            .await
    }

    /// `PUT /channels/{channel}/ack/{message}`: the user has read `channel`
    /// up to and including `message`.
    pub async fn ack_message(&self, channel: &str, message: &str) -> Result<(), ApiError> {
        self.send_raw(
            Method::PUT,
            &format!("/channels/{channel}/ack/{message}"),
            None::<&()>,
        )
        .await
        .map(|_| ())
    }

    /// `PUT /channels/{channel}/messages/{message}/reactions/{emoji}`: react
    /// with `emoji` (a Unicode emoji, or a custom emoji's ID).
    pub async fn react(&self, channel: &str, message: &str, emoji: &str) -> Result<(), ApiError> {
        self.send_raw(
            Method::PUT,
            &reaction_path(channel, message, emoji),
            None::<&()>,
        )
        .await
        .map(|_| ())
    }

    /// `DELETE` on the same path: take our own `emoji` reaction back.
    pub async fn unreact(&self, channel: &str, message: &str, emoji: &str) -> Result<(), ApiError> {
        self.send_raw(
            Method::DELETE,
            &reaction_path(channel, message, emoji),
            None::<&()>,
        )
        .await
        .map(|_| ())
    }

    /// `PUT /servers/{server}/ack`: mark every channel in `server` read.
    pub async fn ack_server(&self, server: &str) -> Result<(), ApiError> {
        self.send_raw(Method::PUT, &format!("/servers/{server}/ack"), None::<&()>)
            .await
            .map(|_| ())
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
        parse(status, &bytes)
    }

    async fn send_raw<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<(reqwest::StatusCode, Vec<u8>), ApiError>
    where
        B: Serialize + ?Sized,
    {
        self.send_raw_with(method, path, body, &[]).await
    }

    /// Send a request with extra `headers` and return the raw success body.
    /// Error statuses become [`ApiError`].
    async fn send_raw_with<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        headers: &[(&str, &str)],
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
        for (name, value) in headers {
            request = request.header(*name, *value);
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

fn parse<T: DeserializeOwned>(status: reqwest::StatusCode, bytes: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Unexpected {
        status: status.as_u16(),
        detail: format!("couldn't parse response: {e}"),
    })
}

/// `value` with every `null` object field removed, at any depth.
fn without_nulls(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, without_nulls(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(without_nulls).collect()),
        other => other,
    }
}

fn reaction_path(channel: &str, message: &str, emoji: &str) -> String {
    format!(
        "/channels/{channel}/messages/{message}/reactions/{}",
        encode_path_segment(emoji)
    )
}

/// Percent-encode everything but RFC 3986's unreserved characters, so a
/// Unicode emoji (`👍` → `%F0%9F%91%8D`) is one path segment. Custom emoji
/// IDs pass through unchanged.
fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The body of a message search. Stoat refuses `pinned` together with
/// `query`, even `pinned: false`, so unset fields are left out rather than
/// sent as `null` (the same way as [`StoatClient::send_message`]).
fn search_body(query: &str, sort: MessageSort, limit: i64) -> serde_json::Value {
    let body = DataMessageSearch {
        query: Some(query.to_string()),
        pinned: None,
        limit: Some(limit),
        before: None,
        after: None,
        sort,
        include_users: Some(true),
    };
    without_nulls(serde_json::to_value(&body).expect("search body serialises"))
}

/// The body of a search for pinned messages: no `query`, because Stoat
/// refuses `pinned` together with one. Newest first, with authors.
fn pins_body(limit: i64) -> serde_json::Value {
    let body = DataMessageSearch {
        query: None,
        pinned: Some(true),
        limit: Some(limit),
        before: None,
        after: None,
        sort: MessageSort::Latest,
        include_users: Some(true),
    };
    without_nulls(serde_json::to_value(&body).expect("pins body serialises"))
}

fn pin_path(channel: &str, message: &str) -> String {
    format!("/channels/{channel}/messages/{message}/pin")
}

/// Path of a server's member list, online members only if asked.
fn members_path(server: &str, exclude_offline: bool) -> String {
    if exclude_offline {
        format!("/servers/{server}/members?exclude_offline=true")
    } else {
        format!("/servers/{server}/members")
    }
}

/// Query string for message history, built from the backend's own
/// `OptionsQueryMessages`. `sort` must be explicit: the server defaults to
/// `Relevance`, which is for search.
fn messages_query(
    before: Option<&str>,
    after: Option<&str>,
    sort: MessageSort,
    limit: i64,
) -> String {
    let options = OptionsQueryMessages {
        limit: Some(limit),
        before: before.map(str::to_string),
        after: after.map(str::to_string),
        sort: Some(sort),
        nearby: None,
        include_users: Some(true),
    };
    serde_urlencoded::to_string(&options).expect("message query is always encodable")
}

#[cfg(test)]
mod tests {
    use revolt_models::v0::MessageSort;

    use serde_json::json;

    use super::{members_path, messages_query, pin_path, pins_body, search_body};

    #[test]
    fn members_path_asks_for_online_members_only_when_told_to() {
        assert_eq!(members_path("srv", false), "/servers/srv/members");
        assert_eq!(
            members_path("srv", true),
            "/servers/srv/members?exclude_offline=true"
        );
    }

    #[test]
    fn messages_query_is_latest_first_with_users() {
        assert_eq!(
            messages_query(None, None, MessageSort::Latest, 50),
            "limit=50&sort=Latest&include_users=true"
        );
        assert_eq!(
            messages_query(
                Some("01JC0000000000000000000009"),
                None,
                MessageSort::Latest,
                50
            ),
            "limit=50&before=01JC0000000000000000000009&sort=Latest&include_users=true"
        );
    }

    #[test]
    fn messages_query_after_is_oldest_first() {
        assert_eq!(
            messages_query(
                None,
                Some("01JC0000000000000000000009"),
                MessageSort::Oldest,
                50
            ),
            "limit=50&after=01JC0000000000000000000009&sort=Oldest&include_users=true"
        );
    }

    #[test]
    fn search_body_has_no_pinned_and_no_nulls() {
        let body = search_body("hello", MessageSort::Latest, 50);
        assert_eq!(
            body,
            json!({
                "query": "hello",
                "limit": 50,
                "sort": "Latest",
                "include_users": true
            })
        );
        assert!(body.get("pinned").is_none());
    }

    #[test]
    fn search_body_carries_the_chosen_sort() {
        for (sort, name) in [
            (MessageSort::Latest, "Latest"),
            (MessageSort::Oldest, "Oldest"),
            (MessageSort::Relevance, "Relevance"),
        ] {
            assert_eq!(search_body("x", sort, 50)["sort"], name);
        }
    }

    #[test]
    fn pins_body_is_pinned_with_no_query() {
        let body = pins_body(100);
        assert_eq!(
            body,
            json!({
                "pinned": true,
                "limit": 100,
                "sort": "Latest",
                "include_users": true
            })
        );
        assert!(body.get("query").is_none());
    }

    #[test]
    fn pin_path_is_under_the_message() {
        assert_eq!(pin_path("c", "m"), "/channels/c/messages/m/pin");
    }
}
