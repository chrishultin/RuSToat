//! A channel's rows as Swift draws them: [`TimelineRow`]s and their
//! [`MessageRow`]s turned into plain records, with every attachment and
//! embed kind desktop draws.

use stoat_gpui::attachment::{AttachmentView, FileKind};
use stoat_gpui::embed::{Description, EmbedImage, EmbedView};
use stoat_gpui::store::{
    MessageRow, PendingState, PickerEmoji, ReactionChip, ReactionEmoji, ReplyPreview, ReplyState,
    TimelineRow,
};

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
    /// Reactions, in the order they were first added.
    pub reactions: Vec<ReactionItem>,
}

/// An emoji, as a reaction or a picker choice shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum EmojiItem {
    Unicode { text: String },
    /// `url` is `None` until the file server is known.
    Custom { id: String, name: String, url: Option<String> },
}

impl EmojiItem {
    pub fn of(emoji: &ReactionEmoji) -> Self {
        match emoji {
            ReactionEmoji::Unicode(text) => EmojiItem::Unicode { text: text.clone() },
            ReactionEmoji::Custom { id, url, name } => EmojiItem::Custom {
                id: id.clone(),
                name: name.clone(),
                url: url.clone(),
            },
        }
    }
}

/// One reaction under a message.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReactionItem {
    /// As the API names it (the Unicode emoji, or a custom emoji's ID):
    /// what toggling it sends.
    pub key: String,
    pub emoji: EmojiItem,
    pub count: u32,
    /// The user is one of them.
    pub mine: bool,
    /// Others who used it, as the channel names them (at most 8, and only
    /// those known); the rest are "others".
    pub names: Vec<String>,
}

/// An emoji the reaction picker offers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PickerEmojiItem {
    pub key: String,
    pub emoji: EmojiItem,
}

impl PickerEmojiItem {
    pub fn of(choice: &PickerEmoji) -> Self {
        Self {
            key: choice.key.clone(),
            emoji: EmojiItem::of(&choice.emoji),
        }
    }
}

fn reaction(chip: &ReactionChip) -> ReactionItem {
    ReactionItem {
        key: chip.key.clone(),
        emoji: EmojiItem::of(&chip.emoji),
        count: u32::try_from(chip.count).unwrap_or(u32::MAX),
        mine: chip.mine,
        names: chip.names.clone(),
    }
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

/// What a file card's icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FileKindItem {
    Image,
    Video,
    Audio,
    Text,
    File,
}

impl From<FileKind> for FileKindItem {
    fn from(kind: FileKind) -> Self {
        match kind {
            FileKind::Image => FileKindItem::Image,
            FileKind::Video => FileKindItem::Video,
            FileKind::Audio => FileKindItem::Audio,
            FileKind::Text => FileKindItem::Text,
            FileKind::File => FileKindItem::File,
        }
    }
}

