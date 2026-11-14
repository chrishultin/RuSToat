//! The order of a server's channels: the top section (with the web
//! client's `"default"` category), and the categories a move sends.

use std::collections::HashSet;

use serde_json::{Value, json};
use stoat_gpui::api::models::{Category, Server};
use stoat_gpui::channel_order::{
    CategoryDrop, ChannelDrop, ChannelHover, DEFAULT_CATEGORY, Layout, category_target,
    channel_target, layout, move_category, move_channel, move_to_end,
};

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

fn before(id: &str) -> CategoryDrop {
    CategoryDrop::Before(id.into())
}

fn after(id: &str) -> CategoryDrop {
    CategoryDrop::After(id.into())
}

#[test]
fn a_category_lands_before_or_after_the_one_its_over() {
    let shown = ids(&["k1", "k2", "k3"]);
    assert_eq!(
        category_target(&shown, "k1", Some("k3"), 0.2),
        Some(before("k3"))
    );
    assert_eq!(
        category_target(&shown, "k1", Some("k3"), 0.8),
        Some(after("k3"))
    );
    assert_eq!(
        category_target(&shown, "k3", Some("k1"), 0.4),
        Some(before("k1"))
    );
    // Over the top section, it goes first.
    assert_eq!(category_target(&shown, "k3", None, 0.9), Some(before("k1")));
}

#[test]
fn a_category_dropped_where_it_is_shows_no_target() {
    let shown = ids(&["k1", "k2", "k3"]);
    // Over itself.
    assert_eq!(category_target(&shown, "k2", Some("k2"), 0.2), None);
    // Just after the one before it, or just before the one after it.
    assert_eq!(category_target(&shown, "k2", Some("k1"), 0.8), None);
    assert_eq!(category_target(&shown, "k2", Some("k3"), 0.2), None);
    // Already first.
    assert_eq!(category_target(&shown, "k1", None, 0.5), None);
    // Nothing shown, or not one of them.
    assert_eq!(category_target(&[], "k1", None, 0.5), None);
    assert_eq!(category_target(&shown, "k9", Some("k1"), 0.2), None);
}

/// `srv` with categories `k1` (`x`), `k2` (`y`) and `k3` (`z`), and `a`
/// uncategorized.
fn three() -> Server {
    server(
        &["a", "x", "y", "z"],
        &[
            ("k1", "One", &["x"]),
            ("k2", "Two", &["y"]),
            ("k3", "Three", &["z"]),
        ],
    )
}

#[test]
fn moving_a_category_keeps_every_channel_in_it() {
    let server = three();
    assert_eq!(
        move_category(&server, "k1", &after("k3")).unwrap(),
        [
            category("k2", "Two", &["y"]),
            category("k3", "Three", &["z"]),
            category("k1", "One", &["x"])
        ]
    );
    assert_eq!(
        move_category(&server, "k3", &before("k2")).unwrap(),
        [
            category("k1", "One", &["x"]),
            category("k3", "Three", &["z"]),
            category("k2", "Two", &["y"])
        ]
    );
}

#[test]
fn moving_a_category_where_it_is_or_unknown_changes_nothing() {
    let server = three();
    assert_eq!(move_category(&server, "k2", &after("k1")), None);
    assert_eq!(move_category(&server, "k2", &before("k2")), None);
    assert_eq!(move_category(&server, "k9", &before("k1")), None);
    assert_eq!(move_category(&server, "k1", &before("k9")), None);
}

#[test]
fn moving_a_category_keeps_default_first_and_doesnt_add_it() {
    let with_default = server(
        &["a", "x", "y"],
        &[
            ("k1", "One", &["x"]),
            (DEFAULT_CATEGORY, "Default", &["a"]),
            ("k2", "Two", &["y"]),
        ],
    );
    assert_eq!(
        move_category(&with_default, "k2", &before("k1")).unwrap(),
        [
            category(DEFAULT_CATEGORY, "Default", &["a"]),
            category("k2", "Two", &["y"]),
            category("k1", "One", &["x"])
        ]
    );
    // Without one, none is written.
    let edited = move_category(&three(), "k3", &before("k1")).unwrap();
    assert!(edited.iter().all(|c| c.id != DEFAULT_CATEGORY));
}

#[test]
fn moving_a_category_keeps_hidden_channels_and_categories() {
    // `k2` lists only a channel the user can't see, so it isn't shown, but
    // it keeps its place relative to the others.
    let server = server(
        &["x", "secret", "z"],
        &[
            ("k1", "One", &["x"]),
            ("k2", "Hidden", &["secret"]),
            ("k3", "Three", &["z"]),
        ],
    );
    assert_eq!(
        move_category(&server, "k3", &before("k1")).unwrap(),
        [
            category("k3", "Three", &["z"]),
            category("k1", "One", &["x"]),
            category("k2", "Hidden", &["secret"])
        ]
    );
}

#[test]
fn a_channel_lands_by_the_half_of_what_its_over() {
    let row = ChannelHover::Channel("x");
    assert_eq!(channel_target(row, 0.3), ChannelDrop::Before("x".into()));
    assert_eq!(channel_target(row, 0.7), ChannelDrop::After("x".into()));
    let heading = ChannelHover::Heading {
        category: "voice",
        above: Some("chat"),
    };
    assert_eq!(
        channel_target(heading, 0.3),
        ChannelDrop::EndOf(Some("chat".into()))
    );
    assert_eq!(
        channel_target(heading, 0.7),
        ChannelDrop::StartOf("voice".into())
    );
    // Above the first heading is the top section, even when it's empty.
    let first = ChannelHover::Heading {
        category: "chat",
        above: None,
    };
    assert_eq!(channel_target(first, 0.1), ChannelDrop::EndOf(None));
}

