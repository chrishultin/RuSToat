//! Stoat's events websocket (the "Bonfire" service).
//!
//! The backend's own event enum (`EventV1`) lives in the AGPL `revolt-database`
//! crate, so this client decodes the handful of events it needs itself. Every
//! payload is a `revolt-models` type. Decoding is lenient: unknown event types
//! become [`ServerEvent::Unknown`], and a `Ready` list item that doesn't parse
//! is skipped and counted instead of failing the whole payload.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use revolt_models::v0::{
    Channel, ChannelUnread, Embed, Emoji, FieldsChannel, FieldsMessage, Member, Message,
    PartialChannel, PartialMessage, Server, User,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::{ApiError, StoatClient};
use crate::notify::NotificationSettings;
use crate::runtime::runtime;

/// How often to ping. Stoat drops connections that are quiet for 10–30 s.
pub const PING_INTERVAL: Duration = Duration::from_secs(20);
/// Give up on a connection that hasn't answered a ping for this long.
const PONG_TIMEOUT: Duration = Duration::from_secs(65);
const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// `Ready` fields to request. Everything else (emojis, unreads, voice states…)
/// is skipped to keep the initial payload small.
const READY_FIELDS: &[&str] = &[
    "users",
    "servers",
    "channels",
    "members",
    "emojis",
    "channel_unreads",
    "user_settings[ordering]",
    "user_settings[notifications]",
];

/// A decoded event from the server.
#[derive(Debug)]
pub enum ServerEvent {
    Authenticated,
    Ready(Box<Ready>),
    Pong,
    Error(revolt_result::Error),
    Logout,
    /// A new message in any channel the user can see.
    Message(Box<Message>),
    /// An edit: merge `data` into the message, then clear the `clear` fields.
    MessageUpdate {
        id: String,
        channel: String,
        data: Box<PartialMessage>,
        clear: Vec<FieldsMessage>,
    },
    MessageDelete {
        id: String,
        channel: String,
    },
    /// Embeds (link previews) worked out after the message was sent. They
    /// arrive a moment after the message itself.
    MessageAppend {
        id: String,
        channel: String,
        embeds: Vec<Embed>,
    },
    BulkMessageDelete {
        channel: String,
        ids: Vec<String>,
    },
    /// `user` reacted to a message with `emoji` (a Unicode emoji, or a
    /// custom emoji's ID).
    MessageReact {
        id: String,
        channel: String,
        user: String,
        emoji: String,
    },
    /// `user` took their `emoji` reaction back.
    MessageUnreact {
        id: String,
        channel: String,
        user: String,
        emoji: String,
    },
    /// A moderator cleared every `emoji` reaction from a message.
    MessageRemoveReaction {
        id: String,
        channel: String,
        emoji: String,
    },
    EmojiCreate(Box<Emoji>),
    /// Some user settings changed. Only `notifications` is kept; `None` if
    /// this update didn't touch it.
    UserSettingsUpdate {
        notifications: Option<NotificationSettings>,
    },
    /// A channel was read up to `message_id` (by `user`: one of our own
    /// sessions, possibly another client).
    ChannelAck {
        channel: String,
        user: String,
        message_id: String,
    },
    EmojiDelete {
        id: String,
    },
    /// `user` started typing in `channel` (again: it's resent while they type).
    ChannelStartTyping {
        channel: String,
        user: String,
    },
    /// `user` stopped typing in `channel`.
    ChannelStopTyping {
        channel: String,
        user: String,
    },
    /// A channel the user can now see: a DM or group someone just opened with
    /// them, or a new server channel. It comes before the channel's first
    /// message.
    ChannelCreate(Box<Channel>),
    /// A channel changed: merge `data`, then clear the `clear` fields. A DM
    /// reopening (`active: true`) arrives this way.
    ChannelUpdate {
        id: String,
        data: Box<PartialChannel>,
        clear: Vec<FieldsChannel>,
    },
    /// An event type this client doesn't handle (yet). Holds the `type` tag.
    Unknown(String),
}

