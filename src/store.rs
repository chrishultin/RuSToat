//! The client's in-memory picture of the account, built from the events
//! connection. Plain data with no GPUI, so ordering and naming rules are
//! unit-testable.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::api::events::Ready;
use crate::api::models::{
    BulkMessageResponse, Channel, Embed, Emoji, FieldsChannel, FieldsMessage, File, Member,
    Message, MessageFlags, PartialChannel, PartialMessage, Presence, Server, SystemMessage, User,
};
use crate::attachment::AttachmentView;
use crate::embed::EmbedView;
use crate::media::MediaUrls;
use crate::notify::{self, NotificationSettings};

/// One entry in the channel list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEntry {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    /// The other person's avatar for DMs, the group icon for groups.
    pub avatar: Option<String>,
    /// Has messages newer than the user's read position.
    pub unread: bool,
    /// Unread messages that mention the user.
    pub mentions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// A server channel. `voice` channels also have text chat.
    Text {
        voice: bool,
    },
    DirectMessage,
    Group,
    SavedMessages,
}

/// A server's channels under one category heading (`None` = uncategorized).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSection {
    pub title: Option<String>,
    pub channels: Vec<ChannelEntry>,
}

/// A channel's loaded messages, oldest first.
#[derive(Debug, Default)]
pub struct History {
    pub messages: Vec<Message>,
    /// The server returned fewer than requested: there's nothing older.
    pub exhausted: bool,
}

/// One row of a channel's message view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineRow {
    /// Shown above the oldest message once there's no more history.
    StartOfChannel {
        name: String,
    },
    Message(MessageRow),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    pub id: String,
    pub author: String,
    /// Author avatar URL. `None` for system messages, or when there's no
    /// image to show (the UI draws the author's initial instead).
    pub avatar: Option<String>,
    /// "Today at 14:05" etc.
    pub time: String,
    /// Markdown (see [`crate::markdown::prepare`]). For system messages, a
    /// plain-text description instead.
    pub body: Option<String>,
    /// Changes whenever what's drawn for `body` changes (an edit, a renamed
    /// mention), so the Markdown view's cached parse is replaced. The view
    /// keeps its parse per element ID and only reads the source the first time.
    pub render_key: String,
    pub attachments: Vec<AttachmentView>,
    /// Link previews and bot embeds, in order.
    pub embeds: Vec<EmbedView>,
    pub system: bool,
    pub edited: bool,
    /// Same author as the previous message, soon after it: drawn without a
    /// name/time header.
    pub continuation: bool,
    /// The next message doesn't continue this one: the last bubble of its
    /// group (where the avatar goes, and the corner rounds off).
    pub group_end: bool,
    /// Sent by the logged-in user: drawn on the right.
    pub mine: bool,
    /// Nothing but a few emoji: drawn large, without a bubble surface.
    pub jumbo: bool,
}

/// Consecutive messages from one author closer together than this share a header.
pub const GROUP_WINDOW_MINUTES: u64 = 7;

/// One icon in the server rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    pub id: String,
    pub name: String,
    pub initials: String,
    /// The server's uploaded icon, if it has one.
    pub icon: Option<File>,
    /// Any of its channels is unread.
    pub unread: bool,
    /// Unread mentions across its channels.
    pub mentions: usize,
}

/// The user's read position in a channel, from Stoat's `ChannelUnread`.
#[derive(Debug, Default, Clone)]
struct ReadState {
    /// The newest message the user has read.
    last_id: Option<String>,
    /// Unread messages that mention the user.
    mentions: BTreeSet<String>,
}

#[derive(Default)]
pub struct ChatStore {
    me: String,
    users: HashMap<String, User>,
    servers: HashMap<String, Server>,
    /// Display order: the user's saved ordering, then any others in `Ready` order.
    server_order: Vec<String>,
    channels: HashMap<String, Channel>,
    /// Keyed by (server, user).
    members: HashMap<(String, String), Member>,
    /// Loaded message history, by channel ID.
    histories: HashMap<String, History>,
    /// Where images come from. Set once `GET /` has loaded.
    media: MediaUrls,
    /// Custom emoji of the user's servers, by ID.
    emojis: HashMap<String, Emoji>,
    /// Large images the user chose to load ("Show image"), by file ID.
    revealed_images: HashSet<String>,
    /// Read positions by channel ID.
    read_states: HashMap<String, ReadState>,
    /// The user's notification levels and mutes.
    notifications: NotificationSettings,
}

impl ChatStore {
    pub fn new(me: impl Into<String>) -> Self {
        Self {
            me: me.into(),
            ..Default::default()
        }
    }

    pub fn me(&self) -> &str {
        &self.me
    }

