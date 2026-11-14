//! Sending through `AccountController`: pending rows, confirmation by the
//! POST and the live echo, failures, retry, and who may send.

mod support;

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use stoat_ffi::controller::AccountController;
use stoat_ffi::timeline::{MessageItem, PendingItem, TimelineItem};
use stoat_gpui::api::StoatClient;
use support::{Events, Recorder, api, connected};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn page() -> Value {
    json!({ "messages": [
        { "_id": "01JD0000000000000000000001", "channel": "ch-general",
          "author": "u-ada", "content": "hello" }
    ], "users": [] })
}

const SENT_ID: &str = "01JD0000000000000000000900";

/// The message Stoat creates for a send, carrying its idempotency key.
fn created(nonce: &str, content: &str) -> Value {
    json!({ "_id": SENT_ID, "channel": "ch-general", "author": "u-me",
            "content": content, "nonce": nonce })
}

fn nonce_of(request: &Request) -> String {
    request
        .headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn mount_basics(api: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/channels/ch-general/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page()))
        .mount(api)
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(r"^/channels/ch-general/ack/.+$"))
        .respond_with(ResponseTemplate::new(204))
        .mount(api)
        .await;
}

fn messages(controller: &AccountController) -> Vec<MessageItem> {
    controller
        .timeline("ch-general".into())
        .into_iter()
        .filter_map(|row| match row {
            TimelineItem::Message { message } => Some(message),
            _ => None,
        })
        .collect()
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
async fn a_send_shows_pending_then_one_confirmed_row() {
    let events = Events::start().await;
    let api = api(&events).await;
    mount_basics(&api).await;
    // Answer slowly, so the pending row can be seen, and after the echo.
    let nonces = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .respond_with({
            let nonces = nonces.clone();
            move |request: &Request| {
                let nonce = nonce_of(request);
                nonces.lock().unwrap().push(nonce.clone());
                ResponseTemplate::new(200)
                    .set_body_json(created(&nonce, "hi there"))
                    .set_delay(std::time::Duration::from_millis(200))
            }
        })
        .expect(1)
        .mount(&api)
        .await;
    let (controller, _) = connected(&api).await;
    controller.open_channel("ch-general".into()).await.unwrap();

    let key = controller
        .send("ch-general".into(), "  hi there ".into(), None)
        .unwrap();
    let last = messages(&controller).pop().unwrap();
    assert_eq!(last.key, key);
    assert_eq!(last.pending, Some(PendingItem::Sending));
    assert_eq!(last.body.as_deref(), Some("hi there"));
    assert!(last.mine);

    // The live echo arrives before the POST's answer.
    wait_until("the POST", || !nonces.lock().unwrap().is_empty()).await;
    let nonce = nonces.lock().unwrap()[0].clone();
    assert_eq!(nonce, key, "the key is the first idempotency key");
    let mut echo = created(&nonce, "hi there");
    echo["type"] = json!("Message");
    events.send(echo);

    wait_until("confirmed", || {
        messages(&controller).last().is_some_and(|m| m.pending.is_none())
    })
    .await;
    // And once the POST answers too, still exactly one.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let rows = messages(&controller);
    let mine: Vec<_> = rows.iter().filter(|m| m.mine).collect();
    assert_eq!(mine.len(), 1, "{rows:#?}");
    assert_eq!(mine[0].id, SENT_ID);
    assert_eq!(mine[0].key, key, "the same row throughout");
    assert!(rows.iter().all(|m| m.pending.is_none()));
}

#[tokio::test]
async fn a_server_error_fails_the_row_and_retry_sends_it() {
    let events = Events::start().await;
    let api = api(&events).await;
    mount_basics(&api).await;
    let nonces = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .respond_with({
            let nonces = nonces.clone();
            move |request: &Request| {
                let nonce = nonce_of(request);
                let mut nonces = nonces.lock().unwrap();
                nonces.push(nonce.clone());
                if nonces.len() == 1 {
                    ResponseTemplate::new(500).set_body_string("oops")
                } else {
                    ResponseTemplate::new(200).set_body_json(created(&nonce, "hi"))
                }
            }
        })
        .expect(2)
        .mount(&api)
        .await;
    let (controller, _) = connected(&api).await;
    controller.open_channel("ch-general".into()).await.unwrap();

    let key = controller.send("ch-general".into(), "hi".into(), None).unwrap();
    wait_until("failed", || {
        matches!(messages(&controller).last().unwrap().pending, Some(PendingItem::Failed { .. }))
    })
    .await;
    let Some(PendingItem::Failed { reason }) = messages(&controller).last().unwrap().pending.clone()
    else {
        unreachable!()
    };
    assert!(reason.contains("500"), "{reason}");

    controller.retry("ch-general".into(), key.clone());
    wait_until("sent", || {
        messages(&controller).last().is_some_and(|m| m.pending.is_none() && m.id == SENT_ID)
    })
    .await;
    // The retry had a fresh key, so the confirmed row is keyed by it: still
    // exactly one row of the user's, and nothing pending.
    let rows = messages(&controller);
    assert_eq!(rows.iter().filter(|m| m.mine).count(), 1, "{rows:#?}");
    assert!(rows.iter().all(|m| m.pending.is_none()));
    let nonces = nonces.lock().unwrap();
    // Stoat answered the first attempt, so it has seen that key: the
    // retry needs a fresh one (as on desktop).
    assert_ne!(nonces[0], nonces[1]);
}

/// An API that drops the first message POST without answering (a lost
/// connection), and answers the rest. It records each POST's
/// Idempotency-Key.
async fn flaky_api() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let keys = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn({
        let keys = keys.clone();
        async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let keys = keys.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16 * 1024];
                    let mut read = 0;
                    // Headers, then the body by Content-Length.
                    let request = loop {
                        let n = socket.read(&mut buf[read..]).await.unwrap_or(0);
                        if n == 0 { return; }
                        read += n;
                        let text = String::from_utf8_lossy(&buf[..read]).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let length = text.lines()
                                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                                .unwrap_or(0);
                            if read >= end + 4 + length { break text; }
                        }
                    };
                    let key = request.lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("idempotency-key:").map(|v| v.trim().to_string()))
                        .unwrap_or_default();
                    // Only message POSTs count; the ack after a send doesn't.
                    let attempt = if request.starts_with("POST") {
                        let mut keys = keys.lock().unwrap();
                        keys.push(key.clone());
                        keys.len()
                    } else {
                        usize::MAX
                    };
                    if attempt == 1 {
                        return; // Dropped without an answer.
                    }
                    let body = created(&key, "hi").to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        }
    });
    (url, keys)
}