/// The initial state the server sends after authenticating.
#[derive(Debug, Default)]
pub struct Ready {
    pub users: Vec<User>,
    pub servers: Vec<Server>,
    pub channels: Vec<Channel>,
    /// The logged-in user's own membership in each server.
    pub members: Vec<Member>,
    /// Custom emoji of the servers the user is in.
    pub emojis: Vec<Emoji>,
    /// The user's read position (and unread mentions) per channel.
    pub unreads: Vec<ChannelUnread>,
    /// Server IDs in the order the user arranged them, if they have.
    pub server_order: Vec<String>,
    /// The `notifications` user setting, if the user has one.
    pub notifications: Option<NotificationSettings>,
    /// List items that didn't parse (for example a new channel type) and were dropped.
    pub skipped: usize,
}

/// Decode one websocket text frame. `Bulk` frames are flattened.
pub fn parse_frame(text: &str) -> Vec<ServerEvent> {
    let mut events = Vec::new();
    match serde_json::from_str::<Value>(text) {
        Ok(value) => decode(value, &mut events),
        Err(err) => log::warn!("ignoring unparseable event frame: {err}"),
    }
    events
}

fn decode(mut value: Value, out: &mut Vec<ServerEvent>) {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match kind.as_str() {
        "Bulk" => {
            if let Some(Value::Array(items)) = value.get_mut("v").map(Value::take) {
                for item in items {
                    decode(item, out);
                }
            }
        }
        "Authenticated" => out.push(ServerEvent::Authenticated),
        "Logout" => out.push(ServerEvent::Logout),
        "Pong" => out.push(ServerEvent::Pong),
        "Error" => match value
            .get_mut("data")
            .map(Value::take)
            .map(serde_json::from_value::<revolt_result::Error>)
        {
            Some(Ok(err)) => out.push(ServerEvent::Error(err)),
            _ => out.push(ServerEvent::Unknown("Error (unrecognised body)".into())),
        },
        "Ready" => out.push(ServerEvent::Ready(Box::new(Ready::from_value(value)))),
        "Message" => out.push(decode_as(value, &kind, |m: Message| {
            ServerEvent::Message(Box::new(m))
        })),
        "MessageUpdate" => {
            #[derive(Deserialize)]
            struct Update {
                id: String,
                channel: String,
                data: PartialMessage,
                #[serde(default)]
                clear: Vec<FieldsMessage>,
            }
            out.push(decode_as(value, &kind, |u: Update| {
                ServerEvent::MessageUpdate {
                    id: u.id,
                    channel: u.channel,
                    data: Box::new(u.data),
                    clear: u.clear,
                }
            }))
        }
        "MessageDelete" => {
            #[derive(Deserialize)]
            struct Delete {
                id: String,
                channel: String,
            }
            out.push(decode_as(value, &kind, |d: Delete| {
                ServerEvent::MessageDelete {
                    id: d.id,
                    channel: d.channel,
                }
            }))
        }
        "MessageAppend" => {
            #[derive(Deserialize)]
            struct Append {
                id: String,
                channel: String,
                #[serde(default)]
                append: Value,
            }
            // Embeds one by one, so a kind this client doesn't know (a new
            // `special`) costs that embed, not the others.
            out.push(decode_as(value, &kind, |mut a: Append| {
                let mut skipped = 0;
                let embeds = lenient_list(
                    a.append.get_mut("embeds").map(Value::take),
                    "embed in MessageAppend",
                    &mut skipped,
                );
                ServerEvent::MessageAppend {
                    id: a.id,
                    channel: a.channel,
                    embeds,
                }
            }))
        }
        // Unlike the other message events, these name the channel `channel_id`.
        "MessageReact" | "MessageUnreact" => {
            #[derive(Deserialize)]
            struct React {
                id: String,
                channel_id: String,
                user_id: String,
                emoji_id: String,
            }
            let add = kind == "MessageReact";
            out.push(decode_as(value, &kind, |r: React| {
                if add {
                    ServerEvent::MessageReact {
                        id: r.id,
                        channel: r.channel_id,
                        user: r.user_id,
                        emoji: r.emoji_id,
                    }
                } else {
                    ServerEvent::MessageUnreact {
                        id: r.id,
                        channel: r.channel_id,
                        user: r.user_id,
                        emoji: r.emoji_id,
                    }
                }
            }))
        }
        "MessageRemoveReaction" => {
            #[derive(Deserialize)]
            struct Remove {
                id: String,
                channel_id: String,
                emoji_id: String,
            }
            out.push(decode_as(value, &kind, |r: Remove| {
                ServerEvent::MessageRemoveReaction {
                    id: r.id,
                    channel: r.channel_id,
                    emoji: r.emoji_id,
                }
            }))
        }
        "BulkMessageDelete" => {
            #[derive(Deserialize)]
            struct BulkDelete {
                channel: String,
                ids: Vec<String>,
            }
            out.push(decode_as(value, &kind, |d: BulkDelete| {
                ServerEvent::BulkMessageDelete {
                    channel: d.channel,
                    ids: d.ids,
                }
            }))
        }
        "ChannelAck" => {
            #[derive(Deserialize)]
            struct Ack {
                id: String,
                user: String,
                message_id: String,
            }
            out.push(decode_as(value, &kind, |a: Ack| ServerEvent::ChannelAck {
                channel: a.id,
                user: a.user,
                message_id: a.message_id,
            }))
        }
        "UserSettingsUpdate" => {
            let update = value.get_mut("update").map(Value::take);
            out.push(ServerEvent::UserSettingsUpdate {
                notifications: update.and_then(notifications_from_settings),
            });
        }
        "EmojiCreate" => out.push(decode_as(value, &kind, |e: Emoji| {
            ServerEvent::EmojiCreate(Box::new(e))
        })),
        "EmojiDelete" => {
            #[derive(Deserialize)]
            struct Delete {
                id: String,
            }
            out.push(decode_as(value, &kind, |d: Delete| {
                ServerEvent::EmojiDelete { id: d.id }
            }))
        }
        // `id` is the channel.
        "ChannelStartTyping" | "ChannelStopTyping" => {
            #[derive(Deserialize)]
            struct Typing {
                id: String,
                user: String,
            }
            let start = kind == "ChannelStartTyping";
            out.push(decode_as(value, &kind, |t: Typing| {
                if start {
                    ServerEvent::ChannelStartTyping {
                        channel: t.id,
                        user: t.user,
                    }
                } else {
                    ServerEvent::ChannelStopTyping {
                        channel: t.id,
                        user: t.user,
                    }
                }
            }))
        }
        "ChannelCreate" => out.push(decode_as(value, &kind, |c: Channel| {
            ServerEvent::ChannelCreate(Box::new(c))
        })),
        "ChannelUpdate" => {
            #[derive(Deserialize)]
            struct Update {
                id: String,
                data: PartialChannel,
                #[serde(default)]
                clear: Vec<FieldsChannel>,
            }
            out.push(decode_as(value, &kind, |u: Update| {
                ServerEvent::ChannelUpdate {
                    id: u.id,
                    data: Box::new(u.data),
                    clear: u.clear,
                }
            }))
        }
        other => out.push(ServerEvent::Unknown(other.to_string())),
    }
}

