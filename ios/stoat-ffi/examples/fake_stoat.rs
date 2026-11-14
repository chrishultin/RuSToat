//! A fake Stoat on localhost, serving the test fixtures, for trying the
//! iPhone app in the simulator without a real account:
//!
//!   cargo run -p stoat-ffi --example fake_stoat
//!
//! It prints the API's address. Log in there with any email and password
//! (a DEBUG build can do it at launch: see `AppModel.devLogin`).

#[path = "../tests/support/mod.rs"]
mod support;

use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, Request, ResponseTemplate};

#[tokio::main]
async fn main() {
    let events = support::Events::start().await;
    let api = support::api(&events).await;
    let me = support::fixture("users_me.json");
    let mut me = me.as_object().unwrap().clone();
    me.insert("_id".into(), json!(support::ME));
    let mut login = support::fixture("login_success.json");
    login["user_id"] = json!(support::ME);
    login["token"] = json!(support::TOKEN);
    Mock::given(method("POST"))
        .and(path("/auth/session/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(login))
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me))
        .mount(&api)
        .await;
    // #random has a message with every kind of block, to see them drawn.
    Mock::given(method("GET"))
        .and(path("/channels/ch-random/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(support::fixture("messages_rich.json")),
        )
        .mount(&api)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/channels/[^/]+/messages$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(support::fixture("messages_page.json")),
        )
        .mount(&api)
        .await;
    // Sending: Stoat creates the message with the send's idempotency key as
    // its nonce, and echoes it on the events socket.
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let echo = events.sender();
    Mock::given(method("POST"))
        .and(path_regex(r"^/channels/[^/]+/messages$"))
        .respond_with(move |request: &Request| {
            let n = sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
            let channel = request.url.path().split('/').nth(2).unwrap_or_default();
            let nonce = request
                .headers
                .get("Idempotency-Key")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            let message = json!({
                "_id": format!("01JE00000000000000000S{n:04}"), "channel": channel,
                "author": support::ME, "content": body["content"], "nonce": nonce,
                "replies": body["replies"].as_array().map(|r| r.iter().map(|r| r["id"].clone()).collect::<Vec<_>>()),
            });
            let mut frame = message.clone();
            frame["type"] = json!("Message");
            echo(frame);
            ResponseTemplate::new(200).set_body_json(message)
        })
        .mount(&api)
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(r"^/channels/[^/]+/ack/.+$"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&api)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/session/logout"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&api)
        .await;
    println!("{}", api.uri());
    // Every 20 s, a new message in #random, to watch unread state live.
    let mut n = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        n += 1;
        events.send(json!({ "type": "Message", "_id": format!("01JC00000000000000000F{n:04}"),
                            "channel": "ch-random", "author": "u-ada",
                            "content": format!("tick {n}") }));
    }
}