    /// Replace everything with a fresh `Ready` (on first connect and every reconnect).
    pub fn apply_ready(&mut self, ready: Ready) {
        let ready_order: Vec<String> = ready.servers.iter().map(|s| s.id.clone()).collect();
        self.users = ready.users.into_iter().map(|u| (u.id.clone(), u)).collect();
        self.servers = ready
            .servers
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
        self.channels = ready
            .channels
            .into_iter()
            .map(|c| (channel_id(&c).to_string(), c))
            .collect();
        self.members = ready
            .members
            .into_iter()
            .map(|m| ((m.id.server.clone(), m.id.user.clone()), m))
            .collect();
        self.emojis = ready
            .emojis
            .into_iter()
            .map(|e| (e.id.clone(), e))
            .collect();
        self.notifications = ready.notifications.unwrap_or_default();
        self.read_states = ready
            .unreads
            .into_iter()
            .filter(|u| u.id.user == self.me)
            .map(|u| {
                // A mention at or before the read position has been read.
                let mentions = u
                    .mentions
                    .into_iter()
                    .filter(|m| u.last_id.as_deref().is_none_or(|last| m.as_str() > last))
                    .collect();
                (
                    u.id.channel,
                    ReadState {
                        last_id: u.last_id,
                        mentions,
                    },
                )
            })
            .collect();
        for (id, state) in &self.read_states {
            if !state.mentions.is_empty()
                && matches!(
                    self.channels.get(id),
                    Some(Channel::DirectMessage { active: false, .. })
                )
            {
                log::debug!(
                    "closed DM {id} has {} unread mention(s); not counted",
                    state.mentions.len()
                );
            }
        }

        let mut seen = HashSet::new();
        self.server_order = ready
            .server_order
            .into_iter()
            .chain(ready_order)
            .filter(|id| self.servers.contains_key(id) && seen.insert(id.clone()))
            .collect();
    }

    /// Store a freshly fetched page of the latest messages for `channel`,
    /// replacing what was there. The authors that come with it are cached too.
    /// `limit` is what was requested, so a short page means "no more history".
    pub fn set_history(&mut self, channel: &str, page: BulkMessageResponse, limit: usize) {
        let mut messages = self.absorb_page(page);
        let exhausted = messages.len() < limit;
        sort_messages(&mut messages);
        self.histories.insert(
            channel.to_string(),
            History {
                messages,
                exhausted,
            },
        );
    }

    /// Merge a page of older messages (fetched with `before`) into a loaded
    /// channel. Returns false if the channel isn't loaded.
    pub fn prepend_page(&mut self, channel: &str, page: BulkMessageResponse, limit: usize) -> bool {
        let older = self.absorb_page(page);
        let Some(history) = self.histories.get_mut(channel) else {
            return false;
        };
        if older.len() < limit {
            history.exhausted = true;
        }
        history.messages.extend(older);
        sort_messages(&mut history.messages);
        true
    }

    /// Cache a page's authors and return its messages.
    fn absorb_page(&mut self, page: BulkMessageResponse) -> Vec<Message> {
        let (messages, users, members) = match page {
            BulkMessageResponse::JustMessages(messages) => (messages, Vec::new(), Vec::new()),
            BulkMessageResponse::MessagesAndUsers {
                messages,
                users,
                members,
            } => (messages, users, members.unwrap_or_default()),
        };
        for user in users {
            self.users.insert(user.id.clone(), user);
        }
        for member in members {
            self.members
                .insert((member.id.server.clone(), member.id.user.clone()), member);
        }
        messages
    }

    pub fn history(&self, channel: &str) -> Option<&History> {
        self.histories.get(channel)
    }

    /// The oldest loaded message, for fetching the page before it.
    pub fn oldest_message_id(&self, channel: &str) -> Option<&str> {
        self.histories
            .get(channel)?
            .messages
            .first()
            .map(|m| m.id.as_str())
    }

    /// Forget all loaded history (after a reconnect, to avoid gaps).
    pub fn clear_histories(&mut self) {
        self.histories.clear();
    }

    /// A live message. It's added to its channel's history if that's loaded,
    /// and keeps the channel's "last message" fresh for Home ordering either
    /// way. Returns true if a loaded history changed.
    pub fn apply_message(&mut self, message: Message) -> bool {
        self.bump_last_message(&message.channel, &message.id);
        if message.author == self.me {
            // Sending something means everything before it has been seen.
            self.apply_ack(&message.channel, &message.id);
        } else if self.is_mentioned(&message) {
            self.read_states
                .entry(message.channel.clone())
                .or_default()
                .mentions
                .insert(message.id.clone());
        }
        if let Some(user) = &message.user {
            self.users
                .entry(user.id.clone())
                .or_insert_with(|| user.clone());
        }
        if let Some(member) = &message.member {
            self.members
                .entry((member.id.server.clone(), member.id.user.clone()))
                .or_insert_with(|| member.clone());
        }
        let Some(history) = self.histories.get_mut(&message.channel) else {
            return false;
        };
        match history
            .messages
            .binary_search_by(|m| m.id.as_str().cmp(&message.id))
        {
            Ok(ix) => history.messages[ix] = message,
            Err(ix) => history.messages.insert(ix, message),
        }
        true
    }

    /// The channel has been read up to `message_id` (here, or by another of
    /// the user's sessions via `ChannelAck`). Never moves the position back.
    pub fn apply_ack(&mut self, channel: &str, message_id: &str) {
        let state = self.read_states.entry(channel.to_string()).or_default();
        if state
            .last_id
            .as_deref()
            .is_none_or(|last| last < message_id)
        {
            state.last_id = Some(message_id.to_string());
        }
        state.mentions.retain(|m| m.as_str() > message_id);
    }

