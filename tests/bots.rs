//! The user's bots: the list, the username check, the account's limit,
//! the store keeping their users, and the texts.

use serde_json::{Value, json};
use stoat_gpui::api::models::{Bot, DataEditBot, FieldsBot, RelationshipStatus, User};
use stoat_gpui::api::{ApiError, RootConfig};
use stoat_gpui::bots::{
    BotForm, INTERACTIONS_URL_MAX, invite_url, rename_edit, reset_prompt, token_reset_edit,
    username_change,
};
use stoat_gpui::bots::{
    BotRow, DEFAULT_BOT_LIMIT, UsernameProblem, at_limit, bot_error_text, bot_limit, bot_rows,
    check_bot_username, count_line, delete_prompt,
};
use stoat_gpui::media::MediaUrls;
use stoat_gpui::store::ChatStore;

fn bot(id: &str, public: bool) -> Bot {
    Bot {
        id: id.into(),
        owner_id: "u-me".into(),
        token: format!("token-{id}"),
        public,
        ..Default::default()
    }
}

fn user(id: &str, extra: Value) -> User {
    let mut user = json!({
        "_id": id, "username": id.trim_start_matches("b-"), "discriminator": "0001",
        "relationship": "User", "online": false, "bot": { "owner": "u-me" }
    });
    user.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(user).unwrap()
}

fn media() -> MediaUrls {
    MediaUrls::new(
        "https://api.example",
        Some("https://cdn.example".into()),
        None,
    )
}

fn api_error(status: u16, body: &str) -> ApiError {
    ApiError::from_response(
        reqwest::StatusCode::from_u16(status).unwrap(),
        body.as_bytes(),
    )
}

#[test]
fn rows_match_users_by_id_and_sort_by_name() {
    // Stoat sorts both lists by ID; here the users come in another order,
    // and one bot's user is missing.
    let bots = [
        bot("b-1", true),
        bot("b-2", false),
        bot("b-3", false),
        bot("b-4", true),
    ];
    let users = [
        user("b-4", json!({ "display_name": "  " })),
        user(
            "b-1",
            json!({
                "display_name": "Zed",
                "avatar": {
                    "_id": "av1", "tag": "avatars", "filename": "a.png",
                    "metadata": { "type": "Image", "width": 1, "height": 1 },
                    "content_type": "image/png", "size": 1
                }
            }),
        ),
        user("b-2", json!({ "display_name": "alpha" })),
    ];
    let rows = bot_rows(&bots, &users, &media());
    assert_eq!(
        rows,
        vec![
            BotRow {
                id: "b-4".into(),
                username: "4".into(),
                // A blank display name falls back to the username.
                display_name: "4".into(),
                avatar_url: None,
                public: true,
            },
            BotRow {
                id: "b-2".into(),
                username: "2".into(),
                display_name: "alpha".into(),
                avatar_url: None,
                public: false,
            },
            BotRow {
                id: "b-3".into(),
                username: String::new(),
                // Its user is missing: its ID.
                display_name: "b-3".into(),
                avatar_url: None,
                public: false,
            },
            BotRow {
                id: "b-1".into(),
                username: "1".into(),
                display_name: "Zed".into(),
                avatar_url: Some("https://cdn.example/avatars/av1".into()),
                public: true,
            },
        ]
    );
}

