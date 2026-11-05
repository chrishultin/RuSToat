//! What a person's profile card shows (`profile::profile_card`).

use chrono::{TimeZone, Utc};
use revolt_permissions::DEFAULT_PERMISSION_SERVER;
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::RelationshipStatus;
use stoat_gpui::profile::{CardRole, profile_card};
use stoat_gpui::store::{ChatStore, PresenceKind};

/// 2026-01-01T00:00:00Z as a ULID prefix, so "account created" is known.
const ADA: &str = "01KDVDNA00ADAADAADAADAADAA";

fn user(id: &str, extra: Value) -> Value {
    let mut user = json!({
        "_id": id, "username": id.trim_start_matches("u-").to_lowercase(), "discriminator": "1234",
        "relationship": "None", "online": true
    });
    user.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    user
}

fn member(user: &str, roles: &[&str], extra: Value) -> Value {
    let mut member = json!({
        "_id": { "server": "srv", "user": user },
        "joined_at": "2026-03-04T10:00:00.000Z",
        "roles": roles,
    });
    member
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    member
}

fn role(id: &str, name: &str, rank: i64, colour: Option<&str>) -> Value {
    let mut role =
        json!({ "_id": id, "name": name, "rank": rank, "permissions": { "a": 0, "d": 0 } });
    if let Some(colour) = colour {
        role["colour"] = json!(colour);
    }
    role
}

fn store() -> ChatStore {
    let ready = json!({
        "type": "Ready",
        "users": [
            user("u-me", json!({})),
            user(ADA, json!({
                "username": "ada", "display_name": "Ada Lovelace", "pronouns": "she/her",
                "status": { "text": "  counting  ", "presence": "Busy" }
            })),
            user("u-bob", json!({ "online": false, "relationship": "Friend" })),
            user("u-bot", json!({ "bot": { "owner": "u-me" }, "status": { "text": "   " } }))
        ],
        "servers": [{
            "_id": "srv", "owner": "u-me", "name": "Test Server", "channels": ["ch"],
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "approximate_member_count": 4,
            "roles": {
                "admin": role("admin", "Admin", 0, Some("#ff0000")),
                "fancy": role("fancy", "Fancy", 1, Some("linear-gradient(90deg, red, blue)")),
                "plain": role("plain", "Plain", 2, None)
            }
        }],
        "channels": [
            { "channel_type": "TextChannel", "_id": "ch", "server": "srv", "name": "general" },
            { "channel_type": "DirectMessage", "_id": "dm", "active": true, "recipients": ["u-me", ADA] },
            {
                "channel_type": "Group", "_id": "grp", "name": "Group", "owner": "u-me",
                "recipients": ["u-me", ADA, "u-bob"]
            }
        ],
        "members": [
            member("u-me", &[], json!({})),
            member(ADA, &["plain", "admin", "fancy", "deleted-role"], json!({
                "nickname": "Countess", "pronouns": "they/them"
            })),
            member("u-bob", &[], json!({ "nickname": "   " }))
        ]
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

fn role_names(roles: &[CardRole]) -> Vec<&str> {
    roles.iter().map(|r| r.name.as_str()).collect()
}

#[test]
fn in_a_server_the_nickname_and_server_pronouns_win() {
    let card = profile_card(&store(), ADA, Some("ch")).unwrap();
    assert_eq!(card.name, "Countess");
    assert_eq!(card.pronouns.as_deref(), Some("they/them"));
    assert_eq!(card.username_tag, "ada#1234");
    assert_eq!(card.status.as_deref(), Some("counting"));
    assert_eq!(card.presence, PresenceKind::Busy);
    assert!(!card.bot && !card.is_me);
}

#[test]
fn elsewhere_the_display_name_and_own_pronouns_show() {
    for channel in [Some("dm"), Some("grp"), None] {
        let card = profile_card(&store(), ADA, channel).unwrap();
        assert_eq!(card.name, "Ada Lovelace", "{channel:?}");
        assert_eq!(card.pronouns.as_deref(), Some("she/her"));
    }
    // A blank nickname falls back too, then to the username.
    let bob = profile_card(&store(), "u-bob", Some("ch")).unwrap();
    assert_eq!(bob.name, "bob");
    assert_eq!(bob.pronouns, None);
    assert_eq!(bob.presence, PresenceKind::Offline);
    assert_eq!(bob.relationship, RelationshipStatus::Friend);
}

#[test]
fn roles_are_highest_first_with_solid_colours_only() {
    let card = profile_card(&store(), ADA, Some("ch")).unwrap();
    // The unknown role is left out.
    assert_eq!(role_names(&card.roles), ["Admin", "Fancy", "Plain"]);
    assert_eq!(card.roles[0].colour.as_deref(), Some("#ff0000"));
    // A gradient can't be a dot.
    assert_eq!(card.roles[1].colour, None);
    assert_eq!(card.roles[2].colour, None);
}

#[test]
fn roles_follow_a_ranks_change() {
    let mut store = store();
    store.apply_role_ranks("srv", &["plain".into(), "fancy".into(), "admin".into()]);
    let card = profile_card(&store, ADA, Some("ch")).unwrap();
    assert_eq!(role_names(&card.roles), ["Plain", "Fancy", "Admin"]);
}

#[test]
fn there_are_no_roles_or_server_join_date_outside_server_channels() {
    for channel in [Some("dm"), Some("grp"), None] {
        let card = profile_card(&store(), ADA, channel).unwrap();
        assert!(card.roles.is_empty(), "{channel:?}");
        assert_eq!(card.joined_server, None);
    }
}

#[test]
fn join_dates_come_from_the_membership_and_the_id() {
    let card = profile_card(&store(), ADA, Some("ch")).unwrap();
    assert_eq!(
        card.joined_server,
        Some((
            "Test Server".to_string(),
            Utc.with_ymd_and_hms(2026, 3, 4, 10, 0, 0).unwrap()
        ))
    );
    assert_eq!(
        card.created,
        Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap())
    );
    // "u-bob" isn't a ULID: no creation date.
    assert_eq!(profile_card(&store(), "u-bob", None).unwrap().created, None);
}

#[test]
fn bots_are_flagged_and_blank_statuses_hidden() {
    let card = profile_card(&store(), "u-bot", None).unwrap();
    assert!(card.bot);
    assert_eq!(card.status, None);
}

#[test]
fn the_users_own_card_says_so() {
    assert!(profile_card(&store(), "u-me", Some("ch")).unwrap().is_me);
}

#[test]
fn an_unknown_user_has_no_card() {
    assert_eq!(profile_card(&store(), "u-nobody", Some("ch")), None);
}
