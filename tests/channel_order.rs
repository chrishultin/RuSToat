//! The order of a server's channels: the top section (with the web
//! client's `"default"` category), and the categories a move sends.

use std::collections::HashSet;

use serde_json::{Value, json};
use stoat_gpui::api::models::{Category, Server};
use stoat_gpui::channel_order::{DEFAULT_CATEGORY, Layout, layout, move_to_end};

/// A server with `channels` in its own order, and `categories` as
/// `(id, title, channels)`.
fn server(channels: &[&str], categories: &[(&str, &str, &[&str])]) -> Server {
    let categories: Vec<Value> = categories
        .iter()
        .map(|(id, title, channels)| json!({ "id": id, "title": title, "channels": channels }))
        .collect();
    serde_json::from_value(json!({
        "_id": "srv", "owner": "u-owner", "name": "Test",
        "channels": channels,
        "categories": categories,
        "approximate_member_count": 1,
        "default_permissions": 0,
    }))
    .unwrap()
}

fn category(id: &str, title: &str, channels: &[&str]) -> Category {
    Category {
        id: id.into(),
        title: title.into(),
        channels: channels.iter().map(|c| c.to_string()).collect(),
    }
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|c| c.to_string()).collect()
}

/// No channel is listed twice in `categories`.
fn listed_once(categories: &[Category]) -> bool {
    let mut seen = HashSet::new();
    categories
        .iter()
        .flat_map(|c| &c.channels)
        .all(|id| seen.insert(id))
}

/// `srv`: `a`, `b` and `c` uncategorized (in that order on the server),
/// `x` and `y` in Chat, `z` in Voice.
fn plain() -> Server {
    server(
        &["a", "x", "b", "y", "c", "z"],
        &[("chat", "Chat", &["x", "y"]), ("voice", "Voice", &["z"])],
    )
}

#[test]
fn without_categories_the_top_section_is_the_servers_order() {
    let server = server(&["b", "a", "c"], &[]);
    assert_eq!(
        layout(&server),
        Layout {
            top: ids(&["b", "a", "c"]),
            categories: Vec::new(),
        }
    );
}

#[test]
fn uncategorized_channels_keep_the_servers_order() {
    let layout = layout(&plain());
    assert_eq!(layout.top, ids(&["a", "b", "c"]));
    assert_eq!(
        layout.categories,
        [
            category("chat", "Chat", &["x", "y"]),
            category("voice", "Voice", &["z"])
        ]
    );
}

#[test]
fn default_lists_the_top_section_first_wherever_it_is() {
    let server = server(
        &["a", "x", "b", "c"],
        &[
            ("chat", "Chat", &["x"]),
            (DEFAULT_CATEGORY, "Default", &["c", "a"]),
        ],
    );
    let layout = layout(&server);
    // Its own channels, then the rest uncategorized, in server order.
    assert_eq!(layout.top, ids(&["c", "a", "b"]));
    assert_eq!(layout.categories, [category("chat", "Chat", &["x"])]);
}

#[test]
fn hidden_and_stale_ids_stay_in_the_layout() {
    let server = server(&["a", "x"], &[("chat", "Chat", &["gone", "x", "secret"])]);
    let layout = layout(&server);
    assert_eq!(layout.categories[0].channels, ids(&["gone", "x", "secret"]));
}

#[test]
fn moving_into_a_category_puts_it_last_there() {
    let server = plain();
    let edited = move_to_end(&server, "b", Some("chat")).unwrap();
    assert_eq!(
        edited,
        [
            category("chat", "Chat", &["x", "y", "b"]),
            category("voice", "Voice", &["z"])
        ]
    );
    // From one category to another.
    let edited = move_to_end(&server, "x", Some("voice")).unwrap();
    assert_eq!(
        edited,
        [
            category("chat", "Chat", &["y"]),
            category("voice", "Voice", &["z", "x"])
        ]
    );
}

#[test]
fn moving_out_writes_default_only_when_the_order_needs_it() {
    let server = plain();
    // On the server `y` is before `c`, so the end of the top section isn't
    // where Stoat would put it.
    let edited = move_to_end(&server, "y", None).unwrap();
    assert_eq!(
        edited,
        [
            category(DEFAULT_CATEGORY, "Default", &["a", "b", "c", "y"]),
            category("chat", "Chat", &["x"]),
            category("voice", "Voice", &["z"])
        ]
    );
    // `z` is last on the server, so the end is where it goes anyway.
    let edited = move_to_end(&server, "z", None).unwrap();
    assert_eq!(
        edited,
        [
            category("chat", "Chat", &["x", "y"]),
            category("voice", "Voice", &[])
        ]
    );
}

#[test]
fn an_existing_default_is_kept_first_with_its_title() {
    let server = server(
        &["a", "x", "b"],
        &[("chat", "Chat", &["x"]), (DEFAULT_CATEGORY, "Top", &["b"])],
    );
    let edited = move_to_end(&server, "x", None).unwrap();
    assert_eq!(
        edited,
        [
            category(DEFAULT_CATEGORY, "Top", &["b", "a", "x"]),
            category("chat", "Chat", &[])
        ]
    );
}

#[test]
fn moving_to_where_it_is_changes_nothing() {
    let server = plain();
    assert_eq!(move_to_end(&server, "y", Some("chat")), None);
    assert_eq!(move_to_end(&server, "c", None), None);
}

#[test]
fn an_unknown_channel_or_category_changes_nothing() {
    let server = plain();
    assert_eq!(move_to_end(&server, "nope", Some("chat")), None);
    assert_eq!(move_to_end(&server, "a", Some("nope")), None);
}

#[test]
fn hidden_channels_keep_their_places() {
    // `secret` is in Chat, between `x` and `y`, whether or not the user
    // can see it.
    let server = server(
        &["a", "x", "secret", "y"],
        &[("chat", "Chat", &["x", "secret", "y"])],
    );
    let edited = move_to_end(&server, "x", None).unwrap();
    assert_eq!(edited.last().unwrap().channels, ids(&["secret", "y"]));
}

#[test]
fn every_move_lists_each_channel_once() {
    let server = server(
        &["a", "x", "b", "y"],
        &[
            (DEFAULT_CATEGORY, "Default", &["b"]),
            ("chat", "Chat", &["x", "y"]),
        ],
    );
    for channel in ["a", "b", "x", "y"] {
        for section in [None, Some("chat")] {
            if let Some(edited) = move_to_end(&server, channel, section) {
                assert!(listed_once(&edited), "{channel} to {section:?}");
            }
        }
    }
}