    pub fn notification_settings(&self) -> &NotificationSettings {
        &self.notifications
    }

    /// New notification settings (`UserSettingsUpdate`).
    pub fn set_notification_settings(&mut self, settings: NotificationSettings) {
        self.notifications = settings;
    }

    /// The user's own presence (Online, Focus, Busy...), if known.
    pub fn my_presence(&self) -> Option<Presence> {
        self.users.get(&self.me)?.status.as_ref()?.presence.clone()
    }

    /// Muted in the user's notification settings (the channel or its server),
    /// right now.
    pub fn is_muted(&self, channel_id: &str) -> bool {
        self.notifications
            .is_muted(channel_id, self.server_of(channel_id), notify::now_ms())
    }

    /// The message to acknowledge to mark `channel` read: its newest message,
    /// if there's anything unread (or an unread mention) in it.
    pub fn unacked_latest(&self, channel_id: &str) -> Option<String> {
        if !self.is_unread(channel_id) && self.mention_count(channel_id) == 0 {
            return None;
        }
        last_message_id(self.channels.get(channel_id)?).map(str::to_string)
    }

    /// Mark every text channel in `server` read up to its newest message
    /// (what `PUT /servers/{id}/ack` does on the server).
    pub fn apply_server_ack(&mut self, server_id: &str) {
        let Some(server) = self.servers.get(server_id) else {
            return;
        };
        let acks: Vec<(String, String)> = server
            .channels
            .iter()
            .filter_map(|id| {
                let latest = last_message_id(self.channels.get(id)?)?;
                Some((id.clone(), latest.to_string()))
            })
            .collect();
        for (channel, latest) in acks {
            self.apply_ack(&channel, &latest);
        }
    }

    /// Whether a channel has messages newer than the user's read position.
    /// A channel with no read position at all counts as unread if it has any
    /// messages, as in Stoat's web client.
    pub fn is_unread(&self, channel_id: &str) -> bool {
        let Some(channel) = self.channels.get(channel_id) else {
            return false;
        };
        let last_message = match channel {
            Channel::SavedMessages { .. } | Channel::DirectMessage { active: false, .. } => {
                return false;
            }
            Channel::DirectMessage {
                last_message_id, ..
            }
            | Channel::Group {
                last_message_id, ..
            }
            | Channel::TextChannel {
                last_message_id, ..
            } => last_message_id.as_deref(),
        };
        let Some(last_message) = last_message else {
            return false;
        };
        self.read_states
            .get(channel_id)
            .and_then(|s| s.last_id.as_deref())
            .is_none_or(|read| read < last_message)
    }

    /// Whether the user's read position in `channel_id` has reached
    /// `message_id` (by reading it here or elsewhere, or by posting after it).
    pub fn has_read(&self, channel_id: &str, message_id: &str) -> bool {
        self.read_states
            .get(channel_id)
            .and_then(|s| s.last_id.as_deref())
            .is_some_and(|read| read >= message_id)
    }

    /// Unread messages in a channel that mention the user.
    pub fn mention_count(&self, channel_id: &str) -> usize {
        self.read_states
            .get(channel_id)
            .map_or(0, |s| s.mentions.len())
    }

    /// Whether a message mentions the user: directly, through one of their
    /// roles in the server, with @everyone, or with @online while online.
    pub fn is_mentioned(&self, message: &Message) -> bool {
        if message.author == self.me {
            return false;
        }
        if message
            .mentions
            .as_ref()
            .is_some_and(|m| m.contains(&self.me))
        {
            return true;
        }
        if let Some(roles) = &message.role_mentions
            && let Some(server) = self.server_of(&message.channel)
            && let Some(member) = self.member(server, &self.me)
            && roles.iter().any(|r| member.roles.contains(r))
        {
            return true;
        }
        if has_flag(message.flags, MessageFlags::MentionsEveryone) {
            return true;
        }
        let online = self.users.get(&self.me).is_some_and(|u| u.online);
        online && has_flag(message.flags, MessageFlags::MentionsOnline)
    }

    /// The Dock badge count: unread mentions everywhere, plus each unread DM
    /// or group whose level is "all" (once, even if it also has mentions).
    /// Muted channels don't count.
    pub fn attention_count(&self) -> usize {
        self.attention_breakdown().iter().map(|(_, n)| n).sum()
    }

