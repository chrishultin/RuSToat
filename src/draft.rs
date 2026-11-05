//! The message box's rules, with no GPUI: what can be sent, the length
//! limit, replies, attached files, and drafts kept per channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::api::models::ReplyIntent;
use crate::api::{DEFAULT_ATTACHMENT_SIZE_LIMIT, DEFAULT_MAX_ATTACHMENTS, RootConfig};
use crate::attachment::{FileKind, size_label};
use crate::store::MAX_REPLIES;

/// Longest message Stoat accepts, in characters (`limits.global.message_length`).
pub const MAX_MESSAGE_CHARS: usize = 2000;
/// From this many characters, the message box shows a count.
pub const SHOW_COUNT_FROM: usize = 1800;

/// Whether the message box's text can be sent as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendCheck {
    /// Send this: the text without leading blank lines or trailing whitespace.
    Ready(String),
    /// Nothing but whitespace: nothing to send.
    Empty,
    /// More than [`MAX_MESSAGE_CHARS`] characters (this many).
    TooLong(usize),
}

/// `has_attachments`: files go with it, so the text may be empty.
pub fn check_send(text: &str, has_attachments: bool) -> SendCheck {
    let text = trim_message(text);
    if text.is_empty() {
        return if has_attachments {
            SendCheck::Ready(String::new())
        } else {
            SendCheck::Empty
        };
    }
    let chars = text.chars().count();
    if chars > MAX_MESSAGE_CHARS {
        return SendCheck::TooLong(chars);
    }
    SendCheck::Ready(text.to_string())
}

/// Trailing whitespace and leading blank lines go; leading spaces on the
/// first line stay (they may be indentation).
fn trim_message(text: &str) -> &str {
    let text = text.trim_end();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        if !line.trim().is_empty() {
            break;
        }
        start += line.len();
    }
    &text[start..]
}

/// The character count to show under the message box, if any: from
/// [`SHOW_COUNT_FROM`] characters. `true` when over the limit.
pub fn char_count(text: &str) -> Option<(usize, bool)> {
    let chars = trim_message(text).chars().count();
    (chars >= SHOW_COUNT_FROM).then_some((chars, chars > MAX_MESSAGE_CHARS))
}

/// A message the user is replying to, from the "Replying to" bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    pub id: String,
    /// Who wrote it, for the bar.
    pub author: String,
    /// Ping them (the bar's "@" toggle).
    pub mention: bool,
}

/// Replying to more messages than Stoat allows (`MAX_REPLIES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyReplies;

/// A file attached to the message being typed, waiting in the tray under
/// the box. It's read and uploaded when the message is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftAttachment {
    /// Tells it apart from the other files in the tray (and, once sent, in
    /// its pending message).
    pub local_id: u64,
    pub name: String,
    /// In bytes.
    pub size: u64,
    /// MIME type, from the name's extension.
    pub content_type: String,
    pub kind: FileKind,
    /// An image's width and height, if they could be read locally (so its
    /// pending row has the size the sent one will).
    pub dimensions: Option<(u32, u32)>,
    /// Where it is on disk. (Pasted images are saved to a file first, see
    /// [`save_pasted_image`].)
    pub path: PathBuf,
}

impl DraftAttachment {
    /// The file at `path`, `size` bytes long, named and typed after it.
    pub fn from_path(path: PathBuf, size: u64) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let content_type = mime_guess::from_path(&name)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        Self {
            local_id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            kind: crate::attachment::file_kind(&crate::api::models::Metadata::File, &content_type),
            name,
            size,
            content_type,
            dimensions: None,
            path,
        }
    }

    pub fn with_dimensions(mut self, dimensions: Option<(u32, u32)>) -> Self {
        self.dimensions = dimensions;
        self
    }
}

/// Save a pasted image (`extension` from its clipboard format, e.g. `png`)
/// in `dir` as `pasted-image-{n}`, so it's attached like any other file.
/// TIFF and BMP, which some apps copy, become PNG, which Stoat previews.
pub fn save_pasted_image(
    dir: &Path,
    extension: &str,
    bytes: &[u8],
) -> std::io::Result<DraftAttachment> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let converted = matches!(extension, "tiff" | "tif" | "bmp")
        .then(|| to_png(bytes))
        .flatten();
    let (extension, bytes) = match &converted {
        Some(png) => ("png", png.as_slice()),
        None => (extension, bytes),
    };
    std::fs::create_dir_all(dir)?;
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("pasted-image-{n}.{extension}"));
    std::fs::write(&path, bytes)?;
    let dimensions = crate::attachment::local_image_size(&path);
    Ok(DraftAttachment::from_path(path, bytes.len() as u64).with_dimensions(dimensions))
}

/// `bytes` (an image in any format this build reads) as a PNG.
fn to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// How many files a message may carry, and how big each may be (from
/// `GET /`, see [`RootConfig::attachment_size_limit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentLimits {
    pub max_bytes: u64,
    pub max_count: usize,
}

impl Default for AttachmentLimits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_ATTACHMENT_SIZE_LIMIT,
            max_count: DEFAULT_MAX_ATTACHMENTS,
        }
    }
}

impl AttachmentLimits {
    pub fn from_root(root: &RootConfig) -> Self {
        Self {
            max_bytes: root.attachment_size_limit(),
            max_count: root.max_attachments(),
        }
    }
}

