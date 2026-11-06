//! Attachments: inline image sizing, the large-image guard and "Show image",
//! file cards (kind, size label, link), and how they reach the timeline.

use serde_json::json;
use stoat_gpui::api::models::{BulkMessageResponse, File, Metadata};
use stoat_gpui::attachment::{
    AttachmentView, FALLBACK_IMAGE_SIZE, FileKind, MAX_AUTO_LOAD_BYTES, file_kind, fit_into,
    fit_within, size_label,
};
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::{ChatStore, RowOp, TimelineRow, diff_rows};

const CDN: &str = "https://cdn.example";

fn media() -> MediaUrls {
    MediaUrls::new("https://api.example", Some(CDN.into()), None)
}

fn file(id: &str, name: &str, metadata: serde_json::Value, content_type: &str, size: u64) -> File {
    serde_json::from_value(json!({
        "_id": id,
        "tag": "attachments",
        "filename": name,
        "metadata": metadata,
        "content_type": content_type,
        "size": size,
    }))
    .unwrap()
}

fn image(id: &str, width: usize, height: usize, size: u64) -> File {
    file(
        id,
        &format!("{id}.png"),
        json!({ "type": "Image", "width": width, "height": height }),
        "image/png",
        size,
    )
}

#[test]
fn images_fit_within_the_box_keeping_their_shape() {
    // Exactly the box's aspect ratio.
    assert_eq!(fit_within(640, 480), (400, 300));
    // Wide: width-limited. 1920×200 × (400/1920) = 400×41.67, rounded.
    assert_eq!(fit_within(1920, 200), (400, 42));
    // Tall: height-limited. 1080×2400 × (300/2400) = 135×300.
    assert_eq!(fit_within(1080, 2400), (135, 300));
    // Rounds to nearest: 1000×333 × 0.4 = 400×133.2.
    assert_eq!(fit_within(1000, 333), (400, 133));
    // Never scaled up.
    assert_eq!(fit_within(64, 32), (64, 32));
    assert_eq!(fit_within(400, 300), (400, 300));
    // Extreme shapes keep at least a pixel.
    assert_eq!(fit_within(10_000, 1), (400, 1));
    assert_eq!(fit_within(1, 10_000), (1, 300));
}

#[test]
fn missing_dimensions_get_a_fallback_box() {
    assert_eq!(fit_within(0, 0), FALLBACK_IMAGE_SIZE);
    assert_eq!(fit_within(640, 0), FALLBACK_IMAGE_SIZE);
    assert_eq!(fit_within(0, 480), FALLBACK_IMAGE_SIZE);
    let view = AttachmentView::new(&image("f-zero", 0, 0, 100), &media(), false);
    assert!(matches!(
        view,
        AttachmentView::Image {
            width: 300,
            height: 200,
            original: (0, 0),
            ..
        }
    ));
}

#[test]
fn images_show_inline_from_the_file_server() {
    let view = AttachmentView::new(&image("f-cat", 640, 480, 12_345), &media(), false);
    assert_eq!(
        view,
        AttachmentView::Image {
            id: "f-cat".into(),
            url: format!("{CDN}/attachments/f-cat"),
            name: "f-cat.png".into(),
            size_label: "12.1 KB".into(),
            width: 400,
            height: 300,
            original: (640, 480),
            thumbhash: None,
            too_big: false,
        }
    );
}

#[test]
fn images_carry_their_thumbhash_for_a_blurred_preview() {
    // Stoat sends it as a JSON array of bytes.
    let file = file(
        "f-hash",
        "hash.jpg",
        json!({ "type": "Image", "width": 4032, "height": 3024,
                "thumbhash": [1, 2, 3, 250] }),
        "image/jpeg",
        1000,
    );
    let AttachmentView::Image { thumbhash, .. } = AttachmentView::new(&file, &media(), false)
    else {
        panic!();
    };
    assert_eq!(thumbhash.as_deref(), Some(&[1u8, 2, 3, 250][..]));
}

#[test]
fn the_viewer_fits_images_to_the_window() {
    // Bigger than the window: scaled down to fit, keeping the shape.
    assert_eq!(fit_into(4000, 3000, 1200, 800), Some((1067, 800)));
    assert_eq!(fit_into(3000, 1000, 1200, 800), Some((1200, 400)));
    // Smaller than the window: shown at its own size.
    assert_eq!(fit_into(640, 480, 1200, 800), Some((640, 480)));
    // A tiny window still gets a visible pixel.
    assert_eq!(fit_into(4000, 10, 1, 1), Some((1, 1)));
    // Unknown dimensions: the caller decides (the viewer letterboxes).
    assert_eq!(fit_into(0, 480, 1200, 800), None);
    assert_eq!(fit_into(640, 0, 1200, 800), None);
}

#[test]
fn large_images_wait_for_a_click() {
    let at_limit = image("f-ok", 4000, 3000, MAX_AUTO_LOAD_BYTES);
    let over = image("f-big", 4000, 3000, MAX_AUTO_LOAD_BYTES + 1);
    assert!(matches!(
        AttachmentView::new(&at_limit, &media(), false),
        AttachmentView::Image { too_big: false, .. }
    ));
    assert!(matches!(
        AttachmentView::new(&over, &media(), false),
        AttachmentView::Image { too_big: true, .. }
    ));
    // Revealed: loads, at the same reserved size.
    assert!(matches!(
        AttachmentView::new(&over, &media(), true),
        AttachmentView::Image {
            too_big: false,
            width: 400,
            height: 300,
            ..
        }
    ));
}

