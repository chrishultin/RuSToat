//! Poster frames for video attachments: a frame from half a second in,
//! made by AVFoundation from the file server's copy (see
//! [`crate::media_player::poster`]). Stoat stores no thumbnail for videos.
//!
//! Making one reads part of the video, so it runs on the background
//! executor, through GPUI's asset cache: each video's poster is made once
//! however often its row is drawn.

use std::sync::Arc;

use gpui_kit::{App, Asset, ImageCacheError, ImageSource, Img, RenderImage, Window, img};
use image::{Frame, RgbaImage};

/// The poster for a video (`None` if one couldn't be made: a format
/// AVFoundation can't read, or the file failed to load).
pub struct VideoPoster;

/// Which video, and the most pixels its poster needs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PosterSource {
    pub url: String,
    pub max: (u32, u32),
}

impl Asset for VideoPoster {
    type Source = PosterSource;
    type Output = Option<Arc<RenderImage>>;

    // Not `async fn`: that would borrow `cx` into the future, and assets'
    // futures must be `'static`.
    #[allow(clippy::manual_async_fn)]
    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let executor = cx.background_executor().clone();
        async move {
            let made = executor
                .spawn(async move {
                    crate::media_player::poster(&source.url, source.max)
                        .map_err(|err| format!("no poster for {}: {err}", source.url))
                })
                .await;
            match made {
                Ok(frame) => Some(Arc::new(render_image(frame))),
                Err(err) => {
                    log::warn!("{err}");
                    None
                }
            }
        }
    }
}

/// A frame as GPUI draws it: BGRA.
pub fn render_image(mut frame: RgbaImage) -> RenderImage {
    let pixels: &mut [u8] = &mut frame;
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    RenderImage::new(vec![Frame::new(frame)])
}

/// An image element showing `url`'s poster, made at most `max` pixels.
/// Callers size it, and add what shows while it's made and if it can't be.
pub fn poster(url: String, max: (u32, u32)) -> Img {
    let source = PosterSource { url, max };
    img(ImageSource::Custom(Arc::new(
        move |window: &mut Window, cx: &mut App| match window
            .use_asset::<VideoPoster>(&source, cx)?
        {
            Some(image) => Some(Ok(image)),
            None => Some(Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                "no poster"
            ))))),
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::render_image;
    use image::RgbaImage;

    #[test]
    fn frames_become_bgra_at_their_size() {
        // 3×2, all red (RGBA).
        let frame = RgbaImage::from_fn(3, 2, |_, _| image::Rgba([255, 0, 0, 255]));
        let image = render_image(frame);
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (3, 2));
        let bytes = image.as_bytes(0).unwrap();
        // Stored BGRA: blue byte first, red third.
        assert_eq!(&bytes[..4], &[0, 0, 255, 255]);
    }
}
