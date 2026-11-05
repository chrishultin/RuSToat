//! Blurred previews of uploaded images while the real ones download. Stoat
//! stores a ThumbHash (a ~25-byte summary of the picture) in each image's
//! metadata; decoded, it's a tiny image that looks like a blurred copy once
//! stretched to the display size. This is what Stoat's web client shows too.
//!
//! Decoding goes through GPUI's asset cache, keyed by the hash, so each one
//! is decoded once however often its row is drawn.

use std::sync::Arc;

use gpui_kit::{App, Asset, ImageCacheError, ImageSource, Img, RenderImage, Window, img};
use image::{Frame, RgbaImage};

/// The decoded preview for a hash (`None` if the hash doesn't decode).
pub struct Thumbhash;

impl Asset for Thumbhash {
    type Source = Arc<[u8]>;
    type Output = Option<Arc<RenderImage>>;

    // Not `async fn`: that would borrow `cx` into the future, and assets'
    // futures must be `'static`.
    #[allow(clippy::manual_async_fn)]
    fn load(
        source: Self::Source,
        _cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        async move { decode(&source).map(Arc::new) }
    }
}

/// The preview as GPUI draws it: BGRA pixels, about 32 px on the long side.
pub fn decode(hash: &[u8]) -> Option<RenderImage> {
    let (width, height, mut pixels) = thumbhash::thumb_hash_to_rgba(hash).ok()?;
    // GPUI's image buffers are BGRA.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = RgbaImage::from_raw(width as u32, height as u32, pixels)?;
    Some(RenderImage::new(vec![Frame::new(buffer)]))
}

/// An image element showing the preview for `hash`. Callers size it (it's
/// stretched to fill) and add a fallback for hashes that don't decode.
pub fn preview(hash: Arc<[u8]>) -> Img {
    img(ImageSource::Custom(Arc::new(
        move |window: &mut Window, cx: &mut App| match window.use_asset::<Thumbhash>(&hash, cx)? {
            Some(image) => Some(Ok(image)),
            None => Some(Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                "thumbhash didn't decode"
            ))))),
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::decode;

    /// A 4×2 image: the left half red, the right half blue.
    fn red_and_blue() -> Vec<u8> {
        let mut rgba = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                rgba.extend_from_slice(if x < 2 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
        }
        rgba
    }

    #[test]
    fn hashes_decode_to_bgra_previews_with_the_right_shape() {
        let hash = thumbhash::rgba_to_thumb_hash(4, 2, &red_and_blue());
        let image = decode(&hash).expect("decodes");
        let size = image.size(0);
        // Landscape, about 2:1.
        assert!(size.width.0 > size.height.0, "{size:?}");
        let bytes = image.as_bytes(0).unwrap();
        let width = size.width.0 as usize;
        let pixel = |x: usize, y: usize| &bytes[(y * width + x) * 4..][..4];
        // Left edge is red, stored BGRA: blue byte low, red byte high.
        let left = pixel(0, 0);
        assert!(left[2] > 150 && left[0] < 100, "{left:?}");
        // Right edge is blue: the other way round.
        let right = pixel(width - 1, 0);
        assert!(right[0] > 150 && right[2] < 100, "{right:?}");
    }

    #[test]
    fn junk_never_panics() {
        assert!(decode(&[]).is_none());
        let hash = thumbhash::rgba_to_thumb_hash(4, 2, &red_and_blue());
        // Every truncation of a real hash, and some arbitrary bytes.
        for len in 0..hash.len() {
            let _ = decode(&hash[..len]);
        }
        for seed in 0..=255u8 {
            let junk: Vec<u8> = (0..24)
                .map(|i| seed.wrapping_mul(31).wrapping_add(i * 7))
                .collect();
            let _ = decode(&junk);
        }
    }
}