/// Why a file wasn't attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachError {
    /// Over the size limit (`limit` bytes).
    TooBig { name: String, limit: u64 },
    /// The message already has the most files it can (`max`).
    TooMany { max: usize },
}

impl std::fmt::Display for AttachError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttachError::TooBig { name, limit } => write!(
                f,
                "{name} is too big to send. Files can be up to {}.",
                decimal_size_label(*limit)
            ),
            AttachError::TooMany { max } => {
                write!(f, "A message can have at most {max} files.")
            }
        }
    }
}

/// A limit as Stoat states it: decimal units, so 20 000 000 bytes is
/// "20 MB" (not 19.1). Falls back to [`size_label`] for odd sizes.
fn decimal_size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 && bytes.is_multiple_of(1_000_000) {
        format!("{} MB", bytes / 1_000_000)
    } else {
        size_label(bytes)
    }
}

/// What's in the message box: text, the messages it replies to, and the
/// files attached to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub text: String,
    pub replies: Vec<ReplyTarget>,
    pub attachments: Vec<DraftAttachment>,
}

impl Draft {
    /// Nothing typed, replied to or attached.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.replies.is_empty() && self.attachments.is_empty()
    }

    /// Attach `file` too, unless it's over `limits.max_bytes` or the
    /// message already has `limits.max_count` files.
    pub fn add_attachment(
        &mut self,
        file: DraftAttachment,
        limits: &AttachmentLimits,
    ) -> Result<(), AttachError> {
        if file.size > limits.max_bytes {
            return Err(AttachError::TooBig {
                name: file.name,
                limit: limits.max_bytes,
            });
        }
        if self.attachments.len() >= limits.max_count {
            return Err(AttachError::TooMany {
                max: limits.max_count,
            });
        }
        self.attachments.push(file);
        Ok(())
    }

    pub fn remove_attachment(&mut self, local_id: u64) {
        self.attachments.retain(|a| a.local_id != local_id);
    }

    /// Reply to `target` as well. Replying to the same message twice
    /// changes nothing; past [`MAX_REPLIES`] it's refused.
    pub fn add_reply(&mut self, target: ReplyTarget) -> Result<(), TooManyReplies> {
        if self.replies.iter().any(|r| r.id == target.id) {
            return Ok(());
        }
        if self.replies.len() >= MAX_REPLIES {
            return Err(TooManyReplies);
        }
        self.replies.push(target);
        Ok(())
    }

    pub fn toggle_mention(&mut self, id: &str) {
        if let Some(reply) = self.replies.iter_mut().find(|r| r.id == id) {
            reply.mention = !reply.mention;
        }
    }

    pub fn remove_reply(&mut self, id: &str) {
        self.replies.retain(|r| r.id != id);
    }

    /// The replies as Stoat's send body takes them.
    pub fn reply_intents(&self) -> Vec<ReplyIntent> {
        self.replies
            .iter()
            .map(|r| ReplyIntent {
                id: r.id.clone(),
                mention: r.mention,
                fail_if_not_exists: None,
            })
            .collect()
    }
}

/// Unsent drafts, kept per channel while the app runs.
#[derive(Debug, Default)]
pub struct Drafts {
    by_channel: HashMap<String, Draft>,
}

impl Drafts {
    /// Keep `draft` as `channel`'s (forgetting it if it's empty).
    pub fn save(&mut self, channel: &str, draft: Draft) {
        if draft.is_empty() {
            self.by_channel.remove(channel);
        } else {
            self.by_channel.insert(channel.to_string(), draft);
        }
    }

    /// Take `channel`'s draft out, to put back in the message box.
    pub fn take(&mut self, channel: &str) -> Draft {
        self.by_channel.remove(channel).unwrap_or_default()
    }
}

/// Editing a message in the message box: what was being typed is set aside
/// until the edit is saved or cancelled.
#[derive(Debug, Default)]
pub struct EditStash {
    editing: Option<(String, Draft)>,
}

impl EditStash {
    /// Start editing `id`, setting `current` aside. Switching from one edit
    /// to another keeps the first draft (the edit's text isn't a draft).
    pub fn begin(&mut self, id: String, current: Draft) {
        let stash = match self.editing.take() {
            Some((_, stash)) => stash,
            None => current,
        };
        self.editing = Some((id, stash));
    }

    /// The message being edited.
    pub fn editing(&self) -> Option<&str> {
        self.editing.as_ref().map(|(id, _)| id.as_str())
    }

    /// Stop editing (saved or cancelled): the draft to put back, if editing.
    pub fn end(&mut self) -> Option<Draft> {
        self.editing.take().map(|(_, stash)| stash)
    }
}

/// A new `Idempotency-Key` for a send: the time, a counter, and a random
/// part (Stoat's duplicate check is shared by every client, so time alone
/// isn't enough). Well under Stoat's 64-byte limit.
pub fn new_nonce(now_ms: u64) -> String {
    use std::hash::{BuildHasher, RandomState};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    // A fresh `RandomState` is randomly keyed: hashing anything gives noise.
    let random = RandomState::new().hash_one(count);
    format!("rustoat-{now_ms}-{count}-{random:016x}")
}