    /// What [`Self::attention_count`] is made of: (channel label, count) for
    /// every channel that contributes. For debugging the Dock badge.
    pub fn attention_breakdown(&self) -> Vec<(String, usize)> {
        let mut parts: Vec<(String, usize)> = self
            .channels
            .iter()
            .map(|(id, channel)| (id, self.attention_for(id, channel)))
            .filter(|(_, n)| *n > 0)
            .map(|(id, n)| {
                let entry = self.channel_entry(id);
                let label = match (entry, self.server_of(id).and_then(|s| self.servers.get(s))) {
                    (Some(entry), Some(server)) => format!("#{} ({})", entry.name, server.name),
                    (Some(entry), None) => format!("{:?} {}", entry.kind, entry.name),
                    (None, _) => id.clone(),
                };
                let read = self
                    .read_states
                    .get(id.as_str())
                    .map_or("no read position", |s| {
                        if s.last_id.is_some() {
                            "behind"
                        } else {
                            "never read"
                        }
                    });
                (
                    format!(
                        "{label} [{id}, {read}, {} mentions]",
                        self.mention_count(id)
                    ),
                    n,
                )
            })
            .collect();
        parts.sort();
        parts
    }

    /// One channel's share of [`Self::attention_count`].
    fn attention_for(&self, id: &str, channel: &Channel) -> usize {
        // Closed DMs aren't listed anywhere, so the user can't read them; an
        // old mention left in one must not pin the badge.
        if self.is_muted(id) || matches!(channel, Channel::DirectMessage { active: false, .. }) {
            return 0;
        }
        let mentions = self.mention_count(id);
        let conversation = matches!(
            channel,
            Channel::DirectMessage { .. } | Channel::Group { .. }
        );
        if conversation
            && self.is_unread(id)
            && self.notifications.level(id, None) == notify::Level::All
        {
            mentions.max(1)
        } else {
            mentions
        }
    }

    /// Unread state for the Home button: open DMs and groups.
    pub fn home_summary(&self) -> (bool, usize) {
        self.home_channels()
            .iter()
            .fold((false, 0), |(unread, mentions), c| {
                (unread || c.unread, mentions + c.mentions)
            })
    }

    /// An edit, merged with the backend's own `Message::apply_options`.
    /// Returns true if a loaded message changed.
    pub fn apply_update(
        &mut self,
        channel: &str,
        id: &str,
        data: PartialMessage,
        clear: &[FieldsMessage],
    ) -> bool {
        let Some(message) = self
            .histories
            .get_mut(channel)
            .and_then(|h| h.messages.iter_mut().find(|m| m.id == id))
        else {
            return false;
        };
        message.apply_options(data);
        for field in clear {
            match field {
                FieldsMessage::Pinned => message.pinned = None,
            }
        }
        true
    }

    /// Link previews that arrived after their message (`MessageAppend`).
    /// Returns true if a loaded message changed.
    pub fn apply_append(&mut self, channel: &str, id: &str, embeds: Vec<Embed>) -> bool {
        if embeds.is_empty() {
            return false;
        }
        let Some(message) = self
            .histories
            .get_mut(channel)
            .and_then(|h| h.messages.iter_mut().find(|m| m.id == id))
        else {
            return false;
        };
        message.embeds.get_or_insert_default().extend(embeds);
        true
    }

    /// Remove messages from a loaded channel. Returns true if any were removed.
    pub fn apply_delete(&mut self, channel: &str, ids: &[String]) -> bool {
        let Some(history) = self.histories.get_mut(channel) else {
            return false;
        };
        let before = history.messages.len();
        history.messages.retain(|m| !ids.contains(&m.id));
        history.messages.len() != before
    }

    fn bump_last_message(&mut self, channel: &str, id: &str) {
        if let Some(
            Channel::DirectMessage {
                last_message_id, ..
            }
            | Channel::Group {
                last_message_id, ..
            }
            | Channel::TextChannel {
                last_message_id, ..
            },
        ) = self.channels.get_mut(channel)
            && last_message_id.as_deref().is_none_or(|last| last < id)
        {
            *last_message_id = Some(id.to_string());
        }
    }

    /// A channel new to the user (`ChannelCreate`): a DM or group someone
    /// opened with them, or a new server channel (added to its server's list).
    pub fn add_channel(&mut self, channel: Channel) {
        let id = channel_id(&channel).to_string();
        if let Channel::TextChannel { server, .. } = &channel
            && let Some(server) = self.servers.get_mut(server)
            && !server.channels.contains(&id)
        {
            server.channels.push(id.clone());
        }
        self.channels.insert(id, channel);
    }

