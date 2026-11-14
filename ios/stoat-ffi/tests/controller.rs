//! `AccountController` against a fake Stoat: connecting, the lists Swift
//! reads, live updates, and pausing.

mod support;

use serde_json::json;
use stoat_ffi::controller::{ChannelKindItem, ConnectionStatus};
use support::{Events, Heard, api, connected};

#[tokio::test]
async fn connecting_reports_status_and_lists_the_servers() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, recorder) = connected(&api).await;

    let heard = recorder.heard();
    assert!(matches!(heard[0], Heard::Status(ConnectionStatus::Connecting { attempt: 1 })));
    let ready = heard
        .iter()
        .find_map(|h| match h {
            Heard::Changed(c) if c.ready => Some(c.clone()),
            _ => None,
        })
        .unwrap();
    assert!(ready.lists);

    let servers: Vec<_> = controller.servers().into_iter().map(|s| s.name).collect();
    assert_eq!(servers, ["beta lounge", "Alpha Team"]);
    let alpha = controller.server("srv-alpha".into()).unwrap();
    assert_eq!(alpha.initials, "AT");

    let sections = controller.server_channels("srv-alpha".into());
    let titles: Vec<_> = sections.iter().map(|s| s.title.clone()).collect();
    assert!(titles.contains(&Some("Chat".into())), "{titles:?}");
    let voice = sections
        .iter()
        .flat_map(|s| &s.channels)
        .find(|c| c.id == "ch-voice")
        .unwrap();
    assert_eq!(voice.kind, ChannelKindItem::Voice);

    let home = controller.home_channels();
    assert_eq!(home[0].kind, ChannelKindItem::SavedNotes);
    let ada = home.iter().find(|c| c.id == "dm-ada").unwrap();
    assert_eq!(ada.name, "Ada L.");
    assert_eq!(ada.kind, ChannelKindItem::DirectMessage);
    assert!(
        ada.avatar_url.as_deref().is_some_and(|url| url.starts_with(&api.uri())),
        "a default avatar from the API: {:?}",
        ada.avatar_url
    );
    assert_eq!(events.connections.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn server_icons_use_the_file_server_from_root() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, recorder) = connected(&api).await;
    // `GET /` may land after `Ready`; it says so with a change of its own.
    for _ in 0..100 {
        if controller.server("srv-alpha".into()).unwrap().icon_url.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let icon = controller.server("srv-alpha".into()).unwrap().icon_url;
    assert_eq!(
        icon.as_deref(),
        Some("https://cdn.stoatusercontent.com/icons/file-alpha-icon"),
        "{:#?}",
        recorder.heard()
    );
}

#[tokio::test]
async fn a_live_message_marks_the_channel_unread() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, recorder) = connected(&api).await;
    let random = |c: &stoat_ffi::controller::AccountController| {
        c.server_channels("srv-alpha".into())
            .into_iter()
            .flat_map(|s| s.channels)
            .find(|c| c.id == "ch-random")
            .unwrap()
    };
    assert!(!random(&controller).unread);
    recorder.clear();

    events.send(json!({ "type": "Message", "_id": "01JC00000000000000000000A0",
                        "channel": "ch-random", "author": "u-ada", "content": "hi" }));
    recorder
        .wait_for("a list change", |h| matches!(h, Heard::Changed(c) if c.lists))
        .await;
    assert!(random(&controller).unread);
    assert!(controller.summary().servers_unread);
}

#[tokio::test]
async fn a_burst_of_events_is_told_once() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (_controller, recorder) = connected(&api).await;
    recorder.clear();

    for n in 0..20 {
        events.send(json!({ "type": "Message", "_id": format!("01JC000000000000000000B{n:03}"),
                            "channel": "ch-random", "author": "u-ada", "content": "hi" }));
    }
    recorder
        .wait_for("a list change", |h| matches!(h, Heard::Changed(c) if c.lists))
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let changes = recorder
        .heard()
        .into_iter()
        .filter(|h| matches!(h, Heard::Changed(_)))
        .count();
    assert!(changes <= 2, "20 messages, {changes} changes: {:#?}", recorder.heard());
}

#[tokio::test]
async fn disconnect_pauses_and_connect_resumes() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, recorder) = connected(&api).await;

    controller.disconnect();
    recorder.wait_for_status(ConnectionStatus::Paused).await;
    // Still drawable while paused.
    assert_eq!(controller.servers().len(), 2);

    recorder.clear();
    controller.connect();
    recorder.wait_for_status(ConnectionStatus::Connected).await;
    recorder
        .wait_for("Ready", |h| matches!(h, Heard::Changed(c) if c.ready))
        .await;
    assert_eq!(events.connections.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_dropped_connection_reconnects() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (_controller, recorder) = connected(&api).await;
    recorder.clear();

    events.drop_connections();
    recorder
        .wait_for("Disconnected", |h| {
            matches!(h, Heard::Status(ConnectionStatus::Disconnected { .. }))
        })
        .await;
    recorder.wait_for_status(ConnectionStatus::Connected).await;
    recorder
        .wait_for("Ready", |h| matches!(h, Heard::Changed(c) if c.ready))
        .await;
}

#[tokio::test]
async fn a_rejected_session_is_reported() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (_controller, recorder) = connected(&api).await;
    events.send(json!({ "type": "Logout" }));
    recorder.wait_for_status(ConnectionStatus::SessionRejected).await;
}

#[tokio::test]
async fn me_is_the_signed_in_user() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = connected(&api).await;
    let me = controller.me().unwrap();
    assert_eq!(me.user_id, "u-me");
    assert!(me.username.starts_with("me#"), "{}", me.username);
    assert!(me.avatar_url.starts_with(&api.uri()), "a default avatar: {}", me.avatar_url);
}