/// Decode `value` as `T` and wrap it, or report it as an unparseable `kind`.
fn decode_as<T: DeserializeOwned>(
    value: Value,
    kind: &str,
    wrap: impl FnOnce(T) -> ServerEvent,
) -> ServerEvent {
    match serde_json::from_value(value) {
        Ok(parsed) => wrap(parsed),
        Err(err) => {
            log::warn!("ignoring {kind} event that didn't parse: {err}");
            ServerEvent::Unknown(format!("{kind} (unparseable)"))
        }
    }
}

impl Ready {
    fn from_value(mut value: Value) -> Self {
        let mut skipped = 0;
        let mut take = |field: &str| value.get_mut(field).map(Value::take);
        let users = lenient_list(take("users"), "user in Ready", &mut skipped);
        let servers = lenient_list(take("servers"), "server in Ready", &mut skipped);
        let channels = lenient_list(take("channels"), "channel in Ready", &mut skipped);
        let members = lenient_list(take("members"), "member in Ready", &mut skipped);
        let emojis = lenient_list(take("emojis"), "emoji in Ready", &mut skipped);
        let unreads = lenient_list(
            take("channel_unreads"),
            "channel unread in Ready",
            &mut skipped,
        );
        let settings = take("user_settings");
        let server_order = settings
            .clone()
            .map(server_order_from_settings)
            .unwrap_or_default();
        let notifications = settings.and_then(notifications_from_settings);
        Ready {
            users,
            servers,
            channels,
            members,
            emojis,
            unreads,
            server_order,
            notifications,
            skipped,
        }
    }
}