#[test]
fn other_files_are_cards_that_link_to_the_file() {
    let pdf = file(
        "f-pdf",
        "report.pdf",
        json!({ "type": "File" }),
        "application/pdf",
        1_258_291,
    );
    assert_eq!(
        AttachmentView::new(&pdf, &media(), false),
        AttachmentView::File {
            name: "report.pdf".into(),
            size_label: "1.2 MB".into(),
            url: Some(format!("{CDN}/attachments/f-pdf")),
            kind: FileKind::File,
        }
    );
    // Videos are cards too (no in-app playback).
    let video = file(
        "f-mov",
        "clip.mp4",
        json!({ "type": "Video", "width": 1920, "height": 1080 }),
        "video/mp4",
        5_000_000,
    );
    assert!(matches!(
        AttachmentView::new(&video, &media(), false),
        AttachmentView::File {
            kind: FileKind::Video,
            ..
        }
    ));
    // Without a file server there's no link (and no inline image).
    assert!(matches!(
        AttachmentView::new(&image("f-cat", 640, 480, 1), &MediaUrls::default(), false),
        AttachmentView::File {
            url: None,
            kind: FileKind::Image,
            ..
        }
    ));
}

#[test]
fn file_kinds_come_from_metadata_then_mime_type() {
    let image = Metadata::Image {
        width: 1,
        height: 1,
        thumbhash: None,
        animated: None,
    };
    assert_eq!(
        file_kind(&image, "application/octet-stream"),
        FileKind::Image
    );
    assert_eq!(
        file_kind(
            &Metadata::Video {
                width: 1,
                height: 1
            },
            ""
        ),
        FileKind::Video
    );
    assert_eq!(file_kind(&Metadata::Audio, ""), FileKind::Audio);
    assert_eq!(file_kind(&Metadata::Text, ""), FileKind::Text);
    // Uncategorised uploads fall back to the MIME type.
    assert_eq!(file_kind(&Metadata::File, "image/svg+xml"), FileKind::Image);
    assert_eq!(file_kind(&Metadata::File, "video/webm"), FileKind::Video);
    assert_eq!(file_kind(&Metadata::File, "audio/ogg"), FileKind::Audio);
    assert_eq!(file_kind(&Metadata::File, "text/plain"), FileKind::Text);
    assert_eq!(
        file_kind(&Metadata::File, "application/zip"),
        FileKind::File
    );
    assert_eq!(file_kind(&Metadata::File, ""), FileKind::File);
}

#[test]
fn sizes_read_like_a_file_browser() {
    assert_eq!(size_label(0), "0 B");
    assert_eq!(size_label(1023), "1023 B");
    assert_eq!(size_label(1024), "1.0 KB");
    assert_eq!(size_label(12_345), "12.1 KB");
    assert_eq!(size_label(1_258_291), "1.2 MB");
    assert_eq!(size_label(MAX_AUTO_LOAD_BYTES), "8.0 MB");
    assert_eq!(size_label(3 * 1024 * 1024 * 1024), "3.0 GB");
    // Just under a unit boundary moves up rather than showing "1024.0 KB".
    assert_eq!(size_label(1024 * 1024 - 1), "1.0 MB");
}

/// A channel with one message carrying `files`.
fn store_with(files: Vec<File>) -> ChatStore {
    let mut store = ChatStore::new("u-me");
    store.set_media(media());
    let page: BulkMessageResponse = serde_json::from_value(json!([{
        "_id": "01M6GDKW80AAAAAAAAAAAAAAAA",
        "channel": "ch-pics",
        "author": "u-ada",
        "attachments": files,
    }]))
    .unwrap();
    store.set_history("ch-pics", page, 50);
    store
}

fn attachments(rows: &[TimelineRow]) -> Vec<AttachmentView> {
    rows.iter()
        .find_map(|row| match row {
            TimelineRow::Message(m) => Some(m.attachments.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn timeline_rows_carry_attachment_views_in_order() {
    let zip = file(
        "f-zip",
        "src.zip",
        json!({ "type": "File" }),
        "application/zip",
        2048,
    );
    let store = store_with(vec![image("f-cat", 640, 480, 100), zip]);
    let views = attachments(&store.timeline("ch-pics"));
    assert!(matches!(&views[0], AttachmentView::Image { id, .. } if id == "f-cat"));
    assert!(matches!(&views[1], AttachmentView::File { name, .. } if name == "src.zip"));
}

#[test]
fn show_image_reveals_it_and_remeasures_the_row() {
    let mut store = store_with(vec![image("f-big", 4000, 3000, MAX_AUTO_LOAD_BYTES * 2)]);
    let before = store.timeline("ch-pics");
    assert!(matches!(
        attachments(&before)[0],
        AttachmentView::Image { too_big: true, .. }
    ));

    store.reveal_image("f-big");
    let after = store.timeline("ch-pics");
    assert!(matches!(
        attachments(&after)[0],
        AttachmentView::Image { too_big: false, .. }
    ));
    // Row 0 is the start-of-channel marker (a one-message page is the whole history).
    assert_eq!(diff_rows(&before, &after), vec![RowOp::Remeasure { at: 1 }]);
}
