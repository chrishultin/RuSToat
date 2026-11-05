//! How a message's attachments are shown: images inline at a fixed display
//! size, everything else as a file card. Plain data with no GPUI, so the
//! sizing maths and the size guard are unit-testable.
//!
//! The file server only serves originals (it ignores resize parameters), so
//! every image arrives at full size and is scaled down for display. The
//! display size comes from the upload's metadata, so a row's height is known
//! before the bytes arrive and never changes when they do.

use crate::api::models::{File, Metadata};
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
        /// Over [`MAX_AUTO_LOAD_BYTES`] and not revealed yet: drawn as a card
        /// with a "Show image" button instead.
        too_big: bool,
    },
    /// A card (icon, name, size) that opens the file in the browser.
    File {
        name: String,
        size_label: String,
        /// `None` until the file server is known.
        url: Option<String>,
        kind: FileKind,
    },
}

impl AttachmentView {
    /// `revealed`: the user asked to load this image despite its size.
    pub fn new(file: &File, media: &MediaUrls, revealed: bool) -> Self {
        let size = u64::try_from(file.size).unwrap_or(0);
        let size_label = size_label(size);
        let url = media.file(file);
        match (&file.metadata, url) {
            (Metadata::Image { width, height, .. }, Some(url)) => {
                let original = (*width, *height);
                let (width, height) = fit_within(*width, *height);
                AttachmentView::Image {
                    id: file.id.clone(),
                    url,
                    name: file.filename.clone(),
                    size_label,
                    width,
                    height,
                    original,
                    too_big: size > MAX_AUTO_LOAD_BYTES && !revealed,
                }
            }
            (metadata, url) => AttachmentView::File {
                name: file.filename.clone(),
                size_label,
                url,
                kind: file_kind(metadata, &file.content_type),
            },
        }
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
