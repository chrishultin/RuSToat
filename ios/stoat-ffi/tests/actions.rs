//! Message actions through `AccountController`: copying, links and pins
//! (then reactions, editing and deleting), against a fake Stoat.

mod support;

use std::sync::Arc;

use serde_json::{Value, json};
use stoat_ffi::controller::AccountController;
use stoat_ffi::timeline::{MessageItem, TimelineItem};
use support::{Events, Recorder, api, connected};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MINE: &str = "01JD0000000000000000000002";
const THEIRS: &str = "01JD0000000000000000000001";

fn page() -> Value {
    json!({ "messages": [
        { "_id": MINE, "channel": "ch-general", "author": "u-me", "content": "**mine**" },
        { "_id": THEIRS, "channel": "ch-general", "author": "u-ada", "content": "hello <@u-me>" }
    ], "users": [] })
}

/// Connected, with #general open on two messages: Ada's, then the user's.
async fn opened(api: &MockServer) -> (Arc<AccountController>, Recorder) {
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page()))
        .mount(api)
        .await;
    let (controller, recorder) = connected(api).await;
    controller.open_channel("ch-general".into()).await.unwrap();
    (controller, recorder)
}

fn message(controller: &AccountController, id: &str) -> MessageItem {
    controller
        .timeline("ch-general".into())
        .into_iter()
        .find_map(|row| match row {
            TimelineItem::Message { message } if message.id == id => Some(message),
            _ => None,
        })
        .unwrap()
}

async fn wait_until(what: &str, check: impl Fn() -> bool) {
    for _ in 0..500 {
        if check() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("never: {what}");
}

#[tokio::test]
async fn copy_text_is_the_markdown_as_typed() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    assert_eq!(
        controller.raw_text("ch-general".into(), THEIRS.into()).as_deref(),
        Some("hello <@u-me>")
    );
    assert_eq!(controller.raw_text("ch-general".into(), "nope".into()), None);
}

#[tokio::test]
async fn message_links_use_the_web_client() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    // `GET /` answers in the background.
    wait_until("the root config", || {
        controller.message_link("ch-general".into(), MINE.into()).is_some()
    })
    .await;
    assert_eq!(
        controller.message_link("ch-general".into(), MINE.into()).as_deref(),
        Some(format!("https://stoat.chat/server/srv-alpha/channel/ch-general/{MINE}").as_str())
    );
    assert_eq!(
        controller.message_link("dm-ada".into(), MINE.into()).as_deref(),
        Some(format!("https://stoat.chat/channel/dm-ada/{MINE}").as_str()),
        "DMs have no server in the link"
    );
}

#[tokio::test]
async fn pinning_shows_once_stoat_says_so() {
    let events = Events::start().await;
    let api = api(&events).await;
    Mock::given(method("POST"))
        .and(path(format!("/channels/ch-general/messages/{THEIRS}/pin")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    let (controller, _) = opened(&api).await;
    // The fixture's member has no Manage Messages, so the menu wouldn't
    // offer it there; Stoat decides either way.
    assert!(!controller.can_pin("ch-general".into()));
    assert!(!message(&controller, THEIRS).pinned);

    controller
        .set_pinned("ch-general".into(), THEIRS.into(), true)
        .await
        .unwrap();
    events.send(json!({ "type": "MessageUpdate", "id": THEIRS, "channel": "ch-general",
                        "data": { "pinned": true }, "clear": [] }));
    wait_until("pinned", || message(&controller, THEIRS).pinned).await;
}

#[tokio::test]
async fn already_pinned_catches_up() {
    let events = Events::start().await;
    let api = api(&events).await;
    Mock::given(method("POST"))
        .and(path(format!("/channels/ch-general/messages/{THEIRS}/pin")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "type": "AlreadyPinned" })))
        .mount(&api)
        .await;
    let (controller, _) = opened(&api).await;
    controller
        .set_pinned("ch-general".into(), THEIRS.into(), true)
        .await
        .unwrap();
    assert!(message(&controller, THEIRS).pinned);
}

#[tokio::test]
async fn a_refused_pin_says_why() {
    let events = Events::start().await;
    let api = api(&events).await;
    Mock::given(method("DELETE"))
        .and(path(format!("/channels/ch-general/messages/{THEIRS}/pin")))
        .respond_with(ResponseTemplate::new(403).set_body_json(
            json!({ "type": "MissingPermission", "permission": "ManageMessages" }),
        ))
        .mount(&api)
        .await;
    let (controller, _) = opened(&api).await;
    let err = controller
        .set_pinned("ch-general".into(), THEIRS.into(), false)
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "You can't unpin messages here.");
}