#[test]
fn usernames_are_checked_as_stoat_does() {
    assert_eq!(check_bot_username(" helper "), Ok("helper".into()));
    assert_eq!(check_bot_username("a.b-c_d9"), Ok("a.b-c_d9".into()));
    // Letters in any script.
    assert_eq!(check_bot_username("ботик"), Ok("ботик".into()));
    assert_eq!(check_bot_username("ロボット"), Ok("ロボット".into()));
    assert_eq!(check_bot_username(""), Err(UsernameProblem::TooShort));
    assert_eq!(check_bot_username("a"), Err(UsernameProblem::TooShort));
    assert_eq!(check_bot_username(&"a".repeat(32)), Ok("a".repeat(32)));
    assert_eq!(
        check_bot_username(&"a".repeat(33)),
        Err(UsernameProblem::TooLong)
    );
    // 32 characters, more bytes.
    assert!(check_bot_username(&"é".repeat(32)).is_ok());
    assert_eq!(
        check_bot_username("my bot"),
        Err(UsernameProblem::BadCharacter)
    );
    assert_eq!(
        check_bot_username("bot!"),
        Err(UsernameProblem::BadCharacter)
    );
    assert_eq!(
        check_bot_username("bot🤖"),
        Err(UsernameProblem::BadCharacter)
    );
    assert_eq!(check_bot_username("Stoat"), Err(UsernameProblem::Reserved));
    assert_eq!(
        UsernameProblem::BadCharacter.to_string(),
        "Use letters, numbers, _, . and - only."
    );
}

fn root(limits: Value) -> RootConfig {
    serde_json::from_value(json!({
        "revolt": "0.16.4", "ws": "wss://events.example",
        "features": { "limits": limits }
    }))
    .unwrap()
}

#[test]
fn the_limit_is_the_established_accounts() {
    let both = root(json!({ "default": { "bots": 5 }, "new_user": { "bots": 2 } }));
    assert_eq!(bot_limit(&both), Some(5));
    let new_only = root(json!({ "new_user": { "bots": 2 } }));
    assert_eq!(bot_limit(&new_only), Some(2));
    assert_eq!(bot_limit(&root(json!({}))), None);
    assert_eq!(DEFAULT_BOT_LIMIT, 5);

    assert!(!at_limit(4, 5));
    assert!(at_limit(5, 5));
    assert!(at_limit(6, 5));
    assert_eq!(count_line(2, 5), "2 of 5 bots");
}

#[test]
fn the_store_keeps_its_relationship_with_a_bot() {
    let mut store = ChatStore::new("u-me");
    store.set_media(media());
    // New: Stoat says `User` (the bot as it sees itself); the store says none.
    assert!(!store.add_bot_user(user("b-1", json!({}))));
    assert_eq!(
        store.user("b-1").unwrap().relationship,
        RelationshipStatus::None
    );
    // Known, and blocked: kept. A new display name counts as a change.
    let mut blocked = user("b-1", json!({}));
    blocked.relationship = RelationshipStatus::Blocked;
    store.add_user(blocked);
    assert!(store.add_bot_user(user("b-1", json!({ "display_name": "Helper" }))));
    let kept = store.user("b-1").unwrap();
    assert_eq!(kept.relationship, RelationshipStatus::Blocked);
    assert_eq!(kept.display_name.as_deref(), Some("Helper"));
    assert!(!store.add_bot_user(user("b-1", json!({ "display_name": "Helper" }))));
}

