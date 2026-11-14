//! How link previews and bot embeds are shown. Plain data with no GPUI, so
//! the mapping from `revolt-models` embeds is unit-testable.
//!
//! Every remote picture (preview images, site icons) goes through Stoat's
//! media proxy ([`MediaUrls::external`]), so the sites never see the user's
//! IP. Without a proxy, they're left out. Links that are clicked open in the
//! browser directly, like links in message text.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::api::models::{Embed, Image, ImageSize, Special, Text, Video, WebsiteMetadata};
use crate::attachment::{AttachmentView, FALLBACK_IMAGE_SIZE, fit_into};
use crate::markdown::Names;
use crate::media::MediaUrls;

/// Large embed images fit within this box (logical px), like attachments.
pub const EMBED_IMAGE_WIDTH: u32 = 400;
pub const EMBED_IMAGE_HEIGHT: u32 = 300;
/// Small preview images, beside the text.
pub const EMBED_THUMBNAIL_SIZE: u32 = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedView {
    /// A link preview or bot embed: a card with a colour bar.
    Card(Box<EmbedCard>),
    /// Just a picture: a link to an image, or a GIF.
    Image(EmbedImage),
    /// Just a video link. There's no in-app playback, so it's a card that
    /// opens it in the browser.
    Video { url: String },
    /// A Gifbox GIF that only exists as video: its still (proxied), with a
    /// "GIF" badge. Clicking it opens `link`, the GIF's page.
    GifPoster { image: EmbedImage, link: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedCard {
    /// The colour bar, `0xRRGGBBAA`. `None` uses the theme's border colour.
    pub colour: Option<u32>,
    /// The site's icon (proxied).
    pub icon: Option<String>,
    pub site_name: Option<String>,
    pub title: Option<String>,
    /// Where the title links to (http(s) only).
    pub link: Option<String>,
    pub description: Option<Description>,
    /// A small picture beside the text (`ImageSize::Preview`).
    pub thumbnail: Option<EmbedImage>,
    /// A large picture under the text (`ImageSize::Large`).
    pub image: Option<EmbedImage>,
    /// The picture stands for a video (YouTube and the like): it gets a play
    /// badge, and clicking it opens `link`.
    pub playable: bool,
    /// A bot embed's uploaded file.
    pub media: Option<AttachmentView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Description {
    /// Website descriptions: shown as written.
    Plain(String),
    /// Bot embed descriptions are Markdown (see [`crate::markdown::prepare`]).
    /// `render_key` changes with the text, like a message's.
    Markdown { source: String, render_key: String },
}

/// A remote picture at its display size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedImage {
    /// Proxied.
    pub url: String,
    /// The picture's own address (not proxied): what Copy link copies.
    pub source: String,
    pub width: u32,
    pub height: u32,
    /// As reported by the server (zero if unknown), for the full-size viewer.
    pub original: (usize, usize),
}

impl EmbedView {
    /// `None` for embeds with nothing to show. `key` identifies the embed
    /// within the timeline (message ID and position), for Markdown state.
    /// `revealed`: large images the user chose to load (by file ID).
    /// `shown`: spoilers the user clicked to see (by file ID).
    pub fn new(
        embed: &Embed,
        media: &MediaUrls,
        names: &impl Names,
        revealed: &HashSet<String>,
        shown: &HashSet<String>,
        key: &str,
    ) -> Option<Self> {
        match embed {
            Embed::Website(site) => website(site, media),
            Embed::Image(image) => {
                EmbedImage::new(image, media, EMBED_IMAGE_WIDTH, EMBED_IMAGE_HEIGHT)
                    .map(EmbedView::Image)
            }
            Embed::Video(video) => gif_poster(video, media)
                .or_else(|| link(&video.url).map(|url| EmbedView::Video { url })),
            Embed::Text(text) => text_embed(text, media, names, revealed, shown, key),
            Embed::None => None,
        }
    }
}

impl EmbedImage {
    fn new(image: &Image, media: &MediaUrls, max_width: u32, max_height: u32) -> Option<Self> {
        let url = media.external(&image.url)?;
        let (width, height) = fit_into(image.width, image.height, max_width, max_height)
            .unwrap_or_else(|| {
                let (w, h) = FALLBACK_IMAGE_SIZE;
                (w.min(max_width), h.min(max_height))
            });
        Some(EmbedImage {
            url,
            source: image.url.clone(),
            width,
            height,
            original: (image.width, image.height),
        })
    }
}

/// The ID in a Gifbox GIF's page URL (`https://gifbox.me/view/{id}`), if
/// that's what `url` is. Only that exact host: not lookalikes.
pub fn gifbox_id(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://gifbox.me/view/")?;
    let id = rest.split(['?', '#']).next()?.trim_end_matches('/');
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    valid.then_some(id)
}

/// A video embed of a Gifbox GIF with no GIF version (Stoat's embed service
/// makes one an image when there is): its still instead, at the video's
/// size. `None` for other videos, or with no proxy to load the still.
fn gif_poster(video: &Video, media: &MediaUrls) -> Option<EmbedView> {
    let id = gifbox_id(&video.url)?;
    let poster = Image {
        url: format!("https://media.gifbox.me/media/post/{id}/poster"),
        width: video.width,
        height: video.height,
        size: ImageSize::Large,
    };
    let image = EmbedImage::new(&poster, media, EMBED_IMAGE_WIDTH, EMBED_IMAGE_HEIGHT)?;
    Some(EmbedView::GifPoster {
        image,
        link: video.url.clone(),
    })
}

fn website(site: &WebsiteMetadata, media: &MediaUrls) -> Option<EmbedView> {
    // A Gifbox GIF (Stoat 0.16.4's own kind) shows as its poster, linking
    // to its Gifbox page, like a Gifbox video embed.
    if let Some(Special::Gifbox { poster_url }) = &site.special
        && let Some(video) = &site.video
    {
        let page = site.url.clone().unwrap_or_else(|| video.url.clone());
        let poster = poster_url.as_ref().and_then(|url| {
            let poster = Image {
                url: url.clone(),
                width: video.width,
                height: video.height,
                size: ImageSize::Large,
            };
            EmbedImage::new(&poster, media, EMBED_IMAGE_WIDTH, EMBED_IMAGE_HEIGHT)
        });
        // Without one, the still Gifbox keeps for the page's GIF.
        let page_video = Video {
            url: page.clone(),
            width: video.width,
            height: video.height,
        };
        return match poster {
            Some(image) => Some(EmbedView::GifPoster { image, link: page }),
            None => gif_poster(&page_video, media)
                .or_else(|| link(&video.url).map(|url| EmbedView::Video { url })),
        };
    }
    // A GIF link shows as the GIF alone (the image, else the video).
    if matches!(site.special, Some(Special::GIF)) {
        if let Some(image) = &site.image {
            return EmbedImage::new(image, media, EMBED_IMAGE_WIDTH, EMBED_IMAGE_HEIGHT)
                .map(EmbedView::Image);
        }
        if let Some(video) = &site.video {
            return link(&video.url).map(|url| EmbedView::Video { url });
        }
    }

    let picture = site.image.as_ref().and_then(|image| match image.size {
        ImageSize::Preview => {
            EmbedImage::new(image, media, EMBED_THUMBNAIL_SIZE, EMBED_THUMBNAIL_SIZE)
                .map(|i| (i, false))
        }
        ImageSize::Large => {
            EmbedImage::new(image, media, EMBED_IMAGE_WIDTH, EMBED_IMAGE_HEIGHT).map(|i| (i, true))
        }
    });
    let (thumbnail, image) = match picture {
        Some((picture, true)) => (None, Some(picture)),
        Some((picture, false)) => (Some(picture), None),
        None => (None, None),
    };
    let playable = image.is_some()
        && (site.video.is_some()
            || matches!(
                site.special,
                Some(
                    Special::YouTube { .. }
                        | Special::Twitch { .. }
                        | Special::Lightspeed { .. }
                        | Special::Streamable { .. }
                )
            ));

    let card = EmbedCard {
        colour: site.colour.as_deref().and_then(crate::math::parse_color),
        icon: site.icon_url.as_deref().and_then(|url| media.external(url)),
        site_name: non_empty(&site.site_name),
        title: non_empty(&site.title),
        link: site
            .url
            .as_deref()
            .or(site.original_url.as_deref())
            .and_then(link),
        description: non_empty(&site.description).map(Description::Plain),
        thumbnail,
        image,
        playable,
        media: None,
    };
    has_content(&card).then(|| EmbedView::Card(Box::new(card)))
}

fn text_embed(
    text: &Text,
    media: &MediaUrls,
    names: &impl Names,
    revealed: &HashSet<String>,
    shown: &HashSet<String>,
    key: &str,
) -> Option<EmbedView> {
    let description = non_empty(&text.description).map(|source| {
        let source = crate::markdown::prepare(&source, names);
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let render_key = format!("{key}:{:016x}", hasher.finish());
        Description::Markdown { source, render_key }
    });
    let card = EmbedCard {
        colour: text.colour.as_deref().and_then(crate::math::parse_color),
        icon: text.icon_url.as_deref().and_then(|url| media.external(url)),
        site_name: None,
        title: non_empty(&text.title),
        link: text.url.as_deref().and_then(link),
        description,
        thumbnail: None,
        image: None,
        playable: false,
        media: text.media.as_ref().map(|file| {
            AttachmentView::new(file, media, revealed.contains(&file.id))
                .uncovered(shown.contains(&file.id))
        }),
    };
    has_content(&card).then(|| EmbedView::Card(Box::new(card)))
}

/// Something to show beyond a colour bar.
fn has_content(card: &EmbedCard) -> bool {
    card.site_name.is_some()
        || card.title.is_some()
        || card.description.is_some()
        || card.thumbnail.is_some()
        || card.image.is_some()
        || card.media.is_some()
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A link that's safe to open: http(s) only.
fn link(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::gifbox_id;

    #[test]
    fn gifbox_ids_come_from_gifbox_pages_only() {
        assert_eq!(
            gifbox_id("https://gifbox.me/view/smGy30kWfDtNbJyWRnq8a"),
            Some("smGy30kWfDtNbJyWRnq8a")
        );
        assert_eq!(gifbox_id("https://gifbox.me/view/a-b_c/"), Some("a-b_c"));
        assert_eq!(gifbox_id("https://gifbox.me/view/abc?x=1"), Some("abc"));
        for url in [
            "https://gifbox.me.evil.com/view/abc",
            "https://notgifbox.me/view/abc",
            "https://gifbox.me@evil.com/view/abc",
            "http://gifbox.me/view/abc",
            "https://gifbox.me/view/",
            "https://gifbox.me/view/../../x",
            "https://gifbox.me/upload",
            "https://media.gifbox.me/media/post/abc/mp4",
        ] {
            assert_eq!(gifbox_id(url), None, "{url}");
        }
    }
}
