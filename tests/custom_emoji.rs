//! A server's custom emoji, as Server Settings shows them: the list, who
//! may manage them, and the texts.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::custom_emoji::{
    EmojiRow, can_manage_emoji, count_line, delete_prompt, emoji_error_text, emoji_rows,
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
