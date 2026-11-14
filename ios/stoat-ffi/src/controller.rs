//! One signed-in account's live state: its events connection and its
//! [`ChatStore`], with snapshots for Swift to draw.
//!
//! Events are applied to the store with [`live::apply_event`] on the IO
//! runtime. Swift hears that something changed through
//! [`ControllerListener`] (a burst of events is coalesced into one call) and
//! reads plain-record snapshots back. The store's lock is never held across
//! an `.await` or a call into Swift.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::api::events::{self, Connection, ConnectionEvent};
use stoat_gpui::live::{self, Changed};
use stoat_gpui::media::MediaUrls;
use stoat_gpui::notify::{self, NotificationSettings};
use stoat_gpui::rail::{self, Ordering, ServerFolders};
use stoat_gpui::runtime::{runtime, spawn_io};
use stoat_gpui::api::models::ReplyIntent;
use stoat_gpui::draft::new_nonce;
use stoat_gpui::store::{
    ChannelEntry, ChannelKind, ChannelSection, ChatStore, PendingMessage, PendingState,
    ServerEntry, with_unread_divider,
};
use stoat_gpui::typing::{IDLE_AFTER, OutgoingTyping, TypingSignal};

use crate::CoreError;
use crate::session::Core;
use crate::rich_text::RichCache;
use crate::timeline::{self, TimelineItem};

/// How long to gather a burst of events (a `Ready`, a busy channel) before
/// telling Swift once.
const COALESCE: Duration = Duration::from_millis(50);

/// Messages per page, as on desktop.
pub const HISTORY_PAGE: usize = 50;

/// Most reply originals fetched at once for one channel.
const REPLY_FETCHES: usize = 5;

/// What changed, so Swift knows what to read again.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Change {
    /// A fresh `Ready` replaced everything. After a reconnect, loaded
    /// messages were dropped too: reopen the channel on screen.
    pub ready: bool,
    /// The server, channel or conversation lists, or their unread state.
    pub lists: bool,
    /// Channels whose loaded messages changed.
    pub timelines: Vec<String>,
}

impl From<Changed> for Change {
    fn from(changed: Changed) -> Self {
        Self {
            ready: changed.ready,
            lists: changed.lists,
            timelines: changed.timelines,
        }
    }
}

/// The events connection's state, for the connection banner.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectionStatus {
    Connecting { attempt: u32 },
    Connected,
    /// Dropped; it tries again in `retry_in_seconds`.
    Disconnected { reason: String, retry_in_seconds: u32 },
    /// Stoat ended the session: the user has to log in again.
    SessionRejected,
    /// Disconnected on purpose (the app went to the background).
    Paused,
}

/// Swift's side of a controller. Called from the IO runtime's threads.
#[uniffi::export(callback_interface)]
pub trait ControllerListener: Send + Sync {
    fn changed(&self, change: Change);
    fn status(&self, status: ConnectionStatus);
}

/// A server, as the Servers tab lists it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ServerItem {
    pub id: String,
    pub name: String,
    /// Up to two letters, for a server without an icon.
    pub initials: String,
    pub icon_url: Option<String>,
    pub unread: bool,
    pub mentions: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ChannelKindItem {
    Text,
    /// A voice channel (which has text chat too).
    Voice,
    DirectMessage,
    Group,
    SavedNotes,
}

/// A channel or conversation in a list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChannelItem {
    pub id: String,
    pub name: String,
    pub kind: ChannelKindItem,
    /// The other person's avatar for DMs, the group's icon for groups.
    pub avatar_url: Option<String>,
    pub unread: bool,
    pub mentions: u32,
    /// Muted (here or by its server): drawn dimmed, with no unread dot.
    pub muted: bool,
}

/// A server's channels under one category (`title` `None`: uncategorised).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChannelSectionItem {
    pub id: Option<String>,
    pub title: Option<String>,
    pub channels: Vec<ChannelItem>,
}

/// The signed-in user, for the You tab.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MeItem {
    pub user_id: String,
    /// Display name, or username.
    pub name: String,
    /// `username#1234`.
    pub username: String,
    pub avatar_url: String,
    /// Online, Idle, Focus, Busy or Invisible.
    pub presence: String,
    /// Custom status text, if any.
    pub status: Option<String>,
}