#[test]
fn texts_read_plainly() {
    let (title, detail) = delete_prompt("Helper");
    assert_eq!(title, "Delete Helper?");
    assert_eq!(
        detail,
        "It leaves every server and group it's in. This can't be undone."
    );

    let cases = [
        (
            r#"{"type":"InvalidUsername"}"#,
            "That username can't be used. Try another.",
        ),
        (r#"{"type":"UsernameTaken"}"#, "That username is taken."),
        (r#"{"type":"IsBot"}"#, "Bots can't manage bots."),
        (r#"{"type":"BotIsPrivate"}"#, "That bot is private."),
        (
            r#"{"type":"NotFound"}"#,
            "That bot doesn't exist, or it's private.",
        ),
        (
            r#"{"type":"MissingPermission","permission":"ManageServer"}"#,
            "You don't have permission to add bots there.",
        ),
        (r#"{"type":"AlreadyInServer"}"#, "The bot is already there."),
        (r#"{"type":"AlreadyInGroup"}"#, "The bot is already there."),
        (
            r#"{"type":"CannotGiveMissingPermissions"}"#,
            "You can't grant permissions you don't have.",
        ),
        (
            r#"{"type":"FailedValidation","error":"name: length"}"#,
            "Check the bot's name and interactions URL.",
        ),
        (r#"{"type":"InternalError"}"#, "Couldn't change the bot."),
    ];
    for (body, text) in cases {
        assert_eq!(bot_error_text(&api_error(400, body), None), text, "{body}");
    }

    let full = api_error(400, r#"{"type":"ReachedMaximumBots"}"#);
    assert_eq!(
        bot_error_text(&full, Some(5)),
        "You have 5 bots, the most Stoat allows."
    );
    assert_eq!(
        bot_error_text(&full, Some(1)),
        "You have 1 bot, the most Stoat allows."
    );
    assert_eq!(
        bot_error_text(&full, None),
        "You have as many bots as Stoat allows."
    );
}

#[test]
fn a_bots_settings_send_only_what_changed() {
    let mut stored = bot("b-1", false);
    stored.interactions_url = "https://bot.example/hook".into();
    stored.default_permissions = Some(8);
    let form = BotForm::of(&stored);
    assert_eq!(
        form,
        BotForm {
            public: false,
            interactions_url: "https://bot.example/hook".into(),
            default_permissions: 8,
        }
    );
    assert_eq!(form.changes(&stored), None);

    let public = BotForm {
        public: true,
        ..form.clone()
    };
    assert_eq!(
        public.changes(&stored),
        Some(DataEditBot {
            public: Some(true),
            ..Default::default()
        })
    );

    // Trimmed; the same URL with spaces is no change.
    let spaced = BotForm {
        interactions_url: "  https://bot.example/hook ".into(),
        ..form.clone()
    };
    assert_eq!(spaced.changes(&stored), None);
    let moved = BotForm {
        interactions_url: " https://new.example ".into(),
        ..form.clone()
    };
    assert_eq!(
        moved.changes(&stored),
        Some(DataEditBot {
            interactions_url: Some("https://new.example".into()),
            ..Default::default()
        })
    );
    let blank = BotForm {
        interactions_url: "  ".into(),
        ..form.clone()
    };
    assert_eq!(
        blank.changes(&stored),
        Some(DataEditBot {
            remove: vec![FieldsBot::InteractionsURL],
            ..Default::default()
        })
    );
    let too_long = BotForm {
        interactions_url: "h".repeat(INTERACTIONS_URL_MAX + 1),
        ..form.clone()
    };
    assert!(!too_long.is_valid());
    assert_eq!(too_long.changes(&stored), None);

    let permissions = BotForm {
        default_permissions: 8 | 16,
        ..form.clone()
    };
    assert_eq!(
        permissions.changes(&stored),
        Some(DataEditBot {
            default_permissions: Some(24),
            ..Default::default()
        })
    );
    // None stored reads as nothing requested.
    let plain = bot("b-2", false);
    assert_eq!(BotForm::of(&plain).default_permissions, 0);
    assert_eq!(BotForm::of(&plain).changes(&plain), None);
}

#[test]
fn renames_tokens_and_links() {
    assert_eq!(
        username_change(" helper2 ", "helper"),
        Some("helper2".into())
    );
    assert_eq!(username_change("helper", "helper"), None);
    assert_eq!(username_change("Helper", "helper"), Some("Helper".into()));
    assert_eq!(username_change("no good", "helper"), None);
    assert_eq!(
        rename_edit("helper2".into()),
        DataEditBot {
            name: Some("helper2".into()),
            ..Default::default()
        }
    );

    assert_eq!(
        token_reset_edit(),
        DataEditBot {
            remove: vec![FieldsBot::Token],
            ..Default::default()
        }
    );
    let (title, detail) = reset_prompt("Helper");
    assert_eq!(title, "Reset Helper's token?");
    assert_eq!(
        detail,
        "The old token stops working at once. Anything using it needs the new one."
    );

    assert_eq!(
        invite_url("https://stoat.chat", "01ABC"),
        "https://stoat.chat/bot/01ABC"
    );
    assert_eq!(
        invite_url("https://chat.example/", "01ABC"),
        "https://chat.example/bot/01ABC"
    );
}
