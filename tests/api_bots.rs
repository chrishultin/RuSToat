//! The user's bots against a mock Stoat API, from Settings' My Bots:
//! `/bots/@me`, `/bots/create` and `/bots/{id}`.

use serde_json::{Value, json};
use stoat_gpui::api::StoatClient;
use stoat_gpui::bots::bot_error_text;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

fn bot(id: &str) -> Value {
    json!({ "_id": id, "owner": "u-me", "token": format!("token-{id}"), "public": false })
}

fn bot_user(id: &str, username: &str) -> Value {
    json!({
        "_id": id, "username": username, "discriminator": "0001",
        "relationship": "User", "online": false, "bot": { "owner": "u-me" }
    })
}

#[tokio::test]
async fn the_list_comes_with_users_and_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/bots/@me"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "bots": [
                bot("b1"),
                { "_id": "b2", "owner": "u-me", "token": "t2", "public": true,
                  "default_permissions": 8, "interactions_url": "https://bot.example/hook" }
            ],
            "users": [bot_user("b1", "one"), bot_user("b2", "two")]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let owned = client(&server).fetch_owned_bots().await.unwrap();
    assert_eq!(owned.bots.len(), 2);
    assert_eq!(owned.bots[0].token, "token-b1");
    assert!(owned.bots[1].public);
    assert_eq!(owned.bots[1].default_permissions, Some(8));
    assert_eq!(owned.bots[1].interactions_url, "https://bot.example/hook");
    assert_eq!(owned.users[1].username, "two");
    assert!(
        owned.users[1]
            .bot
            .as_ref()
            .is_some_and(|b| b.owner_id == "u-me")
    );
}

#[tokio::test]
async fn creating_a_bot_sends_only_its_name() {
    let server = MockServer::start().await;
    let mut answer = bot("b1");
    answer["user"] = bot_user("b1", "helper");
    Mock::given(method("POST"))
        .and(path("/bots/create"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "name": "helper" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer))
        .expect(1)
        .mount(&server)
        .await;

    let made = client(&server).create_bot("helper").await.unwrap();
    assert_eq!(made.bot.id, "b1");
    assert_eq!(made.bot.token, "token-b1");
    assert_eq!(made.user.username, "helper");
}

#[tokio::test]
async fn deleting_a_bot_sends_a_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/bots/b1"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).delete_bot("b1").await.unwrap();
}

#[tokio::test]
async fn refusals_read_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bots/create"))
        .and(body_json(json!({ "name": "full" })))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "type": "ReachedMaximumBots" })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/bots/create"))
        .and(body_json(json!({ "name": "admin" })))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "type": "InvalidUsername" })),
        )
        .mount(&server)
        .await;

    let client = client(&server);
    let err = client.create_bot("full").await.unwrap_err();
    assert_eq!(
        bot_error_text(&err, Some(5)),
        "You have 5 bots, the most Stoat allows."
    );
    let err = client.create_bot("admin").await.unwrap_err();
    assert_eq!(
        bot_error_text(&err, None),
        "That username can't be used. Try another."
    );
}

/// Mounts a `PATCH /bots/b1` expecting exactly `body`, answering with the
/// bot as `answer` (plus its user).
async fn expect_edit(server: &MockServer, body: Value, mut answer: Value) {
    answer["user"] = bot_user("b1", "helper");
    Mock::given(method("PATCH"))
        .and(path("/bots/b1"))
        .and(header("x-session-token", "tok"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn edits_send_only_what_changed() {
    use stoat_gpui::api::models::DataEditBot;
    use stoat_gpui::bots::{rename_edit, token_reset_edit};

    let server = MockServer::start().await;
    let mut public = bot("b1");
    public["public"] = json!(true);
    expect_edit(&server, json!({ "public": true }), public).await;
    let mut reset = bot("b1");
    reset["token"] = json!("fresh");
    expect_edit(&server, json!({ "remove": ["Token"] }), reset).await;
    let mut asks = bot("b1");
    asks["default_permissions"] = json!(24);
    expect_edit(&server, json!({ "default_permissions": 24 }), asks).await;
    expect_edit(&server, json!({ "name": "helper2" }), bot("b1")).await;
    expect_edit(&server, json!({ "remove": ["InteractionsURL"] }), bot("b1")).await;

    let client = client(&server);
    let saved = client
        .edit_bot(
            "b1",
            &DataEditBot {
                public: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(saved.bot.public);
    assert_eq!(saved.user.username, "helper");
    let saved = client.edit_bot("b1", &token_reset_edit()).await.unwrap();
    assert_eq!(saved.bot.token, "fresh");
    let saved = client
        .edit_bot(
            "b1",
            &DataEditBot {
                default_permissions: Some(24),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.bot.default_permissions, Some(24));
    client
        .edit_bot("b1", &rename_edit("helper2".into()))
        .await
        .unwrap();
    client
        .edit_bot(
            "b1",
            &DataEditBot {
                remove: vec![stoat_gpui::api::models::FieldsBot::InteractionsURL],
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_taken_username_reads_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/bots/b1"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({ "type": "UsernameTaken" })))
        .mount(&server)
        .await;

    let err = client(&server)
        .edit_bot("b1", &stoat_gpui::bots::rename_edit("taken".into()))
        .await
        .unwrap_err();
    assert_eq!(bot_error_text(&err, None), "That username is taken.");
}
