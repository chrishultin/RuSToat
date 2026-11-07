//! Invite links in messages: the codes on each row, for their cards, and
//! website previews of those links left out.

use serde_json::{Value, json};
use stoat_gpui::api::models::BulkMessageResponse;
use stoat_gpui::embed::EmbedView;
use stoat_gpui::invites::InviteHosts;
use stoat_gpui::store::{ChatStore, MessageRow, TimelineRow};

const MSG: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";

/// A store whose invite links are `hosts`, with one message, `message`
/// (its `_id`, `channel` and `author` are filled in).
fn store_with(message: Value, hosts: InviteHosts) -> ChatStore {
    let mut store = ChatStore::new("u-me");
    store.set_invite_hosts(hosts);
    let mut message = message;
    message.as_object_mut().unwrap().extend(
        json!({ "_id": MSG, "channel": "ch-1", "author": "u-ada" })
            .as_object()
            .unwrap()
            .clone(),
    );
    let page: BulkMessageResponse = serde_json::from_value(json!([message])).unwrap();
    store.set_history("ch-1", page, 50);
    store
}

fn official() -> InviteHosts {
    InviteHosts::new(Some("https://stoat.chat"))
}

fn row(store: &ChatStore) -> MessageRow {
    store
        .timeline("ch-1")
        .into_iter()
        .find_map(|row| match row {
            TimelineRow::Message(message) => Some(message),
            _ => None,
        })
        .expect("a message row")
}

#[test]
fn a_message_with_invite_links_has_their_codes() {
    let store = store_with(
        json!({ "content": "come join https://stt.gg/abc and stoat.chat/invite/def" }),
        official(),
    );
    assert_eq!(row(&store).invites, ["abc", "def"]);
}

#[test]
fn without_the_instance_known_there_are_no_cards() {
    let store = store_with(
        json!({ "content": "https://stt.gg/abc" }),
        InviteHosts::default(),
    );
    assert!(row(&store).invites.is_empty());
}

#[test]
fn a_system_message_has_no_cards() {
    let store = store_with(
        json!({ "system": { "type": "text", "content": "https://stt.gg/abc" } }),
        official(),
    );
    assert!(row(&store).invites.is_empty());
}

#[test]
fn a_website_preview_of_an_invite_is_left_out() {
    let store = store_with(
        json!({
            "content": "https://stt.gg/abc https://news.example/story",
            "embeds": [
                { "type": "Website", "url": "https://stt.gg/abc", "title": "Join Rust on Stoat" },
                { "type": "Website", "url": "https://news.example/story", "title": "A story" }
            ]
        }),
        official(),
    );
    let row = row(&store);
    assert_eq!(row.invites, ["abc"]);
    let titles: Vec<Option<String>> = row
        .embeds
        .iter()
        .map(|embed| match embed {
            EmbedView::Card(card) => card.title.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(titles, [Some("A story".to_string())]);
}