/// Unread totals for the tab bar's badges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Summary {
    pub home_unread: bool,
    pub home_mentions: u32,
    pub servers_unread: bool,
    pub servers_mentions: u32,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn channel_item(entry: ChannelEntry) -> ChannelItem {
    ChannelItem {
        kind: match entry.kind {
            ChannelKind::Text { voice: false } => ChannelKindItem::Text,
            ChannelKind::Text { voice: true } => ChannelKindItem::Voice,
            ChannelKind::DirectMessage => ChannelKindItem::DirectMessage,
            ChannelKind::Group => ChannelKindItem::Group,
            ChannelKind::SavedMessages => ChannelKindItem::SavedNotes,
        },
        id: entry.id,
        name: entry.name,
        avatar_url: entry.avatar,
        unread: entry.unread,
        mentions: count(entry.mentions),
        muted: entry.muted,
    }
}

fn section_item(section: ChannelSection) -> ChannelSectionItem {
    ChannelSectionItem {
        id: section.id,
        title: section.title,
        channels: section.channels.into_iter().map(channel_item).collect(),
    }
}

fn server_item(store: &ChatStore, entry: ServerEntry) -> ServerItem {
    ServerItem {
        icon_url: entry.icon.as_ref().and_then(|f| store.media().file(f)),
        id: entry.id,
        name: entry.name,
        initials: entry.initials,
        unread: entry.unread,
        mentions: count(entry.mentions),
    }
}

/// What the controller and its tasks share.
pub(crate) struct Shared {
    pub(crate) client: StoatClient,
    pub(crate) store: Mutex<ChatStore>,
    listener: Box<dyn ControllerListener>,
    /// A `Ready` has arrived before, so the next one is a reconnect.
    has_ready: AtomicBool,
    /// The channel on screen, if any. Only it is marked read, and it's
    /// reloaded after a reconnect.
    viewing: Mutex<Option<String>>,
    /// Where each open channel's "New messages" divider goes: the read
    /// position when it was opened, if it had anything unread.
    dividers: Mutex<HashMap<String, Option<String>>>,
    /// Reply originals being fetched, by message ID.
    reply_fetches: Mutex<HashSet<String>>,
    /// Messages' text, parsed, by render key.
    pub(crate) rich: RichCache,
    /// The instance's web client (`app` from `GET /`), for message links.
    pub(crate) web_app: Mutex<Option<String>>,
    /// Reactions on their way to Stoat, as (message, emoji): a second tap
    /// waits for the first.
    pub(crate) reacting: Mutex<HashSet<(String, String)>>,
}

fn now_ms() -> u64 {
    notify::now_ms().max(0) as u64
}

impl Shared {
    fn changed(&self, changed: Changed) {
        if !changed.is_empty() {
            self.listener.changed(changed.into());
        }
    }

    /// Apply one event to the store. A reconnect's `Ready` also drops
    /// loaded history, since events were missed while offline (as desktop).
    /// Returns what changed, and whether it was a reconnect's `Ready`.
    fn apply(&self, event: events::ServerEvent) -> (Changed, bool) {
        let ready = matches!(event, events::ServerEvent::Ready(_));
        let mut store = self.store.lock().unwrap();
        let changed = live::apply_event(&mut store, event, now_ms());
        let reconnect = ready && self.has_ready.swap(true, AtomicOrdering::SeqCst);
        if reconnect {
            log::info!("reconnected: dropping loaded history");
            store.clear_histories();
        }
        (changed, reconnect)
    }

    pub(crate) fn timeline_changed(&self, channel: &str) {
        self.changed(Changed {
            timelines: vec![channel.to_string()],
            ..Changed::default()
        });
    }

    /// Load `channel`'s latest page, replacing what's loaded (as desktop's
    /// `load_history`).
    async fn load_latest(self: Arc<Self>, channel: String) -> Result<(), ApiError> {
        let page = self
            .client
            .fetch_messages(&channel, None, HISTORY_PAGE as i64)
            .await?;
        self.store
            .lock()
            .unwrap()
            .set_history(&channel, page, HISTORY_PAGE);
        self.timeline_changed(&channel);
        runtime().spawn(self.clone().fetch_replies(channel));
        Ok(())
    }

