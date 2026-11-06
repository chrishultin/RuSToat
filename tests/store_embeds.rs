//! Embeds: `MessageAppend` decoding, every `Embed` variant mapped to an
//! `EmbedView` (proxied pictures, colours, missing fields), and link previews
//! arriving live.

use std::collections::HashSet;

use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{BulkMessageResponse, Embed};
use stoat_gpui::attachment::AttachmentView;
use stoat_gpui::embed::{Description, EmbedCard, EmbedImage, EmbedView};
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::{ChatStore, RowOp, TimelineRow, diff_rows};

const CDN: &str = "https://cdn.example";
const PROXY: &str = "https://proxy.example";
const MSG: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";

fn media() -> MediaUrls {
    MediaUrls::new("https://api.example", Some(CDN.into()), Some(PROXY.into()))
}

fn proxied(url: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
    format!("{PROXY}/proxy?url={encoded}")
}

fn embed(value: Value) -> Embed {
    serde_json::from_value(value).unwrap()
}

fn view(value: Value) -> Option<EmbedView> {
    view_with(value, &media())
}

fn view_with(value: Value, media: &MediaUrls) -> Option<EmbedView> {
    let store = ChatStore::new("u-me");
    EmbedView::new(&embed(value), media, &store, &HashSet::new(), "m:embed0")
}

fn card(value: Value) -> EmbedCard {
    match view(value) {
        Some(EmbedView::Card(card)) => *card,
        other => panic!("expected a card, got {other:?}"),
    }
}

#[test]
fn message_append_decodes_and_skips_unknown_embeds() {
    let events = parse_frame(
        &json!({
            "type": "MessageAppend",
            "id": MSG,
            "channel": "ch-pics",
            "append": { "embeds": [
                { "type": "Website", "title": "Hello", "url": "https://example.com" },
                { "type": "Website", "special": { "type": "SomethingNew" } },
                { "type": "Image", "url": "https://example.com/a.png",
                  "width": 10, "height": 10, "size": "Large" }
            ]}
        })
        .to_string(),
    );
    let [
        ServerEvent::MessageAppend {
            id,
            channel,
            embeds,
        },
    ] = events.as_slice()
    else {
        panic!("{events:?}");
    };
    assert_eq!((id.as_str(), channel.as_str()), (MSG, "ch-pics"));
    // The unknown `special` costs only that embed.
    assert_eq!(embeds.len(), 2);
    assert!(matches!(embeds[0], Embed::Website(_)));
    assert!(matches!(embeds[1], Embed::Image(_)));

    // No `append.embeds` at all: an empty list, not a parse failure.
    let events = parse_frame(
        &json!({ "type": "MessageAppend", "id": MSG, "channel": "c", "append": {} }).to_string(),
    );
    assert!(matches!(
        events.as_slice(),
        [ServerEvent::MessageAppend { embeds, .. }] if embeds.is_empty()
    ));
}

#[test]
fn website_previews_become_cards_with_proxied_pictures() {
    let card = card(json!({
        "type": "Website",
        "url": "https://news.example/story",
        "site_name": "News",
        "title": "A story",
        "description": "What happened.",
        "icon_url": "https://news.example/favicon.ico",
        "colour": "#ff8800",
        "image": { "url": "https://news.example/hero.jpg",
                   "width": 1200, "height": 630, "size": "Large" }
    }));
    assert_eq!(
        card,
        EmbedCard {
            colour: Some(0xff8800ff),
            icon: Some(proxied("https://news.example/favicon.ico")),
            site_name: Some("News".into()),
            title: Some("A story".into()),
            link: Some("https://news.example/story".into()),
            description: Some(Description::Plain("What happened.".into())),
            thumbnail: None,
            // 1200×630 into 400×300: 400×210.
            image: Some(EmbedImage {
                url: proxied("https://news.example/hero.jpg"),
                width: 400,
                height: 210,
                original: (1200, 630),
            }),
            playable: false,
            media: None,
        }
    );
}

