//! A server's custom emoji, as Server Settings shows them: the list, who
//! may manage them, and the texts.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use std::path::Path;
use stoat_gpui::api::ApiError;
use stoat_gpui::api::events::{ServerEvent, parse_frame};

use stoat_gpui::custom_emoji::{
    EmojiRow, at_limit, can_manage_emoji, check_emoji_name, count_line, delete_prompt,
    emoji_error_text, emoji_rows, name_from_file,
};
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::ChatStore;

const CUSTOMISE: u64 = ChannelPermission::ManageCustomisation as u64;
const STOAT: &str = "01ARYZ6S41TSV4RRFFQ69G5FAV";
const OTTER: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

fn user(id: &str, display: Option<&str>) -> Value {
    json!({
        "_id": id, "username": id, "discriminator": "0001", "display_name": display,
        "relationship": "None", "online": true
    })
}

fn member(user: &str, roles: &[&str]) -> Value {
    json!({
        "_id": { "server": "srv", "user": user },
        "joined_at": "2026-01-01T00:00:00.000Z",
        "roles": roles,
    })
}

fn emoji(id: &str, name: &str, creator: &str, server: &str) -> Value {
    json!({
        "_id": id, "parent": { "type": "Server", "id": server },
        "creator_id": creator, "name": name, "animated": name == "otter"
    })
}

/// `srv`, owned by `u-owner`: `u-artist` may manage its emoji, `u-plain`
/// may not. Its emoji are `stoat` (by `u-artist`) and `otter` (by someone
/// not cached); `other` has one too.
fn store(me: &str) -> ChatStore {
    let ready = json!({
        "type": "Ready",
        "users": [
            user("u-owner", None),
            user("u-artist", Some("Artist")),
            user("u-plain", None),
        ],
        "servers": [{
            "_id": "srv", "owner": "u-owner", "name": "Test", "channels": [],
            "approximate_member_count": 3,
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "roles": {
                "art": { "_id": "art", "name": "Art", "rank": 0, "permissions": { "a": CUSTOMISE, "d": 0 } }
            }
        }],
        "channels": [],
        "members": [
            member("u-owner", &[]),
            member("u-artist", &["art"]),
            member("u-plain", &[]),
        ],
        "emojis": [
            emoji(STOAT, "stoat", "u-artist", "srv"),
            emoji(OTTER, "otter", "u-gone", "srv"),
            emoji("01CX5ZZKBKACTAV9WEVGEMMVRZ", "elsewhere", "u-artist", "other"),
        ],
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new(me);
    store.apply_ready(*ready);
    store.set_media(MediaUrls::new(
        "https://api.example",
        Some("https://cdn.example".into()),
        None,
    ));
    store
}

fn api_error(body: Value) -> ApiError {
    ApiError::from_response(
        reqwest::StatusCode::BAD_REQUEST,
        body.to_string().as_bytes(),
    )
}

#[test]
fn a_servers_emoji_are_listed_by_name_with_their_creators() {
    assert_eq!(
        emoji_rows(&store("u-owner"), "srv"),
        [
            EmojiRow {
                id: OTTER.into(),
                name: "otter".into(),
                url: Some(format!("https://cdn.example/emojis/{OTTER}")),
                animated: true,
                creator: "u-gone".into(),
                creator_name: "Unknown user".into(),
            },
            EmojiRow {
                id: STOAT.into(),
                name: "stoat".into(),
                url: Some(format!("https://cdn.example/emojis/{STOAT}")),
                animated: false,
                creator: "u-artist".into(),
                creator_name: "Artist".into(),
            },
        ]
    );
    assert!(emoji_rows(&store("u-owner"), "nope").is_empty());
}

#[test]
fn manage_customisation_is_what_counts() {
    assert!(can_manage_emoji(&store("u-owner"), "srv"));
    assert!(can_manage_emoji(&store("u-artist"), "srv"));
    assert!(!can_manage_emoji(&store("u-plain"), "srv"));
    assert!(!can_manage_emoji(&store("u-owner"), "nope"));
    let access = |me| stoat_gpui::server_settings::access(&store(me), "srv").emojis;
    assert!(access("u-owner") && access("u-artist") && !access("u-plain"));
}

#[test]
fn the_texts_read_plainly() {
    assert_eq!(count_line(2, 100), "2 of 100 emoji");
    assert_eq!(
        delete_prompt("stoat"),
        (
            "Delete :stoat:?".to_string(),
            "Messages and reactions that use it won't show it any more."
        )
    );
    let text = |body| emoji_error_text(&api_error(body));
    assert_eq!(
        text(json!({ "type": "TooManyEmoji", "max": 100 })),
        "This server has 100 emoji, the most Stoat allows."
    );
    assert_eq!(
        text(json!({ "type": "FailedValidation", "error": "name" })),
        "Emoji names are 1–32 lowercase letters, numbers and underscores."
    );
    assert_eq!(
        text(json!({ "type": "MissingPermission", "permission": "ManageCustomisation" })),
        "You don't have permission to manage this server's emoji."
    );
    assert_eq!(
        text(json!({ "type": "NotFound" })),
        "That emoji was deleted."
    );
    assert_eq!(
        text(json!({ "type": "InternalError" })),
        "Couldn't change the emoji."
    );
}

#[test]
fn emoji_names_are_checked_as_stoat_does() {
    assert_eq!(check_emoji_name("  stoat_2 "), Ok("stoat_2".into()));
    assert_eq!(
        check_emoji_name("x".repeat(32).as_str()),
        Ok("x".repeat(32))
    );
    assert_eq!(check_emoji_name("   "), Err("An emoji needs a name."));
    let lowercase = Err("Use lowercase letters, numbers and underscores only.");
    assert_eq!(check_emoji_name("Stoat"), lowercase);
    assert_eq!(check_emoji_name("two words"), lowercase);
    assert_eq!(check_emoji_name("dash-ed"), lowercase);
    assert_eq!(check_emoji_name("café"), lowercase);
    assert_eq!(
        check_emoji_name("x".repeat(33).as_str()),
        Err("Emoji names are at most 32 characters.")
    );
}

#[test]
fn a_name_is_suggested_from_the_file_name() {
    let name = |path: &str| name_from_file(Path::new(path));
    assert_eq!(name("/tmp/My Cat!.png"), "my_cat");
    assert_eq!(name("party--parrot.gif"), "party_parrot");
    assert_eq!(name("__odd__.webp"), "odd");
    assert_eq!(name("ÉTÉ.png"), "t");
    assert_eq!(name("!!!.png"), "");
    assert_eq!(name(&format!("{}.png", "a".repeat(40))), "a".repeat(32));
    // Cut where a `_` would be left at the end.
    assert_eq!(name(&format!("{} b.png", "a".repeat(31))), "a".repeat(31));
    // What's suggested always passes the check, when anything's left.
    for path in ["/tmp/My Cat!.png", "party--parrot.gif", "__odd__.webp"] {
        assert!(check_emoji_name(&name(path)).is_ok(), "{path}");
    }
}

#[test]
fn uploading_stops_at_the_limit() {
    assert_eq!(at_limit(99, 100), None);
    assert_eq!(
        at_limit(100, 100).as_deref(),
        Some("This server has 100 emoji, the most Stoat allows.")
    );
    assert!(at_limit(3, 3).is_some());
}
