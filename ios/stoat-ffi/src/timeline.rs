//! A channel's rows as Swift draws them: [`TimelineRow`]s and their
//! [`MessageRow`]s turned into plain records. Milestone 1 shows text,
//! replies, and compact attachment and embed cards; reactions and full
//! media come later.

use stoat_gpui::attachment::{AttachmentView, FileKind};
use stoat_gpui::embed::{Description, EmbedView};
use stoat_gpui::store::{MessageRow, PendingState, ReplyPreview, ReplyState, TimelineRow};

use crate::rich_text::{RichBlock, RichCache};

/// One row of a channel.
// Nearly every row is a message, as with the store's `TimelineRow`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineItem {
    /// Above the oldest message, once there's no more history.
    StartOfChannel { name: String },
    /// "New messages": where the user left off.
    NewMessages,
    Message { message: MessageItem },
    /// A run of messages from blocked users, hidden.
    Blocked { ids: Vec<String> },
}

impl TimelineItem {
    /// What identifies the row across updates (for SwiftUI's `id`).
    pub fn id(&self) -> String {
        match self {
            TimelineItem::StartOfChannel { .. } => "start".into(),
            TimelineItem::NewMessages => "new-messages".into(),
            TimelineItem::Message { message } => message.key.clone(),
            TimelineItem::Blocked { ids } => format!("blocked-{}", ids.first().map_or("", |s| s)),
        }
    }
}

/// A message, as its row draws it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MessageItem {
    /// The message's ID (a pending message's key).
    pub id: String,
    /// The row's identity: a pending message and the message that confirms
    /// it share it, so the row doesn't flash or appear twice.
    pub key: String,
    pub author: String,
    pub author_id: Option<String>,
    pub avatar_url: Option<String>,
    /// "Today at 14:05" etc.
    pub time: String,
    /// Markdown.
    pub body: Option<String>,
    /// `body` as blocks to draw (empty without a body).
    pub rich: Vec<RichBlock>,
    /// `body` has a relative timestamp ("in 3 hours"): redraw it each minute.
    pub relative_time: bool,
    /// Changes whenever what's drawn for `body` changes.
    pub render_key: String,
    pub system: bool,
    pub edited: bool,
    /// Pinned to the channel.
    pub pinned: bool,
    /// Same author as the row above, soon after: no name or avatar.
    pub continuation: bool,
    pub mine: bool,
    /// Only a few emoji: drawn large.
    pub jumbo: bool,
    pub pending: Option<PendingItem>,
    pub replies: Vec<ReplyItem>,
    pub attachments: Vec<AttachmentItem>,
    pub embeds: Vec<EmbedItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PendingItem {
    Sending,
    /// Why it wasn't sent.
    Failed { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReplyStateItem {
    Loaded,
    Loading,
    Deleted,
    Unavailable,
}

/// The "↱ Name: snippet" line above a reply.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReplyItem {
    pub id: String,
    pub author: String,
    pub snippet: String,
    pub state: ReplyStateItem,
}

/// An attachment: a thumbnail for images, else a card with its name and
/// size. Either opens `url`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AttachmentItem {
    pub name: String,
    pub size_label: String,
    pub url: Option<String>,
    pub is_image: bool,
    /// The display size, for images (zero otherwise).
    pub width: u32,
    pub height: u32,
}

/// A link preview or bot embed, compactly.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EmbedItem {
    pub site_name: Option<String>,
    pub title: Option<String>,
    /// As plain text.
    pub description: Option<String>,
    /// A bot embed's Markdown description as blocks (empty for a website's
    /// plain one).
    pub rich_description: Vec<RichBlock>,
    /// Where it opens.
    pub link: Option<String>,
    pub image_url: Option<String>,
}

