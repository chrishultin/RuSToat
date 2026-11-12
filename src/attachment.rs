//! How a message's attachments are shown: images inline at a fixed display
//! size, everything else as a file card. Plain data with no GPUI, so the
//! sizing maths and the size guard are unit-testable.
//!
//! The file server only serves originals (it ignores resize parameters), so
//! every image arrives at full size and is scaled down for display. The
//! display size comes from the upload's metadata, so a row's height is known
//! before the bytes arrive and never changes when they do.

use std::sync::Arc;

use crate::api::models::{File, Metadata};
use crate::file_actions::{FileTarget, display_name, is_spoiler};
use crate::media::MediaUrls;

/// Inline images fit within this box (logical px), keeping their aspect ratio.
pub const MAX_IMAGE_WIDTH: u32 = 400;
pub const MAX_IMAGE_HEIGHT: u32 = 300;

/// The box for an image whose dimensions are missing or zero. The image is
/// drawn inside it, letterboxed.
pub const FALLBACK_IMAGE_SIZE: (u32, u32) = (300, 200);

/// Images bigger than this aren't downloaded until the user asks: there's no
/// server-side resizing, so they'd arrive in full.
pub const MAX_AUTO_LOAD_BYTES: u64 = 8 * 1024 * 1024;

/// Text files up to this size (bytes) are previewed as soon as their row is
/// built; bigger ones on "Load file". A preview never holds more than this.
pub const TEXT_AUTO_LOAD: u64 = 50_000;

/// How fetching a text file's preview went. The store keeps these by file
/// ID, for the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextLoad {
    Loading,
    Loaded(Arc<str>),
    Failed,
}

/// What a text file shows under its card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPreview {
    /// Too big to load at once: "Load file ({size})".
    NotLoaded,
    Loading,
    Loaded(Arc<str>),
    /// "Couldn't load the file.", with Try again.
    Failed,
    /// A spoiler not shown yet: "Spoiler: click to show".
    Hidden,
}

/// The preview for `file`, if it's a text file (by its metadata): hidden
/// while it's a spoiler not `shown` yet, else as far as `load` has got. A
/// small file not asked for yet is about to be: it's loading.
pub fn text_preview(file: &File, shown: bool, load: Option<&TextLoad>) -> Option<TextPreview> {
    if !matches!(file.metadata, Metadata::Text) {
        return None;
    }
    if is_spoiler(&file.filename) && !shown {
        return Some(TextPreview::Hidden);
    }
    Some(match load {
        Some(TextLoad::Loading) => TextPreview::Loading,
        Some(TextLoad::Loaded(text)) => TextPreview::Loaded(text.clone()),
        Some(TextLoad::Failed) => TextPreview::Failed,
        None if loads_at_once(file) => TextPreview::Loading,
        None => TextPreview::NotLoaded,
    })
}

/// A text file small enough to preview without being asked.
pub fn loads_at_once(file: &File) -> bool {
    u64::try_from(file.size).is_ok_and(|size| size <= TEXT_AUTO_LOAD)
}

/// A downloaded preview as text: UTF-8 (anything else replaced), from at
/// most [`TEXT_AUTO_LOAD`] bytes even if the file was longer than it said,
/// with Windows line ends and tabs (which don't draw) made plain.
pub fn preview_text(mut bytes: Vec<u8>) -> Arc<str> {
    bytes.truncate(TEXT_AUTO_LOAD as usize);
    let text = String::from_utf8_lossy(&bytes)
        .replace("\r\n", "\n")
        .replace('\t', "    ");
    Arc::from(text)
}

/// What a file card's icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Video,
    Audio,
    Text,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentView {
    /// Drawn inline at exactly `width` × `height`.
    Image {
        /// The file's ID, for "Show image".
        id: String,
        url: String,
        name: String,
        size_label: String,
        width: u32,
        height: u32,
        /// The upload's own size from its metadata (zero if unknown), for the
        /// full-size viewer.
        original: (usize, usize),
        /// The upload's ThumbHash, for a blurred preview while it loads.
        thumbhash: Option<Arc<[u8]>>,
        /// Over [`MAX_AUTO_LOAD_BYTES`] and not revealed yet: drawn as a card
        /// with a "Show image" button instead.
        too_big: bool,
        /// A spoiler not shown yet: drawn covered, at the same size, until
        /// it's clicked.
        spoiler: bool,
    },
    /// A card (icon, name, size) that opens the file in the browser.
    File {
        /// The file's ID, for its preview and reveal.
        id: String,
        name: String,
        size_label: String,
        /// `None` until the file server is known.
        url: Option<String>,
        kind: FileKind,
        /// A spoiler: its card has a tag.
        spoiler: bool,
        /// A text file's preview, under the card (see [`text_preview`]).
        preview: Option<TextPreview>,
    },
    /// A file of a message that's still being sent, drawn from the local
    /// copy with how much of it has uploaded.
    Local {
        name: String,
        size_label: String,
        kind: FileKind,
        /// For images: where to draw it from, at this display size (the
        /// size the sent message will have, when known).
        image: Option<(std::path::PathBuf, u32, u32)>,
        /// Uploaded so far, in thousandths.
        progress: u16,
        /// Marked as a spoiler: an image is drawn covered, as the sent one
        /// will be.
        spoiler: bool,
    },
}