#[test]
fn a_channel_moves_within_its_section() {
    let server = plain();
    assert_eq!(
        move_channel(&server, "y", &ChannelDrop::Before("x".into())).unwrap(),
        [
            category("chat", "Chat", &["y", "x"]),
            category("voice", "Voice", &["z"])
        ]
    );
    // In the top section, any new order writes `"default"`.
    assert_eq!(
        move_channel(&server, "a", &ChannelDrop::After("c".into())).unwrap(),
        [
            category(DEFAULT_CATEGORY, "Default", &["b", "c", "a"]),
            category("chat", "Chat", &["x", "y"]),
            category("voice", "Voice", &["z"])
        ]
    );
}

#[test]
fn a_channel_moves_between_sections() {
    let server = plain();
    assert_eq!(
        move_channel(&server, "z", &ChannelDrop::After("x".into())).unwrap(),
        [
            category("chat", "Chat", &["x", "z", "y"]),
            category("voice", "Voice", &[])
        ]
    );
    assert_eq!(
        move_channel(&server, "b", &ChannelDrop::StartOf("voice".into())).unwrap(),
        [
            category("chat", "Chat", &["x", "y"]),
            category("voice", "Voice", &["b", "z"])
        ]
    );
    // Into the top section, beside one of its channels.
    assert_eq!(
        move_channel(&server, "x", &ChannelDrop::Before("a".into())).unwrap(),
        [
            category(DEFAULT_CATEGORY, "Default", &["x", "a", "b", "c"]),
            category("chat", "Chat", &["y"]),
            category("voice", "Voice", &["z"])
        ]
    );
}

#[test]
fn a_channel_into_an_empty_category() {
    let server = server(
        &["a", "x"],
        &[("chat", "Chat", &["x"]), ("empty", "Empty", &[])],
    );
    assert_eq!(
        move_channel(&server, "x", &ChannelDrop::StartOf("empty".into())).unwrap(),
        [
            category("chat", "Chat", &[]),
            category("empty", "Empty", &["x"])
        ]
    );
}

#[test]
fn a_channel_dropped_where_it_is_changes_nothing() {
    let server = plain();
    assert_eq!(
        move_channel(&server, "x", &ChannelDrop::Before("y".into())),
        None
    );
    assert_eq!(
        move_channel(&server, "x", &ChannelDrop::StartOf("chat".into())),
        None
    );
    assert_eq!(
        move_channel(&server, "y", &ChannelDrop::After("x".into())),
        None
    );
    assert_eq!(
        move_channel(&server, "y", &ChannelDrop::EndOf(Some("chat".into()))),
        None
    );
    // Onto itself.
    assert_eq!(
        move_channel(&server, "x", &ChannelDrop::After("x".into())),
        None
    );
}

#[test]
fn a_channel_beside_an_unknown_one_changes_nothing() {
    let server = plain();
    assert_eq!(
        move_channel(&server, "x", &ChannelDrop::Before("nope".into())),
        None
    );
    assert_eq!(
        move_channel(&server, "nope", &ChannelDrop::Before("x".into())),
        None
    );
    assert_eq!(
        move_channel(&server, "x", &ChannelDrop::StartOf("nope".into())),
        None
    );
}

#[test]
fn a_dropped_channel_leaves_hidden_ones_in_place() {
    // `secret` sits between `x` and `y`, unseen; `z` dropped after `x`
    // lands between `x` and `secret`.
    let server = server(
        &["x", "secret", "y", "z"],
        &[
            ("chat", "Chat", &["x", "secret", "y"]),
            ("voice", "Voice", &["z"]),
        ],
    );
    assert_eq!(
        move_channel(&server, "z", &ChannelDrop::After("x".into())).unwrap(),
        [
            category("chat", "Chat", &["x", "z", "secret", "y"]),
            category("voice", "Voice", &[])
        ]
    );
}

#[test]
fn every_channel_drop_lists_each_channel_once() {
    let server = server(
        &["a", "x", "b", "y", "z"],
        &[
            (DEFAULT_CATEGORY, "Default", &["b"]),
            ("chat", "Chat", &["x", "y", "gone"]),
            ("voice", "Voice", &["z"]),
        ],
    );
    let channels = ["a", "b", "x", "y", "z"];
    let mut targets = vec![
        ChannelDrop::EndOf(None),
        ChannelDrop::EndOf(Some("chat".into())),
        ChannelDrop::StartOf("voice".into()),
    ];
    for id in channels {
        targets.push(ChannelDrop::Before(id.into()));
        targets.push(ChannelDrop::After(id.into()));
    }
    for channel in channels {
        for target in &targets {
            if let Some(edited) = move_channel(&server, channel, target) {
                assert!(listed_once(&edited), "{channel} to {target:?}");
                // Every channel is still somewhere: listed, or uncategorized.
                let listed: HashSet<&String> = edited.iter().flat_map(|c| &c.channels).collect();
                assert!(
                    listed.contains(&"gone".to_string()),
                    "{channel} to {target:?}"
                );
            }
        }
    }
}