/// The `notifications` user setting (`[updated_at, "<json>"]`), parsed.
fn notifications_from_settings(settings: Value) -> Option<NotificationSettings> {
    let mut parsed: HashMap<String, (i64, String)> = serde_json::from_value(settings).ok()?;
    let (_, json) = parsed.remove("notifications")?;
    Some(NotificationSettings::parse(&json))
}

fn lenient_list<T: DeserializeOwned>(
    list: Option<Value>,
    what: &str,
    skipped: &mut usize,
) -> Vec<T> {
    let Some(Value::Array(items)) = list else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| match serde_json::from_value(item) {
            Ok(parsed) => Some(parsed),
            Err(err) => {
                *skipped += 1;
                log::warn!("skipping {what} that didn't parse: {err}");
                None
            }
        })
        .collect()
}

/// Server order from the `ordering` user setting.
///
/// User settings are `{key: [updated_at, "<json string>"]}` (`UserSettings`
/// in `revolt-models`). The web client stores `{"servers": [ids…]}` under
/// `ordering`. Anything unexpected yields an empty list (Ready order is used).
fn server_order_from_settings(settings: Value) -> Vec<String> {
    let parsed: Result<HashMap<String, (i64, String)>, _> = serde_json::from_value(settings);
    let Some((_, ordering)) = parsed.ok().and_then(|mut s| s.remove("ordering")) else {
        return Vec::new();
    };
    serde_json::from_str::<Value>(&ordering)
        .ok()
        .and_then(|v| v.get("servers").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// `{"type":"Authenticate","token":…}`, sent right after connecting.
/// (Keeps the token out of the URL.)
pub fn authenticate_frame(token: &str) -> String {
    json!({ "type": "Authenticate", "token": token }).to_string()
}

/// `{"type":"BeginTyping","channel":…}`: the user is typing in `channel`.
pub fn begin_typing_frame(channel: &str) -> String {
    json!({ "type": "BeginTyping", "channel": channel }).to_string()
}

/// `{"type":"EndTyping","channel":…}`: the user stopped typing in `channel`.
pub fn end_typing_frame(channel: &str) -> String {
    json!({ "type": "EndTyping", "channel": channel }).to_string()
}

/// `{"type":"Ping","data":n}`.
pub fn ping_frame(n: u64) -> String {
    json!({ "type": "Ping", "data": n }).to_string()
}

/// The events URL with protocol and `Ready`-trimming query parameters.
///
/// Always includes a `/` path: tungstenite sends the URL's path and query
/// verbatim, and `GET ?version=1… HTTP/1.1` (no path) gets a 400 from Stoat.
pub fn events_url(ws: &str) -> String {
    let ready: String = READY_FIELDS.iter().map(|f| format!("&ready={f}")).collect();
    format!("{}/?version=1&format=json{ready}", ws.trim_end_matches('/'))
}

/// What the connection reports to the UI.
#[derive(Debug)]
pub enum ConnectionEvent {
    Connecting {
        attempt: u32,
    },
    /// Authenticated. `Ready` follows.
    Connected,
    Event(ServerEvent),
    /// The connection dropped. It will retry after `retry_in`.
    Disconnected {
        reason: String,
        retry_in: Duration,
    },
    /// The server rejected the session (`InvalidSession`, `Logout`). The
    /// connection stops; the user has to log in again.
    SessionRejected,
}

/// A running events connection. Dropping it closes the connection.
pub struct Connection {
    abort: tokio::task::AbortHandle,
    /// Frames for the server. Only sent while connected: anything queued
    /// while disconnected is dropped (typing isn't worth replaying).
    outbound: mpsc::UnboundedSender<String>,
}

impl Connection {
    /// Tell the server the user is typing in `channel`.
    pub fn begin_typing(&self, channel: &str) {
        self.send(begin_typing_frame(channel));
    }

    /// Tell the server the user stopped typing in `channel`.
    pub fn end_typing(&self, channel: &str) {
        self.send(end_typing_frame(channel));
    }

    fn send(&self, frame: String) {
        // The task only stops when this `Connection` is dropped.
        let _ = self.outbound.unbounded_send(frame);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

/// Connect to the events websocket for `client`'s instance and keep the
/// connection alive, reconnecting with exponential backoff. `client` must
/// carry a session token. Events arrive on the returned receiver, which any
/// executor can poll.
pub fn connect(client: StoatClient) -> (Connection, mpsc::UnboundedReceiver<ConnectionEvent>) {
    let (tx, rx) = mpsc::unbounded();
    let (outbound, outbound_rx) = mpsc::unbounded();
    let handle = runtime().spawn(run(client, tx, outbound_rx));
    (
        Connection {
            abort: handle.abort_handle(),
            outbound,
        },
        rx,
    )
}

enum SessionEnd {
    /// The server rejected our token. Stop for good.
    Rejected,
    /// The UI dropped the receiver. Stop quietly.
    ClientGone,
    /// Anything else: retry. `authenticated` resets the backoff.
    Dropped { reason: String, authenticated: bool },
}

async fn run(
    client: StoatClient,
    tx: mpsc::UnboundedSender<ConnectionEvent>,
    mut outbound: mpsc::UnboundedReceiver<String>,
) {
    let Some(token) = client.token().map(str::to_string) else {
        let _ = tx.unbounded_send(ConnectionEvent::SessionRejected);
        return;
    };
    let mut backoff = BACKOFF_START;
    let mut attempt = 0;

    loop {
        attempt += 1;
        if tx
            .unbounded_send(ConnectionEvent::Connecting { attempt })
            .is_err()
        {
            return;
        }

        let end = match client.root_config().await {
            Ok(root) => session(&events_url(&root.ws), &token, &tx, &mut outbound).await,
            Err(ApiError::Unauthorized) => SessionEnd::Rejected,
            Err(err) => SessionEnd::Dropped {
                reason: err.to_string(),
                authenticated: false,
            },
        };

        let reason = match end {
            SessionEnd::Rejected => {
                log::warn!("events: session rejected by server");
                let _ = tx.unbounded_send(ConnectionEvent::SessionRejected);
                return;
            }
            SessionEnd::ClientGone => return,
            SessionEnd::Dropped {
                reason,
                authenticated,
            } => {
                if authenticated {
                    backoff = BACKOFF_START;
                }
                reason
            }
        };

        log::info!("events: disconnected ({reason}); retrying in {backoff:?}");
        if tx
            .unbounded_send(ConnectionEvent::Disconnected {
                reason,
                retry_in: backoff,
            })
            .is_err()
        {
            return;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

async fn session(
    url: &str,
    token: &str,
    tx: &mpsc::UnboundedSender<ConnectionEvent>,
    outbound: &mut mpsc::UnboundedReceiver<String>,
) -> SessionEnd {
    let dropped = |reason: String, authenticated| SessionEnd::Dropped {
        reason,
        authenticated,
    };

    log::debug!("events: connecting to {url}");
    let (socket, _) = match tokio_tungstenite::connect_async(url).await {
        Ok(ok) => ok,
        Err(err) => return dropped(format!("couldn't connect: {err}"), false),
    };
    let (mut sink, mut stream) = socket.split();

    if let Err(err) = sink
        .send(WsMessage::Text(authenticate_frame(token).into()))
        .await
    {
        return dropped(format!("couldn't authenticate: {err}"), false);
    }

    let mut authenticated = false;
    let mut pings = tokio::time::interval(PING_INTERVAL);
    pings.tick().await; // The first tick fires immediately; skip it.
    let mut ping_count: u64 = 0;
    let mut last_pong = Instant::now();
    // Whatever was queued while disconnected is stale by now.
    while outbound.try_recv().is_ok() {}

    loop {
        tokio::select! {
            frame = outbound.next() => {
                let Some(frame) = frame else {
                    return SessionEnd::ClientGone;
                };
                // Not authenticated yet: the server would reject it, and it's
                // stale by the time we are.
                if !authenticated {
                    continue;
                }
                if let Err(err) = sink.send(WsMessage::Text(frame.into())).await {
                    return dropped(format!("couldn't send: {err}"), authenticated);
                }
            }
            _ = pings.tick() => {
                if last_pong.elapsed() > PONG_TIMEOUT {
                    return dropped("server stopped answering pings".into(), authenticated);
                }
                ping_count += 1;
                if let Err(err) = sink.send(WsMessage::Text(ping_frame(ping_count).into())).await {
                    return dropped(format!("couldn't send ping: {err}"), authenticated);
                }
            }
            frame = stream.next() => {
                let text = match frame {
                    None => return dropped("connection closed".into(), authenticated),
                    Some(Err(err)) => return dropped(err.to_string(), authenticated),
                    Some(Ok(WsMessage::Text(text))) => text,
                    Some(Ok(WsMessage::Close(close))) => {
                        let reason = close.map(|c| c.reason.to_string()).unwrap_or_default();
                        return dropped(format!("server closed the connection {reason}").trim().to_string(), authenticated);
                    }
                    // tungstenite answers protocol-level pings itself.
                    Some(Ok(_)) => continue,
                };
                log::trace!("events: frame {text}");

                for event in parse_frame(&text) {
                    let forward = match event {
                        ServerEvent::Pong => {
                            last_pong = Instant::now();
                            log::debug!("events: pong");
                            None
                        }
                        ServerEvent::Authenticated => {
                            authenticated = true;
                            last_pong = Instant::now();
                            log::info!("events: authenticated");
                            Some(ConnectionEvent::Connected)
                        }
                        ServerEvent::Logout => return SessionEnd::Rejected,
                        ServerEvent::Error(err) => {
                            use revolt_result::ErrorType::*;
                            if matches!(err.error_type, InvalidSession | NotAuthenticated) {
                                return SessionEnd::Rejected;
                            }
                            log::warn!("events: server error {:?}", err.error_type);
                            None
                        }
                        ServerEvent::Ready(ready) => {
                            log::info!(
                                "events: Ready ({} servers, {} channels, {} users, {} emoji, {} unreads, {} skipped)",
                                ready.servers.len(),
                                ready.channels.len(),
                                ready.users.len(),
                                ready.emojis.len(),
                                ready.unreads.len(),
                                ready.skipped,
                            );
                            Some(ConnectionEvent::Event(ServerEvent::Ready(ready)))
                        }
                        ServerEvent::Unknown(kind) => {
                            log::trace!("events: ignoring {kind}");
                            None
                        }
                        message_event @ (ServerEvent::Message(_)
                        | ServerEvent::MessageUpdate { .. }
                        | ServerEvent::MessageDelete { .. }
                        | ServerEvent::MessageAppend { .. }
                        | ServerEvent::MessageReact { .. }
                        | ServerEvent::MessageUnreact { .. }
                        | ServerEvent::MessageRemoveReaction { .. }
                        | ServerEvent::BulkMessageDelete { .. }
                        | ServerEvent::EmojiCreate(_)
                        | ServerEvent::UserSettingsUpdate { .. }
                        | ServerEvent::ChannelAck { .. }
                        | ServerEvent::ChannelCreate(_)
                        | ServerEvent::ChannelUpdate { .. }
                        | ServerEvent::ChannelStartTyping { .. }
                        | ServerEvent::ChannelStopTyping { .. }
                        | ServerEvent::EmojiDelete { .. }) => {
                            Some(ConnectionEvent::Event(message_event))
                        }
                    };
                    if let Some(event) = forward
                        && tx.unbounded_send(event).is_err()
                    {
                        return SessionEnd::ClientGone;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_and_authenticate_frames_match_the_protocol() {
        let ping: Value = serde_json::from_str(&ping_frame(7)).unwrap();
        assert_eq!(ping, json!({ "type": "Ping", "data": 7 }));
        let auth: Value = serde_json::from_str(&authenticate_frame("tok")).unwrap();
        assert_eq!(auth, json!({ "type": "Authenticate", "token": "tok" }));
    }

    #[test]
    fn events_url_requests_only_needed_ready_fields() {
        assert_eq!(
            events_url("wss://events.stoat.chat/"),
            events_url("wss://events.stoat.chat"),
        );
        assert_eq!(
            events_url("wss://events.stoat.chat"),
            "wss://events.stoat.chat/?version=1&format=json&ready=users&ready=servers\
             &ready=channels&ready=members&ready=emojis&ready=channel_unreads\
             &ready=user_settings[ordering]&ready=user_settings[notifications]"
        );
    }

    #[test]
    fn bulk_frames_are_flattened() {
        let events = parse_frame(
            r#"{"type":"Bulk","v":[{"type":"Authenticated"},{"type":"Pong","data":1}]}"#,
        );
        assert!(matches!(
            events.as_slice(),
            [ServerEvent::Authenticated, ServerEvent::Pong]
        ));
    }

    #[test]
    fn unknown_event_types_are_kept_as_unknown() {
        let events = parse_frame(r#"{"type":"ServerMemberJoin","id":"s","user":"u"}"#);
        assert!(
            matches!(events.as_slice(), [ServerEvent::Unknown(kind)] if kind == "ServerMemberJoin")
        );
    }

    #[test]
    fn typing_frames_match_the_protocol() {
        let begin: Value = serde_json::from_str(&begin_typing_frame("ch")).unwrap();
        assert_eq!(begin, json!({ "type": "BeginTyping", "channel": "ch" }));
        let end: Value = serde_json::from_str(&end_typing_frame("ch")).unwrap();
        assert_eq!(end, json!({ "type": "EndTyping", "channel": "ch" }));
        // Byte for byte, as the plan specifies.
        assert_eq!(begin_typing_frame("ch"), r#"{"type":"BeginTyping","channel":"ch"}"#);
    }

    #[test]
    fn typing_events_decode_with_the_channel_as_id() {
        let events = parse_frame(
            r#"{"type":"Bulk","v":[
                {"type":"ChannelStartTyping","id":"ch","user":"u-ada"},
                {"type":"ChannelStopTyping","id":"ch","user":"u-ada"}
            ]}"#,
        );
        assert!(matches!(
            &events[0],
            ServerEvent::ChannelStartTyping { channel, user } if channel == "ch" && user == "u-ada"
        ));
        assert!(matches!(
            &events[1],
            ServerEvent::ChannelStopTyping { channel, user } if channel == "ch" && user == "u-ada"
        ));
    }

    #[test]
    fn error_events_parse_into_revolt_result_errors() {
        let events = parse_frame(r#"{"type":"Error","data":{"type":"InvalidSession"}}"#);
        assert!(matches!(
            events.as_slice(),
            [ServerEvent::Error(err)] if matches!(err.error_type, revolt_result::ErrorType::InvalidSession)
        ));
    }

    #[test]
    fn message_events_decode() {
        let events = parse_frame(
            r#"{"type":"Bulk","v":[
                {"type":"Message","_id":"01JC0000000000000000000010","channel":"c","author":"u","content":"hi"},
                {"type":"MessageUpdate","id":"01JC0000000000000000000010","channel":"c",
                 "data":{"content":"hi (edited)","edited":"2026-11-04T12:00:00.000Z"},"clear":["Pinned"]},
                {"type":"MessageDelete","id":"01JC0000000000000000000010","channel":"c"},
                {"type":"BulkMessageDelete","channel":"c","ids":["a","b"]}
            ]}"#,
        );
        assert!(
            matches!(&events[0], ServerEvent::Message(m) if m.content.as_deref() == Some("hi"))
        );
        assert!(matches!(
            &events[1],
            ServerEvent::MessageUpdate { data, clear, .. }
                if data.content.as_deref() == Some("hi (edited)") && clear.len() == 1
        ));
        assert!(matches!(&events[2], ServerEvent::MessageDelete { channel, .. } if channel == "c"));
        assert!(matches!(&events[3], ServerEvent::BulkMessageDelete { ids, .. } if ids.len() == 2));
    }

    #[test]
    fn emoji_events_decode() {
        let events = parse_frame(
            r#"{"type":"Bulk","v":[
                {"type":"EmojiCreate","_id":"01ARYZ6S41TSV4RRFFQ69G5FAV","parent":{"type":"Server","id":"s"},"creator_id":"u","name":"stoat"},
                {"type":"EmojiDelete","id":"01ARYZ6S41TSV4RRFFQ69G5FAV"}
            ]}"#,
        );
        assert!(matches!(&events[0], ServerEvent::EmojiCreate(e) if e.name == "stoat"));
        assert!(matches!(&events[1], ServerEvent::EmojiDelete { id } if id.starts_with("01ARYZ")));
    }

    #[test]
    fn channel_ack_decodes() {
        let events = parse_frame(
            r#"{"type":"ChannelAck","id":"c","user":"u","message_id":"01M6GDKW80AAAAAAAAAAAAAAAA"}"#,
        );
        assert!(matches!(
            events.as_slice(),
            [ServerEvent::ChannelAck { channel, user, message_id }]
                if channel == "c" && user == "u" && message_id.starts_with("01M6")
        ));
    }

    #[test]
    fn channel_events_decode() {
        // What another account opening a DM with us sends: the DM (closed),
        // then the update that opens it, as the first message goes out.
        let events = parse_frame(
            r#"{"type":"Bulk","v":[
                {"type":"ChannelCreate","channel_type":"DirectMessage","_id":"dm-new",
                 "active":false,"recipients":["u-me","u-new"]},
                {"type":"ChannelUpdate","id":"dm-new","data":{"active":true},"clear":[]},
                {"type":"ChannelUpdate","id":"ch","data":{"name":"renamed"},"clear":["Description"]}
            ]}"#,
        );
        assert!(matches!(
            &events[0],
            ServerEvent::ChannelCreate(c)
                if matches!(c.as_ref(), Channel::DirectMessage { id, active: false, .. } if id == "dm-new")
        ));
        assert!(matches!(
            &events[1],
            ServerEvent::ChannelUpdate { id, data, clear }
                if id == "dm-new" && data.active == Some(true) && clear.is_empty()
        ));
        assert!(matches!(
            &events[2],
            ServerEvent::ChannelUpdate { data, clear, .. }
                if data.name.as_deref() == Some("renamed")
                    && matches!(clear.as_slice(), [FieldsChannel::Description])
        ));
    }

    #[test]
    fn settings_updates_carry_notification_settings() {
        let events = parse_frame(
            r#"{"type":"Bulk","v":[
                {"type":"UserSettingsUpdate","id":"u","update":{"notifications":[1,"{\"channel\":{\"c\":\"none\"}}"]}},
                {"type":"UserSettingsUpdate","id":"u","update":{"ordering":[1,"{}"]}}
            ]}"#,
        );
        let ServerEvent::UserSettingsUpdate {
            notifications: Some(settings),
        } = &events[0]
        else {
            panic!("expected notifications, got {:?}", events[0]);
        };
        assert_eq!(settings.level("c", Some("s")), crate::notify::Level::None);
        assert!(matches!(
            events[1],
            ServerEvent::UserSettingsUpdate {
                notifications: None
            }
        ));
    }

    #[test]
    fn malformed_message_events_become_unknown() {
        let events = parse_frame(r#"{"type":"Message","content":"no id or channel"}"#);
        assert!(
            matches!(events.as_slice(), [ServerEvent::Unknown(kind)] if kind == "Message (unparseable)")
        );
    }

    #[test]
    fn garbage_frames_are_ignored() {
        assert!(parse_frame("not json").is_empty());
    }

    #[test]
    fn server_order_comes_from_the_ordering_setting() {
        let settings = json!({ "ordering": [1730000000000i64, r#"{"servers":["b","a"]}"#] });
        assert_eq!(server_order_from_settings(settings), vec!["b", "a"]);
        assert!(server_order_from_settings(json!({})).is_empty());
        assert!(server_order_from_settings(json!({ "ordering": [1, "not json"] })).is_empty());
    }
}
