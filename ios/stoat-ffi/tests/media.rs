//! Attachments and embeds through `AccountController`, with every field
//! desktop draws: images (with their ThumbHash, the 8 MB guard and spoiler
//! covers), files, audio, video, and each kind of embed. Against a fake
//! Stoat serving `messages_media.json`.

mod support;

use std::sync::Arc;

use stoat_ffi::controller::AccountController;
use stoat_ffi::media::thumbhash_image;
use stoat_ffi::timeline::{
    AttachmentItem, EmbedImageItem, EmbedItem, FileKindItem, ImageAttachment, MessageItem,
    TimelineItem,
};
use support::{Events, Recorder, api, connected, fixture};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CDN: &str = "https://cdn.stoatusercontent.com";
const PROXY: &str = "https://proxy.stoatusercontent.com";
const LANDSCAPE_HASH: [u8; 21] = [
    87, 23, 10, 45, 136, 118, 136, 135, 140, 120, 135, 128, 120, 168, 119, 136, 118, 128, 112, 119,
    8,
];

/// Connected, with #chat open on the media fixture, once the file server
/// is known (until then images have no address, and show as cards).
async fn opened(api: &MockServer) -> (Arc<AccountController>, Recorder) {
    Mock::given(method("GET"))
        .and(path("/channels/ch-beta/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("messages_media.json")))
        .mount(api)
        .await;
    let (controller, recorder) = connected(api).await;
    controller.open_channel("ch-beta".into()).await.unwrap();
    for _ in 0..500 {
        if matches!(
            message(&controller, "M01").attachments.first(),
            Some(AttachmentItem::Image { .. })
        ) {
            return (controller, recorder);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the file server never became known");
}

/// Message `01JE…{suffix}` in #chat.
fn message(controller: &AccountController, suffix: &str) -> MessageItem {
    let id = format!("01JE0000000000000000000{suffix}");
    controller
        .timeline("ch-beta".into())
        .into_iter()
        .find_map(|row| match row {
            TimelineItem::Message { message } if message.id == id => Some(message),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no message {id}"))
}

fn image(attachment: &AttachmentItem) -> &ImageAttachment {
    match attachment {
        AttachmentItem::Image { image } => image,
        other => panic!("not an image: {other:?}"),
    }
}

fn proxied(url: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
    format!("{PROXY}/proxy?url={encoded}")
}

#[tokio::test]
async fn images_carry_their_display_size_and_thumbhash() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    let row = message(&controller, "M01");
    assert_eq!(
        image(&row.attachments[0]),
        &ImageAttachment {
            id: "f-landscape".into(),
            url: format!("{CDN}/attachments/f-landscape"),
            name: "landscape.jpg".into(),
            size_label: "24.2 KB".into(),
            // 1200×800 fitted within desktop's 400×300 box.
            width: 400,
            height: 267,
            original_width: 1200,
            original_height: 800,
            thumbhash: Some(LANDSCAPE_HASH.to_vec()),
            too_big: false,
            spoiler: false,
        }
    );
    let portrait = image(&row.attachments[1]);
    assert_eq!((portrait.width, portrait.height), (200, 300));
}

#[tokio::test]
async fn big_images_wait_for_show_image() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, recorder) = opened(&api).await;
    let huge = image(&message(&controller, "M02").attachments[0]).clone();
    assert!(huge.too_big, "9.1 MB is over the 8 MB guard");
    assert_eq!(huge.size_label, "9.1 MB");

    recorder.clear();
    controller.reveal_image("ch-beta".into(), "f-huge".into());
    assert!(!image(&message(&controller, "M02").attachments[0]).too_big);
    assert!(
        recorder
            .heard()
            .iter()
            .any(|h| matches!(h, support::Heard::Changed(c) if c.timelines.contains(&"ch-beta".to_string()))),
        "Swift is told to redraw #chat: {:?}",
        recorder.heard()
    );
}

#[tokio::test]
async fn spoiler_images_stay_covered_until_tapped() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    let covered = image(&message(&controller, "M03").attachments[0]).clone();
    assert!(covered.spoiler);
    assert_eq!(covered.name, "landscape.jpg", "shown without its spoiler_ prefix");
    assert_eq!((covered.width, covered.height), (400, 267), "covered at its own shape");

    controller.reveal_attachment("ch-beta".into(), "f-spoiler".into());
    assert!(!image(&message(&controller, "M03").attachments[0]).spoiler);
}

#[tokio::test]
async fn files_audio_and_video_carry_what_their_players_need() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    let row = message(&controller, "M04");
    let [file, audio, video] = row.attachments.as_slice() else {
        panic!("{:?}", row.attachments);
    };
    let AttachmentItem::File { file } = file else {
        panic!("{file:?}")
    };
    assert_eq!(file.kind, FileKindItem::File);
    assert_eq!(file.name, "notes.pdf");
    assert_eq!(file.url.as_deref(), Some(&*format!("{CDN}/attachments/f-notes")));
    let AttachmentItem::Audio { audio } = audio else {
        panic!("{audio:?}")
    };
    // The original by name, which AVPlayer streams without a redirect.
    assert_eq!(audio.url, format!("{CDN}/attachments/f-voice/voice.m4a"));
    assert_eq!(audio.id, "f-voice");
    let AttachmentItem::Video { video } = video else {
        panic!("{video:?}")
    };
    assert_eq!(video.url, format!("{CDN}/attachments/f-clip/clip.mp4"));
    assert_eq!((video.width, video.height), (320, 180), "small enough as it is");
    assert_eq!((video.original_width, video.original_height), (320, 180));
    assert!(!video.spoiler);
}

#[tokio::test]
async fn website_embeds_become_cards_with_every_field() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    let row = message(&controller, "M05");
    let [article, youtube, picture] = row.embeds.as_slice() else {
        panic!("{:?}", row.embeds);
    };
    let EmbedItem::Card { card } = article else {
        panic!("{article:?}")
    };
    assert_eq!(card.colour, Some(0xff8800ff));
    assert_eq!(card.icon, Some(proxied("https://news.example/favicon.png")));
    assert_eq!(card.site_name.as_deref(), Some("News"));
    assert_eq!(card.link.as_deref(), Some("https://news.example/story"));
    assert_eq!(
        card.description.as_deref(),
        Some("A five-year study of evening skies finds more orange than ever.")
    );
    assert!(card.rich_description.is_empty(), "a website's is plain");
    assert_eq!(
        card.image,
        Some(EmbedImageItem {
            url: proxied("https://news.example/landscape.jpg"),
            source: "https://news.example/landscape.jpg".into(),
            width: 400,
            height: 267,
            original_width: 1200,
            original_height: 800,
        })
    );
    assert_eq!(card.thumbnail, None);
    assert!(!card.playable);

    let EmbedItem::Card { card } = youtube else {
        panic!("{youtube:?}")
    };
    assert!(card.playable, "a video's picture gets a play badge");
    assert_eq!(card.link.as_deref(), Some("https://www.youtube.com/watch?v=abc"));

    let EmbedItem::Image { image } = picture else {
        panic!("{picture:?}")
    };
    assert_eq!(image.url, proxied("https://pics.example/portrait.jpg"));
    assert_eq!((image.width, image.height), (200, 300));
}

