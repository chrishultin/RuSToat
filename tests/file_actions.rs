//! What can be done with a file or picture: names, which actions each
//! target has, the clipboard format, and the texts.

use std::io::Cursor;

use serde_json::json;
use stoat_gpui::api::ApiError;
use stoat_gpui::api::models::File;
use stoat_gpui::attachment::AttachmentView;
use stoat_gpui::embed::EmbedImage;
use stoat_gpui::file_actions::{
    ClipboardFormat, FileAction, FileTarget, actions, clipboard_image, display_name,
    download_failure, embed_name, embed_target, save_directory, save_failed_text,
    suggested_save_name,
};
use stoat_gpui::media::MediaUrls;

fn target(picture: bool) -> FileTarget {
    FileTarget {
        name: "a.png".into(),
        url: "https://cdn.example/attachments/a".into(),
        link: "https://cdn.example/attachments/a".into(),
        page: None,
        picture,
    }
}

#[test]
fn pictures_can_be_copied_as_images_and_files_cannot() {
    assert_eq!(
        actions(&target(true)),
        [
            FileAction::Save,
            FileAction::CopyImage,
            FileAction::CopyLink,
            FileAction::OpenInBrowser
        ]
    );
    assert_eq!(
        actions(&target(false)),
        [
            FileAction::Save,
            FileAction::CopyLink,
            FileAction::OpenInBrowser
        ]
    );
    let labels: Vec<_> = actions(&target(true)).iter().map(|a| a.label()).collect();
    assert_eq!(
        labels,
        ["Save…", "Copy image", "Copy link", "Open in browser"]
    );
}

#[test]
fn shown_names_drop_a_spoiler_prefix_while_something_is_left() {
    assert_eq!(display_name("SPOILER_cat.png"), "cat.png");
    assert_eq!(display_name("spoiler_cat.png"), "cat.png");
    assert_eq!(display_name("Spoiler_Cat.PNG"), "Cat.PNG");
    assert_eq!(display_name("spoiler_"), "spoiler_");
    assert_eq!(display_name("SPOILER_"), "SPOILER_");
    assert_eq!(display_name("spoiler.png"), "spoiler.png");
    assert_eq!(display_name("my_spoiler_cat.png"), "my_spoiler_cat.png");
    assert_eq!(display_name("cat.png"), "cat.png");
    assert_eq!(display_name(""), "");
    // A multi-byte character where the prefix would end isn't split.
    assert_eq!(display_name("spoileré.png"), "spoileré.png");
}

#[test]
fn the_save_panel_suggests_a_name_macos_takes() {
    // Given the shown name, which already lost any spoiler prefix.
    assert_eq!(suggested_save_name("cat.png"), "cat.png");
    assert_eq!(suggested_save_name("a/b:c.txt"), "a_b_c.txt");
    assert_eq!(suggested_save_name("  report.pdf "), "report.pdf");
    // Not a hidden file.
    assert_eq!(suggested_save_name(".bashrc"), "bashrc");
    assert_eq!(suggested_save_name(""), "file");
    assert_eq!(suggested_save_name("..."), "file");
}

#[test]
fn saving_starts_in_downloads_else_home() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(save_directory(Some(home.path())), home.path());
    std::fs::create_dir(home.path().join("Downloads")).unwrap();
    assert_eq!(
        save_directory(Some(home.path())),
        home.path().join("Downloads")
    );
    assert_eq!(save_directory(None), std::path::Path::new("/"));
}

/// A 3 × 2 picture in `format`.
fn picture(format: image::ImageFormat) -> Vec<u8> {
    let picture =
        image::RgbImage::from_fn(3, 2, |x, y| image::Rgb([x as u8 * 80, y as u8 * 120, 7]));
    let mut bytes = Cursor::new(Vec::new());
    picture.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

#[test]
fn png_and_jpeg_are_copied_as_they_are() {
    let png = picture(image::ImageFormat::Png);
    assert_eq!(
        clipboard_image(png.clone()).unwrap(),
        (ClipboardFormat::Png, png)
    );
    let jpeg = picture(image::ImageFormat::Jpeg);
    assert_eq!(
        clipboard_image(jpeg.clone()).unwrap(),
        (ClipboardFormat::Jpeg, jpeg)
    );
}

#[test]
fn gif_and_webp_become_png_of_the_same_size() {
    for format in [image::ImageFormat::Gif, image::ImageFormat::WebP] {
        let (clipboard, bytes) = clipboard_image(picture(format)).unwrap();
        assert_eq!(clipboard, ClipboardFormat::Png, "{format:?}");
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Png
        );
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (3, 2), "{format:?}");
    }
}

