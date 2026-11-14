//! Reading a channel through `AccountController`: the first page, older
//! pages, live changes, marking read, and reloading after a reconnect.

mod support;

use serde_json::{Value, json};
use stoat_ffi::controller::AccountController;
use stoat_ffi::timeline::TimelineItem;
use support::{Events, Heard, api, connected};
use wiremock::matchers::{method, path, path_regex, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Message `n`'s ID: ULIDs that sort (and date) in order of `n`.
fn id(n: usize) -> String {
    format!("01JD{n:022}")
}

/// Messages `from..to`, newest first as Stoat sends them, in #general.
fn page(from: usize, to: usize) -> Value {
    let messages: Vec<Value> = (from..to)
        .rev()
        .map(|n| {
            json!({ "_id": id(n), "channel": "ch-general",
                    "author": if n % 2 == 0 { "u-ada" } else { "u-bob" },
                    "content": format!("message {n}") })
        })
        .collect();
    json!({ "messages": messages, "users": [] })
}

/// #general's latest page is messages 100..150; before 100 come 97..100.
async fn mount_history(api: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .and(query_param_is_missing("before"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(100, 150)))
        .mount(api)
        .await;
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .and(query_param("before", id(100)))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(97, 100)))
        .mount(api)
        .await;
}

fn messages(timeline: &[TimelineItem]) -> Vec<&stoat_ffi::timeline::MessageItem> {
    timeline
        .iter()
        .filter_map(|row| match row {
            TimelineItem::Message { message } => Some(message),
            _ => None,
        })
        .collect()
}

async fn opened(api: &MockServer) -> (std::sync::Arc<AccountController>, support::Recorder) {
    mount_history(api).await;
    let (controller, recorder) = connected(api).await;
    controller.open_channel("ch-general".into()).await.unwrap();
    (controller, recorder)
}

#[tokio::test]
async fn opening_a_channel_loads_its_latest_page() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, recorder) = opened(&api).await;

    let timeline = controller.timeline("ch-general".into());
    let rows = messages(&timeline);
    assert_eq!(rows.len(), 50);
    assert_eq!(rows.first().unwrap().id, id(100), "oldest first");
    assert_eq!(rows.last().unwrap().body.as_deref(), Some("message 149"));
    assert!(
        !timeline.iter().any(|r| matches!(r, TimelineItem::StartOfChannel { .. })),
        "a full page: there may be more"
    );
    recorder
        .wait_for("the timeline change", |h| {
            matches!(h, Heard::Changed(c) if c.timelines == ["ch-general"])
        })
        .await;

    // Opening it again doesn't fetch again (mounts would 404 a third call).
    controller.open_channel("ch-general".into()).await.unwrap();
}

#[tokio::test]
async fn older_pages_prepend_until_the_start() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;

    let more = controller.load_older("ch-general".into()).await.unwrap();
    assert!(!more, "3 of 50 asked for: that's the start");
    let timeline = controller.timeline("ch-general".into());
    assert!(matches!(&timeline[0], TimelineItem::StartOfChannel { name } if name == "general"));
    let rows = messages(&timeline);
    assert_eq!(rows.len(), 53);
    assert_eq!(rows[0].id, id(97));
    // Nothing more to fetch: no request is made.
    assert!(!controller.load_older("ch-general".into()).await.unwrap());
}

#[tokio::test]
async fn row_keys_are_unique_and_stable_across_a_prepend() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = opened(&api).await;
    let before: Vec<String> = controller
        .timeline("ch-general".into())
        .iter()
        .map(TimelineItem::id)
        .collect();
    controller.load_older("ch-general".into()).await.unwrap();
    let after: Vec<String> = controller
        .timeline("ch-general".into())
        .iter()
        .map(TimelineItem::id)
        .collect();
    let unique: std::collections::HashSet<_> = after.iter().collect();
    assert_eq!(unique.len(), after.len(), "no duplicate rows");
    assert!(after.ends_with(&before), "the old rows keep their keys, below the new ones");
}

