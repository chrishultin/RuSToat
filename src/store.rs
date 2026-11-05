//! The client's in-memory picture of the account, built from the events
//! connection. Plain data with no GPUI, so ordering and naming rules are
//! unit-testable.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::api::events::Ready;
use crate::api::models::{
    BulkMessageResponse, Channel, Embed, Emoji, EmojiParent, FieldsChannel, FieldsMessage, File,
    Member, Message, MessageFlags, PartialChannel, PartialMessage, Presence, ReplyIntent, Server,
    SystemMessage, User,
};
use crate::attachment::AttachmentView;
use crate::draft::DraftAttachment;
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
// Nearly every row is a message; the one small start-of-channel row isn't
// worth boxing every message for.
#[allow(clippy::large_enum_variant)]
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
    /// The message's ID. For a pending message, its [`PendingMessage::key`].
    pub id: String,
    /// What identifies the row in the list (see [`diff_rows`]): the
    /// idempotency key for the user's own messages that have one, so a
    /// pending row and the message that confirms it are the same row.
    /// Otherwise the ID.
    pub key: String,
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
    /// Reactions, in the order they were first added.
    pub reactions: Vec<ReactionChip>,
    /// The only emoji the picker offers for this message, when its author
    /// restricted reactions. `None`: the channel's usual choices.
    pub allowed_reactions: Option<Vec<PickerEmoji>>,
    /// Not on the server yet: sent and waiting, or failed. `None` for real messages.
    pub pending: Option<PendingState>,
    /// What it replies to, in order (at most [`MAX_REPLIES`]).
    pub replies: Vec<ReplyPreview>,
}

/// Most messages one message can reply to (Stoat's `limits.global.message_replies`).
pub const MAX_REPLIES: usize = 5;

/// Longest reply-preview snippet, in characters.
pub const SNIPPET_CHARS: usize = 100;

/// The "↱ Name: snippet" line above a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyPreview {
    /// The original message's ID.
    pub id: String,
    /// The original's author, as [`ChatStore::author_name`] names them.
    /// Empty unless `state` is `Loaded`.
    pub author: String,
    pub avatar: Option<String>,
    /// One line of the original as plain text (see [`crate::markdown::snippet`]).
    pub snippet: String,
    /// The reply pinged the original's author.
    pub mentions_author: bool,
    pub state: ReplyState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyState {
    Loaded,
    /// Not loaded yet: being fetched (see [`ChatStore::missing_reply_sources`]).
    Loading,
    /// The original was deleted.
    Deleted,
    /// Fetching the original failed.
    Unavailable,
}

/// What a local edit replaced, to put back if Stoat refuses it
/// ([`ChatStore::undo_local_edit`]).
#[derive(Debug, Clone)]
pub struct LocalEdit {
    previous: Box<Message>,
    /// The text the edit put in.
    content: String,
}

/// A cached original of a reply that isn't in the loaded history.
#[derive(Debug, Clone)]
enum ReplySource {
    Found(Box<Message>),
    Deleted,
    Unavailable,
}

/// A message the user sent that Stoat hasn't confirmed yet. It's shown at
/// once, after the loaded messages, and replaced by the real message when
/// that arrives with a matching `nonce` (from the POST response or the live
/// event, whichever comes first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMessage {
    /// Identifies it while it's pending, even if a retry changes `nonce`.
    /// Its first `nonce`.
    pub key: String,
    /// The `Idempotency-Key` it's sent with, which Stoat copies into the
    /// created message's `nonce`.
    pub nonce: String,
    pub channel: String,
    /// What the user typed (Markdown source).
    pub content: String,
    pub replies: Vec<ReplyIntent>,
    /// Its files, uploaded one at a time before it's posted.
    pub attachments: Vec<PendingAttachment>,
    pub state: PendingState,
    /// When it was sent, in milliseconds since the Unix epoch.
    pub sent_ms: u64,
    /// The last attempt got an answer from Stoat (an error), so Stoat has
    /// seen `nonce` and would refuse it as a duplicate. A retry needs a new one.
    pub nonce_used: bool,
}