    /// A channel changed (`ChannelUpdate`): merge what's in `data`, then clear
    /// the `clear` fields. Unknown channels are ignored. The last message
    /// only moves forward, as with live messages.
    pub fn apply_channel_update(
        &mut self,
        id: &str,
        data: PartialChannel,
        clear: &[FieldsChannel],
    ) {
        let Some(channel) = self.channels.get_mut(id) else {
            return;
        };
        let PartialChannel {
            name: new_name,
            owner: new_owner,
            description: new_description,
            icon: new_icon,
            nsfw: new_nsfw,
            active: new_active,
            permissions: new_permissions,
            role_permissions: new_role_permissions,
            default_permissions: new_default_permissions,
            last_message_id: new_last_message,
            voice: new_voice,
            slowmode: new_slowmode,
        } = data;
        match channel {
            Channel::SavedMessages { .. } => {}
            Channel::DirectMessage { active, .. } => {
                if let Some(new) = new_active {
                    *active = new;
                }
            }
            Channel::Group {
                name,
                owner,
                description,
                icon,
                permissions,
                nsfw,
                ..
            } => {
                set(name, new_name);
                set(owner, new_owner);
                set(nsfw, new_nsfw);
                set_some(description, new_description);
                set_some(icon, new_icon);
                set_some(permissions, new_permissions);
                for field in clear {
                    match field {
                        FieldsChannel::Description => *description = None,
                        FieldsChannel::Icon => *icon = None,
                        _ => {}
                    }
                }
            }
            Channel::TextChannel {
                name,
                description,
                icon,
                default_permissions,
                role_permissions,
                nsfw,
                voice,
                slowmode,
                ..
            } => {
                set(name, new_name);
                set(nsfw, new_nsfw);
                set(role_permissions, new_role_permissions);
                set_some(description, new_description);
                set_some(icon, new_icon);
                set_some(default_permissions, new_default_permissions);
                set_some(voice, new_voice);
                set_some(slowmode, new_slowmode);
                for field in clear {
                    match field {
                        FieldsChannel::Description => *description = None,
                        FieldsChannel::Icon => *icon = None,
                        FieldsChannel::DefaultPermissions => *default_permissions = None,
                        FieldsChannel::Voice => *voice = None,
                        FieldsChannel::Slowmode => *slowmode = None,
                    }
                }
            }
        }
        if let Some(last) = new_last_message {
            self.bump_last_message(id, &last);
        }
    }

    /// A server emoji was added (`EmojiCreate`).
    pub fn add_emoji(&mut self, emoji: Emoji) {
        self.emojis.insert(emoji.id.clone(), emoji);
    }

    /// A server emoji was deleted (`EmojiDelete`).
    pub fn remove_emoji(&mut self, id: &str) {
        self.emojis.remove(id);
    }

    pub fn emoji(&self, id: &str) -> Option<&Emoji> {
        self.emojis.get(id)
    }

    pub fn set_media(&mut self, media: MediaUrls) {
        self.media = media;
    }

    pub fn media(&self) -> &MediaUrls {
        &self.media
    }

    /// Load a large image the user asked to see, despite its size. Its row
    /// changes from a card to the image.
    pub fn reveal_image(&mut self, file_id: &str) {
        self.revealed_images.insert(file_id.to_string());
    }

    /// A user's own avatar, or their generated default.
    pub fn user_avatar_url(&self, user_id: &str) -> String {
        self.users
            .get(user_id)
            .and_then(|u| u.avatar.as_ref())
            .and_then(|file| self.media.file(file))
            .unwrap_or_else(|| self.media.default_avatar(user_id))
    }

    /// Which picture to show for a message's author, in the same order as
    /// [`Self::author_name`]: masquerade avatar (external, so proxied),
    /// webhook avatar, server member avatar, user avatar, generated default.
    pub fn avatar_url(&self, message: &Message) -> Option<String> {
        if message.system.is_some() {
            return None;
        }
        if let Some(url) = message
            .masquerade
            .as_ref()
            .and_then(|m| m.avatar.as_deref())
            .filter(|url| !url.trim().is_empty())
        {
            return self.media.external(url);
        }
        if let Some(webhook) = &message.webhook {
            // Webhooks aren't users, so there's no generated default for them.
            return webhook
                .avatar
                .as_deref()
                .and_then(|id| self.media.file_by_id("avatars", id));
        }
        let member_avatar = message
            .member
            .as_ref()
            .and_then(|m| m.avatar.clone())
            .or_else(|| {
                let server = self.server_of(&message.channel)?;
                self.member(server, &message.author)?.avatar.clone()
            });
        if let Some(url) = member_avatar.as_ref().and_then(|f| self.media.file(f)) {
            return Some(url);
        }
        if let Some(url) = message
            .user
            .as_ref()
            .filter(|_| !self.users.contains_key(&message.author))
            .and_then(|u| u.avatar.as_ref())
            .and_then(|f| self.media.file(f))
        {
            return Some(url);
        }
        Some(self.user_avatar_url(&message.author))
    }

    /// Who to show as a message's author. In order: a masquerade name
    /// (bridges and bots), the webhook's name, the author's nickname in this
    /// server, their display name, their username.
    pub fn author_name(&self, message: &Message) -> String {
        if let Some(name) = message.masquerade.as_ref().and_then(|m| m.name.as_deref())
            && !name.trim().is_empty()
        {
            return name.to_string();
        }
        if let Some(webhook) = &message.webhook {
            return webhook.name.clone();
        }
        let nickname = message
            .member
            .as_ref()
            .and_then(|m| m.nickname.clone())
            .or_else(|| {
                let server = self.server_of(&message.channel)?;
                self.member(server, &message.author)?.nickname.clone()
            });
        if let Some(nickname) = nickname
            && !nickname.trim().is_empty()
        {
            return nickname;
        }
        self.user_name(&message.author)
            .or_else(|| message.user.as_ref().map(|u| display_name(u).to_string()))
            .unwrap_or_else(|| "Unknown user".into())
    }

