use stoat_gpui::api::RootConfig;
use stoat_gpui::invites::InviteHosts;
use stoat_gpui::links::{ChannelLink, channel_from_link, invite_link, message_link};

const SERVER: &str = "01FD4WDPDDRW4B003G94V1Y73X";
const CHANNEL: &str = "01M4Y2PA27KZK3V5G28MZKGH9N";
const MESSAGE: &str = "01M6VW0J4YMRGZ5SRBCRVVYCXY";

fn official() -> InviteHosts {
    InviteHosts::new(Some("https://stoat.chat"))
}

fn to(message: Option<&str>) -> Option<ChannelLink> {
    Some(ChannelLink {
        channel: CHANNEL.into(),
        message: message.map(Into::into),
    })
}

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

#[test]
fn the_largest_group_comes_from_the_root_config_when_sent() {
    let root: RootConfig =
        serde_json::from_str(include_str!("fixtures/root.json")).expect("root.json");
    assert_eq!(root.features.limits.global.group_size, Some(100));

    let root: RootConfig = serde_json::from_value(serde_json::json!({
        "revolt": "0.16.2", "ws": "wss://events.stoat.chat",
        "features": { "limits": { "global": {} } }
    }))
    .unwrap();
    assert_eq!(root.features.limits.global.group_size, None);
}

#[test]
fn message_links_lead_to_their_channel_and_message() {
    let hosts = official();
    for link in [
        format!("https://stoat.chat/server/{SERVER}/channel/{CHANNEL}/{MESSAGE}"),
        format!("https://stoat.chat/channel/{CHANNEL}/{MESSAGE}"),
        format!("https://stoat.chat/server/{SERVER}/channel/{CHANNEL}/{MESSAGE}/"),
        format!("https://STOAT.chat/channel/{CHANNEL}/{MESSAGE}?x=1#top"),
        format!("https://app.revolt.chat/channel/{CHANNEL}/{MESSAGE}"),
        format!("stoat.chat/channel/{CHANNEL}/{MESSAGE}"),
    ] {
        assert_eq!(
            channel_from_link(&link, &hosts),
            to(Some(MESSAGE)),
            "{link}"
        );
    }
}

#[test]
fn channel_links_lead_to_their_channel() {
    let hosts = official();
    for link in [
        format!("https://stoat.chat/server/{SERVER}/channel/{CHANNEL}"),
        format!("https://stoat.chat/channel/{CHANNEL}/"),
    ] {
        assert_eq!(channel_from_link(&link, &hosts), to(None), "{link}");
    }
}

#[test]
fn a_made_message_link_reads_back() {
    let hosts = InviteHosts::new(Some("https://chat.example.org:8443"));
    let link = message_link(
        "https://chat.example.org:8443",
        Some(SERVER),
        CHANNEL,
        MESSAGE,
    );
    assert_eq!(channel_from_link(&link, &hosts), to(Some(MESSAGE)));
}

#[test]
fn other_links_are_not_channel_links() {
    let hosts = official();
    for link in [
        // Elsewhere, or not the web client.
        format!("https://example.com/channel/{CHANNEL}/{MESSAGE}"),
        format!("https://stt.gg/channel/{CHANNEL}/{MESSAGE}"),
        format!("https://stoat.chat:8080/channel/{CHANNEL}/{MESSAGE}"),
        format!("https://stoat.chat@example.com/channel/{CHANNEL}/{MESSAGE}"),
        format!("ftp://stoat.chat/channel/{CHANNEL}/{MESSAGE}"),
        // Not a channel or message.
        format!("https://stoat.chat/server/{SERVER}"),
        "https://stoat.chat/invite/wVEJDGVs".to_string(),
        "https://stoat.chat/".to_string(),
        format!("https://stoat.chat/channel/{CHANNEL}/{MESSAGE}/more"),
        format!("https://stoat.chat/server/{SERVER}/channel/{CHANNEL}/settings"),
        "https://stoat.chat/channel/general".to_string(),
        format!("https://stoat.chat/server/x/channel/{CHANNEL}"),
    ] {
        assert_eq!(channel_from_link(&link, &hosts), None, "{link}");
    }
}

#[test]
fn self_hosted_instances_only_know_their_own_links() {
    let hosts = InviteHosts::new(Some("https://chat.example.org"));
    let own = format!("https://chat.example.org/channel/{CHANNEL}/{MESSAGE}");
    assert_eq!(channel_from_link(&own, &hosts), to(Some(MESSAGE)));
    let official = format!("https://stoat.chat/channel/{CHANNEL}/{MESSAGE}");
    assert_eq!(channel_from_link(&official, &hosts), None);

    // An instance that doesn't say where its web client is has none.
    let unknown = InviteHosts::new(None);
    assert_eq!(channel_from_link(&own, &unknown), None);
}
