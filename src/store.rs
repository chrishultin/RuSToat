//! The client's in-memory picture of the account, built from the events
//! connection. Plain data with no GPUI, so ordering and naming rules are
//! unit-testable.

use std::collections::{HashMap, HashSet};

use crate::api::events::Ready;
use crate::api::models::{
    BulkMessageResponse, Channel, File, Member, Message, Server, SystemMessage, User,
};

/// One entry in the channel list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEntry {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
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
    /// "Today at 14:05" etc.
    pub time: String,
    /// Plain text with mentions resolved. For system messages, the description.
    pub body: Option<String>,
    pub attachments: Vec<String>,
    pub system: bool,
    pub edited: bool,
    /// Same author as the previous message, soon after it: drawn without a
    /// name/time header.
    pub continuation: bool,
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
        let (mut messages, users, members) = match page {
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
        let exhausted = messages.len() < limit;
        // The API returns newest first. ULIDs sort by time, so sorting by ID
        // gives oldest-first display order.
        messages.sort_by(|a, b| a.id.cmp(&b.id));
        messages.dedup_by(|a, b| a.id == b.id);
        self.histories.insert(
            channel.to_string(),
            History {
                messages,
                exhausted,
            },
        );
    }

    pub fn history(&self, channel: &str) -> Option<&History> {
        self.histories.get(channel)
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

    /// Message text with `<@user>` and `<#channel>` mentions replaced by names.
    pub fn render_content(&self, content: &str) -> String {
        let mut out = String::with_capacity(content.len());
        let mut rest = content;
        while let Some(start) = rest.find('<') {
            out.push_str(&rest[..start]);
            let tail = &rest[start..];
            let replacement = tail.find('>').and_then(|end| {
                let inner = &tail[1..end];
                let text = match (inner.strip_prefix('@'), inner.strip_prefix('#')) {
                    (Some(user), _) => format!("@{}", self.user_name(user)?),
                    (_, Some(channel)) => format!("#{}", self.channel_entry(channel)?.name),
                    _ => return None,
                };
                Some((text, end + 1))
            });
            match replacement {
                Some((text, consumed)) => {
                    out.push_str(&text);
                    rest = &tail[consumed..];
                }
                None => {
                    out.push('<');
                    rest = &tail[1..];
                }
            }
        }
        out.push_str(rest);
        out
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
                    .map(|c| self.render_content(c)),
            };
            rows.push(TimelineRow::Message(MessageRow {
                id: message.id.clone(),
                author: author.clone(),
                time: crate::time::message_time_label(&message.id),
                body,
                attachments: message
                    .attachments
                    .iter()
                    .flatten()
                    .map(|f| f.filename.clone())
                    .collect(),
                system,
                edited: message.edited.is_some(),
                continuation,
            }));
            previous = Some((message, author));
        }
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
            .map(|s| ServerEntry {
                id: s.id.clone(),
                name: s.name.clone(),
                initials: initials(&s.name),
                icon: s.icon.clone(),
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
        let (name, kind) = match channel {
            Channel::TextChannel { name, voice, .. } => (
                name.clone(),
                ChannelKind::Text {
                    voice: voice.is_some(),
                },
            ),
            Channel::SavedMessages { .. } => ("Saved Notes".into(), ChannelKind::SavedMessages),
            Channel::Group { name, .. } => (name.clone(), ChannelKind::Group),
            Channel::DirectMessage { recipients, .. } => {
                let other = recipients.iter().find(|id| **id != self.me);
                let name = other
                    .and_then(|id| self.users.get(id))
                    .map(|u| display_name(u).to_string())
                    .unwrap_or_else(|| "Unknown user".into());
                (name, ChannelKind::DirectMessage)
            }
        };
        Some(ChannelEntry {
            id: channel_id(channel).to_string(),
            name,
            kind,
        })
    }
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