fn attachment(view: &AttachmentView) -> AttachmentItem {
    match view {
        AttachmentView::Image {
            url,
            name,
            size_label,
            width,
            height,
            ..
        } => AttachmentItem {
            name: name.clone(),
            size_label: size_label.clone(),
            url: Some(url.clone()),
            is_image: true,
            width: *width,
            height: *height,
        },
        AttachmentView::File {
            name,
            size_label,
            url,
            ..
        } => AttachmentItem {
            name: name.clone(),
            size_label: size_label.clone(),
            url: url.clone(),
            is_image: false,
            width: 0,
            height: 0,
        },
        // Played in Safari for now: inline media is iOS milestone 3.
        AttachmentView::Audio {
            url,
            name,
            size_label,
            ..
        }
        | AttachmentView::Video {
            url,
            name,
            size_label,
            ..
        } => AttachmentItem {
            name: name.clone(),
            size_label: size_label.clone(),
            url: Some(url.clone()),
            is_image: false,
            width: 0,
            height: 0,
        },
        AttachmentView::Local {
            name,
            size_label,
            kind,
            ..
        } => AttachmentItem {
            name: name.clone(),
            size_label: size_label.clone(),
            url: None,
            is_image: *kind == FileKind::Image,
            width: 0,
            height: 0,
        },
    }
}

fn embed(view: &EmbedView, cache: &RichCache) -> EmbedItem {
    match view {
        EmbedView::Card(card) => EmbedItem {
            site_name: card.site_name.clone(),
            title: card.title.clone(),
            description: card.description.as_ref().map(|d| match d {
                Description::Plain(text) => text.clone(),
                Description::Markdown { source, .. } => source.clone(),
            }),
            rich_description: match &card.description {
                Some(Description::Markdown { source, render_key }) => {
                    cache.get(render_key, source).as_ref().clone()
                }
                _ => Vec::new(),
            },
            link: card.link.clone(),
            image_url: card
                .image
                .as_ref()
                .or(card.thumbnail.as_ref())
                .map(|i| i.url.clone()),
        },
        EmbedView::Image(image) => EmbedItem {
            site_name: None,
            title: None,
            description: None,
            rich_description: Vec::new(),
            link: Some(image.url.clone()),
            image_url: Some(image.url.clone()),
        },
        EmbedView::Video { url } => EmbedItem {
            site_name: None,
            title: Some("Video".into()),
            description: None,
            rich_description: Vec::new(),
            link: Some(url.clone()),
            image_url: None,
        },
        EmbedView::GifPoster { image, link } => EmbedItem {
            site_name: Some("GIF".into()),
            title: None,
            description: None,
            rich_description: Vec::new(),
            link: Some(link.clone()),
            image_url: Some(image.url.clone()),
        },
    }
}

fn reply(preview: &ReplyPreview) -> ReplyItem {
    ReplyItem {
        id: preview.id.clone(),
        author: preview.author.clone(),
        snippet: preview.snippet.clone(),
        state: match preview.state {
            ReplyState::Loaded => ReplyStateItem::Loaded,
            ReplyState::Loading => ReplyStateItem::Loading,
            ReplyState::Deleted => ReplyStateItem::Deleted,
            ReplyState::Unavailable => ReplyStateItem::Unavailable,
        },
    }
}

pub fn message(row: MessageRow, cache: &RichCache) -> MessageItem {
    MessageItem {
        rich: row
            .body
            .as_deref()
            .map(|body| cache.get(&row.render_key, body).as_ref().clone())
            .unwrap_or_default(),
        relative_time: row
            .body
            .as_deref()
            .is_some_and(stoat_gpui::markdown::has_relative_time),
        replies: row.replies.iter().map(reply).collect(),
        attachments: row.attachments.iter().map(attachment).collect(),
        embeds: row.embeds.iter().map(|e| embed(e, cache)).collect(),
        pending: row.pending.map(|state| match state {
            PendingState::Sending => PendingItem::Sending,
            PendingState::Failed(reason) => PendingItem::Failed { reason },
        }),
        id: row.id,
        key: row.key,
        author: row.author,
        author_id: row.author_id,
        avatar_url: row.avatar,
        time: row.time,
        body: row.body,
        render_key: row.render_key,
        system: row.system,
        edited: row.edited,
        pinned: row.pinned,
        continuation: row.continuation,
        mine: row.mine,
        jumbo: row.jumbo,
    }
}

pub fn item(row: TimelineRow, cache: &RichCache) -> TimelineItem {
    match row {
        TimelineRow::StartOfChannel { name } => TimelineItem::StartOfChannel { name },
        TimelineRow::NewMessages => TimelineItem::NewMessages,
        TimelineRow::Message(row) => TimelineItem::Message {
            message: message(row, cache),
        },
        TimelineRow::Blocked { ids } => TimelineItem::Blocked { ids },
    }
}