/// A file of a pending message, and how far its upload has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAttachment {
    pub file: DraftAttachment,
    pub upload: UploadState,
}

impl PendingAttachment {
    pub fn new(file: DraftAttachment) -> Self {
        Self {
            file,
            upload: UploadState::Waiting,
        }
    }

    /// The uploaded file's ID, once it has one.
    pub fn uploaded_id(&self) -> Option<&str> {
        match &self.upload {
            UploadState::Uploaded(id) => Some(id),
            _ => None,
        }
    }

    /// How much has uploaded, in thousandths.
    fn progress(&self) -> u16 {
        match self.upload {
            UploadState::Waiting => 0,
            UploadState::Uploading { total: 0, .. } => 0,
            UploadState::Uploading { sent, total } => (sent.min(total) * 1000 / total) as u16,
            UploadState::Uploaded(_) => 1000,
        }
    }

    /// How it's drawn in its pending row.
    fn view(&self) -> AttachmentView {
        let file = &self.file;
        let image = (file.kind == crate::attachment::FileKind::Image).then(|| {
            let (width, height) = file
                .dimensions
                .map_or(crate::attachment::FALLBACK_IMAGE_SIZE, |(w, h)| {
                    crate::attachment::fit_within(w as usize, h as usize)
                });
            (file.path.clone(), width, height)
        });
        AttachmentView::Local {
            name: file.name.clone(),
            size_label: crate::attachment::size_label(file.size),
            kind: file.kind,
            image,
            progress: self.progress(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadState {
    Waiting,
    /// `sent` of `total` bytes handed over so far.
    Uploading { sent: u64, total: u64 },
    /// Uploaded: the file's ID, to send with the message.
    Uploaded(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingState {
    Sending,
    /// Why it wasn't sent, for the user.
    Failed(String),
}

/// An emoji the reaction picker offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEmoji {
    /// As the API names it: the Unicode emoji, or a custom emoji's ID.
    pub key: String,
    pub emoji: ReactionEmoji,
}

/// Most different emoji a message can have (Stoat's
/// `limits.global.message_reactions`, 20 by default). Adding a new one past
/// it is refused with `InvalidOperation`; joining an existing one still works.
pub const MAX_REACTIONS: usize = 20;

/// What a message's reaction picker offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerChoices {
    pub emoji: Vec<PickerEmoji>,
    /// The message is at [`MAX_REACTIONS`]: only its existing emoji are offered.
    pub full: bool,
}

impl MessageRow {
    /// The message already has as many different emoji as it can.
    pub fn reactions_full(&self) -> bool {
        self.reactions.len() >= MAX_REACTIONS
    }

    /// The picker for this message: its existing emoji when it's full, else
    /// the emoji its author allowed, else the channel's usual `choices`.
    pub fn picker_choices(&self, choices: &[PickerEmoji]) -> PickerChoices {
        if self.reactions_full() {
            return PickerChoices {
                emoji: self
                    .reactions
                    .iter()
                    .map(|chip| PickerEmoji {
                        key: chip.key.clone(),
                        emoji: chip.emoji.clone(),
                    })
                    .collect(),
                full: true,
            };
        }
        PickerChoices {
            emoji: self
                .allowed_reactions
                .clone()
                .unwrap_or_else(|| choices.to_vec()),
            full: false,
        }
    }
}

/// The Unicode emoji every reaction picker offers, before custom ones.
pub const COMMON_REACTIONS: [&str; 40] = [
    "👍", "👎", "❤️", "😂", "😮", "😢", "😡", "🎉", "🔥", "👀", //
    "✅", "❌", "💯", "🙏", "👏", "🤔", "😍", "🥳", "😎", "🤣", //
    "😊", "😅", "🙃", "😭", "😬", "🤯", "🥺", "😴", "🤝", "💪", //
    "🚀", "⭐", "💡", "🐛", "📌", "👋", "🙌", "💀", "🫡", "➕",
];

/// One reaction under a message: an emoji and how many people used it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionChip {
    /// The emoji as the API names it: the Unicode emoji, or a custom emoji's ID.
    pub key: String,
    pub emoji: ReactionEmoji,
    pub count: usize,
    /// The logged-in user is one of them.
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReactionEmoji {
    Unicode(String),
    Custom {
        id: String,
        /// `None` until the file server is known.
        url: Option<String>,
        /// `:name:` for the tooltip, or `:emoji:` if it isn't one of the
        /// user's servers' emoji.
        name: String,
    },
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
    /// Messages the user sent that aren't confirmed yet, by channel, in send order.
    pending: HashMap<String, Vec<PendingMessage>>,
    /// Originals of replies that aren't in the loaded history, by message ID.
    reply_sources: HashMap<String, ReplySource>,
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
        self.confirm_pending_in_history(channel);
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
        self.confirm_pending_in_history(channel);
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
    /// Originals that couldn't be fetched are tried again.
    pub fn clear_histories(&mut self) {
        self.histories.clear();
        self.reply_sources
            .retain(|_, source| !matches!(source, ReplySource::Unavailable));
    }

    /// Originals that replies in `channel` (loaded or pending) refer to but
    /// that aren't loaded or cached, oldest reply first, each once.
    pub fn missing_reply_sources(&self, channel: &str) -> Vec<String> {
        let Some(history) = self.histories.get(channel) else {
            return Vec::new();
        };
        let replied = history
            .messages
            .iter()
            .filter_map(|m| m.replies.as_ref())
            .flat_map(|ids| ids.iter().take(MAX_REPLIES))
            .chain(
                self.pending(channel)
                    .iter()
                    .flat_map(|p| p.replies.iter().map(|r| &r.id)),
            );
        let mut seen = HashSet::new();
        replied
            .filter(|id| {
                self.find_message(channel, id).is_none()
                    && !self.reply_sources.contains_key(*id)
                    && seen.insert(id.as_str())
            })
            .cloned()
            .collect()
    }

    /// Originals fetched for `wanted` replies in `channel` (with whatever
    /// else the page brought, and its authors). Any of `wanted` not in it
    /// has been deleted.
    pub fn add_reply_sources(&mut self, page: BulkMessageResponse, wanted: &[String]) {
        for message in self.absorb_page(page) {
            self.reply_sources
                .insert(message.id.clone(), ReplySource::Found(Box::new(message)));
        }
        for id in wanted {
            self.reply_sources
                .entry(id.clone())
                .or_insert(ReplySource::Deleted);
        }
    }

    /// Fetching the original `id` failed: show that, not "Loading…" forever.
    pub fn reply_source_unavailable(&mut self, id: &str) {
        self.reply_sources
            .entry(id.to_string())
            .or_insert(ReplySource::Unavailable);
    }

    /// Show an edit of a loaded message at once, marked edited, before
    /// Stoat confirms it. Returns what it replaced, or `None` if the
    /// message isn't loaded.
    pub fn apply_local_edit(&mut self, channel: &str, id: &str, content: &str) -> Option<LocalEdit> {
        let message = self.message_mut(channel, id)?;
        let edit = LocalEdit {
            previous: Box::new(message.clone()),
            content: content.to_string(),
        };
        message.content = Some(content.to_string());
        // The echo brings Stoat's own time; until then, now.
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        message.edited = serde_json::from_value(serde_json::Value::String(now)).ok();
        Some(edit)
    }

    /// Stoat refused an edit: put the old text back, unless the message has
    /// changed again since (another edit, from here or elsewhere). Returns
    /// true if it was put back.
    pub fn undo_local_edit(&mut self, channel: &str, id: &str, edit: LocalEdit) -> bool {
        let Some(message) = self.message_mut(channel, id) else {
            return false;
        };
        if message.content.as_deref() != Some(edit.content.as_str()) {
            return false;
        }
        message.content = edit.previous.content;
        message.edited = edit.previous.edited;
        true
    }

    /// Take a loaded message out at once, before Stoat confirms the delete.
    /// Replies to it say it was deleted. Returns it, to put back with
    /// [`Self::restore_local`] if Stoat refuses.
    pub fn remove_local(&mut self, channel: &str, id: &str) -> Option<Message> {
        let messages = &mut self.histories.get_mut(channel)?.messages;
        let ix = messages.binary_search_by(|m| m.id.as_str().cmp(id)).ok()?;
        let message = messages.remove(ix);
        self.reply_sources
            .insert(id.to_string(), ReplySource::Deleted);
        Some(message)
    }

    /// Put a message taken out by [`Self::remove_local`] back where it was
    /// (IDs sort by time, so its place is exact). Returns false if the
    /// channel isn't loaded any more, or the message is already back.
    pub fn restore_local(&mut self, channel: &str, message: Message) -> bool {
        if matches!(self.reply_sources.get(&message.id), Some(ReplySource::Deleted)) {
            self.reply_sources.remove(&message.id);
        }
        let Some(history) = self.histories.get_mut(channel) else {
            return false;
        };
        match history
            .messages
            .binary_search_by(|m| m.id.as_str().cmp(&message.id))
        {
            Ok(_) => false,
            Err(ix) => {
                history.messages.insert(ix, message);
                true
            }
        }
    }

    /// One line of a loaded message as plain text (for the delete dialog).
    pub fn message_snippet(&self, channel: &str, id: &str) -> Option<String> {
        Some(self.snippet(self.find_message(channel, id)?))
    }

    /// The newest loaded message the user wrote (not a system message):
    /// what Up in an empty message box edits.
    pub fn last_own_message(&self, channel: &str) -> Option<&str> {
        self.histories
            .get(channel)?
            .messages
            .iter()
            .rev()
            .find(|m| m.author == self.me && m.system.is_none() && m.webhook.is_none())
            .map(|m| m.id.as_str())
    }

    /// A loaded message's text as typed (its Markdown source), or a pending
    /// one's (by its key).
    pub fn raw_content(&self, channel: &str, id: &str) -> Option<&str> {
        if let Some(pending) = self.pending(channel).iter().find(|p| p.key == id) {
            return Some(&pending.content);
        }
        self.find_message(channel, id)?.content.as_deref()
    }

    /// A pending message as the [`Message`] it will become, mentioning the
    /// authors of the originals its replies ping (so previews show "@").
    fn pending_message(&self, pending: &PendingMessage) -> Message {
        let mut message = pending_as_message(pending, &self.me);
        let mentions: Vec<String> = pending
            .replies
            .iter()
            .filter(|r| r.mention)
            .filter_map(|r| self.original_author(&pending.channel, &r.id))
            .collect();
        message.mentions = (!mentions.is_empty()).then_some(mentions);
        message
    }

    /// Who wrote the original `id`, if it's loaded or cached.
    fn original_author(&self, channel: &str, id: &str) -> Option<String> {
        match self.find_message(channel, id) {
            Some(message) => Some(message.author.clone()),
            None => match self.reply_sources.get(id) {
                Some(ReplySource::Found(message)) => Some(message.author.clone()),
                _ => None,
            },
        }
    }

    /// A loaded message by ID.
    fn find_message(&self, channel: &str, id: &str) -> Option<&Message> {
        let messages = &self.histories.get(channel)?.messages;
        let ix = messages
            .binary_search_by(|m| m.id.as_str().cmp(id))
            .ok()?;
        Some(&messages[ix])
    }

    /// The preview of `id` for `reply`: from the loaded history, else the cache.
    fn reply_preview(&self, reply: &Message, id: &str) -> ReplyPreview {
        let original = match self.find_message(&reply.channel, id) {
            Some(message) => Ok(message),
            None => match self.reply_sources.get(id) {
                Some(ReplySource::Found(message)) => Ok(&**message),
                Some(ReplySource::Deleted) => Err(ReplyState::Deleted),
                Some(ReplySource::Unavailable) => Err(ReplyState::Unavailable),
                None => Err(ReplyState::Loading),
            },
        };
        match original {
            Ok(original) => ReplyPreview {
                id: id.to_string(),
                author: self.author_name(original),
                avatar: self.avatar_url(original),
                snippet: self.snippet(original),
                mentions_author: reply
                    .mentions
                    .as_ref()
                    .is_some_and(|m| m.contains(&original.author)),
                state: ReplyState::Loaded,
            },
            Err(state) => ReplyPreview {
                id: id.to_string(),
                author: String::new(),
                avatar: None,
                snippet: String::new(),
                mentions_author: false,
                state,
            },
        }
    }

    /// One line of a message as plain text, for a reply preview.
    fn snippet(&self, message: &Message) -> String {
        if let Some(system) = &message.system {
            return self.system_text(system);
        }
        let text = message
            .content
            .as_deref()
            .map(|c| crate::markdown::snippet(c, self, SNIPPET_CHARS))
            .unwrap_or_default();
        if !text.is_empty() {
            return text;
        }
        match message.attachments.as_deref().and_then(<[File]>::first) {
            Some(file) if file.content_type.starts_with("image/") => "[image]".into(),
            Some(_) => "[file]".into(),
            None if message.embeds.as_ref().is_some_and(|e| !e.is_empty()) => "[embed]".into(),
            None => String::new(),
        }
    }

    /// A live message. It's added to its channel's history if that's loaded,
    /// and keeps the channel's "last message" fresh for Home ordering either
    /// way. Returns true if a loaded history changed.
    pub fn apply_message(&mut self, message: Message) -> bool {
        // Ours, confirming a pending message (the POST response or its echo,
        // whichever arrives first; the second just replaces it by ID).
        self.confirm_pending(&message);
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

    /// Drop the pending message `message` confirms: the user's own, with the
    /// same nonce. Returns true if there was one.
    fn confirm_pending(&mut self, message: &Message) -> bool {
        if message.author != self.me {
            return false;
        }
        let Some(nonce) = message.nonce.as_deref() else {
            return false;
        };
        let Some(pending) = self.pending.get_mut(&message.channel) else {
            return false;
        };
        let before = pending.len();
        pending.retain(|p| p.nonce != nonce);
        pending.len() != before
    }

    /// Drop pending messages that a freshly loaded page already has (sent
    /// while the events connection was down, so their echo never came).
    fn confirm_pending_in_history(&mut self, channel: &str) {
        let (Some(history), Some(pending)) =
            (self.histories.get(channel), self.pending.get_mut(channel))
        else {
            return;
        };
        let me = &self.me;
        pending.retain(|p| {
            !history
                .messages
                .iter()
                .any(|m| m.author == *me && m.nonce.as_deref() == Some(p.nonce.as_str()))
        });
    }

    /// A message the user just sent, shown until Stoat confirms it.
    pub fn add_pending(&mut self, message: PendingMessage) {
        self.pending
            .entry(message.channel.clone())
            .or_default()
            .push(message);
    }

    /// The channel's unconfirmed messages, in send order.
    pub fn pending(&self, channel: &str) -> &[PendingMessage] {
        self.pending.get(channel).map_or(&[], Vec::as_slice)
    }

    fn pending_mut(&mut self, channel: &str, key: &str) -> Option<&mut PendingMessage> {
        self.pending
            .get_mut(channel)?
            .iter_mut()
            .find(|p| p.key == key)
    }

    /// Sending failed, for `reason`. `answered`: Stoat replied (with an
    /// error), so it has seen the nonce; if not (no connection), it may or
    /// may not have. Returns false if there's no such pending message.
    pub fn fail_pending(&mut self, channel: &str, key: &str, reason: String, answered: bool) -> bool {
        let Some(pending) = self.pending_mut(channel, key) else {
            return false;
        };
        pending.state = PendingState::Failed(reason);
        pending.nonce_used = answered;
        // An upload that was cut off starts over.
        for file in &mut pending.attachments {
            if matches!(file.upload, UploadState::Uploading { .. }) {
                file.upload = UploadState::Waiting;
            }
        }
        true
    }

    /// Send a failed message again: it's Sending again, and returned for
    /// sending. It keeps its nonce unless Stoat answered the last attempt
    /// (nothing was created, but the nonce is spent): then it gets
    /// `fresh_nonce`. Keeping it otherwise means a send that did get through
    /// can't be duplicated.
    pub fn retry_pending(
        &mut self,
        channel: &str,
        key: &str,
        fresh_nonce: impl FnOnce() -> String,
    ) -> Option<PendingMessage> {
        let pending = self.pending_mut(channel, key)?;
        if !matches!(pending.state, PendingState::Failed(_)) {
            return None;
        }
        if pending.nonce_used {
            pending.nonce = fresh_nonce();
            pending.nonce_used = false;
            // Stoat may have claimed the files for the refused message (it
            // does that after spending the key): a file can only be used
            // once, so they're uploaded again.
            for file in &mut pending.attachments {
                file.upload = UploadState::Waiting;
            }
        }
        pending.state = PendingState::Sending;
        Some(pending.clone())
    }

    /// Forget an unconfirmed message. Returns false if there's no such one.
    /// `sent` of `total` bytes of the pending message `key`'s file
    /// `local_id` have gone. Returns false if there's no such file, or it's
    /// already uploaded.
    pub fn set_upload_progress(
        &mut self,
        channel: &str,
        key: &str,
        local_id: u64,
        sent: u64,
        total: u64,
    ) -> bool {
        let Some(file) = self.pending_file_mut(channel, key, local_id) else {
            return false;
        };
        if matches!(file.upload, UploadState::Uploaded(_)) {
            return false;
        }
        file.upload = UploadState::Uploading { sent, total };
        true
    }

    /// The pending message `key`'s file `local_id` is uploaded, as `id`.
    pub fn set_uploaded(&mut self, channel: &str, key: &str, local_id: u64, id: String) -> bool {
        let Some(file) = self.pending_file_mut(channel, key, local_id) else {
            return false;
        };
        file.upload = UploadState::Uploaded(id);
        true
    }

    fn pending_file_mut(
        &mut self,
        channel: &str,
        key: &str,
        local_id: u64,
    ) -> Option<&mut PendingAttachment> {
        self.pending_mut(channel, key)?
            .attachments
            .iter_mut()
            .find(|a| a.file.local_id == local_id)
    }

    pub fn discard_pending(&mut self, channel: &str, key: &str) -> bool {
        let Some(pending) = self.pending.get_mut(channel) else {
            return false;
        };
        let before = pending.len();
        pending.retain(|p| p.key != key);
        pending.len() != before
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
        let loaded = self
            .histories
            .get_mut(channel)
            .and_then(|h| h.messages.iter_mut().find(|m| m.id == id));
        // An original that's only cached for a reply preview: keep it fresh.
        let cached = match self.reply_sources.get_mut(id) {
            Some(ReplySource::Found(message)) => Some(&mut **message),
            _ => None,
        };
        let Some(message) = loaded.or(cached) else {
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
        let Some(message) = self.message_mut(channel, id) else {
            return false;
        };
        message.embeds.get_or_insert_default().extend(embeds);
        true
    }

    /// Someone added (`add`) or took back their `emoji` reaction to a loaded
    /// message. Returns true if that changed anything: repeats (our own
    /// reaction echoed back, say) are no-ops. A reaction nobody's left on is
    /// removed.
    pub fn apply_react(
        &mut self,
        channel: &str,
        id: &str,
        user: &str,
        emoji: &str,
        add: bool,
    ) -> bool {
        let Some(message) = self.message_mut(channel, id) else {
            return false;
        };
        if add {
            return message
                .reactions
                .entry(emoji.to_string())
                .or_default()
                .insert(user.to_string());
        }
        let Some(users) = message.reactions.get_mut(emoji) else {
            return false;
        };
        let removed = users.shift_remove(user);
        if users.is_empty() {
            message.reactions.shift_remove(emoji);
        }
        removed
    }

    /// Every `emoji` reaction cleared from a loaded message (by a moderator).
    pub fn apply_remove_reaction(&mut self, channel: &str, id: &str, emoji: &str) -> bool {
        self.message_mut(channel, id)
            .is_some_and(|message| message.reactions.shift_remove(emoji).is_some())
    }

    fn message_mut(&mut self, channel: &str, id: &str) -> Option<&mut Message> {
        self.histories
            .get_mut(channel)?
            .messages
            .iter_mut()
            .find(|m| m.id == id)
    }

    /// The chips for a message's reactions, in order.
    fn reaction_chips(&self, message: &Message) -> Vec<ReactionChip> {
        message
            .reactions
            .iter()
            .filter(|(_, users)| !users.is_empty())
            .map(|(key, users)| ReactionChip {
                key: key.clone(),
                emoji: self.reaction_emoji(key),
                count: users.len(),
                mine: users.contains(&self.me),
            })
            .collect()
    }

    /// How a reaction key is drawn: custom emoji are told apart by being a ULID.
    fn reaction_emoji(&self, key: &str) -> ReactionEmoji {
        if crate::time::is_ulid(key) {
            let name = self.emoji(key).map_or("emoji", |e| e.name.as_str());
            ReactionEmoji::Custom {
                id: key.to_string(),
                url: self.media.emoji(key),
                name: format!(":{name}:"),
            }
        } else {
            ReactionEmoji::Unicode(key.to_string())
        }
    }

    /// What the reaction picker offers in `channel`: the common Unicode emoji,
    /// then custom emoji by name. Those are the channel's server's in a
    /// server, or all of the user's servers' in DMs and groups.
    pub fn reaction_choices(&self, channel: &str) -> Vec<PickerEmoji> {
        let server = self.server_of(channel);
        let mut custom: Vec<&Emoji> = self
            .emojis
            .values()
            .filter(|emoji| match (&emoji.parent, server) {
                (EmojiParent::Server { id }, Some(server)) => id == server,
                (EmojiParent::Server { .. }, None) => true,
                (EmojiParent::Detached, _) => false,
            })
            .collect();
        custom.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        COMMON_REACTIONS
            .iter()
            .map(|key| key.to_string())
            .chain(custom.into_iter().map(|emoji| emoji.id.clone()))
            .map(|key| PickerEmoji {
                emoji: self.reaction_emoji(&key),
                key,
            })
            .collect()
    }

    /// The emoji a message's author limited reactions to, if they did.
    fn allowed_reactions(&self, message: &Message) -> Option<Vec<PickerEmoji>> {
        let interactions = &message.interactions;
        if !interactions.restrict_reactions {
            return None;
        }
        let allowed = interactions.reactions.as_ref()?;
        Some(
            allowed
                .iter()
                .map(|key| PickerEmoji {
                    key: key.clone(),
                    emoji: self.reaction_emoji(key),
                })
                .collect(),
        )
    }

    /// Whether adding `emoji` to a loaded message would be a new kind of
    /// reaction past [`MAX_REACTIONS`].
    pub fn would_exceed_reactions(&self, channel: &str, id: &str, emoji: &str) -> bool {
        self.histories
            .get(channel)
            .and_then(|h| h.messages.iter().find(|m| m.id == id))
            .is_some_and(|m| !m.reactions.contains_key(emoji) && m.reactions.len() >= MAX_REACTIONS)
    }

    /// Whether the logged-in user has reacted to a loaded message with `emoji`.
    pub fn has_reacted(&self, channel: &str, id: &str, emoji: &str) -> bool {
        self.histories
            .get(channel)
            .and_then(|h| h.messages.iter().find(|m| m.id == id))
            .and_then(|m| m.reactions.get(emoji))
            .is_some_and(|users| users.contains(&self.me))
    }

    /// Remove messages from a loaded channel. Returns true if any were removed.
    pub fn apply_delete(&mut self, channel: &str, ids: &[String]) -> bool {
        // Replies to them say so, without fetching to find out.
        let mut cached = false;
        for id in ids {
            let old = self.reply_sources.insert(id.clone(), ReplySource::Deleted);
            cached |= matches!(old, Some(ReplySource::Found(_)));
        }
        let Some(history) = self.histories.get_mut(channel) else {
            return false;
        };
        let before = history.messages.len();
        history.messages.retain(|m| !ids.contains(&m.id));
        cached || history.messages.len() != before
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

        // Unconfirmed messages go last, in send order, drawn like any other.
        let pending: Vec<(Message, &PendingMessage)> = self
            .pending(channel)
            .iter()
            .map(|p| (self.pending_message(p), p))
            .collect();
        let loaded = history
            .messages
            .iter()
            .map(|m| (m, None, crate::time::ulid_millis(&m.id)));
        let unconfirmed = pending
            .iter()
            .map(|(m, p)| (m, Some(*p), Some(p.sent_ms)));

        // The previous row's author ID and name, time, and whether it was a
        // system message.
        let mut previous: Option<(&str, String, Option<u64>, bool)> = None;
        for (message, pending, millis) in loaded.chain(unconfirmed) {
            let author = self.author_name(message);
            let system = message.system.is_some();
            // A reply starts a group, so its preview sits under a header.
            let replies = message.replies.as_ref().is_some_and(|r| !r.is_empty());
            let continuation = !system
                && !replies
                && previous
                    .as_ref()
                    .is_some_and(|(prev_author, prev_name, prev_millis, prev_system)| {
                        !prev_system
                            && *prev_author == message.author
                            && *prev_name == author
                            && millis.zip(*prev_millis).is_some_and(|(at, prev)| {
                                at.saturating_sub(prev) / 60_000 < GROUP_WINDOW_MINUTES
                            })
                    });
            let mut row = self.message_row(message, author.clone(), continuation);
            if let Some(pending) = pending {
                row.key = pending.key.clone();
                row.time = crate::time::millis_time_label(pending.sent_ms);
                row.pending = Some(pending.state.clone());
                row.attachments = pending.attachments.iter().map(PendingAttachment::view).collect();
            }
            rows.push(TimelineRow::Message(row));
            previous = Some((&message.author, author, millis, system));
        }
        mark_group_ends(&mut rows);
        rows
    }

    /// A message's row, by `author` (its [`Self::author_name`]).
    /// `group_end` is fixed up once the next row is known.
    fn message_row(&self, message: &Message, author: String, continuation: bool) -> MessageRow {
        let system = message.system.is_some();
        let mine = message.author == self.me;
        let body = match &message.system {
            Some(system) => Some(self.system_text(system)),
            None => message
                .content
                .as_deref()
                .filter(|c| !c.is_empty())
                .map(|c| crate::markdown::prepare(c, self)),
        };
        MessageRow {
            id: message.id.clone(),
            key: match &message.nonce {
                Some(nonce) if mine => nonce.clone(),
                _ => message.id.clone(),
            },
            render_key: render_key(&message.id, body.as_deref()),
            avatar: self.avatar_url(message),
            author,
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
            group_end: true,
            mine,
            jumbo: !system
                && message
                    .content
                    .as_deref()
                    .is_some_and(crate::markdown::is_jumbo),
            reactions: self.reaction_chips(message),
            allowed_reactions: self.allowed_reactions(message),
            pending: None,
            replies: message
                .replies
                .iter()
                .flatten()
                .take(MAX_REPLIES)
                .map(|id| self.reply_preview(message, id))
                .collect(),
        }
    }

    fn user_name(&self, id: &str) -> Option<String> {
        self.users.get(id).map(|u| display_name(u).to_string())
    }

    /// What `user` is called in `channel`: their nickname in its server, else
    /// their display name (for "Ada is typing…").
    pub fn name_in(&self, channel: &str, user: &str) -> String {
        self.server_of(channel)
            .and_then(|server| self.member(server, user))
            .and_then(|m| m.nickname.clone())
            .filter(|n| !n.trim().is_empty())
            .or_else(|| self.user_name(user))
            .unwrap_or_else(|| "Someone".into())
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
        TimelineRow::Message(message) => &message.key,
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

/// A pending message as the [`Message`] it will become, so it's drawn the
/// same way. It has no ID yet: its `key` stands in.
fn pending_as_message(pending: &PendingMessage, me: &str) -> Message {
    Message {
        id: pending.key.clone(),
        nonce: Some(pending.nonce.clone()),
        channel: pending.channel.clone(),
        author: me.to_string(),
        user: None,
        member: None,
        webhook: None,
        content: Some(pending.content.clone()),
        system: None,
        attachments: None,
        edited: None,
        embeds: None,
        mentions: None,
        role_mentions: None,
        replies: (!pending.replies.is_empty())
            .then(|| pending.replies.iter().map(|r| r.id.clone()).collect()),
        reactions: Default::default(),
        interactions: Default::default(),
        masquerade: None,
        pinned: None,
        flags: 0,
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