#[tokio::test]
async fn a_live_edit_changes_the_rows_render_key() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, recorder) = opened(&api).await;
    let render_key = |c: &AccountController| {
        messages(&c.timeline("ch-general".into()))
            .last()
            .map(|m| (m.render_key.clone(), m.edited))
            .unwrap()
    };
    let (before, edited) = render_key(&controller);
    assert!(!edited);
    recorder.clear();

    events.send(json!({ "type": "MessageUpdate", "id": id(149), "channel": "ch-general",
                        "data": { "content": "fixed", "edited": "2026-11-13T12:00:00.000Z" },
                        "clear": [] }));
    recorder
        .wait_for("the edit", |h| {
            matches!(h, Heard::Changed(c) if c.timelines.contains(&"ch-general".to_string()))
        })
        .await;
    let (after, edited) = render_key(&controller);
    assert_ne!(before, after);
    assert!(edited);

    events.send(json!({ "type": "MessageDelete", "id": id(149), "channel": "ch-general" }));
    recorder
        .wait_for("the delete", |_| {
            messages(&controller.timeline("ch-general".into()))
                .last()
                .is_some_and(|m| m.id == id(148))
        })
        .await;
}

#[tokio::test]
async fn mark_read_acks_only_the_channel_on_screen() {
    let events = Events::start().await;
    let api = api(&events).await;
    Mock::given(method("PUT"))
        .and(path_regex(r"^/channels/ch-general/ack/.+$"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&api)
        .await;
    mount_history(&api).await;
    let (controller, _) = connected(&api).await;
    let unread = |c: &AccountController| {
        c.server_channels("srv-alpha".into())
            .into_iter()
            .flat_map(|s| s.channels)
            .find(|c| c.id == "ch-general")
            .unwrap()
            .unread
    };
    assert!(unread(&controller));

    // Not open: nothing is marked.
    assert!(!controller.mark_read("ch-general".into()));
    controller.open_channel("ch-general".into()).await.unwrap();
    assert!(controller.mark_read("ch-general".into()));
    assert!(!unread(&controller));
    // Nothing new since: no second ack.
    assert!(!controller.mark_read("ch-general".into()));

    // Closed, or the app in the background: never.
    controller.close_channel("ch-general".into());
    assert!(!controller.mark_read("ch-general".into()));
    controller.open_channel("ch-general".into()).await.unwrap();
    controller.disconnect();
    events.send(json!({ "type": "Message", "_id": id(200), "channel": "ch-general",
                        "author": "u-ada", "content": "while away" }));
    assert!(!controller.mark_read("ch-general".into()));

    // The one ack (`expect(1)`) is checked when the mock server drops.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn a_reconnect_reloads_the_open_channel() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, recorder) = opened(&api).await;
    assert_eq!(messages(&controller.timeline("ch-general".into())).len(), 50);
    controller.load_older("ch-general".into()).await.unwrap();
    assert_eq!(messages(&controller.timeline("ch-general".into())).len(), 53);
    recorder.clear();

    events.drop_connections();
    recorder.wait_for_status(stoat_ffi::controller::ConnectionStatus::Connected).await;
    // History is dropped on the reconnect's Ready, then reloaded.
    recorder
        .wait_for("the reload", |_| {
            messages(&controller.timeline("ch-general".into())).len() == 50
        })
        .await;
    recorder
        .wait_for("the Ready", |h| matches!(h, Heard::Changed(c) if c.ready))
        .await;
}

#[tokio::test]
async fn the_divider_marks_where_the_user_left_off() {
    let events = Events::start().await;
    let api = api(&events).await;
    mount_history(&api).await;
    let (controller, recorder) = connected(&api).await;
    // Read up to 120 (elsewhere), then 150 arrives.
    events.send(json!({ "type": "ChannelAck", "id": "ch-general", "user": "u-me",
                        "message_id": id(120) }));
    recorder
        .wait_for("the ack", |_| {
            controller.channel("ch-general".into()).is_some_and(|c| !c.unread)
        })
        .await;
    events.send(json!({ "type": "Message", "_id": id(150), "channel": "ch-general",
                        "author": "u-ada", "content": "new" }));
    recorder
        .wait_for("the message", |_| {
            controller.channel("ch-general".into()).is_some_and(|c| c.unread)
        })
        .await;

    controller.open_channel("ch-general".into()).await.unwrap();
    let timeline = controller.timeline("ch-general".into());
    let divider = timeline
        .iter()
        .position(|r| matches!(r, TimelineItem::NewMessages))
        .expect("a divider");
    assert!(matches!(&timeline[divider + 1], TimelineItem::Message { message } if message.id == id(121)));

    // Reading it doesn't move the divider while it's open…
    assert!(controller.mark_read("ch-general".into()));
    assert!(
        controller
            .timeline("ch-general".into())
            .iter()
            .any(|r| matches!(r, TimelineItem::NewMessages))
    );
    // …but it's gone the next time.
    controller.close_channel("ch-general".into());
    controller.open_channel("ch-general".into()).await.unwrap();
    assert!(
        !controller
            .timeline("ch-general".into())
            .iter()
            .any(|r| matches!(r, TimelineItem::NewMessages))
    );
}

#[tokio::test]
async fn reply_previews_fetch_their_originals() {
    let events = Events::start().await;
    let api = api(&events).await;
    let mut latest = page(100, 150);
    latest["messages"][0]["replies"] = json!([id(5)]);
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .and(query_param_is_missing("before"))
        .and(query_param_is_missing("nearby"))
        .respond_with(ResponseTemplate::new(200).set_body_json(latest))
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .and(query_param("nearby", id(5)))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(4, 7)))
        .mount(&api)
        .await;
    let (controller, _) = connected(&api).await;
    controller.open_channel("ch-general".into()).await.unwrap();

    let reply = |c: &AccountController| {
        messages(&c.timeline("ch-general".into()))
            .last()
            .unwrap()
            .replies
            .first()
            .cloned()
            .unwrap()
    };
    for _ in 0..200 {
        if reply(&controller).state == stoat_ffi::timeline::ReplyStateItem::Loaded {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let reply = reply(&controller);
    assert_eq!(reply.state, stoat_ffi::timeline::ReplyStateItem::Loaded);
    assert_eq!(reply.snippet, "message 5");
}

#[tokio::test]
async fn revealing_a_spoiler_redraws_its_message() {
    use stoat_ffi::rich_text::{RichBlock, SpanKind};
    let events = Events::start().await;
    let api = api(&events).await;
    let mut latest = page(100, 150);
    latest["messages"][0]["content"] = json!("the answer is ||42||");
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(latest))
        .mount(&api)
        .await;
    let (controller, recorder) = connected(&api).await;
    controller.open_channel("ch-general".into()).await.unwrap();

    let last = |c: &AccountController| messages(&c.timeline("ch-general".into())).pop().cloned().unwrap();
    let hidden = |m: &stoat_ffi::timeline::MessageItem| {
        m.rich.iter().any(|b| matches!(b, RichBlock::Paragraph { spans }
            if spans.iter().any(|s| matches!(s.kind, SpanKind::Spoiler { index: 0 }))))
    };
    let before = last(&controller);
    assert!(hidden(&before));
    recorder.clear();

    controller.reveal_spoiler("ch-general".into(), id(149), 0);
    let after = last(&controller);
    assert!(!hidden(&after));
    assert_ne!(before.render_key, after.render_key);
    recorder
        .wait_for("the redraw", |h| {
            matches!(h, Heard::Changed(c) if c.timelines == ["ch-general"])
        })
        .await;
}
