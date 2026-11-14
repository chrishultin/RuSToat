//! Media URLs (files, emoji, default avatars, proxied external images) and
//! which avatar a message author gets.

use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{BulkMessageResponse, Message};
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::ChatStore;

const API: &str = "https://api.stoat.chat";
const CDN: &str = "https://cdn.stoatusercontent.com";
const PROXY: &str = "https://proxy.stoatusercontent.com";

fn media() -> MediaUrls {
    MediaUrls::new(
        format!("{API}/"),
        Some(format!("{CDN}/")),
        Some(PROXY.to_string()),
    )
}

fn file(tag: &str, id: &str) -> serde_json::Value {
    serde_json::json!({
        "_id": id, "tag": tag, "filename": "a.png",
        "metadata": { "type": "Image", "width": 64, "height": 64 },
        "content_type": "image/png", "size": 100
    })
}

#[test]
fn file_emoji_and_default_avatar_urls() {
    let media = media();
    let avatar = serde_json::from_value(file("avatars", "file-1")).unwrap();
    assert_eq!(
        media.file(&avatar).as_deref(),
        Some("https://cdn.stoatusercontent.com/avatars/file-1")
    );
    assert_eq!(
        media.emoji("01EMOJI").as_deref(),
        Some("https://cdn.stoatusercontent.com/emojis/01EMOJI")
    );
    assert_eq!(
        media.default_avatar("u-1"),
        "https://api.stoat.chat/users/u-1/default_avatar"
    );
    // Without a file server, uploaded files can't be shown.
    assert_eq!(MediaUrls::new(API, None, None).file(&avatar), None);
}

#[test]
fn originals_are_addressed_by_their_encoded_name() {
    let mut song: stoat_gpui::api::models::File =
        serde_json::from_value(file("attachments", "file-2")).unwrap();
    song.filename = "SPOILER_my song #1 (live).mp3".into();
    // The name is the original's own, spoiler prefix and all, encoded so
    // the file server's route matches it.
    assert_eq!(
        media().original(&song).as_deref(),
        Some(
            "https://cdn.stoatusercontent.com/attachments/file-2/\
             SPOILER_my%20song%20%231%20%28live%29.mp3"
        )
    );
    song.filename = "naïve/../x.m4a".into();
    assert_eq!(
        media().original(&song).as_deref(),
        Some("https://cdn.stoatusercontent.com/attachments/file-2/na%C3%AFve%2F..%2Fx.m4a")
    );
    assert_eq!(MediaUrls::new(API, None, None).original(&song), None);
}

#[test]
fn external_images_are_proxied_and_encoded() {
    let media = media();
    assert_eq!(
        media
            .external("https://example.com/a b.png?x=1&y=é")
            .as_deref(),
        Some(
            "https://proxy.stoatusercontent.com/proxy?url=\
             https%3A%2F%2Fexample.com%2Fa+b.png%3Fx%3D1%26y%3D%C3%A9"
        )
    );
    // Our own file server is used directly.
    assert_eq!(
        media
            .external("https://cdn.stoatusercontent.com/avatars/x")
            .as_deref(),
        Some("https://cdn.stoatusercontent.com/avatars/x")
    );
    // A look-alike host is not our file server.
    assert!(
        media
            .external("https://cdn.stoatusercontent.com.evil.example/x")
            .unwrap()
            .starts_with("https://proxy.stoatusercontent.com/proxy?url=")
    );
    // No proxy: external images aren't loaded at all.
    assert_eq!(
        MediaUrls::new(API, Some(CDN.into()), None).external("https://example.com/x.png"),
        None
    );
}

