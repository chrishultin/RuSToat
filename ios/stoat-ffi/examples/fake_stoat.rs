//! A fake Stoat on localhost, serving the test fixtures, for trying the
//! iPhone app in the simulator without a real account:
//!
//!   cargo run -p stoat-ffi --example fake_stoat
//!
//! It prints the API's address. Log in there with any email and password
//! (a DEBUG build can do it at launch: see `AppModel.devLogin`).

#[path = "../tests/support/mod.rs"]
mod support;

use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, Request, ResponseTemplate};

#[tokio::main]
async fn main() {
    let events = support::Events::start().await;
    let api = support::api(&events).await;
    let me = support::fixture("users_me.json");
    let mut me = me.as_object().unwrap().clone();
    me.insert("_id".into(), json!(support::ME));
    let mut login = support::fixture("login_success.json");
    login["user_id"] = json!(support::ME);
    login["token"] = json!(support::TOKEN);
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(login))
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me))
        .mount(&api)
        .await;
    // #random has a message with every kind of block, to see them drawn.
    Mock::given(method("GET"))
        .and(path("/channels/ch-random/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(support::fixture("messages_rich.json")),
        )
        .mount(&api)
        .await;
    // Beta's #chat has every kind of attachment and embed.
    Mock::given(method("GET"))
        .and(path("/channels/ch-beta/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(support::fixture("messages_media.json")),
        )
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/channels/[^/]+/messages$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(support::fixture("messages_page.json")),
        )
        .mount(&api)
        .await;
    // Sending: Stoat creates the message with the send's idempotency key as
    // its nonce, and echoes it on the events socket.
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let echo = events.sender();
    Mock::given(method("POST"))
        .and(path_regex(r"^/channels/[^/]+/messages$"))
        .respond_with(move |request: &Request| {
            let n = sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
            let channel = request.url.path().split('/').nth(2).unwrap_or_default();
            let nonce = request
                .headers
                .get("Idempotency-Key")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            let message = json!({
                "_id": format!("01JE00000000000000000S{n:04}"), "channel": channel,
                "author": support::ME, "content": body["content"], "nonce": nonce,
                "replies": body["replies"].as_array().map(|r| r.iter().map(|r| r["id"].clone()).collect::<Vec<_>>()),
            });
            let mut frame = message.clone();
            frame["type"] = json!("Message");
            echo(frame);
            ResponseTemplate::new(200).set_body_json(message)
        })
        .mount(&api)
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(r"^/channels/[^/]+/ack/.+$"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/session/logout"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&api)
        .await;
    // Custom emoji and attachments: the file server and the media proxy are
    // this fake. Every emoji is the app icon.
    let mut root = support::fixture("root.json");
    root["ws"] = json!(events.url);
    root["features"]["autumn"]["url"] = json!(api.uri());
    root["features"]["january"]["url"] = json!(api.uri());
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root))
        .with_priority(1)
        .mount(&api)
        .await;
    let icon = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../RuSToat/Assets.xcassets/AppIcon.appiconset/AppIcon.png"
    ))
    .expect("the app icon");
    Mock::given(method("GET"))
        .and(path_regex(r"^/emojis/[^/]+$"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(icon.clone(), "image/png"))
        .mount(&api)
        .await;
    serve_media(&api, icon).await;
    println!("{}", api.uri());
    // Every 20 s, a new message in #random, to watch unread state live.
    let mut n = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        n += 1;
        events.send(json!({ "type": "Message", "_id": format!("01JC00000000000000000F{n:04}"),
                            "channel": "ch-random", "author": "u-ada",
                            "content": format!("tick {n}") }));
    }
}

/// `tests/fixtures/media/{name}`.
fn media(name: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/media/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

/// The files `messages_media.json` refers to, and the proxied pictures of
/// its embeds (by the name at the end of their address). `f-slow` takes
/// 15 seconds, to see the blurred preview.
async fn serve_media(api: &wiremock::MockServer, icon: Vec<u8>) {
    let landscape = media("landscape.jpg");
    let portrait = media("portrait.jpg");
    for (id, bytes) in [
        ("f-landscape", &landscape),
        ("f-spoiler", &landscape),
        ("f-badge", &landscape),
        ("f-portrait", &portrait),
        ("f-huge", &portrait),
    ] {
        Mock::given(method("GET"))
            .and(path_regex(format!("^/attachments/{id}(/.*)?$")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(bytes.clone(), "image/jpeg"))
            .mount(api)
            .await;
    }
    Mock::given(method("GET"))
        .and(path_regex("^/attachments/f-slow(/.*)?$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(landscape.clone(), "image/jpeg")
                .set_delay(std::time::Duration::from_secs(15)),
        )
        .mount(api)
        .await;
    // The video and voice note, by their originals' own names. AVPlayer
    // streams with `Range` requests, and won't play without them.
    for (route, name, kind) in [
        ("/attachments/f-clip/clip.mp4", "clip.mp4", "video/mp4"),
        ("/attachments/f-voice/voice.m4a", "voice.m4a", "audio/mp4"),
    ] {
        let bytes = media(name);
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(move |request: &Request| ranged(request, &bytes, kind))
            .mount(api)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/proxy"))
        .respond_with(move |request: &Request| {
            let url = request
                .url
                .query_pairs()
                .find(|(k, _)| k == "url")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            let (bytes, kind) = if url.ends_with("portrait.jpg") || url.ends_with("/poster") {
                (portrait.clone(), "image/jpeg")
            } else if url.ends_with(".jpg") {
                (landscape.clone(), "image/jpeg")
            } else {
                (icon.clone(), "image/png")
            };
            ResponseTemplate::new(200).set_body_raw(bytes, kind)
        })
        .mount(api)
        .await;
}

/// `bytes`, or the part a `Range: bytes=a-b` header asks for (206).
fn ranged(request: &Request, bytes: &[u8], kind: &str) -> ResponseTemplate {
    let len = bytes.len();
    let range = request
        .headers
        .get("Range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .and_then(|(start, end)| {
            let start: usize = start.parse().ok()?;
            let end = if end.is_empty() {
                len - 1
            } else {
                end.parse::<usize>().ok()?.min(len - 1)
            };
            (start <= end).then_some((start, end))
        });
    match range {
        Some((start, end)) => ResponseTemplate::new(206)
            .insert_header("Content-Range", format!("bytes {start}-{end}/{len}"))
            .insert_header("Accept-Ranges", "bytes")
            .set_body_raw(bytes[start..=end].to_vec(), kind),
        None => ResponseTemplate::new(200)
            .insert_header("Accept-Ranges", "bytes")
            .set_body_raw(bytes.to_vec(), kind),
    }
}
