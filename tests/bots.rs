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

mod adding {
    use revolt_permissions::ChannelPermission;
    use serde_json::{Value, json};
    use stoat_gpui::api::events::{ServerEvent, parse_frame};
    use stoat_gpui::api::models::InviteBotDestination;
    use stoat_gpui::bots::{
        DestinationKind, added_text, destinations, filter_destinations, grantable,
        invite_permissions, requested, ungrantable_reason,
    };
    use stoat_gpui::media::MediaUrls;
    use stoat_gpui::permission_matrix::{MatrixContext, entries};
    use stoat_gpui::store::ChatStore;

    const MANAGE_SERVER: u64 = ChannelPermission::ManageServer as u64;
    const MANAGE_ROLE: u64 = ChannelPermission::ManageRole as u64;
    const SEND: u64 = ChannelPermission::SendMessage as u64;
    const KICK: u64 = ChannelPermission::KickMembers as u64;
    const MESSAGES: u64 = ChannelPermission::ManageMessages as u64;
    const VIEW: u64 = ChannelPermission::ViewChannel as u64;
    const INVITE: u64 = ChannelPermission::InviteOthers as u64;

    fn user(id: &str) -> Value {
        json!({
            "_id": id, "username": id, "discriminator": "0001",
            "relationship": "None", "online": true
        })
    }

    fn server(id: &str, name: &str, owner: &str, roles: Value) -> Value {
        json!({
            "_id": id, "owner": owner, "name": name, "channels": [],
            "approximate_member_count": 2, "default_permissions": 0, "roles": roles
        })
    }

    fn role(id: &str, permissions: u64) -> Value {
        json!({ "_id": id, "name": id, "rank": 0, "permissions": { "a": permissions, "d": 0 } })
    }

    fn member(server: &str, user: &str, roles: &[&str]) -> Value {
        json!({
            "_id": { "server": server, "user": user },
            "joined_at": "2026-01-01T00:00:00.000Z",
            "roles": roles,
        })
    }

    fn group(id: &str, name: &str, owner: &str, recipients: &[&str], permissions: u64) -> Value {
        json!({
            "channel_type": "Group", "_id": id, "name": name, "owner": owner,
            "recipients": recipients, "permissions": permissions
        })
    }

    /// The user (`u-me`):
    /// - owns `srv-own` ("Zeta") and `srv-has-bot` (the bot's in it);
    /// - may manage `srv-mod` ("alpha") but not its roles;
    /// - may manage `srv-roles` ("Middle") and its roles, holding Send
    ///   Messages but not Kick Members;
    /// - has nothing in `srv-plain`;
    /// - owns `grp-own` ("Friends") and `grp-has-bot`; is a member of
    ///   `grp-open` ("book club", members may invite) and `grp-closed`.
    fn store() -> ChatStore {
        let ready = json!({
            "type": "Ready",
            "users": [user("u-me"), user("u-owner"), user("b-bot")],
            "servers": [
                server("srv-own", "Zeta", "u-me", json!({})),
                server("srv-has-bot", "Has bot", "u-me", json!({})),
                server("srv-mod", "alpha", "u-owner", json!({ "mod": role("mod", MANAGE_SERVER) })),
                server(
                    "srv-roles", "Middle", "u-owner",
                    json!({ "admin": role("admin", MANAGE_SERVER | MANAGE_ROLE | SEND) }),
                ),
                server("srv-plain", "Plain", "u-owner", json!({})),
            ],
            "channels": [
                group("grp-own", "Friends", "u-me", &["u-me"], 0),
                group("grp-has-bot", "Bots", "u-me", &["u-me", "b-bot"], 0),
                group("grp-open", "book club", "u-owner", &["u-me", "u-owner"], VIEW | INVITE),
                group("grp-closed", "Closed", "u-owner", &["u-me", "u-owner"], VIEW),
            ],
            "members": [
                member("srv-own", "u-me", &[]),
                member("srv-has-bot", "u-me", &[]),
                member("srv-has-bot", "b-bot", &[]),
                member("srv-mod", "u-me", &["mod"]),
                member("srv-roles", "u-me", &["admin"]),
                member("srv-plain", "u-me", &[]),
            ],
        });
        let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
            panic!("ready");
        };
        let mut store = ChatStore::new("u-me");
        store.apply_ready(*ready);
        store.set_media(MediaUrls::new(
            "https://api.example",
            Some("https://cdn.example".into()),
            None,
        ));
        store
    }

    #[test]
    fn places_are_servers_then_groups_the_user_may_add_to() {
        let places = destinations(&store(), "b-bot");
        let shown: Vec<(DestinationKind, &str)> = places
            .iter()
            .map(|place| (place.kind, place.id.as_str()))
            .collect();
        assert_eq!(
            shown,
            vec![
                (DestinationKind::Server, "srv-mod"),
                (DestinationKind::Server, "srv-roles"),
                (DestinationKind::Server, "srv-own"),
                (DestinationKind::Group, "grp-open"),
                (DestinationKind::Group, "grp-own"),
            ]
        );
        assert_eq!(
            places[0].invite(),
            InviteBotDestination::Server {
                server: "srv-mod".into()
            }
        );
        assert_eq!(
            places[3].invite(),
            InviteBotDestination::Group {
                group: "grp-open".into()
            }
        );

        let filtered: Vec<&str> = filter_destinations(&places, " CLUB ")
            .iter()
            .map(|place| place.id.as_str())
            .collect();
        assert_eq!(filtered, vec!["grp-open"]);
        assert_eq!(filter_destinations(&places, "").len(), places.len());
    }

    #[test]
    fn requested_permissions_follow_the_matrix() {
        let bits = KICK | SEND | MESSAGES | (1 << 62);
        let order: Vec<u64> = entries(MatrixContext::Server)
            .iter()
            .map(|shown| shown.entry.bit())
            .filter(|bit| bits & bit != 0)
            .collect();
        let listed: Vec<u64> = requested(bits).iter().map(|entry| entry.bit()).collect();
        assert_eq!(listed, order);
        assert_eq!(listed.len(), 3, "an unknown bit is left out");
        assert!(requested(0).is_empty());
    }

    #[test]
    fn only_held_permissions_are_grantable_and_only_with_manage_roles() {
        let store = store();
        let asks = SEND | KICK | MESSAGES;
        // The owner holds everything.
        assert_eq!(grantable(&store, "srv-own", asks), asks);
        assert_eq!(ungrantable_reason(&store, "srv-own", KICK), None);
        // Without Manage Roles, nothing.
        assert_eq!(grantable(&store, "srv-mod", asks), 0);
        assert_eq!(
            ungrantable_reason(&store, "srv-mod", SEND),
            Some("You need Manage Roles to give it permissions.")
        );
        // With it, only what's held.
        assert_eq!(grantable(&store, "srv-roles", asks), SEND);
        assert_eq!(ungrantable_reason(&store, "srv-roles", SEND), None);
        assert_eq!(
            ungrantable_reason(&store, "srv-roles", KICK),
            Some("You don't have this permission.")
        );
        assert_eq!(grantable(&store, "srv-plain", asks), 0);
    }

    #[test]
    fn what_is_sent_never_exceeds_what_is_grantable() {
        for ticked in 0..32_u64 {
            for grantable in 0..32_u64 {
                let sent = invite_permissions(ticked, grantable);
                assert_eq!(sent.is_none(), ticked & grantable == 0);
                let sent = sent.unwrap_or(0);
                assert_eq!(sent & !grantable, 0, "{ticked} {grantable}");
                assert_eq!(sent & !ticked, 0, "{ticked} {grantable}");
            }
        }
        assert_eq!(invite_permissions(SEND | KICK, SEND), Some(SEND));
        assert_eq!(invite_permissions(KICK, SEND), None);
    }

    #[test]
    fn the_toast_names_both() {
        assert_eq!(added_text("helper", "Zeta"), "Added helper to Zeta.");
    }
}