    /// Load the page before `channel`'s oldest loaded message. Whether
    /// there's more before it.
    async fn load_older(self: Arc<Self>, channel: String) -> Result<bool, ApiError> {
        let before = {
            let store = self.store.lock().unwrap();
            match store.history(&channel) {
                Some(history) if !history.exhausted => store.oldest_message_id(&channel).map(str::to_string),
                _ => None,
            }
        };
        let Some(before) = before else {
            return Ok(false);
        };
        let page = self
            .client
            .fetch_messages(&channel, Some(&before), HISTORY_PAGE as i64)
            .await?;
        let more = {
            let mut store = self.store.lock().unwrap();
            store.prepend_page(&channel, page, HISTORY_PAGE);
            store.history(&channel).is_some_and(|h| !h.exhausted)
        };
        self.timeline_changed(&channel);
        runtime().spawn(self.clone().fetch_replies(channel));
        Ok(more)
    }

    /// `POST` a pending message (as desktop's `post_pending`, without
    /// attachments). Success confirms it, unless its live echo already
    /// has; failure marks it failed with the reason.
    async fn post(self: Arc<Self>, pending: PendingMessage) {
        let PendingMessage {
            key,
            channel,
            content,
            replies,
            nonce,
            ..
        } = pending;
        let result = self
            .client
            .send_message(&channel, &content, &[], &replies, &nonce)
            .await;
        match result {
            Ok(message) => {
                let id = message.id.clone();
                log::debug!("sent {id} in {channel}");
                self.store.lock().unwrap().apply_message(message);
                self.changed(Changed {
                    lists: true,
                    timelines: vec![channel.clone()],
                    ..Changed::default()
                });
                // Stoat doesn't mark the user's own message read for them.
                if let Err(err) = self.client.ack_message(&channel, &id).await {
                    log::warn!("couldn't mark {channel} read after sending: {err}");
                }
            }
            // An earlier attempt with this key got through: the message
            // exists, and shows once its echo or the next load brings it.
            Err(err) if err.is_duplicate_nonce() => {
                log::info!("{key} in {channel} was already sent");
                self.store.lock().unwrap().discard_pending(&channel, &key);
                self.timeline_changed(&channel);
            }
            Err(err) => {
                log::warn!("couldn't send in {channel}: {err:?}");
                let answered = !matches!(err, ApiError::Network(_));
                let failed = self.store.lock().unwrap().fail_pending(
                    &channel,
                    &key,
                    err.send_failure_text(),
                    answered,
                );
                if failed {
                    self.timeline_changed(&channel);
                }
            }
        }
    }

    /// Fetch the originals of replies that aren't loaded (as desktop's
    /// `fetch_reply_sources`), a few at a time.
    async fn fetch_replies(self: Arc<Self>, channel: String) {
        let missing = self.store.lock().unwrap().missing_reply_sources(&channel);
        let wanted: Vec<String> = {
            let mut fetching = self.reply_fetches.lock().unwrap();
            missing
                .into_iter()
                .filter(|id| fetching.len() < REPLY_FETCHES && fetching.insert(id.clone()))
                .collect()
        };
        for id in wanted {
            let result = self.client.fetch_messages_around(&channel, &id, 1).await;
            {
                let mut store = self.store.lock().unwrap();
                match result {
                    Ok(page) => store.add_reply_sources(page, std::slice::from_ref(&id)),
                    // Tried again with the next load.
                    Err(ApiError::RateLimited { .. }) => {}
                    Err(err) => {
                        log::warn!("couldn't load reply source {id}: {err}");
                        store.reply_source_unavailable(&id);
                    }
                }
            }
            self.reply_fetches.lock().unwrap().remove(&id);
            self.timeline_changed(&channel);
        }
    }

