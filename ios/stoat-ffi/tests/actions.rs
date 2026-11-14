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

// Reactions.

use stoat_ffi::timeline::EmojiItem;
use wiremock::matchers::path_regex;

const THUMBS: &str = "👍";

fn reactions_path() -> String {
    format!("^/channels/ch-general/messages/{THEIRS}/reactions/.+$")
}

#[tokio::test]
async fn a_reaction_shows_at_once_and_its_echo_doesnt_double_it() {
    let events = Events::start().await;
    let api = api(&events).await;
    Mock::given(method("PUT"))
        .and(path_regex(reactions_path()))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    Mock::given(method("DELETE"))
        .and(path_regex(reactions_path()))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    let (controller, _) = opened(&api).await;

    controller
        .toggle_reaction("ch-general".into(), THEIRS.into(), THUMBS.into())
        .await
        .unwrap();
    let reactions = message(&controller, THEIRS).reactions;
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].key, THUMBS);
    assert_eq!(reactions[0].emoji, EmojiItem::Unicode { text: THUMBS.into() });
    assert_eq!((reactions[0].count, reactions[0].mine), (1, true));

    // Stoat's echo of the same reaction.
    events.send(json!({ "type": "MessageReact", "id": THEIRS, "channel_id": "ch-general",
                        "user_id": "u-me", "emoji_id": THUMBS }));
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(message(&controller, THEIRS).reactions[0].count, 1);

    // Tapping it again takes it back.
    controller
        .toggle_reaction("ch-general".into(), THEIRS.into(), THUMBS.into())
        .await
        .unwrap();
    assert!(message(&controller, THEIRS).reactions.is_empty());
}

#[tokio::test]
async fn a_refused_reaction_is_undone_with_desktops_text() {
    let events = Events::start().await;
    let api = api(&events).await;
    Mock::given(method("PUT"))
        .and(path_regex(reactions_path()))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "type": "InvalidOperation" })))
        .mount(&api)
        .await;
    let (controller, _) = opened(&api).await;
    let err = controller
        .toggle_reaction("ch-general".into(), THEIRS.into(), THUMBS.into())
        .await
        .unwrap_err();
    assert!(err.to_string().starts_with("Stoat didn't accept that reaction."), "{err}");
    assert!(message(&controller, THEIRS).reactions.is_empty());
}

/// #general with Ada's message carrying `extra` fields.
async fn opened_with(api: &MockServer, extra: Value) -> Arc<AccountController> {
    let mut page = page();
    let theirs = page["messages"][1].as_object_mut().unwrap();
    theirs.extend(extra.as_object().unwrap().clone());
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page))
        .mount(api)
        .await;
    let (controller, _) = connected(api).await;
    controller.open_channel("ch-general".into()).await.unwrap();
    controller
}

#[tokio::test]
async fn a_full_message_takes_no_new_emoji_but_offers_its_own() {
    let events = Events::start().await;
    let api = api(&events).await;
    // Twenty different emoji already: Stoat's limit.
    let twenty: serde_json::Map<String, Value> = stoat_gpui::store::COMMON_REACTIONS[..20]
        .iter()
        .map(|emoji| (emoji.to_string(), json!(["u-ada"])))
        .collect();
    let controller = opened_with(&api, json!({ "reactions": twenty })).await;

    // Not in the twenty: refused here, no request (none is mounted).
    let new = stoat_gpui::store::COMMON_REACTIONS[30];
    let err = controller
        .toggle_reaction("ch-general".into(), THEIRS.into(), new.into())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("as many different reactions"), "{err}");

    let picker = controller.reaction_picker("ch-general".into(), THEIRS.into());
    assert!(picker.full);
    assert_eq!(picker.choices.len(), 20);
    assert!(picker.note.is_some());
}

#[tokio::test]
async fn the_picker_offers_only_what_the_author_allowed() {
    let events = Events::start().await;
    let api = api(&events).await;
    let controller = opened_with(
        &api,
        json!({ "interactions": { "reactions": ["👍", "🎉"], "restrict_reactions": true } }),
    )
    .await;
    let picker = controller.reaction_picker("ch-general".into(), THEIRS.into());
    assert!(picker.restricted && !picker.full);
    let keys: Vec<_> = picker.choices.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["👍", "🎉"]);

    // An ordinary message gets the usual choices.
    let usual = controller.reaction_picker("ch-general".into(), MINE.into());
    assert!(!usual.restricted);
    assert_eq!(usual.choices[0].key, stoat_gpui::store::COMMON_REACTIONS[0]);
}

#[tokio::test]
async fn emoji_search_finds_unicode_and_the_servers_custom_emoji() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    let found = controller.search_emoji("ch-general".into(), ":thumbsup".into());
    assert_eq!(found[0].key, "👍");

    events.send(json!({ "type": "EmojiCreate", "_id": "01JE0000000000000000000E01",
                        "parent": { "type": "Server", "id": "srv-alpha" },
                        "creator_id": "u-ada", "name": "partyotter" }));
    wait_until("the emoji", || {
        !controller.search_emoji("ch-general".into(), "partyot".into()).is_empty()
    })
    .await;
    let found = controller.search_emoji("ch-general".into(), "partyot".into());
    assert_eq!(found[0].key, "01JE0000000000000000000E01");
    assert!(matches!(&found[0].emoji, EmojiItem::Custom { name, .. } if name == ":partyotter:"));
    // And it's offered in the picker after the common ones.
    let picker = controller.reaction_picker("ch-general".into(), MINE.into());
    assert_eq!(picker.choices.last().unwrap().key, "01JE0000000000000000000E01");
}
