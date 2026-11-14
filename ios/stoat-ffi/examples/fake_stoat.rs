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
use wiremock::{Mock, ResponseTemplate};

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
    Mock::given(method("GET"))
        .and(path_regex(r"^/channels/[^/]+/messages$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(support::fixture("messages_page.json")),
        )
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