#[tokio::test]
async fn a_retry_after_a_lost_connection_keeps_the_idempotency_key() {
    let (url, keys) = flaky_api().await;
    let client = StoatClient::new(url).with_token(support::TOKEN);
    let controller =
        AccountController::for_client(client, support::ME.into(), Box::new(Recorder::default()));

    let key = controller.send("ch-general".into(), "hi".into(), None).unwrap();
    wait_until("the first attempt", || keys.lock().unwrap().len() == 1).await;
    // `retry` does nothing until the first attempt has failed.
    for _ in 0..500 {
        controller.retry("ch-general".into(), key.clone());
        if keys.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    wait_until("the second attempt", || keys.lock().unwrap().len() == 2).await;

    let keys = keys.lock().unwrap();
    assert_eq!(keys[0], key);
    assert_eq!(keys[1], key, "Stoat may have got the first: the same key can't duplicate it");
}

#[tokio::test]
async fn replies_are_sent_with_the_message() {
    let events = Events::start().await;
    let api = api(&events).await;
    mount_basics(&api).await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .and(wiremock::matchers::body_partial_json(json!({
            "content": "agreed",
            "replies": [{ "id": "01JD0000000000000000000001", "mention": false }]
        })))
        .respond_with(|request: &Request| {
            ResponseTemplate::new(200).set_body_json(created(&nonce_of(request), "agreed"))
        })
        .expect(1)
        .mount(&api)
        .await;
    let (controller, _) = connected(&api).await;
    controller.open_channel("ch-general".into()).await.unwrap();
    controller
        .send("ch-general".into(), "agreed".into(), Some("01JD0000000000000000000001".into()))
        .unwrap();
    let pending = messages(&controller).pop().unwrap();
    assert_eq!(pending.replies.len(), 1);
    assert_eq!(pending.replies[0].snippet, "hello");
    wait_until("sent", || messages(&controller).last().is_some_and(|m| m.pending.is_none())).await;
}

#[tokio::test]
async fn discard_removes_a_failed_message() {
    let events = Events::start().await;
    let api = api(&events).await;
    mount_basics(&api).await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&api)
        .await;
    let (controller, _) = connected(&api).await;
    controller.open_channel("ch-general".into()).await.unwrap();
    let key = controller.send("ch-general".into(), "nope".into(), None).unwrap();
    wait_until("failed", || {
        matches!(messages(&controller).last().unwrap().pending, Some(PendingItem::Failed { .. }))
    })
    .await;
    controller.discard("ch-general".into(), key);
    assert!(messages(&controller).iter().all(|m| !m.mine));
}

#[tokio::test]
async fn empty_messages_are_refused() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = connected(&api).await;
    assert!(controller.send("ch-general".into(), "   ".into(), None).is_err());
}

#[tokio::test]
async fn who_may_send() {
    let events = Events::start().await;
    let api = api(&events).await;
    let (controller, _) = connected(&api).await;
    // The fixture's DMs and saved notes, and an ordinary server channel.
    assert!(controller.can_send("ch-saved".into()));
    assert!(controller.can_send("ch-general".into()));
    // Unknown channels: no.
    assert!(!controller.can_send("ch-nowhere".into()));
}
