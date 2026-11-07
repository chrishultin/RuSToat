//! The client's in-memory picture of the account, built from the events
//! connection. Plain data with no GPUI, so ordering and naming rules are
//! unit-testable.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::api::events::Ready;
use crate::api::models::{
    AllMemberResponse, BulkMessageResponse, Channel, ChannelSlowmode, Embed, Emoji, EmojiParent, FieldsChannel, FieldsMember,
    FieldsMessage, FieldsRole, FieldsServer, FieldsUser, File, Member, MemberCompositeKey, Message,
    MessageFlags, PartialChannel, PartialMember, PartialMessage, PartialRole, PartialServer, PartialUser,
    Presence, ReplyIntent, Role, Server, SystemMessage, User, UserStatus,
};
use crate::attachment::AttachmentView;
use crate::complete::{Candidate, RoleCandidate, TokenRef};
use crate::emoji::CustomEmoji;
use crate::draft::DraftAttachment;
use crate::embed::EmbedView;
use crate::media::MediaUrls;
use crate::notify::{self, NotificationSettings};
use crate::slowmode::Cooldowns;

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
    /// Muted now, by its own mute or its server's. Drawn dimmed.
    pub muted: bool,
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
    /// The category's ID (`None` = uncategorized).
    pub id: Option<String>,
    pub channels: Vec<ChannelEntry>,
}

/// `sections` as the channel list shows them: a folded category (its ID in
/// `collapsed`) keeps only the `open` channel and channels with unread
/// mentions, so a ping is never hidden. Its heading stays, to unfold it.
pub fn fold_sections(
    mut sections: Vec<ChannelSection>,
    collapsed: &HashSet<String>,
    open: Option<&str>,
) -> Vec<ChannelSection> {
    for section in &mut sections {
        if section.id.as_ref().is_some_and(|id| collapsed.contains(id)) {
            section
                .channels
                .retain(|c| Some(c.id.as_str()) == open || c.mentions > 0);
        }
    }
    sections
}

/// A channel's loaded messages, oldest first.
#[derive(Debug, Default)]
pub struct History {
    pub messages: Vec<Message>,
    /// The server returned fewer than requested: there's nothing older.
    pub exhausted: bool,
    /// The newest loaded message is the channel's newest. False in a window
    /// around an old message (see [`ChatStore::set_window`]) until newer
    /// pages reach the present.
    pub at_present: bool,
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
    /// "New messages": where the user left off (see [`with_unread_divider`]).
    NewMessages,
    Message(MessageRow),
    /// A run of consecutive messages from users the user blocked, hidden
    /// behind "N blocked messages" until clicked (see
    /// [`ChatStore::reveal_blocked`]). Their IDs, oldest first.
    Blocked {
        ids: Vec<String>,
    },
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
    /// Whose profile card the author's name and avatar open: the real
    /// author, even under a masquerade. `None` for system messages and
    /// webhooks, which aren't people.
    pub author_id: Option<String>,
    /// Author avatar URL. `None` for system messages, or when there's no
    /// image to show (the UI draws the author's initial instead).
    pub avatar: Option<String>,
    /// "Today at 14:05" etc.
    pub time: String,
    /// Markdown (see [`crate::markdown::prepare`]). For system messages, the
    /// description with each person's name a `stoat:user/` link (see
    /// [`ChatStore::system_markdown`]).
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
    /// Just jumped to: drawn tinted for a moment (see [`ChatStore::set_highlight`]).
    pub highlighted: bool,
    /// Pinned to the channel.
    pub pinned: bool,
    /// The user may pin or unpin it (see [`ChatStore::can_pin`]): the
    /// right-click menu offers it. Always false in search results and pins.
    pub can_pin: bool,
}

/// Most messages one message can reply to (Stoat's `limits.global.message_replies`).
pub const MAX_REPLIES: usize = 5;

/// Longest reply-preview snippet, in characters.
pub const SNIPPET_CHARS: usize = 100;

/// Who a reply preview says wrote the original, when it's a blocked user.
pub const BLOCKED_AUTHOR: &str = "Blocked user";

/// The "↱ Name: snippet" line above a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyPreview {
    /// The original message's ID.
    pub id: String,
    /// The original's author, as [`ChatStore::author_name`] names them.
    /// Empty unless `state` is `Loaded`.
    pub author: String,
    /// Whose card the author opens: `None` unless `Loaded`, and for
    /// webhooks and system messages.
    pub author_id: Option<String>,
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

/// Servers with more members than this list only the members who are
/// online: that's all the web client fetches for them, too.
pub const LARGE_SERVER_MEMBERS: usize = 1000;

/// How a person shows in the member list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceKind {
    Online,
    Idle,
    Focus,
    Busy,
    /// Offline, or online but invisible.
    Offline,
}

impl PresenceKind {
    /// The status line of someone with no custom status text. Empty for
    /// offline people: their section already says so.
    pub fn label(self) -> &'static str {
        match self {
            PresenceKind::Online => "Online",
            PresenceKind::Idle => "Idle",
            PresenceKind::Focus => "Focus",
            PresenceKind::Busy => "Busy",
            PresenceKind::Offline => "",
        }
    }
}

/// One row of the right-hand member list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberListRow {
    /// A section title: a hoisted role, "Online" or "Offline".
    Header { name: String, count: usize },
    Member {
        user_id: String,
        /// Nickname in the server, else display name, else username.
        name: String,
        avatar_url: String,
        /// The colour of the member's highest-ranked role that has one, as
        /// `#rrggbb`. `None` if they have none, or it's not a solid colour.
        colour: Option<String>,
        presence: PresenceKind,
        /// The custom status, else the presence's name.
        status: String,
        online: bool,
    },
}

