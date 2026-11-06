use stoat_gpui::api::RootConfig;
use stoat_gpui::links::message_link;

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
