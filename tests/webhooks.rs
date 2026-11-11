//! A channel's webhooks: who may manage them, the list as the store keeps
//! it (fetched, then kept by events), the URL, and the texts.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::Webhook;
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::ChatStore;
use stoat_gpui::webhooks::{
    WebhookRow, can_manage_webhooks, check_webhook_name, delete_prompt, list_error_text, shown_url,
    webhook_error_text, webhook_rows, webhook_url, webhooks_unavailable,
};

const HOOKS: u64 = ChannelPermission::ManageWebhooks as u64;

fn user(id: &str) -> Value {
    json!({
        "_id": id, "username": id, "discriminator": "0001",
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

/// `srv`, owned by `u-owner`: `u-hooks` may manage webhooks everywhere,
/// `u-local` only in `ch-b` (its override), `u-plain` nowhere. `grp` is
/// owned by `u-owner`.
fn store(me: &str) -> ChatStore {
    let everyone = ["u-owner", "u-hooks", "u-local", "u-plain"];
    let users: Vec<Value> = everyone.iter().map(|id| user(id)).collect();
    let ready = json!({
        "type": "Ready",
        "users": users,
        "servers": [{
            "_id": "srv", "owner": "u-owner", "name": "Test", "channels": ["ch-a", "ch-b"],
            "approximate_member_count": 4,
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "roles": {
                "hooks": { "_id": "hooks", "name": "Hooks", "rank": 0, "permissions": { "a": HOOKS, "d": 0 } },
                "local": { "_id": "local", "name": "Local", "rank": 1, "permissions": { "a": 0, "d": 0 } }
            }
        }],
        "channels": [
            { "channel_type": "TextChannel", "_id": "ch-a", "server": "srv", "name": "a" },
            {
                "channel_type": "TextChannel", "_id": "ch-b", "server": "srv", "name": "b",
                "role_permissions": { "local": { "a": HOOKS, "d": 0 } }
            },
            {
                "channel_type": "Group", "_id": "grp", "name": "Friends", "owner": "u-owner",
                "recipients": everyone
            }
        ],
        "members": [
            member("u-owner", &[]),
            member("u-hooks", &["hooks"]),
            member("u-local", &["local"]),
            member("u-plain", &[]),
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

fn webhook(id: &str, name: &str, token: Option<&str>) -> Webhook {
    serde_json::from_value(json!({
        "id": id, "name": name, "creator_id": "u-owner", "channel_id": "ch-a",
        "permissions": 0, "token": token
    }))
    .unwrap()
}

fn api_error(status: u16, body: &str) -> ApiError {
    ApiError::from_response(
        reqwest::StatusCode::from_u16(status).unwrap(),
        body.as_bytes(),
    )
}

#[test]
fn manage_webhooks_in_the_channel_is_what_counts() {
    for me in ["u-owner", "u-hooks"] {
        let store = store(me);
        assert!(can_manage_webhooks(&store, "ch-a"), "{me}");
        assert!(can_manage_webhooks(&store, "ch-b"), "{me}");
    }
    let local = store("u-local");
    assert!(can_manage_webhooks(&local, "ch-b"));
    assert!(!can_manage_webhooks(&local, "ch-a"));
    assert!(!can_manage_webhooks(&store("u-plain"), "ch-a"));
    // A group's owner may; its members don't by default.
    assert!(can_manage_webhooks(&store("u-owner"), "grp"));
    assert!(!can_manage_webhooks(&store("u-plain"), "grp"));
    // And it's the Webhooks page's access.
    let access = |me, channel| stoat_gpui::channel_settings::access(&store(me), channel).webhooks;
    assert!(access("u-local", "ch-b") && !access("u-local", "ch-a"));
}

#[test]
fn the_list_is_none_until_fetched_then_by_name() {
    let mut store = store("u-owner");
    assert_eq!(webhook_rows(&store, "ch-a"), None);
    let mut with_avatar = webhook("w2", "Alerts", None);
    with_avatar.avatar = Some(
        serde_json::from_value(json!({
            "_id": "av", "tag": "avatars", "filename": "a.png",
            "metadata": { "type": "Image", "width": 64, "height": 64 },
            "content_type": "image/png", "size": 100
        }))
        .unwrap(),
    );
    store.set_webhooks(
        "ch-a",
        vec![webhook("w1", "Builds", Some("tok1")), with_avatar],
    );
    assert_eq!(
        webhook_rows(&store, "ch-a"),
        Some(vec![
            WebhookRow {
                id: "w2".into(),
                name: "Alerts".into(),
                avatar_url: Some("https://cdn.example/avatars/av".into()),
                token_known: false,
            },
            WebhookRow {
                id: "w1".into(),
                name: "Builds".into(),
                avatar_url: None,
                token_known: true,
            },
        ])
    );
}

/// The webhook events in `frame`, applied as the chat view does.
fn apply(store: &mut ChatStore, frame: Value) {
    for event in parse_frame(&frame.to_string()) {
        match event {
            ServerEvent::WebhookCreate(webhook) => store.add_webhook(*webhook),
            ServerEvent::WebhookUpdate {
                id,
                channel,
                data,
                remove,
            } => store.apply_webhook_update(&channel, &id, *data, &remove),
            ServerEvent::WebhookDelete { id, channel } => store.remove_webhook(&channel, &id),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn events_keep_a_fetched_list_and_its_tokens() {
    let mut store = store("u-owner");
    store.set_webhooks("ch-a", vec![webhook("w1", "Builds", Some("tok1"))]);
    apply(
        &mut store,
        json!({ "type": "Bulk", "v": [
            { "type": "WebhookCreate", "id": "w2", "name": "Alerts", "creator_id": "u-x",
              "channel_id": "ch-a", "permissions": 0 },
            { "type": "WebhookUpdate", "id": "w1", "channel_id": "ch-a",
              "data": { "name": "CI" }, "remove": [] },
        ]}),
    );
    let list = store.webhooks("ch-a").unwrap();
    assert_eq!(list.len(), 2);
    let w1 = list.iter().find(|w| w.id == "w1").unwrap();
    assert_eq!(
        (w1.name.as_str(), w1.token.as_deref()),
        ("CI", Some("tok1"))
    );
    // Made elsewhere: no token yet.
    assert!(list.iter().any(|w| w.id == "w2" && w.token.is_none()));

    // A create event for a known webhook (or the answer to one) keeps its
    // token.
    store.add_webhook(webhook("w1", "CI", None));
    let w1 = store
        .webhooks("ch-a")
        .unwrap()
        .iter()
        .find(|w| w.id == "w1")
        .unwrap();
    assert_eq!(w1.token.as_deref(), Some("tok1"));

    apply(
        &mut store,
        json!({ "type": "WebhookDelete", "id": "w1", "channel_id": "ch-a" }),
    );
    let ids: Vec<&str> = store
        .webhooks("ch-a")
        .unwrap()
        .iter()
        .map(|w| w.id.as_str())
        .collect();
    assert_eq!(ids, ["w2"]);
}

#[test]
fn events_for_an_unfetched_channel_are_ignored() {
    let mut store = store("u-owner");
    apply(
        &mut store,
        json!({ "type": "WebhookCreate", "id": "w9", "name": "X", "creator_id": "u-x",
                "channel_id": "ch-b", "permissions": 0 }),
    );
    assert_eq!(store.webhooks("ch-b"), None);
}

#[test]
fn a_reconnect_forgets_the_lists() {
    let mut store = store("u-owner");
    store.set_webhooks("ch-a", vec![webhook("w1", "Builds", Some("tok1"))]);
    let ready =
        json!({ "type": "Ready", "users": [], "servers": [], "channels": [], "members": [] });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    store.apply_ready(*ready);
    assert_eq!(store.webhooks("ch-a"), None);
}

#[test]
fn the_url_hides_its_token_until_copied() {
    assert_eq!(
        webhook_url("https://api.stoat.chat/", "w1", "secret"),
        "https://api.stoat.chat/webhooks/w1/secret"
    );
    assert_eq!(
        shown_url("https://api.stoat.chat", "w1"),
        "https://api.stoat.chat/webhooks/w1/••••••"
    );
    assert!(!shown_url("https://api.stoat.chat", "w1").contains("secret"));
}

#[test]
fn webhook_names_and_texts() {
    assert_eq!(check_webhook_name("  CI  "), Some("CI".into()));
    assert_eq!(check_webhook_name(" "), None);
    assert_eq!(check_webhook_name(&"x".repeat(33)), None);
    assert_eq!(check_webhook_name(&"x".repeat(32)), Some("x".repeat(32)));
    assert_eq!(
        delete_prompt("CI"),
        (
            "Delete CI?".to_string(),
            "Anything posting to its URL will stop working."
        )
    );
    // An unmounted route (an HTML 404) is "no webhooks here"; Stoat's own
    // NotFound isn't.
    let missing = api_error(404, "<html>404: Not Found</html>");
    assert!(webhooks_unavailable(&missing));
    assert_eq!(
        list_error_text(&missing),
        "This Stoat instance doesn't have webhooks."
    );
    let gone = api_error(404, r#"{"type":"NotFound"}"#);
    assert!(!webhooks_unavailable(&gone));
    assert_eq!(list_error_text(&gone), "That webhook was deleted.");
    let text = |body: &str| webhook_error_text(&api_error(400, body));
    assert_eq!(
        text(r#"{"type":"MissingPermission","permission":"ManageWebhooks"}"#),
        "You don't have permission to manage webhooks here."
    );
    assert_eq!(
        text(r#"{"type":"FailedValidation","error":"name"}"#),
        "Webhook names are 1–32 characters."
    );
    assert_eq!(
        text(r#"{"type":"InvalidOperation"}"#),
        "Webhooks can't be made here."
    );
    assert_eq!(
        text(r#"{"type":"InternalError"}"#),
        "Couldn't change the webhook."
    );
}