/// What the right-hand panel lists for a channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberList {
    /// "Members" in a server, "N members" in a group.
    pub title: String,
    pub rows: Vec<MemberListRow>,
    /// A large server: only online members are listed (see [`LARGE_SERVER_MEMBERS`]).
    pub offline_hidden: bool,
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
    /// Custom emoji of other servers, fetched by ID when a reaction or
    /// message uses one, so they can be named. Not offered for use.
    other_emojis: HashMap<String, Emoji>,
    /// Large images the user chose to load ("Show image"), by file ID.
    revealed_images: HashSet<String>,
    /// Spoilers the user clicked to see, as (message ID, spoiler index).
    revealed_spoilers: HashSet<(String, usize)>,
    /// Messages from blocked users the user clicked to see, by ID.
    revealed_blocked: HashSet<String>,
    /// Read positions by channel ID.
    read_states: HashMap<String, ReadState>,
    /// The user's notification levels and mutes.
    notifications: NotificationSettings,
    /// When the copy of `notifications` held was written (ms since the
    /// epoch), so an older copy arriving late can't undo a newer one.
    notifications_updated: i64,
    /// Messages the user sent that aren't confirmed yet, by channel, in send order.
    pending: HashMap<String, Vec<PendingMessage>>,
    /// Temporary files (pasted images) of pending messages that are gone
    /// (confirmed, discarded, or their server left), waiting to be deleted.
    released_files: Vec<DraftAttachment>,
    /// Originals of replies that aren't in the loaded history, by message ID.
    reply_sources: HashMap<String, ReplySource>,
    /// The message just jumped to, whose row is highlighted.
    highlight: Option<String>,
    /// The user's slowmode cooldowns. Kept across a new `Ready`: Stoat sends
    /// the running ones again after it.
    cooldowns: Cooldowns,
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
        (self.notifications_updated, self.notifications) = ready.notifications.unwrap_or_default();
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
                at_present: true,
            },
        );
        self.confirm_pending_in_history(channel);
    }

    /// Replace `channel`'s history with a window around `target` (fetched
    /// with `nearby`, asking for `limit` messages): the target and up to
    /// `limit / 2 + 1` messages on each side. A side that came back short
    /// has nothing further: nothing older (`exhausted`), or nothing newer
    /// (`at_present`). The window is also at the present if it holds the
    /// channel's newest message.
    pub fn set_window(
        &mut self,
        channel: &str,
        page: BulkMessageResponse,
        target: &str,
        limit: usize,
    ) {
        let mut messages = self.absorb_page(page);
        sort_messages(&mut messages);
        let side = limit / 2 + 1;
        let older = messages.iter().filter(|m| m.id.as_str() < target).count();
        let newer = messages.len() - older;
        let at_present = newer < side || self.contains_last_message(channel, &messages);
        self.histories.insert(
            channel.to_string(),
            History {
                messages,
                exhausted: older < side,
                at_present,
            },
        );
        self.confirm_pending_in_history(channel);
    }

    /// Merge a page of newer messages (fetched with `after` the newest
    /// loaded one, `limit` asked for) into a loaded channel. A short page, or
    /// one that reaches the channel's newest message, means the history is
    /// at the present again. Returns whether any message was added; false
    /// with the history still away from the present means the page made no
    /// progress, and asking again would only repeat it.
    pub fn append_page(&mut self, channel: &str, page: BulkMessageResponse, limit: usize) -> bool {
        let newer = self.absorb_page(page);
        let reaches_present = self.contains_last_message(channel, &newer);
        let Some(history) = self.histories.get_mut(channel) else {
            return false;
        };
        let before = history.messages.len();
        let short = newer.len() < limit;
        history.messages.extend(newer);
        sort_messages(&mut history.messages);
        if short || reaches_present {
            history.at_present = true;
        }
        let added = history.messages.len() > before;
        self.confirm_pending_in_history(channel);
        added
    }

    /// Whether `messages` include `channel`'s newest message.
    fn contains_last_message(&self, channel: &str, messages: &[Message]) -> bool {
        self.channels
            .get(channel)
            .and_then(last_message_id)
            .is_some_and(|last| messages.iter().any(|m| m.id == last))
    }

    /// Whether the newest loaded message in `channel` is its newest message.
    /// True for a channel that isn't loaded: there's no gap to speak of.
    pub fn is_at_present(&self, channel: &str) -> bool {
        self.histories.get(channel).is_none_or(|h| h.at_present)
    }

    /// The newest loaded message, for fetching the page after it.
    pub fn newest_message_id(&self, channel: &str) -> Option<&str> {
        self.histories
            .get(channel)?
            .messages
            .last()
            .map(|m| m.id.as_str())
    }

    /// Whether `id` is loaded in `channel`.
    pub fn has_message(&self, channel: &str, id: &str) -> bool {
        self.find_message(channel, id).is_some()
    }

    /// Draw `id`'s row highlighted (a jump just landed on it), and no other.
    pub fn set_highlight(&mut self, id: &str) {
        self.highlight = Some(id.to_string());
    }

    /// Stop highlighting `id`, unless a later jump has moved it elsewhere.
    pub fn clear_highlight(&mut self, id: &str) -> bool {
        if self.highlight.as_deref() == Some(id) {
            self.highlight = None;
            return true;
        }
        false
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

    /// Take a search's results: cache the authors that came with them (so
    /// [`Self::result_rows`] can name them) and return the messages, in the
    /// order Stoat sent them. The channel's history is not touched: results
    /// are scattered through it, and merging them in would leave gaps.
    pub fn absorb_results(&mut self, page: BulkMessageResponse) -> Vec<Message> {
        self.absorb_page(page)
    }

    /// Rows for search results in `channel`, in the order given. Every row
    /// carries its author and time (none continues the one before it, since
    /// the results aren't neighbours), and none is highlighted or shows reply
    /// previews: results are for reading and clicking, not a timeline.
    /// Messages from other channels are left out.
    pub fn result_rows(&self, channel: &str, messages: &[Message]) -> Vec<MessageRow> {
        messages
            .iter()
            .filter(|m| m.channel == channel)
            .map(|message| {
                let author = self.author_name(message);
                let mut row = self.message_row(message, author, false);
                row.replies.clear();
                row.highlighted = false;
                row
            })
            .collect()
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
            // Not who it is, nor what they said (as Stoat's web client).
            Ok(original) if self.is_blocked(&original.author) => ReplyPreview {
                id: id.to_string(),
                author: BLOCKED_AUTHOR.into(),
                author_id: None,
                avatar: None,
                snippet: String::new(),
                mentions_author: false,
                state: ReplyState::Loaded,
            },
            Ok(original) => ReplyPreview {
                id: id.to_string(),
                author: self.author_name(original),
                author_id: person_id(original),
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
                author_id: None,
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

    /// A live message. It's added to its channel's history if that's loaded
    /// and at the present, and keeps the channel's "last message" (so unread
    /// state) fresh for Home ordering either way. Returns true if a loaded
    /// history changed.
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
            // Away from the present a new message would sit after a gap. It
            // shows when newer pages reach it (`append_page`).
            Err(_) if !history.at_present => return false,
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
        let (confirmed, kept): (Vec<_>, Vec<_>) =
            std::mem::take(pending).into_iter().partition(|p| p.nonce == nonce);
        *pending = kept;
        let found = !confirmed.is_empty();
        self.release_files(confirmed);
        found
    }

    /// Hand back the temporary files of pending messages that are gone.
    fn release_files(&mut self, gone: impl IntoIterator<Item = PendingMessage>) {
        self.released_files.extend(
            gone.into_iter()
                .flat_map(|p| p.attachments)
                .map(|a| a.file)
                .filter(|file| file.temporary),
        );
    }

    /// Temporary files (pasted images) no pending message needs any more,
    /// each handed out once: delete them. Messages leave the pending list
    /// when confirmed (by the POST response, the live echo or a history
    /// load), discarded, or dropped with their server.
    pub fn take_released_files(&mut self) -> Vec<DraftAttachment> {
        std::mem::take(&mut self.released_files)
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
        let (confirmed, kept): (Vec<_>, Vec<_>) =
            std::mem::take(pending).into_iter().partition(|p| {
                history
                    .messages
                    .iter()
                    .any(|m| m.author == *me && m.nonce.as_deref() == Some(p.nonce.as_str()))
            });
        *pending = kept;
        self.release_files(confirmed);
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

    /// Forget an unconfirmed message (the user chose Discard). Returns it,
    /// or `None` if there's no such one. Its temporary files are released
    /// ([`Self::take_released_files`]).
    pub fn discard_pending(&mut self, channel: &str, key: &str) -> Option<PendingMessage> {
        let pending = self.pending.get_mut(channel)?;
        let ix = pending.iter().position(|p| p.key == key)?;
        let discarded = pending.remove(ix);
        self.release_files([discarded.clone()]);
        Some(discarded)
    }

    /// The newest message the user has read in `channel`, if any.
    pub fn read_position(&self, channel: &str) -> Option<&str> {
        self.read_states.get(channel)?.last_id.as_deref()
    }

    /// Move `channel`'s read position to `message_id` (`None`: nothing read),
    /// even backwards: "Mark as unread". Mentions older than the new position
    /// aren't brought back; the store only keeps newer ones.
    pub fn set_read_position(&mut self, channel: &str, message_id: Option<&str>) {
        let state = self.read_states.entry(channel.to_string()).or_default();
        state.last_id = message_id.map(str::to_string);
    }

    /// The loaded message just before `id` in `channel`. `None` if `id` is
    /// the oldest loaded message, or isn't loaded.
    pub fn message_before(&self, channel: &str, id: &str) -> Option<&str> {
        let messages = &self.histories.get(channel)?.messages;
        let ix = messages.iter().position(|m| m.id == id)?;
        Some(messages.get(ix.checked_sub(1)?)?.id.as_str())
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

    /// When the notification settings held were written (ms since the
    /// epoch).
    pub fn notifications_updated(&self) -> i64 {
        self.notifications_updated
    }

    /// New notification settings, written at `updated` (ms since the
    /// epoch): from another client or session (`UserSettingsUpdate`), or a
    /// change made here. A copy older than the one held is ignored, so the
    /// echo of an earlier change can't undo a later one, and false is
    /// returned.
    ///
    /// Stoat clamps a timestamp from the future to its own clock. If this
    /// Mac's clock runs ahead, the echo of a change made here carries an
    /// earlier time than the change. Its content is the same, so the held
    /// time is moved back to it: changes from other clients, stamped by
    /// Stoat's clock, then still apply.
    pub fn set_notification_settings(
        &mut self,
        updated: i64,
        settings: NotificationSettings,
    ) -> bool {
        if updated < self.notifications_updated {
            let now = notify::now_ms();
            if settings.to_json(now) == self.notifications.to_json(now) {
                self.notifications_updated = updated;
            }
            return false;
        }
        self.notifications_updated = updated;
        self.notifications = settings;
        true
    }

    /// A channel's own mute, if it's muted now (as
    /// [`Self::server_mute_until`]). Its server's mute isn't included.
    pub fn channel_mute_until(&self, channel: &str) -> Option<Option<i64>> {
        self.notifications.channel_mute(channel, notify::now_ms())
    }

    /// The level a channel has without its own: its server's, or All for
    /// DMs, groups and Saved Notes.
    pub fn level_source(&self, channel: &str) -> notify::Level {
        match self.server_of(channel) {
            Some(server) => self.notifications.server_level(server),
            None => notify::Level::All,
        }
    }

    /// Put back notification settings held before a change Stoat didn't
    /// take, whatever their time.
    pub fn restore_notification_settings(&mut self, updated: i64, settings: NotificationSettings) {
        self.notifications_updated = updated;
        self.notifications = settings;
    }

    /// A server's mute, if it's muted now: `Some(Some(until))` (ms since the
    /// epoch), or `Some(None)` until the user unmutes it.
    pub fn server_mute_until(&self, server: &str) -> Option<Option<i64>> {
        self.notifications.server_mute(server, notify::now_ms())
    }

    /// The user's own presence (Online, Focus, Busy...), if known.
    pub fn my_presence(&self) -> Option<Presence> {
        self.users.get(&self.me)?.status.as_ref()?.presence.clone()
    }

    /// The user's own presence (Online if unset) and custom status text
    /// (trimmed; `None` if unset or blank).
    ///
    /// Stoat keeps Invisible out of the users it sends, even the user's
    /// own (`Ready`, and its answer to a change): it leaves the presence
    /// out and says `online: false`. So the user is Invisible if their
    /// presence says so (the live `UserUpdate` echo does), or if Stoat
    /// says they're offline, as the web client's SDK reads it.
    pub fn my_status(&self) -> (Presence, Option<String>) {
        let me = self.users.get(&self.me);
        let status = me.and_then(|u| u.status.as_ref());
        let presence = match status.and_then(|s| s.presence.clone()) {
            _ if me.is_some_and(|u| !u.online) => Presence::Invisible,
            presence => presence.unwrap_or(Presence::Online),
        };
        let text = status
            .and_then(|s| s.text.as_deref())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        (presence, text)
    }

    /// The user's own status and `online` as Stoat answered a change to
    /// them, so the panel needn't wait for the `UserUpdate` echo. Offline
    /// is how the answer says Invisible (see [`Self::my_status`]).
    pub fn set_my_status(&mut self, status: Option<UserStatus>, online: bool) {
        let me = self.me.clone();
        if let Some(user) = self.users.get_mut(&me) {
            user.status = status;
            user.online = online;
        }
    }

    /// The user's own account as Stoat answered a profile change (display
    /// name, avatar, pronouns), so nothing waits for the `UserUpdate` echo.
    /// Their status and `online` stay as they are: the answer says offline
    /// for Invisible (see [`Self::my_status`]), and it isn't a status change.
    pub fn apply_own_user(&mut self, user: User) {
        if user.id != self.me {
            return;
        }
        let mut user = user;
        if let Some(before) = self.users.get(&self.me) {
            user.status = before.status.clone();
            user.online = before.online;
            user.relationship = before.relationship.clone();
        }
        self.users.insert(user.id.clone(), user);
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
    /// roles in the server, with @everyone, or with @online while they
    /// appear online (not invisible).
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
        // Invisible counts as offline, as in the member list.
        let online = self
            .users
            .get(&self.me)
            .is_some_and(|u| presence_kind(u) != PresenceKind::Offline);
        online && has_flag(message.flags, MessageFlags::MentionsOnline)
    }

    /// The Dock badge count: unread mentions everywhere, plus each unread DM
    /// or group whose level is "all" (once, even if it also has mentions),
    /// plus each friend request waiting for the user. Muted channels don't
    /// count.
    pub fn attention_count(&self) -> usize {
        self.attention_breakdown().iter().map(|(_, n)| n).sum()
    }

    /// What [`Self::attention_count`] is made of: (channel label, count) for
    /// every channel that contributes, and "Friend requests" if there are
    /// any. For debugging the Dock badge.
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
        let requests = crate::friends::incoming_count(self);
        if requests > 0 {
            parts.push(("Friend requests".into(), requests));
        }
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

    /// Unread state for the Home button: open DMs and groups. Each friend
    /// request waiting for the user counts as a mention (with no unread dot).
    pub fn home_summary(&self) -> (bool, usize) {
        let requests = crate::friends::incoming_count(self);
        self.home_channels()
            .iter()
            .fold((false, requests), |(unread, mentions), c| {
                (unread || c.unread, mentions + c.mentions)
            })
    }

    /// The whole account, for its switcher button: whether anything shows an
    /// unread dot (Home or any server), and all unread mentions. Muted
    /// channels add mentions but no dot, as on the rail.
    pub fn account_summary(&self) -> (bool, usize) {
        self.servers()
            .iter()
            .fold(self.home_summary(), |(unread, mentions), server| {
                (unread || server.unread, mentions + server.mentions)
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

    /// Record that Stoat says message `id` is (or isn't) pinned, when a
    /// pin or unpin found it already so (`AlreadyPinned`, `NotPinned`) and the
    /// update that would have told us was missed. Returns true if a loaded
    /// message changed.
    pub fn set_pinned(&mut self, channel: &str, id: &str, pinned: bool) -> bool {
        let Some(message) = self.message_mut(channel, id) else {
            return false;
        };
        let changed = message.pinned.unwrap_or(false) != pinned;
        message.pinned = pinned.then_some(true);
        changed
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
        COMMON_REACTIONS
            .iter()
            .map(|key| key.to_string())
            .chain(self.server_emoji(channel).into_iter().map(|emoji| emoji.id.clone()))
            .map(|key| PickerEmoji {
                emoji: self.reaction_emoji(&key),
                key,
            })
            .collect()
    }

    /// The custom emoji usable in `channel`, by name: its server's, or in
    /// DMs and groups those of all the user's servers.
    fn server_emoji(&self, channel: &str) -> Vec<&Emoji> {
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
        custom
    }

    /// The custom emoji `:` and the emoji picker offer in `channel` (the
    /// same ones as the reaction picker).
    pub fn custom_emoji_for(&self, channel: &str) -> Vec<CustomEmoji> {
        self.server_emoji(channel)
            .into_iter()
            .map(|emoji| CustomEmoji {
                id: emoji.id.clone(),
                name: emoji.name.clone(),
                url: self.media.emoji(&emoji.id),
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
        self.other_emojis.remove(id);
    }

    /// Another server's emoji, fetched by ID (see [`ChatStore::missing_emojis`]).
    pub fn add_other_emoji(&mut self, emoji: Emoji) {
        self.other_emojis.insert(emoji.id.clone(), emoji);
    }

    /// A custom emoji by ID: the user's servers', or one fetched from another.
    pub fn emoji(&self, id: &str) -> Option<&Emoji> {
        self.emojis.get(id).or_else(|| self.other_emojis.get(id))
    }

    /// Custom emoji that loaded messages in `channel` use, in reactions or
    /// their text, but that the client doesn't know (another server's), each once.
    pub fn missing_emojis(&self, channel: &str) -> Vec<String> {
        let Some(history) = self.histories.get(channel) else {
            return Vec::new();
        };
        let used = history.messages.iter().flat_map(|m| {
            m.reactions
                .iter()
                .filter(|(key, users)| !users.is_empty() && crate::time::is_ulid(key))
                .map(|(key, _)| key.as_str())
                .chain(
                    m.content
                        .as_deref()
                        .map(crate::markdown::custom_emoji_ids)
                        .unwrap_or_default(),
                )
        });
        let mut seen = HashSet::new();
        used.filter(|id| self.emoji(id).is_none() && seen.insert(*id))
            .map(str::to_string)
            .collect()
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

    /// Show spoiler `index` (counting from 0) of `message` for the rest of
    /// the session. Its row's body changes from bars to the text.
    pub fn reveal_spoiler(&mut self, message: &str, index: usize) {
        self.revealed_spoilers.insert((message.to_string(), index));
    }

    /// Show these messages from blocked users (a [`TimelineRow::Blocked`]
    /// row's IDs) for the rest of the session.
    pub fn reveal_blocked(&mut self, ids: impl IntoIterator<Item = String>) {
        self.revealed_blocked.extend(ids);
    }

    /// If message `id` in `channel` is hidden in a run of blocked messages,
    /// show that run (to jump to it). Returns true if it was hidden.
    pub fn reveal_blocked_run_of(&mut self, channel: &str, id: &str) -> bool {
        let hidden = self
            .timeline(channel)
            .into_iter()
            .find_map(|row| match row {
                TimelineRow::Blocked { ids } if ids.iter().any(|i| i == id) => Some(ids),
                _ => None,
            });
        match hidden {
            Some(ids) => {
                self.reveal_blocked(ids);
                true
            }
            None => false,
        }
    }

    /// Whether the user blocked `user`. Not whether `user` blocked them
    /// (`BlockedOther`), which hides nothing.
    pub fn is_blocked(&self, user: &str) -> bool {
        self.users
            .get(user)
            .is_some_and(|u| u.relationship == crate::api::models::RelationshipStatus::Blocked)
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
        let member_avatar = self.author_member(message).and_then(|m| m.avatar.clone());
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
        let nickname = self.author_member(message).and_then(|m| m.nickname.clone());
        if let Some(nickname) = nickname
            && !nickname.trim().is_empty()
        {
            return nickname;
        }
        self.user_name(&message.author)
            .or_else(|| message.user.as_ref().map(|u| display_name(u).to_string()))
            .unwrap_or_else(|| "Unknown user".into())
    }

    /// The author's membership in the message's server: the cached one, which
    /// live `ServerMemberUpdate`s keep current, else the snapshot the message
    /// came with.
    fn author_member<'a>(&'a self, message: &'a Message) -> Option<&'a Member> {
        self.server_of(&message.channel)
            .and_then(|server| self.member(server, &message.author))
            .or(message.member.as_ref())
    }

    /// A one-line description of a system message, with user names filled in.
    /// Variants this client doesn't describe use the backend's own wording.
    pub fn system_text(&self, system: &SystemMessage) -> String {
        let name = |id: &str| self.user_name(id).unwrap_or_else(|| "Someone".into());
        describe_system(system, &name, &str::to_string)
    }

    /// [`Self::system_text`] as Markdown, for the system row: each known
    /// person's name is a `stoat:user/{id}` link (it opens their card), and
    /// everything else is escaped.
    pub fn system_markdown(&self, system: &SystemMessage) -> String {
        use crate::markdown::{INTERNAL_SCHEME, escape};
        let name = |id: &str| match self.user_name(id) {
            Some(name) => format!("[{}]({INTERNAL_SCHEME}user/{id})", escape(&name)),
            None => "Someone".into(),
        };
        describe_system(system, &name, &escape)
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

        // The same for every message in the channel.
        let can_pin = self.can_pin(channel);

        // Unconfirmed messages go last, in send order, drawn like any other.
        // Away from the present they'd sit after a gap, so they wait for the
        // present to be loaded again.
        let pending: Vec<(Message, &PendingMessage)> = self
            .pending(channel)
            .iter()
            .filter(|_| history.at_present)
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
            previous = Some((&message.author, author.clone(), millis, system));
            // From a blocked user and not clicked open: folded into the run.
            if pending.is_none()
                && !system
                && self.is_blocked(&message.author)
                && !self.revealed_blocked.contains(&message.id)
            {
                match rows.last_mut() {
                    Some(TimelineRow::Blocked { ids }) => ids.push(message.id.clone()),
                    _ => rows.push(TimelineRow::Blocked {
                        ids: vec![message.id.clone()],
                    }),
                }
                continue;
            }
            let mut row = self.message_row(message, author, continuation);
            row.can_pin = can_pin && pending.is_none() && !system;
            if let Some(pending) = pending {
                row.key = pending.key.clone();
                row.time = crate::time::millis_time_label(pending.sent_ms);
                row.pending = Some(pending.state.clone());
                row.attachments = pending.attachments.iter().map(PendingAttachment::view).collect();
            }
            rows.push(TimelineRow::Message(row));
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
            Some(system) => Some(self.system_markdown(system)),
            None => message
                .content
                .as_deref()
                .filter(|c| !c.is_empty())
                .map(|c| {
                    crate::markdown::prepare_with(c, self, |n| {
                        self.revealed_spoilers.contains(&(message.id.clone(), n))
                    })
                }),
        };
        MessageRow {
            id: message.id.clone(),
            key: match &message.nonce {
                Some(nonce) if mine => nonce.clone(),
                _ => message.id.clone(),
            },
            render_key: render_key(&message.id, body.as_deref()),
            avatar: self.avatar_url(message),
            author_id: person_id(message),
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
            highlighted: self.highlight.as_deref() == Some(message.id.as_str()),
            pinned: message.pinned.unwrap_or(false),
            can_pin: false,
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

    /// Who `@` suggests in `channel`, best first: people who wrote there
    /// recently (newest first), then the DM's or group's recipients, or the
    /// server's members known so far (by name). Never the user.
    pub fn mention_candidates(&self, channel: &str) -> Vec<Candidate> {
        let mut seen = HashSet::new();
        let mut people = Vec::new();
        let mut add = |id: &str, recent: bool, people: &mut Vec<Candidate>| {
            if id != self.me && seen.insert(id.to_string())
                && let Some(person) = self.candidate(channel, id, recent)
            {
                people.push(person);
            }
        };
        if let Some(history) = self.histories.get(channel) {
            for message in history.messages.iter().rev() {
                if message.system.is_none() && message.webhook.is_none() {
                    add(&message.author, true, &mut people);
                }
            }
        }
        match self.channels.get(channel) {
            Some(Channel::DirectMessage { recipients, .. } | Channel::Group { recipients, .. }) => {
                for id in recipients {
                    add(id, false, &mut people);
                }
            }
            Some(Channel::TextChannel { server, .. }) => {
                let mut members: Vec<Candidate> = self
                    .members
                    .keys()
                    .filter(|(s, _)| s == server)
                    .filter_map(|(_, user)| self.candidate(channel, user, false))
                    .collect();
                members.sort_by_cached_key(|p| p.name.to_lowercase());
                for member in members {
                    add(&member.id, false, &mut people);
                }
            }
            _ => {}
        }
        people
    }

    /// Whether `a` and `b` share a server (as far as the cached members
    /// show) or a group. The client only knows its own servers' members
    /// in full once they've been fetched, so this can miss a shared server.
    pub fn have_mutual_connection(&self, a: &str, b: &str) -> bool {
        let shared_server = self.members.keys().any(|(server, user)| {
            user == a && self.members.contains_key(&(server.clone(), b.to_string()))
        });
        shared_server
            || self.channels.values().any(|channel| match channel {
                Channel::Group { recipients, .. } => {
                    recipients.iter().any(|id| id == a) && recipients.iter().any(|id| id == b)
                }
                _ => false,
            })
    }

    /// Whether `user` is one of the people [`Self::member_list`] lists for
    /// `channel` (before leaving out those who can't see it).
    pub fn lists_member(&self, channel: &str, user: &str) -> bool {
        match self.channels.get(channel) {
            Some(Channel::TextChannel { server, .. }) => self.member(server, user).is_some(),
            Some(Channel::Group { recipients, .. }) => recipients.iter().any(|id| id == user),
            _ => false,
        }
    }

    /// Whether the user can pin and unpin messages in `channel`
    /// (see [`crate::permissions::can_pin`]).
    pub fn can_pin(&self, channel: &str) -> bool {
        crate::permissions::can_pin(self, &self.me, channel)
    }

    /// What a `UserSlowmodes` event says, received at `now_ms`: the user's
    /// cooldowns in the channels it lists (see [`Cooldowns::apply`]).
    pub fn apply_slowmodes(&mut self, slowmodes: &[ChannelSlowmode], now_ms: u64) {
        self.cooldowns.apply(slowmodes, now_ms);
    }

    /// Make the user wait `secs` seconds from `from_ms` before sending in
    /// `channel` again.
    pub fn start_cooldown(&mut self, channel: &str, secs: u64, from_ms: u64) {
        self.cooldowns.start(channel, secs, from_ms);
    }

    /// Whole seconds (rounded up) before the user may send in `channel`
    /// again, or `None` if they may now.
    pub fn cooldown_left(&self, channel: &str, now_ms: u64) -> Option<u64> {
        self.cooldowns.left(channel, now_ms)
    }

    /// Forget cooldowns that have ended by `now_ms`.
    pub fn prune_cooldowns(&mut self, now_ms: u64) {
        self.cooldowns.prune(now_ms);
    }

    /// When `channel`'s cooldown ends (milliseconds since the epoch), if
    /// it's still running at `now_ms`.
    pub fn cooldown_ends(&self, channel: &str, now_ms: u64) -> Option<u64> {
        self.cooldowns.ends(channel, now_ms)
    }

    /// Whether the user is exempt from `channel`'s slowmode
    /// (see [`crate::permissions::can_bypass_slowmode`]).
    pub fn can_bypass_slowmode(&self, channel: &str) -> bool {
        crate::permissions::can_bypass_slowmode(self, &self.me, channel)
    }

    /// Whether the user may send embeds (a GIF) in `channel`.
    pub fn can_send_embeds(&self, channel: &str) -> bool {
        crate::permissions::can_send_embeds(self, &self.me, channel)
    }

    /// Whether the user may mention roles in `channel`.
    pub fn can_mention_roles(&self, channel: &str) -> bool {
        crate::permissions::can_mention_roles(self, &self.me, channel)
    }

    /// Whether the user may mention `@everyone` and `@online` in `channel`.
    pub fn can_mention_everyone(&self, channel: &str) -> bool {
        crate::permissions::can_mention_everyone(self, &self.me, channel)
    }

    /// Whether `channel` is marked mature (NSFW): a server channel or group
    /// with `nsfw` set. A server's own `nsfw` doesn't count, as in Stoat's
    /// web client, and DMs and Saved Notes can't be mature.
    pub fn is_mature(&self, channel: &str) -> bool {
        matches!(
            self.channels.get(channel),
            Some(Channel::TextChannel { nsfw: true, .. } | Channel::Group { nsfw: true, .. })
        )
    }

    /// Whether the right-hand panel can list `channel`'s people: server
    /// channels and groups, not DMs or Saved Notes.
    pub fn has_member_list(&self, channel: &str) -> bool {
        matches!(
            self.channels.get(channel),
            Some(Channel::TextChannel { .. } | Channel::Group { .. })
        )
    }

    /// The right-hand panel's list for `channel`.
    ///
    /// In a server: each hoisted role in rank order, then Online, then
    /// Offline. An online member goes under the first hoisted role they
    /// hold, and every offline member under Offline. Members who can't see
    /// the channel are left out, and in a large server so are the offline
    /// ones. Each section is sorted by name.
    ///
    /// In a group: its recipients in one name-sorted list. Anything else
    /// (a DM, Saved Notes) has none.
    pub fn member_list(&self, channel: &str) -> MemberList {
        match self.channels.get(channel) {
            Some(Channel::TextChannel { server, .. }) => self.server_member_list(channel, server),
            Some(Channel::Group { recipients, .. }) => {
                let mut rows: Vec<(String, MemberListRow)> = recipients
                    .iter()
                    .filter_map(|id| self.users.get(id))
                    .map(|user| self.member_list_row(channel, None, user))
                    .collect();
                sort_member_rows(&mut rows);
                MemberList {
                    title: format!("{} members", recipients.len()),
                    rows: rows.into_iter().map(|(_, row)| row).collect(),
                    offline_hidden: false,
                }
            }
            _ => MemberList::default(),
        }
    }

    fn server_member_list(&self, channel: &str, server_id: &str) -> MemberList {
        let Some(server) = self.servers.get(server_id) else {
            return MemberList::default();
        };
        let offline_hidden = server.approximate_member_count > LARGE_SERVER_MEMBERS;
        // (ID, role): members refer to roles by the key they have in the map.
        let mut hoisted: Vec<_> = server.roles.iter().filter(|(_, role)| role.hoist).collect();
        hoisted.sort_by(|(a_id, a), (b_id, b)| a.rank.cmp(&b.rank).then_with(|| a_id.cmp(b_id)));

        let mut by_role: Vec<Vec<(String, MemberListRow)>> = vec![Vec::new(); hoisted.len()];
        let (mut online, mut offline) = (Vec::new(), Vec::new());
        for ((member_server, user_id), member) in &self.members {
            if member_server != server_id {
                continue;
            }
            let Some(user) = self.users.get(user_id) else {
                continue;
            };
            if !crate::permissions::can_view(self, user_id, channel) {
                continue;
            }
            let row = self.member_list_row(channel, Some((server_id, member)), user);
            if presence_kind(user) == PresenceKind::Offline {
                if !offline_hidden {
                    offline.push(row);
                }
            } else {
                match hoisted.iter().position(|(id, _)| member.roles.contains(id)) {
                    Some(ix) => by_role[ix].push(row),
                    None => online.push(row),
                }
            }
        }

        let mut sections: Vec<(String, Vec<(String, MemberListRow)>)> = hoisted
            .iter()
            .zip(by_role)
            .map(|((_, role), rows)| (role.name.clone(), rows))
            .collect();
        sections.push(("Online".into(), online));
        sections.push(("Offline".into(), offline));

        let mut rows = Vec::new();
        for (name, mut section) in sections {
            if section.is_empty() {
                continue;
            }
            sort_member_rows(&mut section);
            rows.push(MemberListRow::Header {
                name,
                count: section.len(),
            });
            rows.extend(section.into_iter().map(|(_, row)| row));
        }
        MemberList {
            title: "Members".into(),
            rows,
            offline_hidden,
        }
    }

    /// `user`'s row, with the lowercased name to sort it by. `membership`:
    /// the server and the user's membership of it, in a server.
    fn member_list_row(
        &self,
        channel: &str,
        membership: Option<(&str, &Member)>,
        user: &User,
    ) -> (String, MemberListRow) {
        let presence = presence_kind(user);
        let custom = user
            .status
            .as_ref()
            .and_then(|status| status.text.as_deref())
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let avatar_url = membership
            .and_then(|(_, member)| member.avatar.as_ref())
            .and_then(|file| self.media.file(file))
            .unwrap_or_else(|| self.user_avatar_url(&user.id));
        let name = self.name_in(channel, &user.id);
        let row = MemberListRow::Member {
            user_id: user.id.clone(),
            avatar_url,
            colour: membership.and_then(|(server, _)| self.role_colour(server, &user.id)),
            presence,
            status: custom.unwrap_or(presence.label()).to_string(),
            online: presence != PresenceKind::Offline,
            name: name.clone(),
        };
        (name.to_lowercase(), row)
    }

    /// The colour of `user`'s highest-ranked role in `server` that has one,
    /// as `#rrggbb`. Roles can be given any CSS colour, including gradients,
    /// which can't be drawn as text: those, and anything unparseable, give
    /// `None` (the default text colour).
    pub fn role_colour(&self, server: &str, user: &str) -> Option<String> {
        let server = self.servers.get(server)?;
        let member = self.member(&server.id, user)?;
        let (_, role) = member
            .roles
            .iter()
            .filter_map(|id| Some((id, server.roles.get(id)?)))
            .filter(|(_, role)| role.colour.as_deref().is_some_and(|c| !c.trim().is_empty()))
            // A higher rank is a smaller number.
            .min_by(|(a_id, a), (b_id, b)| a.rank.cmp(&b.rank).then_with(|| a_id.cmp(b_id)))?;
        solid_colour(role.colour.as_deref()?)
    }

    /// Cache a server's full member list and its users (for `@`).
    pub fn add_members(&mut self, response: AllMemberResponse) {
        for user in response.users {
            self.users.insert(user.id.clone(), user);
        }
        for member in response.members {
            self.members
                .insert((member.id.server.clone(), member.id.user.clone()), member);
        }
    }

    /// Someone joined a server (`ServerMemberJoin`). They're mentionable
    /// there once their user is known too (see [`Self::knows_user`]).
    pub fn apply_member_join(&mut self, member: Member) {
        self.members
            .insert((member.id.server.clone(), member.id.user.clone()), member);
    }

    /// A membership as Stoat answered a change to it (the user's own
    /// identity on a server), so nothing waits for the `ServerMemberUpdate`
    /// echo.
    pub fn apply_member(&mut self, member: Member) {
        self.members
            .insert((member.id.server.clone(), member.id.user.clone()), member);
    }

    /// `user` left `server` (`ServerMemberLeave`). Returns true if it was the
    /// user themselves: then the whole server goes (see [`Self::remove_server`]).
    pub fn apply_member_leave(&mut self, server: &str, user: &str) -> bool {
        self.members.remove(&(server.to_string(), user.to_string()));
        user == self.me
    }

    /// A member changed (`ServerMemberUpdate`), merged with the backend's own
    /// `Member::apply_options`. Members that aren't cached are ignored.
    /// Returns true if something shown changed (nickname or avatar).
    pub fn apply_member_update(
        &mut self,
        id: &MemberCompositeKey,
        data: PartialMember,
        clear: &[FieldsMember],
    ) -> bool {
        let Some(member) = self.members.get_mut(&(id.server.clone(), id.user.clone())) else {
            return false;
        };
        let shown = |m: &Member| (m.nickname.clone(), m.avatar.as_ref().map(|f| f.id.clone()));
        let before = shown(member);
        member.apply_options(data);
        for field in clear {
            match field {
                FieldsMember::Nickname => member.nickname = None,
                FieldsMember::Pronouns => member.pronouns = None,
                FieldsMember::Avatar => member.avatar = None,
                FieldsMember::Roles => member.roles.clear(),
                FieldsMember::Timeout => member.timeout = None,
                FieldsMember::CanReceive => member.can_receive = true,
                FieldsMember::CanPublish => member.can_publish = true,
                FieldsMember::JoinedAt | FieldsMember::VoiceChannel => {}
            }
        }
        before != shown(member)
    }

    /// A user changed (`UserUpdate`), merged with the backend's own
    /// `User::apply_options`. Users that aren't cached are ignored. Returns
    /// true if something shown changed (username, display name or avatar);
    /// presence and status changes return false, as they come often and
    /// don't change any message row.
    pub fn apply_user_update(&mut self, id: &str, data: PartialUser, clear: &[FieldsUser]) -> bool {
        let Some(user) = self.users.get_mut(id) else {
            return false;
        };
        let shown = |u: &User| {
            (
                u.username.clone(),
                u.display_name.clone(),
                u.avatar.as_ref().map(|f| f.id.clone()),
            )
        };
        let before = shown(user);
        // The user's own echo with a presence other than Invisible: they're
        // visible again (Stoat sends no `online: true` for that), even if
        // they went visible from another client.
        let visible_again = id == self.me
            && data
                .status
                .as_ref()
                .and_then(|status| status.presence.as_ref())
                .is_some_and(|presence| *presence != Presence::Invisible);
        user.apply_options(data);
        if visible_again {
            user.online = true;
        }
        for field in clear {
            match field {
                FieldsUser::Avatar => user.avatar = None,
                FieldsUser::DisplayName => user.display_name = None,
                FieldsUser::Pronouns => user.pronouns = None,
                FieldsUser::StatusText => {
                    if let Some(status) = &mut user.status {
                        status.text = None;
                    }
                }
                FieldsUser::StatusPresence => {
                    if let Some(status) = &mut user.status {
                        status.presence = None;
                    }
                }
                // Not part of `User` (profiles are fetched separately).
                FieldsUser::ProfileContent | FieldsUser::ProfileBackground | FieldsUser::Internal => {}
            }
        }
        before != shown(user)
    }

    /// A server changed (`ServerUpdate`), merged with the backend's own
    /// `Server::apply_options`. Servers that aren't cached are ignored.
    /// Returns true if it was known.
    pub fn apply_server_update(
        &mut self,
        id: &str,
        data: PartialServer,
        clear: &[FieldsServer],
    ) -> bool {
        let Some(server) = self.servers.get_mut(id) else {
            return false;
        };
        server.apply_options(data);
        for field in clear {
            match field {
                FieldsServer::Description => server.description = None,
                FieldsServer::Categories => server.categories = None,
                FieldsServer::SystemMessages => server.system_messages = None,
                FieldsServer::Icon => server.icon = None,
                FieldsServer::Banner => server.banner = None,
            }
        }
        true
    }

    /// A role changed or was created (`ServerRoleUpdate`). A known role is
    /// merged with the backend's own `Role::apply_options`. An unknown one is
    /// new, and `data` is the whole role; without a name it can't be built,
    /// so it's ignored until the next reconnect. Returns true if the server's
    /// roles changed.
    pub fn apply_role_update(
        &mut self,
        server: &str,
        role_id: &str,
        data: PartialRole,
        clear: &[FieldsRole],
    ) -> bool {
        let Some(server) = self.servers.get_mut(server) else {
            return false;
        };
        let count = server.roles.len() as i64;
        let role = match server.roles.get_mut(role_id) {
            Some(role) => {
                role.apply_options(data);
                role
            }
            None => {
                let Some(name) = data.name else {
                    log::warn!(
                        "ignoring new role {role_id} in {} without a name",
                        server.id
                    );
                    return false;
                };
                let role = Role {
                    id: role_id.to_string(),
                    name,
                    permissions: data.permissions.unwrap_or_default(),
                    colour: data.colour,
                    hoist: data.hoist.unwrap_or(false),
                    // New roles start at the bottom.
                    rank: data.rank.unwrap_or(count),
                    icon: data.icon,
                    owner: data.owner,
                };
                server.roles.entry(role_id.to_string()).or_insert(role)
            }
        };
        // The map's key is the role's ID, whatever `data` said.
        role.id = role_id.to_string();
        for field in clear {
            match field {
                FieldsRole::Colour => role.colour = None,
                FieldsRole::Icon => role.icon = None,
            }
        }
        true
    }

    /// A role was deleted (`ServerRoleDelete`). Stoat sends nothing else, so
    /// it's taken off every cached member and channel override here too.
    /// Returns true if it was known.
    pub fn apply_role_delete(&mut self, server: &str, role_id: &str) -> bool {
        let Some(known) = self.servers.get_mut(server) else {
            return false;
        };
        let removed = known.roles.remove(role_id).is_some();
        for ((member_server, _), member) in &mut self.members {
            if member_server == server {
                member.roles.retain(|id| id != role_id);
            }
        }
        for channel in self.channels.values_mut() {
            if let Channel::TextChannel {
                server: channel_server,
                role_permissions,
                ..
            } = channel
                && channel_server == server
            {
                role_permissions.remove(role_id);
            }
        }
        removed
    }

    /// The server's roles were reordered (`ServerRoleRanksUpdate`): each
    /// listed role's rank becomes its index, so the first is the highest.
    /// Returns true if the server is known.
    pub fn apply_role_ranks(&mut self, server: &str, ranks: &[String]) -> bool {
        let Some(server) = self.servers.get_mut(server) else {
            return false;
        };
        for (rank, id) in ranks.iter().enumerate() {
            if let Some(role) = server.roles.get_mut(id) {
                role.rank = rank as i64;
            }
        }
        true
    }

    /// Someone's relationship to the user changed: a friend or block route's
    /// answer, or `UserRelationship` (both carry the whole user, with the new
    /// `relationship`). They're added if unknown. Returns how they related
    /// before, or `None` if they weren't known.
    pub fn apply_relationship(
        &mut self,
        user: User,
    ) -> Option<crate::api::models::RelationshipStatus> {
        self.users
            .insert(user.id.clone(), user)
            .map(|before| before.relationship)
    }

    /// Cache a user fetched on their own (someone who just joined a server).
    pub fn add_user(&mut self, user: User) {
        self.users.insert(user.id.clone(), user);
    }

    pub fn knows_user(&self, id: &str) -> bool {
        self.users.contains_key(id)
    }

    /// The user joined or created a server (`ServerCreate`): add it with its
    /// channels and emoji, at the end of the rail if it's new. One that's
    /// already known (the event twice) is replaced, keeping its loaded
    /// history and cached members. Returns true if it's new.
    pub fn add_server(
        &mut self,
        server: Server,
        channels: Vec<Channel>,
        emojis: Vec<Emoji>,
    ) -> bool {
        let id = server.id.clone();
        let new = self.servers.insert(id.clone(), server).is_none();
        if new {
            self.server_order.push(id);
        }
        for channel in channels {
            self.add_channel(channel);
        }
        for emoji in emojis {
            self.add_emoji(emoji);
        }
        new
    }

    /// Forget a server the user left or was removed from: the server, its
    /// channels (and their history) and its members. Returns the IDs of the
    /// channels removed.
    pub fn remove_server(&mut self, id: &str) -> Vec<String> {
        let Some(server) = self.servers.remove(id) else {
            return Vec::new();
        };
        self.server_order.retain(|s| s != id);
        // Its listed channels, and any others that point at it.
        let strays: Vec<String> = self
            .channels
            .iter()
            .filter_map(|(cid, c)| match c {
                Channel::TextChannel { server: s, .. }
                    if s == id && !server.channels.contains(cid) =>
                {
                    Some(cid.clone())
                }
                _ => None,
            })
            .collect();
        let mut removed: Vec<String> = server.channels.into_iter().chain(strays).collect();
        removed.retain(|cid| self.channels.remove(cid).is_some());
        for cid in &removed {
            self.histories.remove(cid);
            self.read_states.remove(cid);
            if let Some(gone) = self.pending.remove(cid) {
                self.release_files(gone);
            }
        }
        self.members.retain(|(server, _), _| server != id);
        removed
    }

    /// Loaded channels whose rows show `user`: ones with a message by or
    /// mentioning them, and DMs or groups they're in. Rebuild these when the
    /// user's name or avatar changes.
    pub fn channels_showing(&self, user: &str) -> Vec<String> {
        let mut channels: Vec<String> = self
            .histories
            .iter()
            .filter(|(id, history)| {
                matches!(
                    self.channels.get(*id),
                    Some(Channel::DirectMessage { recipients, .. } | Channel::Group { recipients, .. })
                        if recipients.iter().any(|r| r == user)
                ) || history.messages.iter().any(|m| {
                    m.author == user
                        || m.mentions.as_ref().is_some_and(|ids| ids.iter().any(|id| id == user))
                })
            })
            .map(|(id, _)| id.clone())
            .collect();
        channels.sort();
        channels
    }

    /// The server `channel` belongs to, if it's a server channel.
    pub fn channel_server(&self, channel: &str) -> Option<&str> {
        self.server_of(channel)
    }

    /// What `#` suggests in `channel`: the text channels of its server
    /// that the user can see, in sidebar order. None outside servers.
    pub fn channel_candidates(&self, channel: &str) -> Vec<ChannelEntry> {
        let Some(server) = self.server_of(channel) else {
            return Vec::new();
        };
        self.server_channels(server)
            .into_iter()
            .flat_map(|section| section.channels)
            .filter(|entry| entry.kind == ChannelKind::Text { voice: false })
            .collect()
    }

    /// The roles of `channel`'s server, highest-ranked first, as `@`
    /// suggests them. None outside a server.
    pub fn role_candidates(&self, channel: &str) -> Vec<RoleCandidate> {
        let Some(server) = self.server_of(channel).and_then(|id| self.servers.get(id)) else {
            return Vec::new();
        };
        let mut roles: Vec<_> = server.roles.iter().collect();
        roles.sort_by(|(a_id, a), (b_id, b)| a.rank.cmp(&b.rank).then(a_id.cmp(b_id)));
        roles
            .into_iter()
            .map(|(id, role)| RoleCandidate {
                id: id.clone(),
                name: role.name.clone(),
                colour: role.colour.as_deref().and_then(solid_colour),
            })
            .collect()
    }

    /// `user` as `@` suggests them in `channel`. `None` if they aren't known.
    fn candidate(&self, channel: &str, user: &str, recent: bool) -> Option<Candidate> {
        let known = self.users.get(user)?;
        let name = self.name_in(channel, user);
        let display = display_name(known);
        Some(Candidate {
            id: user.to_string(),
            display_name: (display != name).then(|| display.to_string()),
            name,
            username: known.username.clone(),
            avatar: Some(self.user_avatar_url(user)),
            recent,
        })
    }

    /// What a chip in the message box shows for `token`, in `channel`:
    /// `@Name`, `#channel` or `:emoji:`.
    pub fn token_label(&self, channel: &str, token: &TokenRef) -> String {
        match token {
            TokenRef::User(id) => match self.users.get(id) {
                Some(_) => format!("@{}", self.name_in(channel, id)),
                None => "@Unknown user".into(),
            },
            TokenRef::Role(id) => match crate::markdown::Names::role(self, id) {
                Some((name, _)) => format!("@{name}"),
                None => "@Unknown role".into(),
            },
            TokenRef::Channel(id) => match self.channel_entry(id) {
                Some(entry) => format!("#{}", entry.name),
                None => "#unknown-channel".into(),
            },
            TokenRef::Emoji(id) => match self.emoji(id) {
                Some(emoji) => format!(":{}:", emoji.name),
                None => ":emoji:".into(),
            },
        }
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

    /// Every user the client knows, in no order.
    pub fn users(&self) -> impl Iterator<Item = &User> {
        self.users.values()
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

    /// `channel`'s slowmode, in seconds: how long members must wait between
    /// messages. Only server text channels have one; `None` when it's off.
    pub fn slowmode(&self, channel: &str) -> Option<u64> {
        match self.channel(channel)? {
            Channel::TextChannel {
                slowmode: Some(secs),
                ..
            } if *secs > 0 => Some(*secs),
            _ => None,
        }
    }

    /// Every channel the client knows, in no order.
    pub fn channels(&self) -> impl Iterator<Item = &Channel> {
        self.channels.values()
    }

    /// Reopen a closed DM here (`active = true`), as stoat.js does when
    /// someone is messaged from their profile, so it shows in Home. Stoat
    /// itself only marks it open once a message is sent. Returns true if it
    /// was closed.
    pub fn activate_dm(&mut self, channel: &str) -> bool {
        match self.channels.get_mut(channel) {
            Some(Channel::DirectMessage { active, .. }) if !*active => {
                *active = true;
                true
            }
            _ => false,
        }
    }

    /// Servers for the rail, in the user's order.
    pub fn servers(&self) -> Vec<ServerEntry> {
        self.server_order
            .iter()
            .filter_map(|id| self.servers.get(id))
            .map(|s| {
                let visible = s.channels.iter().filter(|id| {
                    matches!(self.channels.get(*id), Some(Channel::TextChannel { .. }))
                        && self.shows_channel(id)
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

    /// Whether the channel list shows `channel`. Server channels the user
    /// can't view (a role edit since `Ready` hid them) aren't shown, except
    /// while the user's own membership of the server isn't known yet (just
    /// after joining it), when there's nothing to work it out from. Closed
    /// DMs aren't shown in Home either.
    pub fn shows_channel(&self, channel: &str) -> bool {
        match self.channels.get(channel) {
            Some(Channel::TextChannel { server, .. }) => {
                self.member(server, &self.me).is_none()
                    || crate::permissions::can_view(self, &self.me, channel)
            }
            Some(Channel::DirectMessage { active, .. }) => *active,
            Some(_) => true,
            None => false,
        }
    }

    /// A server's text channels: uncategorized ones first (in the server's
    /// order), then each category in order. Channels we can't see (missing
    /// from `Ready`, or hidden since: see [`Self::shows_channel`]) and empty
    /// categories are left out.
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
            .filter(|id| self.shows_channel(id))
            .filter_map(|id| self.channel_entry(id))
            .collect();
        if !uncategorized.is_empty() {
            sections.push(ChannelSection {
                title: None,
                id: None,
                channels: uncategorized,
            });
        }
        for category in categories {
            let channels: Vec<ChannelEntry> = category
                .channels
                .iter()
                .filter(|id| self.shows_channel(id))
                .filter_map(|id| self.channel_entry(id))
                .collect();
            if !channels.is_empty() {
                sections.push(ChannelSection {
                    title: Some(category.title.clone()),
                    id: Some(category.id.clone()),
                    channels,
                });
            }
        }
        sections
    }

    /// Whether any server has a category with this ID.
    pub fn has_category(&self, id: &str) -> bool {
        self.servers.values().any(|server| {
            server
                .categories
                .iter()
                .flatten()
                .any(|category| category.id == id)
        })
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
        let muted = self.is_muted(&id);
        Some(ChannelEntry {
            // Muted: no unread dot, but mentions still show.
            unread: self.is_unread(&id) && !muted,
            mentions: self.mention_count(&id),
            muted,
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
        TimelineRow::NewMessages => "new-messages",
        TimelineRow::Message(message) => &message.key,
        // Its first message's: revealing it turns it into that message's row.
        TimelineRow::Blocked { ids } => ids.first().map_or("", String::as_str),
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

/// `rows` with a "New messages" divider before the first message after
/// `after` (the read position when the channel was opened) that isn't the
/// user's own. The message after it starts a new group. A run of blocked
/// messages counts as its first message: one that starts before `after` and
/// ends after it puts the divider after the run.
///
/// No divider if there's no such message, or if `after` is older than every
/// loaded message and there's older history not loaded: the divider would
/// sit above the oldest loaded message, which isn't where the user left off.
pub fn with_unread_divider(mut rows: Vec<TimelineRow>, after: &str) -> Vec<TimelineRow> {
    /// A sent message's ID, and whether it's the user's own.
    fn sent(row: &TimelineRow) -> Option<(&str, bool)> {
        match row {
            TimelineRow::Message(m) if m.pending.is_none() => Some((&m.id, m.mine)),
            TimelineRow::Blocked { ids } => ids.first().map(|id| (id.as_str(), false)),
            _ => None,
        }
    }
    let whole_history = matches!(rows.first(), Some(TimelineRow::StartOfChannel { .. }));
    let reaches_back = rows.iter().filter_map(sent).any(|(id, _)| id <= after);
    if !whole_history && !reaches_back {
        return rows;
    }
    let Some(ix) = rows
        .iter()
        .position(|row| sent(row).is_some_and(|(id, mine)| !mine && id > after))
    else {
        return rows;
    };
    if let TimelineRow::Message(first) = &mut rows[ix] {
        first.continuation = false;
    }
    rows.insert(ix, TimelineRow::NewMessages);
    mark_group_ends(&mut rows);
    rows
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
        self.emoji(id).map(|e| e.name.clone())
    }

    fn emoji_url(&self, id: &str) -> Option<String> {
        self.media.emoji(id)
    }

    fn role(&self, id: &str) -> Option<(String, Option<String>)> {
        let role = self
            .servers
            .values()
            .find_map(|server| server.roles.get(id))?;
        Some((
            role.name.clone(),
            role.colour.as_deref().and_then(solid_colour),
        ))
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
/// A system message in words. `name` gives a person by ID, and `text` passes
/// on anything else that came with the message (a channel name).
fn describe_system(
    system: &SystemMessage,
    name: &dyn Fn(&str) -> String,
    text: &dyn Fn(&str) -> String,
) -> String {
    match system {
        SystemMessage::Text { content } => text(content),
        SystemMessage::UserAdded { id, by } => format!("{} added {}", name(by), name(id)),
        SystemMessage::UserRemove { id, by } => format!("{} removed {}", name(by), name(id)),
        SystemMessage::UserJoined { id } => format!("{} joined", name(id)),
        SystemMessage::UserLeft { id } => format!("{} left", name(id)),
        SystemMessage::UserKicked { id } => format!("{} was kicked", name(id)),
        SystemMessage::UserBanned { id } => format!("{} was banned", name(id)),
        SystemMessage::ChannelRenamed { name: new, by } => {
            format!("{} renamed the channel to {}", name(by), text(new))
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
        other => text(&String::from(other.clone())),
    }
}

/// Who a message's author is as a person (whose card they open): `None`
/// for system messages and webhooks.
fn person_id(message: &Message) -> Option<String> {
    (message.system.is_none() && message.webhook.is_none()).then(|| message.author.clone())
}

fn sort_messages(messages: &mut Vec<Message>) {
    messages.sort_by(|a, b| a.id.cmp(&b.id));
    messages.dedup_by(|a, b| a.id == b.id);
}

/// A role's CSS colour as `#rrggbb`, if it's a solid colour. Gradients and
/// anything unparseable give `None`.
pub fn solid_colour(css: &str) -> Option<String> {
    use std::str::FromStr as _;
    let colour = svgtypes::Color::from_str(css.trim()).ok()?;
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        colour.red, colour.green, colour.blue
    ))
}

/// How `user` shows in the member list: by their presence, if they're
/// online and not invisible.
pub fn presence_kind(user: &User) -> PresenceKind {
    if !user.online {
        return PresenceKind::Offline;
    }
    match user
        .status
        .as_ref()
        .and_then(|status| status.presence.as_ref())
    {
        Some(Presence::Invisible) => PresenceKind::Offline,
        Some(Presence::Idle) => PresenceKind::Idle,
        Some(Presence::Focus) => PresenceKind::Focus,
        Some(Presence::Busy) => PresenceKind::Busy,
        Some(Presence::Online) | None => PresenceKind::Online,
    }
}

/// Sort member rows by their (lowercased) names; the same name by user ID, so
/// the order is stable.
fn sort_member_rows(rows: &mut [(String, MemberListRow)]) {
    let id = |row: &MemberListRow| match row {
        MemberListRow::Member { user_id, .. } => user_id.clone(),
        MemberListRow::Header { .. } => String::new(),
    };
    rows.sort_by_cached_key(|(key, row)| (key.clone(), id(row)));
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