    /// The user settings `Ready` doesn't bring: notification levels and
    /// mutes, and the server order and folders (as desktop's
    /// `fetch_settings`).
    async fn fetch_settings(self: Arc<Self>) {
        let keys = [notify::SETTINGS_KEY, rail::ORDERING_KEY, rail::FOLDERS_KEY];
        let mut settings = match self.client.fetch_settings(&keys).await {
            Ok(settings) => settings,
            Err(err) => {
                log::warn!("couldn't load the user's settings: {err}");
                return;
            }
        };
        let mut take = |key: &str| settings.remove(key);
        let notifications = take(notify::SETTINGS_KEY)
            .map(|(updated, json)| (updated, NotificationSettings::parse(&json)));
        let ordering =
            take(rail::ORDERING_KEY).map(|(updated, json)| (updated, Ordering::parse(&json)));
        let folders =
            take(rail::FOLDERS_KEY).map(|(updated, json)| (updated, ServerFolders::parse(&json)));
        let lists = {
            let mut store = self.store.lock().unwrap();
            // Loaded even if absent: the user has never written it.
            let mut changed = store.notifications_fetched(notifications);
            if let Some((updated, ordering)) = ordering {
                changed |= store.set_ordering(updated, ordering);
            }
            if let Some((updated, folders)) = folders {
                changed |= store.set_server_folders(updated, folders);
            }
            changed
        };
        if lists {
            self.changed(Changed {
                lists: true,
                ..Changed::default()
            });
        }
    }

    /// Where images come from, once `GET /` answers.
    async fn fetch_media(self: Arc<Self>) {
        match self.client.root_config().await {
            Ok(root) => {
                let media = MediaUrls::from_root(self.client.base_url(), &root);
                *self.web_app.lock().unwrap() = root.app.clone();
                self.store.lock().unwrap().set_media(media);
                self.changed(Changed {
                    lists: true,
                    ..Changed::default()
                });
            }
            Err(err) => log::warn!("couldn't load the API config (no images): {err}"),
        }
    }

    /// Apply the connection's events until it ends, telling Swift about
    /// each burst once.
    async fn drain(self: Arc<Self>, mut events: UnboundedReceiver<ConnectionEvent>) {
        let mut pending = Changed::default();
        let mut deadline: Option<tokio::time::Instant> = None;
        loop {
            let next = match deadline {
                Some(at) => match tokio::time::timeout_at(at, events.next()).await {
                    Ok(next) => next,
                    Err(_) => {
                        self.changed(std::mem::take(&mut pending));
                        deadline = None;
                        continue;
                    }
                },
                None => events.next().await,
            };
            let Some(event) = next else {
                self.changed(pending);
                return;
            };
            let status = match event {
                ConnectionEvent::Event(event) => {
                    let ready = matches!(event, events::ServerEvent::Ready(_));
                    let (changed, reconnect) = self.apply(event);
                    if ready {
                        runtime().spawn(self.clone().fetch_settings());
                    }
                    // Events were missed while offline: reload what's on screen.
                    if reconnect && let Some(channel) = self.viewing.lock().unwrap().clone() {
                        let shared = self.clone();
                        runtime().spawn(async move {
                            if let Err(err) = shared.load_latest(channel.clone()).await {
                                log::warn!("couldn't reload {channel} after reconnecting: {err}");
                            }
                        });
                    }
                    if !changed.is_empty() {
                        pending.merge(changed);
                        deadline.get_or_insert_with(|| tokio::time::Instant::now() + COALESCE);
                    }
                    continue;
                }
                ConnectionEvent::Connecting { attempt } => ConnectionStatus::Connecting { attempt },
                ConnectionEvent::Connected => ConnectionStatus::Connected,
                ConnectionEvent::Disconnected { reason, retry_in } => {
                    ConnectionStatus::Disconnected {
                        reason,
                        retry_in_seconds: count(retry_in.as_secs() as usize),
                    }
                }
                ConnectionEvent::SessionRejected => ConnectionStatus::SessionRejected,
            };
            // Whatever came before a status change is told first.
            self.changed(std::mem::take(&mut pending));
            deadline = None;
            self.listener.status(status);
        }
    }
}