    /// A one-line description of a system message, with user names filled in.
    /// Variants this client doesn't describe use the backend's own wording.
    pub fn system_text(&self, system: &SystemMessage) -> String {
        let name = |id: &str| self.user_name(id).unwrap_or_else(|| "Someone".into());
        match system {
            SystemMessage::Text { content } => content.clone(),
            SystemMessage::UserAdded { id, by } => format!("{} added {}", name(by), name(id)),
            SystemMessage::UserRemove { id, by } => format!("{} removed {}", name(by), name(id)),
            SystemMessage::UserJoined { id } => format!("{} joined", name(id)),
            SystemMessage::UserLeft { id } => format!("{} left", name(id)),
            SystemMessage::UserKicked { id } => format!("{} was kicked", name(id)),
            SystemMessage::UserBanned { id } => format!("{} was banned", name(id)),
            SystemMessage::ChannelRenamed { name: new, by } => {
                format!("{} renamed the channel to {new}", name(by))
            }
            SystemMessage::ChannelDescriptionChanged { by } => {
                format!("{} changed the channel description", name(by))
            }
            SystemMessage::ChannelIconChanged { by } => {
                format!("{} changed the channel icon", name(by))
            }
            SystemMessage::ChannelOwnershipChanged { from, to } => {
                format!("{} gave ownership to {}", name(from), name(to))
            }
            SystemMessage::MessagePinned { by, .. } => format!("{} pinned a message", name(by)),
            SystemMessage::MessageUnpinned { by, .. } => {
                format!("{} unpinned a message", name(by))
            }
            other => String::from(other.clone()),
        }
    }

    /// Display rows for a loaded channel, oldest first. Empty if not loaded.
    pub fn timeline(&self, channel: &str) -> Vec<TimelineRow> {
        let Some(history) = self.histories.get(channel) else {
            return Vec::new();
        };
        let mut rows = Vec::with_capacity(history.messages.len() + 1);
        if history.exhausted {
            let name = self
                .channel_entry(channel)
                .map(|c| c.name)
                .unwrap_or_else(|| "this conversation".into());
            rows.push(TimelineRow::StartOfChannel { name });
        }

        let mut previous: Option<(&Message, String)> = None;
        for message in &history.messages {
            let author = self.author_name(message);
            let system = message.system.is_some();
            let continuation = !system
                && previous.as_ref().is_some_and(|(prev, prev_author)| {
                    prev.system.is_none()
                        && prev.author == message.author
                        && *prev_author == author
                        && crate::time::minutes_between(&prev.id, &message.id)
                            .is_some_and(|m| m < GROUP_WINDOW_MINUTES)
                });
            let body = match &message.system {
                Some(system) => Some(self.system_text(system)),
                None => message
                    .content
                    .as_deref()
                    .filter(|c| !c.is_empty())
                    .map(|c| crate::markdown::prepare(c, self)),
            };
            let render_key = render_key(&message.id, body.as_deref());
            rows.push(TimelineRow::Message(MessageRow {
                id: message.id.clone(),
                render_key,
                author: author.clone(),
                avatar: self.avatar_url(message),
                time: crate::time::message_time_label(&message.id),
                body,
                attachments: message
                    .attachments
                    .iter()
                    .flatten()
                    .map(|file| {
                        let revealed = self.revealed_images.contains(&file.id);
                        AttachmentView::new(file, &self.media, revealed)
                    })
                    .collect(),
                embeds: message
                    .embeds
                    .iter()
                    .flatten()
                    .enumerate()
                    .filter_map(|(ix, embed)| {
                        let key = format!("{}:embed{ix}", message.id);
                        EmbedView::new(embed, &self.media, self, &self.revealed_images, &key)
                    })
                    .collect(),
                system,
                edited: message.edited.is_some(),
                continuation,
                // Fixed up below, once the next row is known.
                group_end: true,
                mine: message.author == self.me,
                jumbo: !system
                    && message
                        .content
                        .as_deref()
                        .is_some_and(crate::markdown::is_jumbo),
            }));
            previous = Some((message, author));
        }
        mark_group_ends(&mut rows);
        rows
    }

    fn user_name(&self, id: &str) -> Option<String> {
        self.users.get(id).map(|u| display_name(u).to_string())
    }

    /// The server a channel belongs to, if it's a server channel.
    fn server_of(&self, channel: &str) -> Option<&str> {
        match self.channels.get(channel)? {
            Channel::TextChannel { server, .. } => Some(server),
            _ => None,
        }
    }

    pub fn user(&self, id: &str) -> Option<&User> {
        self.users.get(id)
    }

    pub fn server(&self, id: &str) -> Option<&Server> {
        self.servers.get(id)
    }