impl AttachmentView {
    /// `revealed`: the user asked to load this image despite its size. A
    /// spoiler image starts covered (see [`Self::uncovered`]). Names are
    /// shown without a spoiler prefix.
    pub fn new(file: &File, media: &MediaUrls, revealed: bool) -> Self {
        let spoiler = is_spoiler(&file.filename);
        let name = display_name(&file.filename).to_string();
        let size = u64::try_from(file.size).unwrap_or(0);
        let size_label = size_label(size);
        let url = media.file(file);
        match (&file.metadata, url) {
            (
                Metadata::Image {
                    width,
                    height,
                    thumbhash,
                    ..
                },
                Some(url),
            ) => {
                let original = (*width, *height);
                let (width, height) = fit_within(*width, *height);
                AttachmentView::Image {
                    id: file.id.clone(),
                    url,
                    name,
                    size_label,
                    width,
                    height,
                    original,
                    thumbhash: thumbhash.as_deref().map(Arc::from),
                    too_big: size > MAX_AUTO_LOAD_BYTES && !revealed,
                    spoiler,
                }
            }
            (metadata, url) => AttachmentView::File {
                id: file.id.clone(),
                name,
                size_label,
                url,
                kind: file_kind(metadata, &file.content_type),
                spoiler,
                preview: None,
            },
        }
    }

    /// A text file's preview (see [`text_preview`]).
    pub fn with_preview(mut self, text: Option<TextPreview>) -> Self {
        if let AttachmentView::File { preview, .. } = &mut self {
            *preview = text;
        }
        self
    }

    /// With `shown` (the user clicked it), a spoiler image isn't covered.
    pub fn uncovered(mut self, shown: bool) -> Self {
        if let AttachmentView::Image { spoiler, .. } = &mut self {
            *spoiler &= !shown;
        }
        self
    }
}

impl AttachmentView {
    /// What Save…, Copy image, Copy link and Open in browser act on: the
    /// file server's original. `None` while it's still uploading, or before
    /// the file server is known.
    pub fn file_target(&self) -> Option<FileTarget> {
        let (name, url, picture) = match self {
            AttachmentView::Image { name, url, .. } => (name, url, true),
            AttachmentView::File {
                name,
                url: Some(url),
                ..
            } => (name, url, false),
            AttachmentView::File { url: None, .. } | AttachmentView::Local { .. } => return None,
        };
        Some(FileTarget {
            name: name.clone(),
            url: url.clone(),
            link: url.clone(),
            page: None,
            picture,
        })
    }
}

/// The display size for an image of `width` × `height`: scaled down to fit
/// within [`MAX_IMAGE_WIDTH`] × [`MAX_IMAGE_HEIGHT`], never scaled up, and
/// rounded to whole pixels (at least 1). Missing or zero dimensions get
/// [`FALLBACK_IMAGE_SIZE`].
pub fn fit_within(width: usize, height: usize) -> (u32, u32) {
    fit_into(width, height, MAX_IMAGE_WIDTH, MAX_IMAGE_HEIGHT).unwrap_or(FALLBACK_IMAGE_SIZE)
}

/// `width` × `height` scaled down to fit within `max_width` × `max_height`
/// (keeping its shape), never scaled up, and rounded to whole pixels (at
/// least 1). `None` when a dimension is missing or zero.
pub fn fit_into(
    width: usize,
    height: usize,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let (w, h) = (width as f64, height as f64);
    let scale = (f64::from(max_width) / w)
        .min(f64::from(max_height) / h)
        .min(1.0);
    let side = |v: f64| ((v * scale).round() as u32).max(1);
    Some((side(w), side(h)))
}

/// An image file's width and height, read from its header (without
/// decoding it). `None` if it isn't an image this build can read.
pub fn local_image_size(path: &std::path::Path) -> Option<(u32, u32)> {
    image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// The card icon for a file, from its metadata, falling back to its MIME type.
pub fn file_kind(metadata: &Metadata, content_type: &str) -> FileKind {
    match metadata {
        Metadata::Image { .. } => FileKind::Image,
        Metadata::Video { .. } => FileKind::Video,
        Metadata::Audio => FileKind::Audio,
        Metadata::Text => FileKind::Text,
        Metadata::File => match content_type.split('/').next() {
            Some("image") => FileKind::Image,
            Some("video") => FileKind::Video,
            Some("audio") => FileKind::Audio,
            Some("text") => FileKind::Text,
            _ => FileKind::File,
        },
    }
}

/// A human-readable file size: `512 B`, `1.2 KB`, `3.4 MB`, `1.0 GB`
/// (1024-based, one decimal place above bytes).
pub fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    // Move up a unit when one decimal place would round to 1024 ("1024.0 KB").
    while value >= 1023.95 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