/// A live connection: dropping it closes the socket and stops the drain.
struct Running {
    connection: Connection,
    drain: tokio::task::AbortHandle,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

/// The signed-in account's live state.
#[derive(uniffi::Object)]
pub struct AccountController {
    pub(crate) shared: Arc<Shared>,
    running: Mutex<Option<Running>>,
    /// When to tell Stoat the user is typing.
    typing: Mutex<OutgoingTyping>,
}

impl AccountController {
    /// A controller for `client`'s account (which must carry its token),
    /// as user `me`.
    pub fn for_client(
        client: StoatClient,
        me: String,
        listener: Box<dyn ControllerListener>,
    ) -> Arc<Self> {
        let mut store = ChatStore::new(me);
        // Until `GET /` answers, only the API (default avatars) is known.
        store.set_media(MediaUrls::new(client.base_url(), None, None));
        Arc::new(Self {
            shared: Arc::new(Shared {
                client,
                store: Mutex::new(store),
                listener,
                has_ready: AtomicBool::new(false),
                viewing: Mutex::new(None),
                dividers: Mutex::new(HashMap::new()),
                reply_fetches: Mutex::new(HashSet::new()),
                rich: RichCache::default(),
                web_app: Mutex::new(None),
                reacting: Mutex::new(HashSet::new()),
            }),
            running: Mutex::new(None),
            typing: Mutex::new(OutgoingTyping::default()),
        })
    }
}

#[uniffi::export]
impl AccountController {
    /// A controller for the account signed in to `core`.
    #[uniffi::constructor]
    pub fn new(
        core: Arc<Core>,
        listener: Box<dyn ControllerListener>,
    ) -> Result<Arc<Self>, CoreError> {
        let (Some(client), Some(account)) = (core.client(), core.account()) else {
            return Err(CoreError::failed("Not signed in."));
        };
        Ok(Self::for_client(client, account.user_id, listener))
    }

    /// Connect to the events service (if not already), and keep
    /// reconnecting until [`Self::disconnect`].
    pub fn connect(&self) {
        let mut running = self.running.lock().unwrap();
        if running.is_some() {
            return;
        }
        let first = !self.shared.has_ready.load(AtomicOrdering::SeqCst);
        if first {
            runtime().spawn(self.shared.clone().fetch_media());
        }
        let (connection, events) = events::connect(self.shared.client.clone());
        let drain = runtime().spawn(self.shared.clone().drain(events));
        *running = Some(Running {
            connection,
            drain: drain.abort_handle(),
        });
    }

    /// Close the connection (the app went to the background). What's
    /// loaded stays until the next `Ready`.
    pub fn disconnect(&self) {
        if self.running.lock().unwrap().take().is_some() {
            self.shared.listener.status(ConnectionStatus::Paused);
        }
    }

    /// The servers, in the user's order.
    pub fn servers(&self) -> Vec<ServerItem> {
        let store = self.shared.store.lock().unwrap();
        store
            .servers()
            .into_iter()
            .map(|entry| server_item(&store, entry))
            .collect()
    }

    /// One server's channels, by category.
    pub fn server_channels(&self, server_id: String) -> Vec<ChannelSectionItem> {
        let store = self.shared.store.lock().unwrap();
        store
            .server_channels(&server_id)
            .into_iter()
            .map(section_item)
            .collect()
    }

    /// One server, if the user is still in it.
    pub fn server(&self, server_id: String) -> Option<ServerItem> {
        let store = self.shared.store.lock().unwrap();
        store
            .server_entry(&server_id)
            .map(|entry| server_item(&store, entry))
    }

    /// Home: Saved Notes, then open DMs and groups, most recent first.
    pub fn home_channels(&self) -> Vec<ChannelItem> {
        let store = self.shared.store.lock().unwrap();
        store
            .home_channels()
            .into_iter()
            .map(channel_item)
            .collect()
    }

    /// One channel or conversation, if it still exists.
    pub fn channel(&self, channel_id: String) -> Option<ChannelItem> {
        let store = self.shared.store.lock().unwrap();
        store.channel_entry(&channel_id).map(channel_item)
    }

    /// The signed-in user, once `Ready` has brought them.
    pub fn me(&self) -> Option<MeItem> {
        let store = self.shared.store.lock().unwrap();
        let user = store.user(store.me())?;
        let (presence, status) = store.my_status();
        Some(MeItem {
            user_id: user.id.clone(),
            name: stoat_gpui::store::display_name(user).to_string(),
            username: format!("{}#{}", user.username, user.discriminator),
            avatar_url: store.user_avatar_url(&user.id),
            presence: format!("{presence:?}"),
            status,
        })
    }