#[test]
fn an_animated_gif_copies_its_first_frame() {
    use image::codecs::gif::GifEncoder;
    use image::{Delay, Frame, Rgba, RgbaImage};

    let mut bytes = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut bytes);
        for colour in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            let frame = Frame::from_parts(
                RgbaImage::from_pixel(4, 4, Rgba(colour)),
                0,
                0,
                Delay::from_numer_denom_ms(100, 1),
            );
            encoder.encode_frame(frame).unwrap();
        }
    }
    let (format, png) = clipboard_image(bytes).unwrap();
    assert_eq!(format, ClipboardFormat::Png);
    let first = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(first.get_pixel(0, 0).0, [255, 0, 0, 255]);
}

#[test]
fn what_isnt_a_picture_isnt_copied() {
    assert_eq!(
        clipboard_image(b"hello, world".to_vec()).unwrap_err(),
        "it isn't a picture"
    );
    // A broken GIF (it has to be decoded) is refused.
    let mut gif = picture(image::ImageFormat::Gif);
    gif.truncate(16);
    assert!(clipboard_image(gif).is_err());
}

#[test]
fn failures_read_plainly() {
    let gone = ApiError::Unexpected {
        status: 404,
        detail: "Not Found".into(),
    };
    assert_eq!(download_failure(&gone), "it's no longer there");
    assert_eq!(
        save_failed_text("cat.png", &download_failure(&gone)),
        "Couldn't save cat.png: it's no longer there."
    );
    let refused = ApiError::Unexpected {
        status: 403,
        detail: String::new(),
    };
    assert_eq!(download_failure(&refused), "the server refused it");
    let broken = ApiError::Unexpected {
        status: 502,
        detail: String::new(),
    };
    assert_eq!(download_failure(&broken), "the server answered HTTP 502");
}

fn file(id: &str, name: &str, metadata: serde_json::Value) -> File {
    serde_json::from_value(json!({
        "_id": id,
        "tag": "attachments",
        "filename": name,
        "metadata": metadata,
        "content_type": "application/octet-stream",
        "size": 1000,
    }))
    .unwrap()
}

#[test]
fn attachments_act_on_the_file_servers_original() {
    let media = MediaUrls::new(
        "https://api.example",
        Some("https://cdn.example".into()),
        None,
    );
    let image = file(
        "f1",
        "SPOILER_cat.png",
        json!({ "type": "Image", "width": 10, "height": 10 }),
    );
    let target = AttachmentView::new(&image, &media, false)
        .file_target()
        .unwrap();
    assert_eq!(target.name, "cat.png");
    assert_eq!(target.url, "https://cdn.example/attachments/f1");
    assert_eq!(target.link, target.url);
    assert!(target.picture);

    let pdf = file("f2", "report.pdf", json!({ "type": "File" }));
    let target = AttachmentView::new(&pdf, &media, false)
        .file_target()
        .unwrap();
    assert_eq!(target.name, "report.pdf");
    assert_eq!(target.url, "https://cdn.example/attachments/f2");
    assert!(!target.picture);

    // No file server known yet: nothing to act on.
    let unknown = MediaUrls::new("https://api.example", None, None);
    assert_eq!(
        AttachmentView::new(&pdf, &unknown, false).file_target(),
        None
    );
}

#[test]
fn embed_pictures_are_named_by_their_address() {
    assert_eq!(embed_name("https://example.com/pics/cat.png"), "cat.png");
    assert_eq!(
        embed_name("https://example.com/pics/cat.png?size=2#x"),
        "cat.png"
    );
    assert_eq!(
        embed_name("https://example.com/pics/a%20cat%C3%A9.png"),
        "a caté.png"
    );
    // A trailing slash: the last segment with something in it.
    assert_eq!(embed_name("https://example.com/pics/cat/"), "cat");
    assert_eq!(embed_name("https://example.com/"), "image");
    assert_eq!(embed_name("https://example.com"), "image");
    assert_eq!(embed_name("not a url"), "image");
    // `+` is a plus in a path, not a space.
    assert_eq!(embed_name("https://example.com/a+b.gif"), "a+b.gif");
}

#[test]
fn embed_pictures_download_through_the_proxy_and_link_to_themselves() {
    let image = EmbedImage {
        url: "https://proxy.example/proxy?url=https%3A%2F%2Fgifs.example%2Fcat.gif".into(),
        source: "https://gifs.example/cat.gif".into(),
        width: 200,
        height: 100,
        original: (200, 100),
    };
    let target = embed_target(&image, None);
    assert_eq!(target.name, "cat.gif");
    assert_eq!(target.url, image.url);
    assert_eq!(target.link, "https://gifs.example/cat.gif");
    assert_eq!(target.page, None);
    assert!(target.picture);
    assert_eq!(actions(&target).len(), 4);

    // A GIF's still: Open in browser goes to the GIF's page.
    let target = embed_target(&image, Some("https://gifbox.me/view/abc"));
    assert_eq!(target.link, "https://gifs.example/cat.gif");
    assert_eq!(target.page.as_deref(), Some("https://gifbox.me/view/abc"));
}