    pub fn member(&self, server: &str, user: &str) -> Option<&Member> {
        self.members.get(&(server.to_string(), user.to_string()))
    }

    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.channels.get(id)
    }

    /// Servers for the rail, in the user's order.
    pub fn servers(&self) -> Vec<ServerEntry> {
        self.server_order
            .iter()
            .filter_map(|id| self.servers.get(id))
            .map(|s| {
                let visible = s.channels.iter().filter(|id| {
                    matches!(self.channels.get(*id), Some(Channel::TextChannel { .. }))
                });
                let (unread, mentions) = visible.fold((false, 0), |(unread, mentions), id| {
                    let shows_dot = self.is_unread(id) && !self.is_muted(id);
                    (unread || shows_dot, mentions + self.mention_count(id))
                });
                ServerEntry {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    initials: initials(&s.name),
                    icon: s.icon.clone(),
                    unread,
                    mentions,
                }
            })
            .collect()
    }

    /// A server's text channels: uncategorized ones first (in the server's
    /// order), then each category in order. Channels we can't see (missing
    /// from `Ready`) and empty categories are left out.
    pub fn server_channels(&self, server_id: &str) -> Vec<ChannelSection> {
        let Some(server) = self.servers.get(server_id) else {
            return Vec::new();
        };
        let categories = server.categories.as_deref().unwrap_or_default();
        let categorized: HashSet<&str> = categories
            .iter()
            .flat_map(|c| c.channels.iter().map(String::as_str))
            .collect();

        let mut sections = Vec::new();
        let uncategorized: Vec<ChannelEntry> = server
            .channels
            .iter()
            .filter(|id| !categorized.contains(id.as_str()))
            .filter_map(|id| self.channel_entry(id))
            .collect();
        if !uncategorized.is_empty() {
            sections.push(ChannelSection {
                title: None,
                channels: uncategorized,
            });
        }
        for category in categories {
            let channels: Vec<ChannelEntry> = category
                .channels
                .iter()
                .filter_map(|id| self.channel_entry(id))
                .collect();
            if !channels.is_empty() {
                sections.push(ChannelSection {
                    title: Some(category.title.clone()),
                    channels,
                });
            }
        }
        sections
    }

    /// Home: Saved Notes first, then open DMs and groups, most recent first.
    pub fn home_channels(&self) -> Vec<ChannelEntry> {
        let mut saved = Vec::new();
        let mut conversations: Vec<(&Channel, ChannelEntry)> = Vec::new();
        for channel in self.channels.values() {
            match channel {
                Channel::SavedMessages { .. } => saved.extend(self.entry_for(channel)),
                Channel::DirectMessage { active: false, .. } => {}
                Channel::DirectMessage { .. } | Channel::Group { .. } => {
                    if let Some(entry) = self.entry_for(channel) {
                        conversations.push((channel, entry));
                    }
                }
                Channel::TextChannel { .. } => {}
            }
        }
        // ULIDs sort by time, so the newest last message sorts highest.
        // Ties (and conversations with no messages) fall back to name.
        conversations.sort_by(|(a, ea), (b, eb)| {
            last_message_id(b)
                .cmp(&last_message_id(a))
                .then_with(|| ea.name.cmp(&eb.name))
        });
        saved
            .into_iter()
            .chain(conversations.into_iter().map(|(_, e)| e))
            .collect()
    }

    pub fn channel_entry(&self, id: &str) -> Option<ChannelEntry> {
        self.channels.get(id).and_then(|c| self.entry_for(c))
    }

    fn entry_for(&self, channel: &Channel) -> Option<ChannelEntry> {
        let (name, kind, avatar) = match channel {
            Channel::TextChannel { name, voice, .. } => (
                name.clone(),
                ChannelKind::Text {
                    voice: voice.is_some(),
                },
                None,
            ),
            Channel::SavedMessages { .. } => {
                ("Saved Notes".into(), ChannelKind::SavedMessages, None)
            }
            Channel::Group { name, icon, .. } => (
                name.clone(),
                ChannelKind::Group,
                icon.as_ref().and_then(|f| self.media.file(f)),
            ),
            Channel::DirectMessage { recipients, .. } => {
                let other = recipients.iter().find(|id| **id != self.me);
                let name = other
                    .and_then(|id| self.users.get(id))
                    .map(|u| display_name(u).to_string())
                    .unwrap_or_else(|| "Unknown user".into());
                let avatar = other.map(|id| self.user_avatar_url(id));
                (name, ChannelKind::DirectMessage, avatar)
            }
        };
        let id = channel_id(channel).to_string();
        Some(ChannelEntry {
            // Muted: no unread dot, but mentions still show.
            unread: self.is_unread(&id) && !self.is_muted(&id),
            mentions: self.mention_count(&id),
            id,
            name,
            kind,
            avatar,
        })
    }
}

/// How to turn a message list's old rows into its new ones without losing
/// the reader's place. Indexes are in the list's coordinates at the moment
/// each operation is applied, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOp {
    /// Start over with this many rows (and jump to the newest).
    Reset(usize),
    /// Rows added above the old first row; the scroll anchor is kept.
    Prepend(usize),
    /// Rows added after the old last row.
    Append(usize),
    Remove {
        at: usize,
        count: usize,
    },
    Insert {
        at: usize,
        count: usize,
    },
    /// Same row, new content (an edit, or a grouping change): re-measure it.
    Remeasure {
        at: usize,
    },
}

fn row_key(row: &TimelineRow) -> &str {
    match row {
        TimelineRow::StartOfChannel { .. } => "",
        TimelineRow::Message(message) => &message.id,
    }
}

