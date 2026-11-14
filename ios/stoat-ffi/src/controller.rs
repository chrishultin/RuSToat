//! One signed-in account's live state: its events connection and its
//! [`ChatStore`], with snapshots for Swift to draw.
//!
//! Events are applied to the store with [`live::apply_event`] on the IO
//! runtime. Swift hears that something changed through
//! [`ControllerListener`] (a burst of events is coalesced into one call) and
//! reads plain-record snapshots back. The store's lock is never held across
//! an `.await` or a call into Swift.

use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use stoat_gpui::api::StoatClient;
use stoat_gpui::api::events::{self, Connection, ConnectionEvent};
use stoat_gpui::live::{self, Changed};
use stoat_gpui::media::MediaUrls;
use stoat_gpui::notify::{self, NotificationSettings};
use stoat_gpui::rail::{self, Ordering, ServerFolders};
use stoat_gpui::runtime::runtime;
use stoat_gpui::store::{ChannelEntry, ChannelKind, ChannelSection, ChatStore, ServerEntry};

use crate::CoreError;
use crate::session::Core;

/// How long to gather a burst of events (a `Ready`, a busy channel) before
/// telling Swift once.
const COALESCE: Duration = Duration::from_millis(50);

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
}

impl Shared {
    fn changed(&self, changed: Changed) {
        if !changed.is_empty() {
            self.listener.changed(changed.into());
        }
    }

    /// Apply one event to the store. A reconnect's `Ready` also drops
    /// loaded history, since events were missed while offline (as desktop).
    fn apply(&self, event: events::ServerEvent) -> Changed {
        let ready = matches!(event, events::ServerEvent::Ready(_));
        let mut store = self.store.lock().unwrap();
        let changed = live::apply_event(&mut store, event, notify::now_ms().max(0) as u64);
        if ready && self.has_ready.swap(true, AtomicOrdering::SeqCst) {
            log::info!("reconnected: dropping loaded history");
            store.clear_histories();
        }
        changed
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
                    let changed = self.apply(event);
                    if ready {
                        runtime().spawn(self.clone().fetch_settings());
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
    _connection: Connection,
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
            }),
            running: Mutex::new(None),
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
            _connection: connection,
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