    /// Unread totals for the tabs.
    pub fn summary(&self) -> Summary {
        let store = self.shared.store.lock().unwrap();
        let (home_unread, home_mentions) = store.home_summary();
        let (servers_unread, servers_mentions) = store
            .servers()
            .iter()
            .fold((false, 0), |(unread, mentions), s| {
                (unread || s.unread, mentions + s.mentions)
            });
        Summary {
            home_unread,
            home_mentions: count(home_mentions),
            servers_unread,
            servers_mentions: count(servers_mentions),
        }
    }
}

/// Reading a channel.
#[uniffi::export]
impl AccountController {
    /// The user opened `channel_id`: it's on screen from now on (marked
    /// read, reloaded after a reconnect) until [`Self::close_channel`].
    /// Loads its latest messages if they aren't loaded.
    pub async fn open_channel(&self, channel_id: String) -> Result<(), CoreError> {
        let shared = &self.shared;
        *shared.viewing.lock().unwrap() = Some(channel_id.clone());
        let loaded = {
            let store = shared.store.lock().unwrap();
            // Where the user left off, before reading here moves it.
            let after = store
                .is_unread(&channel_id)
                .then(|| store.read_position(&channel_id).map(str::to_string))
                .flatten();
            shared
                .dividers
                .lock()
                .unwrap()
                .entry(channel_id.clone())
                .or_insert(after);
            store.history(&channel_id).is_some()
        };
        if loaded {
            runtime().spawn(shared.clone().fetch_replies(channel_id));
            return Ok(());
        }
        spawn_io(shared.clone().load_latest(channel_id))
            .await
            .map_err(CoreError::failed)??;
        Ok(())
    }

    /// Load older messages. Whether there are more before them.
    pub async fn load_older(&self, channel_id: String) -> Result<bool, CoreError> {
        Ok(spawn_io(self.shared.clone().load_older(channel_id))
            .await
            .map_err(CoreError::failed)??)
    }

    /// The user left `channel_id`.
    pub fn close_channel(&self, channel_id: String) {
        let mut viewing = self.shared.viewing.lock().unwrap();
        if viewing.as_deref() == Some(channel_id.as_str()) {
            *viewing = None;
        }
        self.shared.dividers.lock().unwrap().remove(&channel_id);
    }

    /// Show spoiler `index` of message `message_id` for the rest of the
    /// session (the user tapped it).
    pub fn reveal_spoiler(&self, channel_id: String, message_id: String, index: u32) {
        self.shared
            .store
            .lock()
            .unwrap()
            .reveal_spoiler(&message_id, index as usize);
        self.shared.timeline_changed(&channel_id);
    }

    /// `channel_id`'s rows, oldest first, with the "New messages" divider
    /// where the user left off. Empty until loaded.
    pub fn timeline(&self, channel_id: String) -> Vec<TimelineItem> {
        let rows = self.shared.store.lock().unwrap().timeline(&channel_id);
        let divider = self
            .shared
            .dividers
            .lock()
            .unwrap()
            .get(&channel_id)
            .cloned()
            .flatten();
        let rows = match divider {
            Some(after) => with_unread_divider(rows, &after),
            None => rows,
        };
        rows.into_iter()
            .map(|row| timeline::item(row, &self.shared.rich))
            .collect()
    }