#[test]
fn preview_sized_images_are_thumbnails() {
    let card = card(json!({
        "type": "Website",
        "title": "Repo",
        "image": { "url": "https://git.example/og.png",
                   "width": 400, "height": 200, "size": "Preview" }
    }));
    assert_eq!(card.image, None);
    let thumbnail = card.thumbnail.unwrap();
    assert_eq!((thumbnail.width, thumbnail.height), (80, 40));
}

#[test]
fn videos_get_a_play_badge_that_opens_the_link() {
    let card = card(json!({
        "type": "Website",
        "url": "https://www.youtube.com/watch?v=abc",
        "special": { "type": "YouTube", "id": "abc" },
        "title": "A video",
        "image": { "url": "https://i.ytimg.com/vi/abc/hq.jpg",
                   "width": 480, "height": 360, "size": "Large" },
        "video": { "url": "https://www.youtube.com/embed/abc", "width": 1280, "height": 720 }
    }));
    assert!(card.playable);
    assert_eq!(
        card.link.as_deref(),
        Some("https://www.youtube.com/watch?v=abc")
    );
    // Without a picture there's nothing to badge.
    let card = self::card(json!({
        "type": "Website",
        "title": "A video",
        "video": { "url": "https://v.example/v.mp4", "width": 1, "height": 1 }
    }));
    assert!(!card.playable);
}

#[test]
fn gifs_and_image_links_show_just_the_picture() {
    let gif = view(json!({
        "type": "Website",
        "special": { "type": "GIF" },
        "title": "ignored",
        "image": { "url": "https://gifs.example/cat.gif",
                   "width": 200, "height": 100, "size": "Large" }
    }));
    assert_eq!(
        gif,
        Some(EmbedView::Image(EmbedImage {
            url: proxied("https://gifs.example/cat.gif"),
            width: 200,
            height: 100,
            original: (200, 100),
        }))
    );
    let gif_video = view(json!({
        "type": "Website",
        "special": { "type": "GIF" },
        "video": { "url": "https://gifs.example/cat.mp4", "width": 1, "height": 1 }
    }));
    assert_eq!(
        gif_video,
        Some(EmbedView::Video {
            url: "https://gifs.example/cat.mp4".into()
        })
    );
    let image = view(json!({
        "type": "Image", "url": "https://example.com/big.png",
        "width": 3000, "height": 3000, "size": "Large"
    }));
    assert!(matches!(
        image,
        Some(EmbedView::Image(EmbedImage {
            width: 300,
            height: 300,
            ..
        }))
    ));
    // Missing dimensions: the fallback box.
    let no_size = view(json!({
        "type": "Image", "url": "https://example.com/x.png",
        "width": 0, "height": 0, "size": "Large"
    }));
    assert!(matches!(
        no_size,
        Some(EmbedView::Image(EmbedImage {
            width: 300,
            height: 200,
            ..
        }))
    ));
}

#[test]
fn video_links_are_cards_but_only_for_web_urls() {
    assert_eq!(
        view(
            json!({ "type": "Video", "url": "https://v.example/clip.mp4", "width": 1, "height": 1 })
        ),
        Some(EmbedView::Video {
            url: "https://v.example/clip.mp4".into()
        })
    );
    assert_eq!(
        view(json!({ "type": "Video", "url": "file:///etc/passwd", "width": 1, "height": 1 })),
        None
    );
}

#[test]
fn a_video_only_gifbox_gif_shows_its_still_badged() {
    let page = "https://gifbox.me/view/smGy30kWfDtNbJyWRnq8a";
    assert_eq!(
        view(json!({ "type": "Video", "url": page, "width": 640, "height": 360 })),
        Some(EmbedView::GifPoster {
            image: EmbedImage {
                url: proxied("https://media.gifbox.me/media/post/smGy30kWfDtNbJyWRnq8a/poster"),
                // Capped like other embed images.
                width: 400,
                height: 225,
                original: (640, 360),
            },
            link: page.into(),
        })
    );
    // Without a proxy to load the still through, it's a video link as before.
    let no_proxy = MediaUrls::new("https://api.example", Some(CDN.into()), None);
    assert_eq!(
        view_with(
            json!({ "type": "Video", "url": page, "width": 640, "height": 360 }),
            &no_proxy
        ),
        Some(EmbedView::Video { url: page.into() })
    );
}