#[tokio::test]
async fn bot_embeds_gif_posters_videos_and_thumbnails_map_across() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    let row = message(&controller, "M06");
    let [bot, gif, video, repo] = row.embeds.as_slice() else {
        panic!("{:?}", row.embeds);
    };
    let EmbedItem::Card { card } = bot else {
        panic!("{bot:?}")
    };
    assert_eq!(card.colour, Some(0x2ecc71ff));
    assert!(!card.rich_description.is_empty(), "a bot's description is Markdown");
    let media = card.media.as_ref().expect("the bot's uploaded file");
    assert_eq!(image(media).id, "f-badge");

    let EmbedItem::GifPoster { image, link } = gif else {
        panic!("{gif:?}")
    };
    assert_eq!(link, "https://gifbox.me/view/abc123");
    assert_eq!(
        image.url,
        proxied("https://media.gifbox.me/media/post/abc123/poster")
    );
    assert_eq!((image.width, image.height), (320, 240));

    assert_eq!(
        video,
        &EmbedItem::Video {
            url: "https://videos.example/clip.mp4".into()
        }
    );

    let EmbedItem::Card { card } = repo else {
        panic!("{repo:?}")
    };
    let thumbnail = card.thumbnail.as_ref().expect("a small picture");
    assert_eq!((thumbnail.width, thumbnail.height), (80, 40));
    assert_eq!(card.image, None);
}

#[test]
fn thumbhashes_decode_to_small_rgba_pictures() {
    let picture = thumbhash_image(LANDSCAPE_HASH.to_vec()).expect("decodes");
    // 32 px on the long side, and roughly 3:2 (a ThumbHash keeps only an
    // approximate shape: Swift stretches it to the display size anyway).
    assert_eq!((picture.width, picture.height), (32, 23));
    assert_eq!(picture.rgba.len(), 32 * 23 * 4);
    // Opaque, and the sky at the top is bluer than the hills at the bottom.
    assert!(picture.rgba.chunks(4).all(|p| p[3] == 255));
    let top = &picture.rgba[..4];
    let bottom = &picture.rgba[picture.rgba.len() - 4..];
    assert!(top[2] > bottom[2], "{top:?} {bottom:?}");
}

#[test]
fn junk_hashes_decode_to_nothing() {
    assert_eq!(thumbhash_image(Vec::new()), None);
    for len in 0..LANDSCAPE_HASH.len() {
        // Never panics, whatever it gives.
        let _ = thumbhash_image(LANDSCAPE_HASH[..len].to_vec());
    }
}

#[tokio::test]
async fn gifs_come_through_as_images_and_gif_links_as_picture_embeds() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    let row = message(&controller, "M08");
    let gif = image(&row.attachments[0]);
    assert_eq!(gif.name, "orbit.gif");
    assert_eq!((gif.width, gif.height), (160, 120));
    // A GIF link is just its picture: Swift animates it, by its bytes.
    assert_eq!(
        row.embeds,
        vec![EmbedItem::Image {
            image: EmbedImageItem {
                url: proxied("https://gifs.example/orbit.gif"),
                source: "https://gifs.example/orbit.gif".into(),
                width: 160,
                height: 120,
                original_width: 160,
                original_height: 120,
            }
        }]
    );
}
