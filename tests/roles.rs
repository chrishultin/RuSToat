//! A server's roles as the Roles page shows and edits them: the rows and
//! their locks, role names, the editor's form and who may use it, and
//! Stoat's refusals in words.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{FieldsRole, PartialRole, Role};
use stoat_gpui::permissions::RoleLock;
use stoat_gpui::profile_edit::ProfileImage;
use stoat_gpui::roles::{
    ColourChoice, ROLE_NAME_MAX, RoleDrop, RoleForm, RoleRow, can_manage_permissions,
    can_manage_roles, check_role_name, create_error_text, delete_error_text, draggable,
    edit_error_text, empty_role_edit, hex_of, may_delete, may_edit, may_edit_permissions,
    may_reorder, reorder, reorder_error_text, role_lock_notes, role_order, role_rows, role_update,
};
use stoat_gpui::store::ChatStore;

const MANAGE_ROLE: u64 = ChannelPermission::ManageRole as u64;
const MANAGE_PERMISSIONS: u64 = ChannelPermission::ManagePermissions as u64;

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
    store_with(me, MANAGE_ROLE)
}

/// [`store`], with `mods` allowing `permissions`.
fn store_with(me: &str, permissions: u64) -> ChatStore {
    let mut mods = role("mods", "Mods", 1);
    mods["permissions"]["a"] = json!(permissions);
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

fn members_role() -> Role {
    store("u-owner").server("srv").unwrap().roles["members"].clone()
}

#[test]
fn an_unchanged_form_sends_nothing() {
    let role = members_role();
    assert_eq!(
        RoleForm::of(&role),
        RoleForm {
            name: "Members".into(),
            hoist: true,
            colour: ColourChoice::Hex("#e5484d".into()),
            icon: ProfileImage::Current("f-icon".into()),
        }
    );
    assert_eq!(RoleForm::of(&role).changes(&role), None);
    // Spaces around the name aren't a change.
    let spaced = RoleForm {
        name: " Members ".into(),
        ..RoleForm::of(&role)
    };
    assert_eq!(spaced.changes(&role), None);
}

#[test]
fn a_rename_sends_only_the_name_trimmed() {
    let form = RoleForm {
        name: "  Regulars ".into(),
        ..RoleForm::of(&members_role())
    };
    let edit = form.changes(&members_role()).unwrap();
    assert_eq!(
        edit,
        stoat_gpui::api::models::DataEditRole {
            name: Some("Regulars".into()),
            ..empty_role_edit()
        }
    );
}

#[test]
fn toggling_hoist_sends_only_hoist() {
    let form = RoleForm {
        hoist: false,
        ..RoleForm::of(&members_role())
    };
    let edit = form.changes(&members_role()).unwrap();
    assert_eq!(
        edit,
        stoat_gpui::api::models::DataEditRole {
            hoist: Some(false),
            ..empty_role_edit()
        }
    );
}

#[test]
fn an_invalid_name_sends_nothing() {
    for name in ["  ", &"x".repeat(33)] {
        let form = RoleForm {
            name: name.to_string(),
            hoist: false,
            colour: ColourChoice::None,
            icon: ProfileImage::None,
        };
        assert!(!form.is_valid());
        assert_eq!(form.changes(&members_role()), None, "{name:?}");
    }
}

#[test]
fn the_owner_may_edit_any_role_and_delete_all_but_a_bots() {
    let store = store("u-owner");
    for role in ["admin", "mods", "bot", "members", "fans"] {
        assert!(may_edit(&store, "srv", role), "{role}");
        assert_eq!(may_delete(&store, "srv", role), role != "bot", "{role}");
    }
}

#[test]
fn a_moderator_may_edit_and_delete_only_below_their_role() {
    let store = store("u-mod");
    for role in ["admin", "mods"] {
        assert!(!may_edit(&store, "srv", role), "{role}");
        assert!(!may_delete(&store, "srv", role), "{role}");
    }
    // A bot's role below them can be edited, not deleted.
    assert!(may_edit(&store, "srv", "bot"));
    assert!(!may_delete(&store, "srv", "bot"));
    for role in ["members", "fans"] {
        assert!(may_edit(&store, "srv", role), "{role}");
        assert!(may_delete(&store, "srv", role), "{role}");
    }
}

#[test]
fn without_manage_role_or_a_role_nothing_may_be_edited() {
    let store = store("u-plain");
    assert!(!may_edit(&store, "srv", "fans"));
    assert!(!may_delete(&store, "srv", "fans"));
    let owner = self::store("u-owner");
    assert!(!may_edit(&owner, "srv", "r-unknown"));
    assert!(!may_edit(&owner, "srv-unknown", "fans"));
}

#[test]
fn a_moderator_whose_role_moves_below_one_loses_it() {
    let mut store = store("u-mod");
    assert!(may_edit(&store, "srv", "members"));
    store.apply_role_ranks(
        "srv",
        &["admin", "members", "mods", "bot", "fans"].map(String::from),
    );
    assert!(!may_edit(&store, "srv", "members"));
}

#[test]
fn edit_and_delete_refusals_read_plainly() {
    let not_elevated = api_error(json!({ "type": "NotElevated" }));
    assert_eq!(
        edit_error_text(&not_elevated),
        "That role ranks at or above your highest role."
    );
    assert_eq!(
        delete_error_text(&not_elevated),
        "That role ranks at or above your highest role."
    );
    let missing = api_error(json!({ "type": "MissingPermission", "permission": "ManageRole" }));
    assert_eq!(
        edit_error_text(&missing),
        "You don't have permission to manage roles."
    );
    assert_eq!(
        delete_error_text(&missing),
        "You don't have permission to manage roles."
    );
    assert_eq!(
        delete_error_text(&api_error(json!({ "type": "InvalidOperation" }))),
        "Roles that belong to a bot can't be deleted."
    );
    assert_eq!(
        edit_error_text(&api_error(json!({ "type": "InternalError" }))),
        "Couldn't save the role."
    );
    assert_eq!(
        delete_error_text(&api_error(json!({ "type": "InternalError" }))),
        "Couldn't delete the role."
    );
}

#[test]
fn an_edit_applied_then_its_event_is_consistent() {
    let mut store = store("u-owner");
    let saved = Role {
        name: "Regulars".into(),
        hoist: false,
        ..members_role()
    };
    let (data, clear) = role_update(&saved);
    assert!(store.apply_role_update("srv", "members", data, &clear));
    // Stoat's event carries only what changed.
    let event = json!({
        "type": "ServerRoleUpdate", "id": "srv", "role_id": "members",
        "data": { "name": "Regulars", "hoist": false }, "clear": []
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
    assert_eq!(store.server("srv").unwrap().roles["members"], saved);
}

#[test]
fn solid_hex_colours_are_picked_and_anything_else_is_custom() {
    assert_eq!(ColourChoice::of(None), ColourChoice::None);
    assert_eq!(
        ColourChoice::of(Some("#E5484D")),
        ColourChoice::Hex("#e5484d".into())
    );
    assert_eq!(
        ColourChoice::of(Some(" #fa0 ")),
        ColourChoice::Hex("#ffaa00".into())
    );
    for css in ["linear-gradient(red, blue)", "red", "#ff000080", "var(--x)"] {
        assert_eq!(
            ColourChoice::of(Some(css)),
            ColourChoice::Custom(css.into()),
            "{css}"
        );
    }
    // A colour name is still drawn as the solid colour it is.
    assert_eq!(
        ColourChoice::Custom("red".into()).solid().as_deref(),
        Some("#ff0000")
    );
    assert_eq!(
        ColourChoice::Custom("linear-gradient(red, blue)".into()).solid(),
        None
    );
    assert_eq!(
        ColourChoice::Hex("#e5484d".into()).solid().as_deref(),
        Some("#e5484d")
    );
}

#[test]
fn every_palette_colour_round_trips_through_the_picker() {
    for (name, hex) in stoat_gpui::rail::PALETTE {
        let rgba = stoat_gpui::rail::parse_colour(hex).unwrap();
        let hsla = gpui_kit::Hsla::from(rgba);
        assert_eq!(hex_of(hsla), hex.to_lowercase(), "{name}");
    }
    // Every grey step too, where rounding would show.
    for step in 0..=255u32 {
        let hex = format!("#{step:02x}{step:02x}{step:02x}");
        let hsla = gpui_kit::Hsla::from(stoat_gpui::rail::parse_colour(&hex).unwrap());
        assert_eq!(hex_of(hsla), hex);
    }
}

#[test]
fn picking_another_colour_sends_it() {
    let form = RoleForm {
        colour: ColourChoice::Hex("#3e63dd".into()),
        ..RoleForm::of(&members_role())
    };
    assert_eq!(
        form.changes(&members_role()).unwrap(),
        stoat_gpui::api::models::DataEditRole {
            colour: Some("#3e63dd".into()),
            ..empty_role_edit()
        }
    );
}

#[test]
fn the_same_colour_in_another_case_is_no_change() {
    let mut role = members_role();
    role.colour = Some("#E5484D".into());
    let form = RoleForm {
        colour: ColourChoice::Hex("#e5484d".into()),
        ..RoleForm::of(&role)
    };
    assert_eq!(form.changes(&role), None);
}

#[test]
fn no_colour_removes_it() {
    let form = RoleForm {
        colour: ColourChoice::None,
        ..RoleForm::of(&members_role())
    };
    assert_eq!(
        form.changes(&members_role()).unwrap(),
        stoat_gpui::api::models::DataEditRole {
            remove: vec![FieldsRole::Colour],
            ..empty_role_edit()
        }
    );
}

#[test]
fn a_custom_colour_survives_a_rename() {
    let fans = store("u-owner").server("srv").unwrap().roles["fans"].clone();
    let form = RoleForm::of(&fans);
    assert_eq!(
        form.colour,
        ColourChoice::Custom("linear-gradient(red, blue)".into())
    );
    assert_eq!(form.changes(&fans), None);
    let renamed = RoleForm {
        name: "Superfans".into(),
        ..form
    };
    assert_eq!(
        renamed.changes(&fans).unwrap(),
        stoat_gpui::api::models::DataEditRole {
            name: Some("Superfans".into()),
            ..empty_role_edit()
        }
    );
    // Replacing it with a picked colour sends that.
    let picked = RoleForm {
        colour: ColourChoice::Hex("#30a46c".into()),
        ..RoleForm::of(&fans)
    };
    assert_eq!(
        picked.changes(&fans).unwrap().colour.as_deref(),
        Some("#30a46c")
    );
}

#[test]
fn a_removed_colour_saved_clears_it_in_the_store() {
    let mut store = store("u-owner");
    let saved = Role {
        colour: None,
        ..members_role()
    };
    let (data, clear) = role_update(&saved);
    assert!(store.apply_role_update("srv", "members", data, &clear));
    assert_eq!(store.server("srv").unwrap().roles["members"].colour, None);
    let rows = role_rows(&store, "srv");
    assert_eq!(rows[3].colour, None);
}

#[test]
fn the_form_starts_with_the_roles_icon() {
    assert_eq!(
        RoleForm::of(&members_role()).icon,
        ProfileImage::Current("f-icon".into())
    );
    let fans = store("u-owner").server("srv").unwrap().roles["fans"].clone();
    assert_eq!(RoleForm::of(&fans).icon, ProfileImage::None);
}

#[test]
fn a_new_icon_is_sent_by_its_id_without_a_remove() {
    let form = RoleForm {
        icon: ProfileImage::Current("f-new".into()),
        ..RoleForm::of(&members_role())
    };
    assert_eq!(
        form.changes(&members_role()).unwrap(),
        stoat_gpui::api::models::DataEditRole {
            icon: Some("f-new".into()),
            ..empty_role_edit()
        }
    );
}

#[test]
fn a_removed_icon_is_in_remove() {
    let form = RoleForm {
        icon: ProfileImage::None,
        ..RoleForm::of(&members_role())
    };
    assert_eq!(
        form.changes(&members_role()).unwrap(),
        stoat_gpui::api::models::DataEditRole {
            remove: vec![FieldsRole::Icon],
            ..empty_role_edit()
        }
    );
    // Removing both sends both, in one edit.
    let bare = RoleForm {
        colour: ColourChoice::None,
        ..form
    };
    assert_eq!(
        bare.changes(&members_role()).unwrap().remove,
        [FieldsRole::Colour, FieldsRole::Icon]
    );
}

#[test]
fn a_picked_icon_waits_for_its_upload() {
    let picked = RoleForm {
        icon: ProfileImage::New("/tmp/icon.png".into()),
        ..RoleForm::of(&members_role())
    };
    assert_eq!(
        picked.new_file(),
        Some(std::path::Path::new("/tmp/icon.png"))
    );
    assert!(picked.has_changes(&members_role()));
    assert_eq!(picked.changes(&members_role()), None);
    // With a rename, the name goes now and the icon after its upload.
    let renamed = RoleForm {
        name: "Regulars".into(),
        ..picked
    };
    let edit = renamed.changes(&members_role()).unwrap();
    assert_eq!(edit.icon, None);
    assert!(edit.remove.is_empty());
    assert!(!RoleForm::of(&members_role()).has_changes(&members_role()));
}

fn ids(order: &[&str]) -> Vec<String> {
    order.iter().map(|id| id.to_string()).collect()
}

/// The fixture's order: admin, mods, bot, members, fans.
fn start() -> Vec<String> {
    ids(&["admin", "mods", "bot", "members", "fans"])
}

#[test]
fn the_order_is_the_lists() {
    assert_eq!(role_order(&store("u-owner"), "srv"), start());
}

#[test]
fn reorder_moves_a_role_before_or_after_another() {
    let order = start();
    assert_eq!(
        reorder(&order, "fans", &RoleDrop::Before("admin".into())),
        Some(ids(&["fans", "admin", "mods", "bot", "members"]))
    );
    assert_eq!(
        reorder(&order, "admin", &RoleDrop::After("fans".into())),
        Some(ids(&["mods", "bot", "members", "fans", "admin"]))
    );
    assert_eq!(
        reorder(&order, "members", &RoleDrop::After("mods".into())),
        Some(ids(&["admin", "mods", "members", "bot", "fans"]))
    );
}

#[test]
fn a_drop_that_changes_nothing_is_none() {
    let order = start();
    // Beside itself, or where it already is.
    assert_eq!(
        reorder(&order, "bot", &RoleDrop::Before("bot".into())),
        None
    );
    assert_eq!(
        reorder(&order, "bot", &RoleDrop::After("mods".into())),
        None
    );
    assert_eq!(
        reorder(&order, "bot", &RoleDrop::Before("members".into())),
        None
    );
    // Unknown roles.
    assert_eq!(
        reorder(&order, "r-gone", &RoleDrop::Before("admin".into())),
        None
    );
    assert_eq!(
        reorder(&order, "bot", &RoleDrop::Before("r-gone".into())),
        None
    );
}

#[test]
fn the_owner_may_move_anything() {
    let store = store("u-owner");
    let new = ids(&["fans", "bot", "members", "mods", "admin"]);
    assert!(may_reorder(&store, "srv", &start(), &new));
    for role in ["admin", "mods", "bot", "members", "fans"] {
        assert!(draggable(&store, "srv", role), "{role}");
    }
}

#[test]
fn a_moderator_may_reorder_the_roles_below_them() {
    let store = store("u-mod");
    let swapped = ids(&["admin", "mods", "members", "bot", "fans"]);
    assert!(may_reorder(&store, "srv", &start(), &swapped));
    let last = ids(&["admin", "mods", "fans", "bot", "members"]);
    assert!(may_reorder(&store, "srv", &start(), &last));
    // A bot's role below them moves like any other.
    assert!(draggable(&store, "srv", "bot"));
    assert!(draggable(&store, "srv", "members"));
}

#[test]
fn nobody_moves_their_own_role_a_higher_one_or_into_their_place() {
    let store = store("u-mod");
    // Their own role, down.
    let own = ids(&["admin", "bot", "mods", "members", "fans"]);
    assert!(!may_reorder(&store, "srv", &start(), &own));
    // A higher one.
    let higher = ids(&["mods", "admin", "bot", "members", "fans"]);
    assert!(!may_reorder(&store, "srv", &start(), &higher));
    // A lower role into a place at or above theirs pushes theirs down.
    let above = ids(&["admin", "fans", "mods", "bot", "members"]);
    assert!(!may_reorder(&store, "srv", &start(), &above));
    let top = ids(&["fans", "admin", "mods", "bot", "members"]);
    assert!(!may_reorder(&store, "srv", &start(), &top));
    assert!(!draggable(&store, "srv", "admin"));
    assert!(!draggable(&store, "srv", "mods"));
}

#[test]
fn an_order_missing_or_repeating_a_role_is_refused() {
    let store = store("u-owner");
    let missing = ids(&["admin", "mods", "bot", "members"]);
    assert!(!may_reorder(&store, "srv", &start(), &missing));
    let repeated = ids(&["admin", "mods", "bot", "members", "members"]);
    assert!(!may_reorder(&store, "srv", &start(), &repeated));
    let extra = ids(&["admin", "mods", "bot", "members", "fans", "r-gone"]);
    assert!(!may_reorder(&store, "srv", &start(), &extra));
}

#[test]
fn without_manage_role_nothing_moves() {
    let store = store("u-plain");
    let swapped = ids(&["admin", "mods", "bot", "fans", "members"]);
    assert!(!may_reorder(&store, "srv", &start(), &swapped));
    assert!(!draggable(&store, "srv", "fans"));
}

#[test]
fn reorder_refusals_read_plainly() {
    assert_eq!(
        reorder_error_text(&api_error(json!({ "type": "NotElevated" }))),
        "You can only move roles below your highest role, among the places below it."
    );
    assert_eq!(
        reorder_error_text(&api_error(json!({ "type": "InvalidOperation" }))),
        "The server's roles changed meanwhile. Try again."
    );
    assert_eq!(
        reorder_error_text(&api_error(
            json!({ "type": "MissingPermission", "permission": "ManageRole" })
        )),
        "You don't have permission to manage roles."
    );
    assert_eq!(
        reorder_error_text(&api_error(json!({ "type": "InternalError" }))),
        "Couldn't reorder the roles."
    );
}

fn ranks_update(ranks: &[&str]) -> ServerEvent {
    let event = json!({ "type": "ServerRoleRanksUpdate", "id": "srv", "ranks": ranks });
    parse_frame(&event.to_string()).pop().expect("ranks update")
}

#[test]
fn our_order_then_its_event_is_one_consistent_order() {
    let mut store = store("u-owner");
    let new = ids(&["fans", "admin", "mods", "bot", "members"]);
    assert!(store.apply_role_ranks("srv", &new));
    assert_eq!(role_order(&store, "srv"), new);
    let ServerEvent::ServerRoleRanksUpdate { server, ranks } =
        ranks_update(&["fans", "admin", "mods", "bot", "members"])
    else {
        panic!("ranks update");
    };
    assert!(store.apply_role_ranks(&server, &ranks));
    assert_eq!(role_order(&store, "srv"), new);
    let roles = &store.server("srv").unwrap().roles;
    assert_eq!(roles["fans"].rank, 0);
    assert_eq!(roles["members"].rank, 4);
    // Someone else's later order wins.
    let ServerEvent::ServerRoleRanksUpdate { server, ranks } =
        ranks_update(&["admin", "mods", "bot", "members", "fans"])
    else {
        panic!("ranks update");
    };
    assert!(store.apply_role_ranks(&server, &ranks));
    assert_eq!(role_order(&store, "srv"), start());
}

#[test]
fn a_refused_order_is_put_back() {
    let mut store = store("u-owner");
    let old = role_order(&store, "srv");
    let new = ids(&["members", "admin", "mods", "bot", "fans"]);
    store.apply_role_ranks("srv", &new);
    store.apply_role_ranks("srv", &old);
    assert_eq!(role_order(&store, "srv"), start());
}

#[test]
fn who_may_manage_roles_and_permissions() {
    let owner = store("u-owner");
    assert!(can_manage_roles(&owner, "srv"));
    assert!(can_manage_permissions(&owner, "srv"));
    let roles_only = store_with("u-mod", MANAGE_ROLE);
    assert!(can_manage_roles(&roles_only, "srv"));
    assert!(!can_manage_permissions(&roles_only, "srv"));
    let permissions_only = store_with("u-mod", MANAGE_PERMISSIONS);
    assert!(!can_manage_roles(&permissions_only, "srv"));
    assert!(can_manage_permissions(&permissions_only, "srv"));
    let plain = store("u-plain");
    assert!(!can_manage_roles(&plain, "srv"));
    assert!(!can_manage_permissions(&plain, "srv"));
    assert!(!can_manage_permissions(&owner, "srv-unknown"));
}

#[test]
fn the_owner_may_edit_any_roles_permissions() {
    let store = store("u-owner");
    for role in ["admin", "mods", "bot", "members", "fans"] {
        assert!(may_edit_permissions(&store, "srv", role), "{role}");
    }
    assert!(!may_edit_permissions(&store, "srv", "r-unknown"));
}

#[test]
fn a_moderator_may_edit_permissions_only_below_their_role() {
    let store = store_with("u-mod", MANAGE_PERMISSIONS);
    // Above, and their own.
    assert!(!may_edit_permissions(&store, "srv", "admin"));
    assert!(!may_edit_permissions(&store, "srv", "mods"));
    // Below, a bot's role too.
    for role in ["bot", "members", "fans"] {
        assert!(may_edit_permissions(&store, "srv", role), "{role}");
    }
    // Manage Role alone isn't enough.
    let roles_only = store_with("u-mod", MANAGE_ROLE);
    assert!(!may_edit_permissions(&roles_only, "srv", "members"));
}

#[test]
fn manage_permissions_alone_edits_no_roles_fields_and_moves_nothing() {
    let store = store_with("u-mod", MANAGE_PERMISSIONS);
    for role in ["members", "fans", "bot"] {
        assert!(!may_edit(&store, "srv", role), "{role}");
        assert!(!may_delete(&store, "srv", role), "{role}");
        assert!(!draggable(&store, "srv", role), "{role}");
    }
    let swapped = ids(&["admin", "mods", "bot", "fans", "members"]);
    assert!(!may_reorder(&store, "srv", &start(), &swapped));
}

#[test]
fn lock_notes_say_what_the_user_cant_change() {
    let owner = store("u-owner");
    assert!(role_lock_notes(&owner, "srv", "members").is_empty());
    assert_eq!(
        role_lock_notes(&owner, "srv", "bot"),
        ["A bot manages this role. You can edit it, but not delete it."]
    );
    let roles_only = store_with("u-mod", MANAGE_ROLE);
    assert_eq!(
        role_lock_notes(&roles_only, "srv", "admin"),
        ["This role ranks at or above your highest role, so you can't change it."]
    );
    assert_eq!(
        role_lock_notes(&roles_only, "srv", "members"),
        ["You don't have Manage Permissions, so you can't change this role's permissions."]
    );
    let permissions_only = store_with("u-mod", MANAGE_PERMISSIONS);
    // Deleting needs Manage Role anyway, so a bot's role says nothing more.
    assert_eq!(
        role_lock_notes(&permissions_only, "srv", "bot"),
        [
            "You don't have Manage Roles, so you can't change this role's name, colour, icon \
             or hoisting."
        ]
    );
    let both = store_with("u-mod", MANAGE_ROLE | MANAGE_PERMISSIONS);
    assert!(role_lock_notes(&both, "srv", "fans").is_empty());
    assert!(role_lock_notes(&both, "srv", "r-unknown").is_empty());
}