    /// Mark `channel_id` read up to its newest message, if it's the channel
    /// on screen, the app is connected (so in the foreground), and there's
    /// anything to mark. Swift calls this while the newest message is in
    /// view. Whether it marked anything.
    pub fn mark_read(&self, channel_id: String) -> bool {
        if self.running.lock().unwrap().is_none()
            || self.shared.viewing.lock().unwrap().as_deref() != Some(channel_id.as_str())
        {
            return false;
        }
        let latest = {
            let mut store = self.shared.store.lock().unwrap();
            // What's loaded isn't the newest, so reading it says nothing
            // about what's after it.
            if !store.is_at_present(&channel_id) {
                return false;
            }
            let Some(latest) = store.unacked_latest(&channel_id) else {
                return false;
            };
            // Cleared here straight away; the request follows.
            store.apply_ack(&channel_id, &latest);
            latest
        };
        self.shared.changed(Changed {
            lists: true,
            ..Changed::default()
        });
        let client = self.shared.client.clone();
        runtime().spawn(async move {
            match client.ack_message(&channel_id, &latest).await {
                Ok(()) => log::debug!("acked {channel_id} up to {latest}"),
                Err(err) => log::warn!("couldn't mark {channel_id} read: {err}"),
            }
        });
        true
    }
}

impl AccountController {
    fn send_typing(&self, signal: Option<TypingSignal>) {
        let running = self.running.lock().unwrap();
        let Some(running) = running.as_ref() else {
            return;
        };
        match signal {
            Some(TypingSignal::Begin(channel)) => running.connection.begin_typing(&channel),
            Some(TypingSignal::End(channel)) => running.connection.end_typing(&channel),
            None => {}
        }
    }
}

/// Sending.
#[uniffi::export]
impl AccountController {
    /// Whether the user may send messages in `channel_id`.
    pub fn can_send(&self, channel_id: String) -> bool {
        self.shared.store.lock().unwrap().can_send(&channel_id)
    }

    /// Send `text` to `channel_id`, replying to `reply_to` if given. It
    /// shows at once as pending, then sent (or failed, to retry). Returns
    /// its key: the row's ID until and after Stoat confirms it.
    pub fn send(
        &self,
        channel_id: String,
        text: String,
        reply_to: Option<String>,
    ) -> Result<String, CoreError> {
        let content = text.trim().to_string();
        if content.is_empty() {
            return Err(CoreError::failed("Type a message first."));
        }
        // Sending ends typing.
        let stopped = self.typing.lock().unwrap().stop();
        self.send_typing(stopped);
        let sent_ms = now_ms();
        let nonce = new_nonce(sent_ms);
        let pending = PendingMessage {
            key: nonce.clone(),
            nonce,
            channel: channel_id.clone(),
            content,
            replies: reply_to
                .into_iter()
                .map(|id| ReplyIntent {
                    id,
                    mention: false,
                    fail_if_not_exists: None,
                })
                .collect(),
            attachments: Vec::new(),
            state: PendingState::Sending,
            sent_ms,
            nonce_used: false,
        };
        let key = pending.key.clone();
        self.shared.store.lock().unwrap().add_pending(pending.clone());
        // The user has replied, so a divider above their reply says nothing.
        if let Some(divider) = self.shared.dividers.lock().unwrap().get_mut(&channel_id) {
            *divider = None;
        }
        self.shared.timeline_changed(&channel_id);
        runtime().spawn(self.shared.clone().post(pending));
        Ok(key)
    }

    /// Send a failed message again, with the same idempotency key unless
    /// Stoat has already seen that one.
    pub fn retry(&self, channel_id: String, key: String) {
        let pending = self
            .shared
            .store
            .lock()
            .unwrap()
            .retry_pending(&channel_id, &key, || new_nonce(now_ms()));
        if let Some(pending) = pending {
            self.shared.timeline_changed(&channel_id);
            runtime().spawn(self.shared.clone().post(pending));
        }
    }

    /// Give up on an unsent message.
    pub fn discard(&self, channel_id: String, key: String) {
        let discarded = self
            .shared
            .store
            .lock()
            .unwrap()
            .discard_pending(&channel_id, &key);
        if discarded.is_some() {
            self.shared.timeline_changed(&channel_id);
        }
    }

    /// The message box in `channel_id` changed (`empty`: nothing in it
    /// now). Tells Stoat the user is typing, at most every few seconds, and
    /// that they've stopped once it's empty or idle for a while.
    pub fn typing(self: Arc<Self>, channel_id: String, empty: bool) {
        let signals = self
            .typing
            .lock()
            .unwrap()
            .on_input(&channel_id, empty, std::time::Instant::now());
        for signal in signals {
            self.send_typing(Some(signal));
        }
        if !empty {
            let this = self.clone();
            runtime().spawn(async move {
                tokio::time::sleep(IDLE_AFTER).await;
                let idle = this.typing.lock().unwrap().on_tick(std::time::Instant::now());
                this.send_typing(idle);
            });
        }
    }
}