mod marks {
    use serde_json::json;
    use stoat_gpui::api::models::{BulkMessageResponse, User};
    use stoat_gpui::store::{ChatStore, MessageRow, TimelineRow};

    const CH: &str = "ch-marks";
    const BY_BOT: &str = "01M6GDKW80AAAAAAAAAAAAAAAA";
    const MASKED: &str = "01M6GDSC10BBBBBBBBBBBBBBBB";
    const HOOK: &str = "01M6GEW5A0CCCCCCCCCCCCCCCC";
    const BY_ADA: &str = "01M6GEW5A1DDDDDDDDDDDDDDDD";
    const BY_LATE: &str = "01M6GEW5A2EEEEEEEEEEEEEEEE";

    fn user(id: &str, bot: bool) -> User {
        let mut user = json!({
            "_id": id, "username": id, "discriminator": "0001",
            "relationship": "None", "online": true
        });
        if bot {
            user["bot"] = json!({ "owner": "u-me" });
        }
        serde_json::from_value(user).unwrap()
    }

    fn rows(store: &ChatStore) -> Vec<MessageRow> {
        store
            .timeline(CH)
            .into_iter()
            .filter_map(|row| match row {
                TimelineRow::Message(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    fn bot_flag(store: &ChatStore, id: &str) -> bool {
        rows(store)
            .into_iter()
            .find(|row| row.id == id)
            .unwrap()
            .author_bot
    }

    #[test]
    fn only_a_bots_own_messages_are_marked() {
        let mut store = ChatStore::new("u-me");
        store.add_user(user("b-bot", true));
        store.add_user(user("u-ada", false));
        let page: BulkMessageResponse = serde_json::from_value(json!([
            { "_id": BY_BOT, "channel": CH, "author": "b-bot", "content": "beep" },
            { "_id": MASKED, "channel": CH, "author": "b-bot", "content": "relayed",
              "masquerade": { "name": "Someone" } },
            { "_id": HOOK, "channel": CH, "author": "w-hook", "content": "built",
              "webhook": { "name": "CI" } },
            { "_id": BY_ADA, "channel": CH, "author": "u-ada", "content": "hi" },
            { "_id": BY_LATE, "channel": CH, "author": "b-late", "content": "late" },
        ]))
        .unwrap();
        store.set_history(CH, page, 50);

        assert!(bot_flag(&store, BY_BOT));
        assert!(!bot_flag(&store, MASKED), "a masquerade shows its own name");
        assert!(!bot_flag(&store, HOOK), "a webhook isn't a bot user");
        assert!(!bot_flag(&store, BY_ADA));
        // Its user isn't known yet; once it is, the row says so.
        assert!(!bot_flag(&store, BY_LATE));
        store.add_user(user("b-late", true));
        assert!(bot_flag(&store, BY_LATE));
    }
}
