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

/// Whether sending (or, when `editing`, saving) would do anything: what
/// the send button's disabled state follows. It's [`check_send`] being
/// ready, except that an edit can't have files, so it needs text.
pub fn can_send(text: &str, has_attachments: bool, editing: bool) -> bool {
    matches!(
        check_send(text, has_attachments && !editing),
        SendCheck::Ready(_)
    )
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
    /// RuSToat made the file (a pasted image), so it's deleted once it's no
    /// longer needed (see [`remove_temporary`]). The user's own files never are.
    pub temporary: bool,
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
            temporary: false,
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
    let mut file = DraftAttachment::from_path(path, bytes.len() as u64).with_dimensions(dimensions);
    file.temporary = true;
    Ok(file)
}

/// Pasted images are saved in `rustoat-pasted-{pid}` under the temporary
/// directory ([`paste_dir`]).
pub const PASTE_DIR_PREFIX: &str = "rustoat-pasted-";

/// This process's folder for pasted images, under `tmp`.
pub fn paste_dir(tmp: &Path, pid: u32) -> PathBuf {
    tmp.join(format!("{PASTE_DIR_PREFIX}{pid}"))
}

/// Delete `file` if RuSToat made it (a pasted image); the user's own files
/// are left alone. A file that's already gone is fine.
pub fn remove_temporary(file: &DraftAttachment) {
    if !file.temporary {
        return;
    }
    match std::fs::remove_file(&file.path) {
        Ok(()) => log::debug!("removed {}", file.path.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!("couldn't remove {}: {err}", file.path.display()),
    }
}

/// Remove pasted-image folders other RuSToat processes left in `tmp`: those
/// not changed for `max_age` (as of `now`), and never `own_pid`'s. Another
/// RuSToat still running keeps its folder unless it's been idle that long.
/// Returns the folders removed.
pub fn sweep_paste_dirs(
    tmp: &Path,
    own_pid: u32,
    now: std::time::SystemTime,
    max_age: std::time::Duration,
) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(tmp) else {
        return Vec::new();
    };
    let own = paste_dir(tmp, own_pid);
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_paste_dir = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_prefix(PASTE_DIR_PREFIX))
            .is_some_and(|pid| pid.parse::<u32>().is_ok());
        if !is_paste_dir || path == own || !path.is_dir() {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= max_age);
        if !stale {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => removed.push(path),
            Err(err) => log::warn!("couldn't remove {}: {err}", path.display()),
        }
    }
    removed
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

/// `content` as a Markdown quote, ready to type a reply under: each line
/// starts `> ` (a blank one is just `>`), and a blank line follows. As
/// Stoat's web client quotes a message.
pub fn quote(content: &str) -> String {
    let mut quoted = String::new();
    for line in content.lines() {
        if line.is_empty() {
            quoted.push('>');
        } else {
            quoted.push_str("> ");
            quoted.push_str(line);
        }
        quoted.push('\n');
    }
    quoted.push('\n');
    quoted
}

/// What's in the box (`existing`) with `quote` added on a line of its own.
pub fn append_quote(existing: &str, quote: &str) -> String {
    if existing.is_empty() || existing.ends_with('\n') {
        format!("{existing}{quote}")
    } else {
        format!("{existing}\n{quote}")
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    /// A fresh, empty directory for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("stoat-gpui-draft-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn can_send_needs_text_or_files() {
        assert!(can_send("hello", false, false));
        assert!(!can_send("", false, false));
        assert!(!can_send("  \n\t ", false, false));
        // Files go without text.
        assert!(can_send("", true, false));
        assert!(can_send("  \n ", true, false));
    }

    #[test]
    fn can_send_stops_at_the_length_limit() {
        assert!(can_send(&"a".repeat(MAX_MESSAGE_CHARS), false, false));
        assert!(!can_send(&"a".repeat(MAX_MESSAGE_CHARS + 1), false, false));
        // Files don't make a too-long text sendable.
        assert!(!can_send(&"a".repeat(MAX_MESSAGE_CHARS + 1), true, false));
    }

    #[test]
    fn can_send_while_editing_needs_text_within_the_limit() {
        assert!(can_send("fixed", false, true));
        assert!(!can_send("", false, true));
        assert!(!can_send("   ", false, true));
        // An edit changes text only: files don't count.
        assert!(!can_send("", true, true));
        assert!(can_send(&"a".repeat(MAX_MESSAGE_CHARS), false, true));
        assert!(!can_send(&"a".repeat(MAX_MESSAGE_CHARS + 1), false, true));
    }

    #[test]
    fn a_quote_prefixes_every_line_and_ends_with_a_blank_line() {
        assert_eq!(quote("hello"), "> hello\n\n");
        assert_eq!(quote("one\ntwo"), "> one\n> two\n\n");
        // A blank line inside stays part of the quote.
        assert_eq!(quote("one\n\ntwo"), "> one\n>\n> two\n\n");
        // A trailing newline doesn't add an empty quoted line.
        assert_eq!(quote("one\n"), "> one\n\n");
        assert_eq!(quote("one\r\ntwo"), "> one\n> two\n\n");
    }

    #[test]
    fn a_quote_goes_on_a_line_of_its_own() {
        let q = quote("hi");
        assert_eq!(append_quote("", &q), "> hi\n\n");
        assert_eq!(append_quote("look", &q), "look\n> hi\n\n");
        assert_eq!(append_quote("look\n", &q), "look\n> hi\n\n");
        // A second quote goes under the first.
        let twice = append_quote(&append_quote("", &q), &quote("again"));
        assert_eq!(twice, "> hi\n\n> again\n\n");
    }

    #[test]
    fn the_sweep_removes_old_foreign_folders_only() {
        let tmp = temp_dir("sweep");
        let own = paste_dir(&tmp, 100);
        let foreign = paste_dir(&tmp, 200);
        let not_ours = tmp.join("rustoat-pasted-notapid");
        let unrelated = tmp.join("something-else");
        for dir in [&own, &foreign, &not_ours, &unrelated] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("pasted-image-1.png"), b"png").unwrap();
        }

        // Everything was just made: nothing is old enough.
        assert!(sweep_paste_dirs(&tmp, 100, SystemTime::now(), DAY).is_empty());
        assert!(foreign.exists());

        // A day and a bit later, only the other process's folder goes.
        let later = SystemTime::now() + DAY + Duration::from_secs(60);
        assert_eq!(
            sweep_paste_dirs(&tmp, 100, later, DAY),
            vec![foreign.clone()]
        );
        assert!(!foreign.exists());
        assert!(own.exists() && not_ours.exists() && unrelated.exists());

        std::fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn the_sweep_copes_with_a_missing_temporary_directory() {
        let missing = std::env::temp_dir().join("stoat-gpui-draft-does-not-exist");
        assert!(sweep_paste_dirs(&missing, 1, SystemTime::now(), DAY).is_empty());
    }

    #[test]
    fn only_temporary_files_are_removed() {
        let dir = temp_dir("remove");
        let pasted = save_pasted_image(&dir, "png", b"not really a png").unwrap();
        assert!(pasted.temporary);
        let path = dir.join("mine.txt");
        std::fs::write(&path, b"keep me").unwrap();
        let mine = DraftAttachment::from_path(path.clone(), 7);
        assert!(!mine.temporary);

        remove_temporary(&mine);
        assert!(path.exists());
        remove_temporary(&pasted);
        assert!(!pasted.path.exists());
        // Already gone: fine.
        remove_temporary(&pasted);

        std::fs::remove_dir_all(dir).unwrap();
    }
}