#[test]
fn other_videos_and_gifbox_images_are_unchanged() {
    // Other hosts, including lookalikes, stay video links.
    for url in [
        "https://v.example/view/abc",
        "https://gifbox.me.evil.com/view/abc",
        "https://notgifbox.me/view/abc",
        "https://media.gifbox.me/media/post/abc/mp4",
    ] {
        assert_eq!(
            view(json!({ "type": "Video", "url": url, "width": 1, "height": 1 })),
            Some(EmbedView::Video { url: url.into() }),
            "{url}"
        );
    }
    // A Gifbox GIF that has a GIF version is an image, drawn as before.
    let page = "https://gifbox.me/view/cXQwxRj5VVmb4q_qmUAg6";
    assert_eq!(
        view(json!({ "type": "Image", "url": page, "width": 498, "height": 280, "size": "Large" })),
        Some(EmbedView::Image(EmbedImage {
            url: proxied(page),
            width: 400,
            height: 225,
            original: (498, 280),
        }))
    );
}

#[test]
fn bot_embeds_render_markdown_and_their_media() {
    let card = card(json!({
        "type": "Text",
        "icon_url": "https://bot.example/icon.png",
        "url": "https://bot.example/run/1",
        "title": "Build passed",
        "description": "**3** checks <b>ok</b>",
        "colour": "green",
        "media": {
            "_id": "f-shot", "tag": "attachments", "filename": "shot.png",
            "metadata": { "type": "Image", "width": 800, "height": 600 },
            "content_type": "image/png", "size": 1000
        }
    }));
    assert_eq!(card.colour, Some(0x008000ff));
    assert_eq!(card.icon, Some(proxied("https://bot.example/icon.png")));
    assert_eq!(card.title.as_deref(), Some("Build passed"));
    assert_eq!(card.link.as_deref(), Some("https://bot.example/run/1"));
    // Prepared like message text: raw HTML is escaped.
    let Some(Description::Markdown { source, render_key }) = &card.description else {
        panic!("{:?}", card.description);
    };
    assert_eq!(source, "**3** checks \\<b>ok\\</b>");
    assert!(render_key.starts_with("m:embed0:"));
    assert!(matches!(
        card.media,
        Some(AttachmentView::Image {
            width: 400,
            height: 300,
            ..
        })
    ));
}

#[test]
fn missing_and_odd_fields_are_tolerated() {
    // Nothing to show.
    assert_eq!(view(json!({ "type": "None" })), None);
    assert_eq!(view(json!({ "type": "Website" })), None);
    assert_eq!(
        view(json!({ "type": "Website", "title": "  ", "colour": "#fff" })),
        None
    );
    assert_eq!(view(json!({ "type": "Text", "colour": "#fff" })), None);
    // A title-only preview.
    let title_only = card(json!({ "type": "Website", "title": "Just a title" }));
    assert_eq!(title_only.title.as_deref(), Some("Just a title"));
    assert_eq!(
        (title_only.link, title_only.icon, title_only.colour),
        (None, None, None)
    );
    // Colours: hex (short and long) and names; anything else uses the theme.
    let colour = |c: &str| card(json!({ "type": "Website", "title": "t", "colour": c })).colour;
    assert_eq!(colour("#abc"), Some(0xaabbccff));
    assert_eq!(colour("#AABBCC"), Some(0xaabbccff));
    assert_eq!(colour("teal"), Some(0x008080ff));
    assert_eq!(colour("linear-gradient(red, blue)"), None);
    assert_eq!(colour("#12345"), None);
    // Only web links are clickable.
    let link = |u: &str| card(json!({ "type": "Website", "title": "t", "url": u })).link;
    assert_eq!(link("javascript:alert(1)"), None);
    assert_eq!(link("mailto:a@b.c"), None);
    assert_eq!(
        link("HTTPS://EXAMPLE.COM"),
        Some("HTTPS://EXAMPLE.COM".into())
    );
    // `original_url` stands in for a missing `url`.
    let original = card(json!({
        "type": "Website", "title": "t", "original_url": "https://short.example/x"
    }));
    assert_eq!(original.link.as_deref(), Some("https://short.example/x"));
}

