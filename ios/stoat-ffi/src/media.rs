//! Pictures' blurred previews. Stoat keeps a ThumbHash (about 25 bytes) in
//! each uploaded image's metadata; decoded, it's a tiny picture that looks
//! like a blurred copy once stretched to the display size. Decoded here with
//! the crate desktop uses, so Swift needn't implement the format again.

/// A decoded ThumbHash: about 32 px on its long side.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ThumbImage {
    pub width: u32,
    pub height: u32,
    /// `width` × `height` pixels, 4 bytes each (RGBA, not premultiplied).
    pub rgba: Vec<u8>,
}

/// The preview `hash` stands for, or `None` if it doesn't decode.
#[uniffi::export]
pub fn thumbhash_image(hash: Vec<u8>) -> Option<ThumbImage> {
    let (width, height, rgba) = thumbhash::thumb_hash_to_rgba(&hash).ok()?;
    Some(ThumbImage {
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
        rgba,
    })
}
