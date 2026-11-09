//! A server's roles as the Roles page shows and edits them: the rows and
//! their locks, role names, and Stoat's refusals in words.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{FieldsRole, PartialRole, Role};
use stoat_gpui::permissions::RoleLock;
use stoat_gpui::roles::{
    ROLE_NAME_MAX, RoleRow, check_role_name, create_error_text, role_rows, role_update,
};
use stoat_gpui::store::ChatStore;

const MANAGE_ROLE: u64 = ChannelPermission::ManageRole as u64;

fn user(id: &str) -> Value {
    json!({
        "_id": id, "username": id, "discriminator": "0001",
        "relationship": "None", "online": true
    })
}

fn member(user: &str, roles: &[&str]) -> Value {
    json!({
        "_id": { "server": "srv", "user": user },
        "joined_at": "2026-01-01T00:00:00.000Z",
        "roles": roles,
    })
}

fn role(id: &str, name: &str, rank: i64) -> Value {
    json!({ "_id": id, "name": name, "rank": rank, "permissions": { "a": 0, "d": 0 } })
}

/// `srv`, owned by `u-owner`, with roles from highest: `admin`, `mods`
/// (Manage Role, held by `u-mod`), `bot` (a bot's), `members` (red,
/// hoisted, with an icon) and `fans` (a gradient). `u-plain` has none.
fn store(me: &str) -> ChatStore {
    let mut mods = role("mods", "Mods", 1);
    mods["permissions"]["a"] = json!(MANAGE_ROLE);
    let mut bot = role("bot", "Bot", 2);
    bot["owner"] = json!("u-bot");
    let mut members = role("members", "Members", 3);
    members["colour"] = json!("#E5484D");
    members["hoist"] = json!(true);
    members["icon"] = json!({
        "_id": "f-icon", "tag": "icons", "filename": "x.png",
        "metadata": { "type": "Image", "width": 1, "height": 1 },
        "content_type": "image/png", "size": 1
    });
    let mut fans = role("fans", "Fans", 4);
    fans["colour"] = json!("linear-gradient(red, blue)");
    let ready = json!({
        "type": "Ready",
        "users": [user("u-owner"), user("u-mod"), user("u-plain")],
        "servers": [{
            "_id": "srv", "owner": "u-owner", "name": "Test", "channels": [],
            "approximate_member_count": 3,
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "roles": {
                "admin": role("admin", "Admin", 0),
                "mods": mods,
                "bot": bot,
                "members": members,
                "fans": fans,
            }
        }],
        "channels": [],
        "members": [member("u-owner", &[]), member("u-mod", &["mods"]), member("u-plain", &[])],
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new(me);
    store.apply_ready(*ready);
    store
}

fn locks(rows: &[RoleRow]) -> Vec<(&str, Option<RoleLock>)> {
    rows.iter().map(|row| (row.id.as_str(), row.lock)).collect()
}

#[test]
fn rows_are_highest_first_and_the_owner_may_change_all_but_a_bots() {
    let rows = role_rows(&store("u-owner"), "srv");
    assert_eq!(
        locks(&rows),
        [
            ("admin", None),
            ("mods", None),
            ("bot", Some(RoleLock::Managed)),
            ("members", None),
            ("fans", None),
        ]
    );
}

#[test]
fn a_moderator_may_change_only_the_roles_below_their_own() {
    let rows = role_rows(&store("u-mod"), "srv");
    assert_eq!(
        locks(&rows),
        [
            ("admin", Some(RoleLock::Rank)),
            // Their own role ranks at their rank.
            ("mods", Some(RoleLock::Rank)),
            ("bot", Some(RoleLock::Managed)),
            ("members", None),
            ("fans", None),
        ]
    );
    assert_eq!(
        rows[0].lock_text(),
        Some("Ranks at or above your highest role")
    );
    assert_eq!(rows[2].lock_text(), Some("Managed by a bot"));
    assert_eq!(rows[3].lock_text(), None);
}

#[test]
fn a_bots_role_above_the_user_is_locked_by_rank() {
    // Rank is the stronger lock: such a role can't even be edited.
    let mut store = store("u-mod");
    store.apply_role_ranks(
        "srv",
        &["admin", "bot", "mods", "members", "fans"].map(String::from),
    );
    let rows = role_rows(&store, "srv");
    assert_eq!(rows[1].id, "bot");
    assert_eq!(rows[1].lock, Some(RoleLock::Rank));
}

#[test]
fn rows_carry_solid_colours_icons_and_hoisting() {
    let rows = role_rows(&store("u-owner"), "srv");
    let members = &rows[3];
    assert_eq!(members.name, "Members");
    assert_eq!(members.colour.as_deref(), Some("#e5484d"));
    assert_eq!(members.icon.as_deref(), Some("f-icon"));
    assert!(members.hoist);
    assert_eq!(members.rank, 3);
    // A gradient isn't a solid colour, and nothing else is set.
    let fans = &rows[4];
    assert_eq!((fans.colour.as_deref(), fans.icon.as_deref()), (None, None));
    assert!(!fans.hoist);
}

#[test]
fn ties_in_rank_are_broken_by_id() {
    let mut store = store("u-owner");
    for id in ["members", "admin", "fans"] {
        let data = PartialRole {
            rank: Some(0),
            ..PartialRole::default()
        };
        assert!(store.apply_role_update("srv", id, data, &[]));
    }
    let rows: Vec<String> = role_rows(&store, "srv").into_iter().map(|r| r.id).collect();
    assert_eq!(rows, ["admin", "fans", "members", "mods", "bot"]);
}

#[test]
fn nobody_without_a_known_server_sees_rows() {
    assert!(role_rows(&store("u-owner"), "srv-unknown").is_empty());
}

#[test]
fn role_names_are_1_to_32_characters_trimmed() {
    assert_eq!(ROLE_NAME_MAX, 32);
    assert_eq!(check_role_name("  "), None);
    assert_eq!(check_role_name(""), None);
    assert_eq!(check_role_name(" Mods "), Some("Mods".into()));
    let longest = "é".repeat(32);
    assert_eq!(check_role_name(&longest), Some(longest.clone()));
    assert_eq!(check_role_name(&"é".repeat(33)), None);
}

fn api_error(body: Value) -> ApiError {
    ApiError::from_response(
        reqwest::StatusCode::BAD_REQUEST,
        body.to_string().as_bytes(),
    )
}

#[test]
fn create_refusals_read_plainly() {
    assert_eq!(
        create_error_text(&api_error(json!({ "type": "TooManyRoles", "max": 200 }))),
        "This server has 200 roles, the most Stoat allows."
    );
    assert_eq!(
        create_error_text(&api_error(
            json!({ "type": "MissingPermission", "permission": "ManageRole" })
        )),
        "You don't have permission to manage roles."
    );
    assert_eq!(
        create_error_text(&api_error(
            json!({ "type": "FailedValidation", "error": "name: length" })
        )),
        "Role names are 1–32 characters."
    );
    assert_eq!(
        create_error_text(&api_error(json!({ "type": "InternalError" }))),
        "Couldn't create the role."
    );
}

fn created_role() -> Role {
    serde_json::from_value(json!({
        "_id": "testers", "name": "Testers", "rank": 5,
        "permissions": { "a": 0, "d": 0 }
    }))
    .unwrap()
}

#[test]
fn a_whole_role_update_clears_what_the_role_lacks() {
    let (data, clear) = role_update(&created_role());
    assert_eq!(data.name.as_deref(), Some("Testers"));
    assert_eq!(data.rank, Some(5));
    assert_eq!(data.hoist, Some(false));
    assert_eq!(clear, [FieldsRole::Colour, FieldsRole::Icon]);

    // Applied over a role that had both, it takes them away.
    let mut store = store("u-owner");
    let members = store.server("srv").unwrap().roles["members"].clone();
    let plain = Role {
        colour: None,
        icon: None,
        hoist: false,
        ..members.clone()
    };
    let (data, clear) = role_update(&plain);
    assert!(store.apply_role_update("srv", "members", data, &clear));
    assert_eq!(store.server("srv").unwrap().roles["members"], plain);
}

#[test]
fn a_created_role_then_its_event_is_one_role() {
    let mut store = store("u-owner");
    let role = created_role();
    let (data, clear) = role_update(&role);
    assert!(store.apply_role_update("srv", "testers", data, &clear));
    let event = json!({
        "type": "ServerRoleUpdate", "id": "srv", "role_id": "testers",
        "data": { "name": "Testers", "rank": 5, "permissions": { "a": 0, "d": 0 } },
        "clear": []
    });
    let Some(ServerEvent::ServerRoleUpdate {
        server,
        role_id,
        data,
        clear,
    }) = parse_frame(&event.to_string()).pop()
    else {
        panic!("role update");
    };
    assert!(store.apply_role_update(&server, &role_id, *data, &clear));
    let roles = &store.server("srv").unwrap().roles;
    assert_eq!(roles.len(), 6);
    assert_eq!(roles["testers"], role);
    let rows = role_rows(&store, "srv");
    assert_eq!(rows.last().map(|row| row.id.as_str()), Some("testers"));
}