/// An attachment, as desktop shows it (see the store's `AttachmentView`).
// Records inside, so Swift can pass each kind to its view whole.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum AttachmentItem {
    /// Drawn inline at its display size.
    Image { image: ImageAttachment },
    /// A card (icon, name, size) that opens the file.
    File { file: FileAttachment },
    /// A card with a player.
    Audio { audio: AudioAttachment },
    /// A poster frame at its display size, with a play button.
    Video { video: VideoAttachment },
    /// A file of a message still being sent, with its upload progress.
    Local { local: LocalAttachment },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ImageAttachment {
    /// The file's ID, for "Show image" and spoiler reveals.
    pub id: String,
    pub url: String,
    pub name: String,
    pub size_label: String,
    /// The display size: the upload's own, fitted within desktop's box (or
    /// a fallback box when it's unknown). The row is laid out at this shape
    /// before the picture arrives.
    pub width: u32,
    pub height: u32,
    /// The upload's own size (zero if unknown), for the full-size viewer.
    pub original_width: u32,
    pub original_height: u32,
    /// A ThumbHash, for a blurred preview while it loads (see
    /// [`crate::media::thumbhash_image`]).
    pub thumbhash: Option<Vec<u8>>,
    /// Over 8 MB and not revealed yet: a "Show image" card instead.
    pub too_big: bool,
    /// A spoiler not shown yet: covered, at the same size, until tapped.
    pub spoiler: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FileAttachment {
    pub id: String,
    pub name: String,
    pub size_label: String,
    /// `None` until the file server is known.
    pub url: Option<String>,
    pub kind: FileKindItem,
    /// A spoiler: its card has a tag.
    pub spoiler: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AudioAttachment {
    /// The file's ID: what's playing is known by it.
    pub id: String,
    /// The original's own address, to stream.
    pub url: String,
    pub name: String,
    pub size_label: String,
    /// A spoiler: its card has a tag (it isn't covered).
    pub spoiler: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VideoAttachment {
    /// The file's ID, for its reveal and what's playing.
    pub id: String,
    /// The original's own address, to stream.
    pub url: String,
    pub name: String,
    pub size_label: String,
    /// The display size, as an image's.
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    /// A spoiler not shown yet: covered, at the same size, until tapped.
    pub spoiler: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LocalAttachment {
    pub name: String,
    pub size_label: String,
    pub kind: FileKindItem,
    /// For images: the local copy to draw, at `width` × `height` (the size
    /// the sent message will have).
    pub image_path: Option<String>,
    pub width: u32,
    pub height: u32,
    /// Uploaded so far, in thousandths.
    pub progress: u16,
    /// Marked as a spoiler: an image is drawn covered, as the sent one will be.
    pub spoiler: bool,
}

/// A link preview or bot embed, as desktop shows it (see the store's
/// `EmbedView`).
// Most embeds are cards, and UniFFI can't box a variant's record.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum EmbedItem {
    /// A link preview or bot embed: a card with a colour bar.
    Card { card: EmbedCardItem },
    /// Just a picture: a link to an image, or a GIF.
    Image { image: EmbedImageItem },
    /// Just a video link: a card that opens it.
    Video { url: String },
    /// A Gifbox GIF that only exists as video: its still, with a "GIF"
    /// badge, opening `link` (its page).
    GifPoster { image: EmbedImageItem, link: String },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EmbedCardItem {
    /// The colour bar, `0xRRGGBBAA`. `None` uses the separator colour.
    pub colour: Option<u32>,
    /// The site's icon (proxied).
    pub icon: Option<String>,
    pub site_name: Option<String>,
    pub title: Option<String>,
    /// Where the title links to (http(s) only).
    pub link: Option<String>,
    /// A website's description as written, or a bot embed's Markdown source.
    pub description: Option<String>,
    /// A bot embed's Markdown description as blocks (empty for a website's
    /// plain one).
    pub rich_description: Vec<RichBlock>,
    /// A small picture beside the text.
    pub thumbnail: Option<EmbedImageItem>,
    /// A large picture under the text.
    pub image: Option<EmbedImageItem>,
    /// The picture stands for a video (YouTube and the like): it gets a play
    /// badge and opens `link`.
    pub playable: bool,
    /// A bot embed's uploaded file.
    pub media: Option<AttachmentItem>,
}

/// A remote picture at its display size.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EmbedImageItem {
    /// Proxied.
    pub url: String,
    /// The picture's own address (not proxied).
    pub source: String,
    pub width: u32,
    pub height: u32,
    /// As reported by the server (zero if unknown).
    pub original_width: u32,
    pub original_height: u32,
}

fn side(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub fn attachment(view: &AttachmentView) -> AttachmentItem {
    match view {
        AttachmentView::Image {
            id,
            url,
            name,
            size_label,
            width,
            height,
            original,
            thumbhash,
            too_big,
            spoiler,
        } => AttachmentItem::Image {
            image: ImageAttachment {
                id: id.clone(),
                url: url.clone(),
                name: name.clone(),
                size_label: size_label.clone(),
                width: *width,
                height: *height,
                original_width: side(original.0),
                original_height: side(original.1),
                thumbhash: thumbhash.as_deref().map(<[u8]>::to_vec),
                too_big: *too_big,
                spoiler: *spoiler,
            },
        },
        AttachmentView::File {
            id,
            name,
            size_label,
            url,
            kind,
            spoiler,
            ..
        } => AttachmentItem::File {
            file: FileAttachment {
                id: id.clone(),
                name: name.clone(),
                size_label: size_label.clone(),
                url: url.clone(),
                kind: (*kind).into(),
                spoiler: *spoiler,
            },
        },
        AttachmentView::Audio {
            id,
            url,
            name,
            size_label,
            spoiler,
        } => AttachmentItem::Audio {
            audio: AudioAttachment {
                id: id.clone(),
                url: url.clone(),
                name: name.clone(),
                size_label: size_label.clone(),
                spoiler: *spoiler,
            },
        },
        AttachmentView::Video {
            id,
            url,
            name,
            size_label,
            width,
            height,
            original,
            spoiler,
        } => AttachmentItem::Video {
            video: VideoAttachment {
                id: id.clone(),
                url: url.clone(),
                name: name.clone(),
                size_label: size_label.clone(),
                width: *width,
                height: *height,
                original_width: side(original.0),
                original_height: side(original.1),
                spoiler: *spoiler,
            },
        },
        AttachmentView::Local {
            name,
            size_label,
            kind,
            image,
            progress,
            spoiler,
        } => AttachmentItem::Local {
            local: LocalAttachment {
                name: name.clone(),
                size_label: size_label.clone(),
                kind: (*kind).into(),
                image_path: image
                    .as_ref()
                    .map(|(path, _, _)| path.to_string_lossy().into_owned()),
                width: image.as_ref().map_or(0, |(_, w, _)| *w),
                height: image.as_ref().map_or(0, |(_, _, h)| *h),
                progress: *progress,
                spoiler: *spoiler,
            },
        },
    }
}

fn embed_image(image: &EmbedImage) -> EmbedImageItem {
    EmbedImageItem {
        url: image.url.clone(),
        source: image.source.clone(),
        width: image.width,
        height: image.height,
        original_width: side(image.original.0),
        original_height: side(image.original.1),
    }
}

fn embed(view: &EmbedView, cache: &RichCache) -> EmbedItem {
    match view {
        EmbedView::Card(card) => EmbedItem::Card {
            card: EmbedCardItem {
                colour: card.colour,
                icon: card.icon.clone(),
                site_name: card.site_name.clone(),
                title: card.title.clone(),
                link: card.link.clone(),
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
                thumbnail: card.thumbnail.as_ref().map(embed_image),
                image: card.image.as_ref().map(embed_image),
                playable: card.playable,
                media: card.media.as_ref().map(attachment),
            },
        },
        EmbedView::Image(image) => EmbedItem::Image {
            image: embed_image(image),
        },
        EmbedView::Video { url } => EmbedItem::Video { url: url.clone() },
        EmbedView::GifPoster { image, link } => EmbedItem::GifPoster {
            image: embed_image(image),
            link: link.clone(),
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
        reactions: row.reactions.iter().map(reaction).collect(),
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