#[test]
fn without_a_proxy_remote_pictures_are_left_out() {
    let no_proxy = MediaUrls::new("https://api.example", Some(CDN.into()), None);
    let preview = json!({
        "type": "Website",
        "title": "A story",
        "icon_url": "https://news.example/favicon.ico",
        "image": { "url": "https://news.example/hero.jpg",
                   "width": 10, "height": 10, "size": "Large" }
    });
    let Some(EmbedView::Card(card)) = view_with(preview, &no_proxy) else {
        panic!();
    };
    assert_eq!((card.icon, card.image), (None, None));
    let image = json!({ "type": "Image", "url": "https://x.example/a.png",
                        "width": 1, "height": 1, "size": "Large" });
    assert_eq!(view_with(image, &no_proxy), None);
}

/// A channel with one plain message, "look".
fn store_with_message() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    store.set_media(media());
    let page: BulkMessageResponse = serde_json::from_value(json!([{
        "_id": MSG, "channel": "ch-pics", "author": "u-ada",
        "content": "look https://news.example/story"
    }]))
    .unwrap();
    store.set_history("ch-pics", page, 50);
    store
}

fn embeds(rows: &[TimelineRow]) -> Vec<EmbedView> {
    rows.iter()
        .find_map(|row| match row {
            TimelineRow::Message(m) => Some(m.embeds.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_live_preview_lands_on_its_message_and_remeasures_the_row() {
    let mut store = store_with_message();
    let before = store.timeline("ch-pics");
    assert!(embeds(&before).is_empty());

    let frame = json!({
        "type": "MessageAppend", "id": MSG, "channel": "ch-pics",
        "append": { "embeds": [{ "type": "Website", "title": "A story",
                                  "url": "https://news.example/story" }] }
    });
    let Some(ServerEvent::MessageAppend {
        id,
        channel,
        embeds: new,
    }) = parse_frame(&frame.to_string()).pop()
    else {
        panic!();
    };
    assert!(store.apply_append(&channel, &id, new));

    let after = store.timeline("ch-pics");
    assert!(matches!(
        embeds(&after).as_slice(),
        [EmbedView::Card(card)] if card.title.as_deref() == Some("A story")
    ));
    // Row 0 is the start-of-channel marker.
    assert_eq!(diff_rows(&before, &after), vec![RowOp::Remeasure { at: 1 }]);
}

#[test]
fn appends_for_unloaded_messages_or_with_nothing_are_ignored() {
    let mut store = store_with_message();
    let website = embed(json!({ "type": "Website", "title": "t" }));
    assert!(!store.apply_append(
        "ch-pics",
        "01M6GZZZZZZZZZZZZZZZZZZZZZ",
        vec![website.clone()]
    ));
    assert!(!store.apply_append("ch-elsewhere", MSG, vec![website]));
    assert!(!store.apply_append("ch-pics", MSG, Vec::new()));
}

#[test]
fn embeds_already_on_a_message_show_in_order() {
    let mut store = ChatStore::new("u-me");
    store.set_media(media());
    let page: BulkMessageResponse = serde_json::from_value(json!([{
        "_id": MSG, "channel": "ch-pics", "author": "u-bot",
        "embeds": [
            { "type": "Text", "title": "first" },
            { "type": "None" },
            { "type": "Website", "title": "second" }
        ]
    }]))
    .unwrap();
    store.set_history("ch-pics", page, 50);
    let titles: Vec<Option<String>> = embeds(&store.timeline("ch-pics"))
        .into_iter()
        .map(|embed| match embed {
            EmbedView::Card(card) => card.title,
            _ => None,
        })
        .collect();
    assert_eq!(titles, vec![Some("first".into()), Some("second".into())]);
}