/// The Ready fixture, plus a page that brings users and members with avatars.
fn store() -> ChatStore {
    let mut store = ChatStore::new("u-me");
    let Some(ServerEvent::Ready(ready)) = parse_frame(
        &std::fs::read_to_string(format!(
            "{}/tests/fixtures/ready.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .pop() else {
        panic!("ready fixture");
    };
    store.apply_ready(*ready);
    store.set_media(media());
    let page: BulkMessageResponse = serde_json::from_value(serde_json::json!({
        "messages": [],
        "users": [
            { "_id": "u-pic", "username": "pic", "discriminator": "0001", "relationship": "None",
              "online": true, "avatar": file("avatars", "user-avatar") },
            { "_id": "u-ada", "username": "ada", "discriminator": "1815", "display_name": "Ada L.",
              "relationship": "Friend", "online": true, "avatar": file("avatars", "ada-avatar") }
        ],
        "members": [
            { "_id": { "server": "srv-alpha", "user": "u-pic" }, "joined_at": "2026-01-01T00:00:00.000Z",
              "avatar": file("avatars", "member-avatar") }
        ]
    }))
    .unwrap();
    store.set_history("ch-general", page, 50);
    store
}

fn message(channel: &str, author: &str, extra: serde_json::Value) -> Message {
    let mut value = serde_json::json!({
        "_id": "01M6GDKW80AAAAAAAAAAAAAAAA", "channel": channel, "author": author, "content": "hi"
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

#[test]
fn avatar_precedence() {
    let store = store();
    let none = serde_json::json!({});
    let avatar = |m: Message| store.avatar_url(&m);

    // Masquerade avatar wins, and it's external so it's proxied.
    assert_eq!(
        avatar(message(
            "ch-general",
            "u-pic",
            serde_json::json!({ "masquerade": { "name": "Bridge", "avatar": "https://example.com/b.png" } })
        ))
        .as_deref(),
        Some("https://proxy.stoatusercontent.com/proxy?url=https%3A%2F%2Fexample.com%2Fb.png")
    );
    // Webhook avatar is a file ID; a webhook without one has no picture.
    assert_eq!(
        avatar(message(
            "ch-general",
            "01WEBHOOK",
            serde_json::json!({ "webhook": { "name": "CI", "avatar": "hook-avatar" } })
        ))
        .as_deref(),
        Some("https://cdn.stoatusercontent.com/avatars/hook-avatar")
    );
    assert_eq!(
        avatar(message(
            "ch-general",
            "01WEBHOOK",
            serde_json::json!({ "webhook": { "name": "CI", "avatar": null } })
        )),
        None
    );
    // In a server channel, the member's server avatar beats their user avatar…
    assert_eq!(
        avatar(message("ch-general", "u-pic", none.clone())).as_deref(),
        Some("https://cdn.stoatusercontent.com/avatars/member-avatar")
    );
    // …but in a DM there's no member, so the user avatar is used.
    assert_eq!(
        avatar(message("dm-ada", "u-ada", none.clone())).as_deref(),
        Some("https://cdn.stoatusercontent.com/avatars/ada-avatar")
    );
    // No avatar anywhere: the generated default.
    assert_eq!(
        avatar(message("ch-general", "u-bob", none.clone())).as_deref(),
        Some("https://api.stoat.chat/users/u-bob/default_avatar")
    );
    // System messages don't get one.
    assert_eq!(
        avatar(message(
            "ch-general",
            "00000000000000000000000000",
            serde_json::json!({ "system": { "type": "user_joined", "id": "u-bob" } })
        )),
        None
    );
}

#[test]
fn dm_entries_carry_the_other_persons_avatar() {
    let store = store();
    let home = store.home_channels();
    let ada = home.iter().find(|e| e.id == "dm-ada").unwrap();
    assert_eq!(
        ada.avatar.as_deref(),
        Some("https://cdn.stoatusercontent.com/avatars/ada-avatar")
    );
    let bob = home.iter().find(|e| e.id == "dm-bob").unwrap();
    assert_eq!(
        bob.avatar.as_deref(),
        Some("https://api.stoat.chat/users/u-bob/default_avatar")
    );
    let saved = home.iter().find(|e| e.id == "ch-saved").unwrap();
    assert_eq!(saved.avatar, None);
}

#[test]
fn timeline_rows_carry_avatars() {
    let mut store = store();
    store.set_history(
        "ch-general",
        serde_json::from_value(serde_json::json!([
            { "_id": "01M6GDKW80AAAAAAAAAAAAAAAA", "channel": "ch-general", "author": "u-bob", "content": "hi" }
        ]))
        .unwrap(),
        50,
    );
    let rows = store.timeline("ch-general");
    let Some(stoat_gpui::store::TimelineRow::Message(row)) = rows.last() else {
        panic!("expected a message row");
    };
    assert_eq!(
        row.avatar.as_deref(),
        Some("https://api.stoat.chat/users/u-bob/default_avatar")
    );
}
