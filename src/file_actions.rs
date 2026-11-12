//! What can be done with a file or picture in a message: Save…, Copy
//! image, Copy link and Open in browser. Plain data with no GPUI, so the
//! names, the choice of actions and the clipboard format are unit-testable.
//!
//! Attachments are downloaded from the file server's original. Embed
//! pictures come through the media proxy, but their link is their own
//! address (see [`crate::embed::EmbedImage`]).

use std::path::{Path, PathBuf};

use crate::api::ApiError;

/// Copy image downloads at most this much: anything bigger isn't a picture
/// worth putting on the clipboard.
pub const COPY_IMAGE_MAX_BYTES: u64 = 50 * 1024 * 1024;

/// A file or picture the actions apply to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTarget {
    /// As shown, and suggested when saving (see [`display_name`]).
    pub name: String,
    /// Where its bytes are downloaded from (for an embed, the proxy).
    pub url: String,
    /// What Copy link copies and Open in browser opens.
    pub link: String,
    /// A picture: it can be copied as an image.
    pub picture: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    Save,
    CopyImage,
    CopyLink,
    OpenInBrowser,
}

impl FileAction {
    pub fn label(self) -> &'static str {
        match self {
            FileAction::Save => "Save…",
            FileAction::CopyImage => "Copy image",
            FileAction::CopyLink => "Copy link",
            FileAction::OpenInBrowser => "Open in browser",
        }
    }
}

/// The actions `target` offers, in menu order. Copy image is for pictures only.
pub fn actions(target: &FileTarget) -> Vec<FileAction> {
    let mut actions = vec![FileAction::Save];
    if target.picture {
        actions.push(FileAction::CopyImage);
    }
    actions.extend([FileAction::CopyLink, FileAction::OpenInBrowser]);
    actions
}

/// The prefix that makes a file a spoiler, in any case (as Stoat's own
/// clients have it).
const SPOILER_PREFIX: &str = "spoiler_";

/// A file's name as shown: without a leading `spoiler_` (in any case), as
/// long as something is left.
pub fn display_name(filename: &str) -> &str {
    match filename.get(..SPOILER_PREFIX.len()) {
        Some(prefix)
            if prefix.eq_ignore_ascii_case(SPOILER_PREFIX)
                && filename.len() > SPOILER_PREFIX.len() =>
        {
            &filename[SPOILER_PREFIX.len()..]
        }
        _ => filename,
    }
}

/// The name the save panel suggests: the shown name, with characters macOS
/// won't take in a file name (`/`, `:`) replaced, and "file" if nothing's left.
pub fn suggested_save_name(name: &str) -> String {
    let name: String = display_name(name)
        .trim()
        .chars()
        .map(|c| match c {
            '/' | ':' | '\0' => '_',
            c => c,
        })
        .collect();
    let name = name.trim_start_matches('.');
    if name.is_empty() {
        "file".into()
    } else {
        name.into()
    }
}

/// Where the save panel starts: `~/Downloads`, else the home folder.
pub fn save_directory(home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return PathBuf::from("/");
    };
    let downloads = home.join("Downloads");
    if downloads.is_dir() {
        downloads
    } else {
        home.to_path_buf()
    }
}

/// How a picture goes on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardFormat {
    Png,
    Jpeg,
}

/// A downloaded picture as the clipboard takes it. PNG and JPEG go as they
/// are; anything else (GIF, WebP, …) is decoded and re-encoded as PNG, since
/// many macOS apps can't paste the others. An animated GIF gives its first
/// frame. The format comes from the bytes, not the server's word.
pub fn clipboard_image(bytes: Vec<u8>) -> Result<(ClipboardFormat, Vec<u8>), String> {
    use image::ImageFormat;

    let format = image::guess_format(&bytes).map_err(|_| "it isn't a picture".to_string())?;
    match format {
        ImageFormat::Png => Ok((ClipboardFormat::Png, bytes)),
        ImageFormat::Jpeg => Ok((ClipboardFormat::Jpeg, bytes)),
        format => {
            let picture = image::load_from_memory_with_format(&bytes, format)
                .map_err(|_| "it couldn't be read as a picture".to_string())?;
            let mut png = std::io::Cursor::new(Vec::new());
            picture
                .write_to(&mut png, ImageFormat::Png)
                .map_err(|e| format!("it couldn't be converted ({e})"))?;
            Ok((ClipboardFormat::Png, png.into_inner()))
        }
    }
}

/// Why a download failed, to finish "Couldn't save {name}: {reason}."
/// (without a full stop of its own).
pub fn download_failure(err: &ApiError) -> String {
    match err {
        ApiError::Unexpected { status: 404, .. } => "it's no longer there".into(),
        ApiError::Unexpected {
            status: 401 | 403, ..
        }
        | ApiError::Unauthorized => "the server refused it".into(),
        ApiError::Network(_) => "couldn't reach the server".into(),
        ApiError::Unexpected { status, .. } => format!("the server answered HTTP {status}"),
        other => format!("{other}").trim_end_matches('.').to_string(),
    }
}

pub fn saved_text(name: &str) -> String {
    format!("Saved {name}.")
}

pub fn save_failed_text(name: &str, reason: &str) -> String {
    format!("Couldn't save {name}: {reason}.")
}

pub fn copy_failed_text(reason: &str) -> String {
    format!("Couldn't copy the image: {reason}.")
}

pub const IMAGE_COPIED_TEXT: &str = "Image copied.";
pub const LINK_COPIED_TEXT: &str = "Link copied.";
pub const SHOW_IN_FINDER_TEXT: &str = "Show in Finder";
