//! The client's in-memory picture of the account, built from the events
//! connection. Plain data with no GPUI, so ordering and naming rules are
//! unit-testable.

use std::collections::{HashMap, HashSet};

use crate::api::events::Ready;
use crate::api::models::{Channel, File, Member, Server, User};

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
