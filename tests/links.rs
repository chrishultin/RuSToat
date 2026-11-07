use stoat_gpui::api::RootConfig;
use stoat_gpui::links::{invite_link, message_link};

#[test]
fn server_channel_links_include_the_server() {
    assert_eq!(
        message_link("https://stoat.chat", Some("srv"), "ch", "msg"),
        "https://stoat.chat/server/srv/channel/ch/msg"
    );
}

#[test]
fn dm_and_group_links_have_no_server() {
    assert_eq!(
        message_link("https://stoat.chat", None, "dm", "msg"),
        "https://stoat.chat/channel/dm/msg"
    );
}

#[test]
fn a_trailing_slash_on_app_is_dropped() {
    assert_eq!(
        message_link("https://chat.example.org/", Some("srv"), "ch", "msg"),
        "https://chat.example.org/server/srv/channel/ch/msg"
    );
}

#[test]
fn app_comes_from_the_root_config_when_sent() {
    let root: RootConfig =
        serde_json::from_str(include_str!("fixtures/root.json")).expect("root.json");
    assert_eq!(root.app.as_deref(), Some("https://stoat.chat"));

    // Older instances leave it out: no link to offer.
    let old: RootConfig =
        serde_json::from_str(r#"{"revolt":"0.8.0","ws":"wss://ws.example.org"}"#).unwrap();
    assert_eq!(old.app, None);
}

#[test]
fn invite_links_are_short_on_the_official_instance() {
    assert_eq!(
        invite_link("https://stoat.chat", "abc"),
        "https://stt.gg/abc"
    );
    assert_eq!(
        invite_link("https://stoat.chat/", "abc"),
        "https://stt.gg/abc"
    );
}

#[test]
fn invite_links_elsewhere_go_to_the_web_client() {
    assert_eq!(
        invite_link("https://chat.example.org", "abc"),
        "https://chat.example.org/invite/abc"
    );
    assert_eq!(
        invite_link("https://chat.example.org/", "abc"),
        "https://chat.example.org/invite/abc"
    );
}

#[test]
fn the_longest_invite_comes_from_the_root_config_when_sent() {
    let root: RootConfig = serde_json::from_value(serde_json::json!({
        "revolt": "0.16.2", "ws": "wss://events.stoat.chat",
        "features": { "limits": { "global": { "max_invite_duration_days": 30 } } }
    }))
    .unwrap();
    assert_eq!(
        root.features.limits.global.max_invite_duration_days,
        Some(30)
    );

    // The fixture (and older instances) leave it out.
    let root: RootConfig =
        serde_json::from_str(include_str!("fixtures/root.json")).expect("root.json");
    assert_eq!(root.features.limits.global.max_invite_duration_days, None);
}