/// Diff two timelines (both sorted oldest first) into scroller operations.
pub fn diff_rows(old: &[TimelineRow], new: &[TimelineRow]) -> Vec<RowOp> {
    let new_keys: HashSet<&str> = new.iter().map(row_key).collect();
    // The first old row that survives anchors the diff.
    let Some(first_kept) = old.iter().position(|r| new_keys.contains(row_key(r))) else {
        return vec![RowOp::Reset(new.len())];
    };
    let anchor = new
        .iter()
        .position(|r| row_key(r) == row_key(&old[first_kept]))
        .expect("key is in new");

    let mut ops = Vec::new();
    if first_kept > 0 {
        ops.push(RowOp::Remove {
            at: 0,
            count: first_kept,
        });
    }
    if anchor > 0 {
        ops.push(RowOp::Prepend(anchor));
    }
    let (mut i, mut j) = (first_kept, anchor);
    while i < old.len() && j < new.len() {
        let (old_key, new_key) = (row_key(&old[i]), row_key(&new[j]));
        if old_key == new_key {
            if old[i] != new[j] {
                ops.push(RowOp::Remeasure { at: j });
            }
            i += 1;
            j += 1;
        } else if !new_keys.contains(old_key) {
            ops.push(RowOp::Remove { at: j, count: 1 });
            i += 1;
        } else {
            ops.push(RowOp::Insert { at: j, count: 1 });
            j += 1;
        }
    }
    if i < old.len() {
        ops.push(RowOp::Remove {
            at: j,
            count: old.len() - i,
        });
    }
    if j < new.len() {
        ops.push(RowOp::Append(new.len() - j));
    }
    ops
}

/// Whether a message's `flags` has `flag` set. `MessageFlags` values are bit
/// positions (the backend tests `flags & (1 << flag)`), not masks.
fn has_flag(flags: u32, flag: MessageFlags) -> bool {
    flags & (1 << flag as u32) != 0
}

/// A message row ends its group unless the next row continues it.
fn mark_group_ends(rows: &mut [TimelineRow]) {
    for i in 0..rows.len() {
        let next_continues = matches!(
            rows.get(i + 1),
            Some(TimelineRow::Message(next)) if next.continuation
        );
        if let TimelineRow::Message(row) = &mut rows[i] {
            row.group_end = !next_continues;
        }
    }
}

/// `"{id}:{hash of body}"`, see [`MessageRow::render_key`].
pub fn render_key(id: &str, body: Option<&str>) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    format!("{id}:{:016x}", hasher.finish())
}

impl crate::markdown::Names for ChatStore {
    fn user_name(&self, id: &str) -> Option<String> {
        self.users.get(id).map(|u| display_name(u).to_string())
    }

    fn channel_name(&self, id: &str) -> Option<String> {
        self.channel_entry(id).map(|c| c.name)
    }

    fn emoji_name(&self, id: &str) -> Option<String> {
        self.emojis.get(id).map(|e| e.name.clone())
    }

    fn emoji_url(&self, id: &str) -> Option<String> {
        self.media.emoji(id)
    }
}

/// Oldest first, no duplicates. ULIDs sort by time, so ID order is time order.
fn sort_messages(messages: &mut Vec<Message>) {
    messages.sort_by(|a, b| a.id.cmp(&b.id));
    messages.dedup_by(|a, b| a.id == b.id);
}

/// A user's display name, falling back to their username.
pub fn display_name(user: &User) -> &str {
    match user.display_name.as_deref() {
        Some(name) if !name.trim().is_empty() => name,
        _ => &user.username,
    }
}

/// Up to two letters for a server icon: "Stoat Testers" → "ST", "rust" → "R".
pub fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() {
        name.chars().take(1).collect()
    } else {
        letters
    }
}

pub fn channel_id(channel: &Channel) -> &str {
    match channel {
        Channel::SavedMessages { id, .. }
        | Channel::DirectMessage { id, .. }
        | Channel::Group { id, .. }
        | Channel::TextChannel { id, .. } => id,
    }
}

/// Overwrite `field` if a partial update carries a new value.
fn set<T>(field: &mut T, new: Option<T>) {
    if let Some(new) = new {
        *field = new;
    }
}

/// [`set`] for optional fields.
fn set_some<T>(field: &mut Option<T>, new: Option<T>) {
    if new.is_some() {
        *field = new;
    }
}

fn last_message_id(channel: &Channel) -> Option<&str> {
    match channel {
        Channel::DirectMessage {
            last_message_id, ..
        }
        | Channel::Group {
            last_message_id, ..
        }
        | Channel::TextChannel {
            last_message_id, ..
        } => last_message_id.as_deref(),
        Channel::SavedMessages { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn initials_use_first_letters_of_up_to_two_words() {
        assert_eq!(initials("Stoat Testers"), "ST");
        assert_eq!(initials("rust"), "R");
        assert_eq!(initials("The Rust Programming Language"), "TR");
        assert_eq!(initials("  [beta]  lounge "), "BL");
        assert_eq!(initials("🦦"), "🦦");
    }
}
