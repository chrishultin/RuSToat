//! A server's roles against a mock Stoat API, from Server Settings' Roles
//! page: `POST /servers/{id}/roles` (create one), `PATCH
//! /servers/{id}/roles/{role}` (edit one, after uploading its icon to the
//! file server), `DELETE /servers/{id}/roles/{role}` (delete one),
//! `PATCH /servers/{id}/roles/ranks` (reorder them),
//! `PUT /servers/{id}/permissions/{role}` (a role's permissions) and
//! `PUT /servers/{id}/permissions/default` (Everyone's permissions).

use revolt_permissions::{ChannelPermission, Override};
use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::StoatClient;
use stoat_gpui::api::models::{DataEditRole, Role};
use stoat_gpui::permission_matrix::permissions_error_text;
use stoat_gpui::roles::RoleForm;
use stoat_gpui::roles::{
    create_error_text, delete_error_text, edit_error_text, empty_role_edit, reorder_error_text,
};
use wiremock::matchers::{body_json, body_string, header, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn creating_a_role_sends_only_its_name() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/servers/srv/roles"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "name": "Mods" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "r-new",
            "role": {
                "_id": "r-new", "name": "Mods", "rank": 3,
                "permissions": { "a": 0, "d": 0 }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let created = client(&server).create_role("srv", "Mods").await.unwrap();
    assert_eq!(created.id, "r-new");
    assert_eq!(created.role.id, "r-new");
    assert_eq!(created.role.name, "Mods");
    assert_eq!(created.role.rank, 3);
    assert!(!created.role.hoist);
    assert_eq!((created.role.colour, created.role.icon), (None, None));
}

#[tokio::test]
async fn too_many_roles_reads_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/servers/srv/roles"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "type": "TooManyRoles", "max": 200 })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .create_role("srv", "Mods")
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::TooManyRoles { max: 200 })),
        "{err:?}"
    );
    assert_eq!(
        create_error_text(&err),
        "This server has 200 roles, the most Stoat allows."
    );
}

#[tokio::test]
async fn creating_without_manage_role_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/servers/srv/roles"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "type": "MissingPermission", "permission": "ManageRole"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .create_role("srv", "Mods")
        .await
        .unwrap_err();
    assert_eq!(
        create_error_text(&err),
        "You don't have permission to manage roles."
    );
}

#[tokio::test]
async fn editing_a_role_sends_only_what_changed() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/r1"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "name": "Mods" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "r1", "name": "Mods", "rank": 1, "hoist": true,
            "permissions": { "a": 0, "d": 0 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let edit = DataEditRole {
        name: Some("Mods".into()),
        ..empty_role_edit()
    };
    let saved = client(&server).edit_role("srv", "r1", &edit).await.unwrap();
    assert_eq!((saved.id.as_str(), saved.name.as_str()), ("r1", "Mods"));
    assert!(saved.hoist);
}

#[tokio::test]
async fn hoisting_a_role_sends_only_hoist() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/r1"))
        .and(body_json(json!({ "hoist": false })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "r1", "name": "Mods", "rank": 1, "permissions": { "a": 0, "d": 0 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let edit = DataEditRole {
        hoist: Some(false),
        ..empty_role_edit()
    };
    client(&server).edit_role("srv", "r1", &edit).await.unwrap();
}

#[tokio::test]
async fn editing_a_higher_role_is_refused_in_words() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/r1"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "type": "NotElevated" })))
        .mount(&server)
        .await;

    let edit = DataEditRole {
        name: Some("Mods".into()),
        ..empty_role_edit()
    };
    let err = client(&server)
        .edit_role("srv", "r1", &edit)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotElevated)),
        "{err:?}"
    );
    assert_eq!(
        edit_error_text(&err),
        "That role ranks at or above your highest role."
    );
}

#[tokio::test]
async fn deleting_a_role_sends_one_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/servers/srv/roles/r1"))
        .and(header("x-session-token", "tok"))
        .and(body_string(""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).delete_role("srv", "r1").await.unwrap();
}

#[tokio::test]
async fn deleting_a_bots_role_is_refused_in_words() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/servers/srv/roles/r1"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "type": "InvalidOperation" })),
        )
        .mount(&server)
        .await;

    let err = client(&server).delete_role("srv", "r1").await.unwrap_err();
    assert_eq!(
        delete_error_text(&err),
        "Roles that belong to a bot can't be deleted."
    );
}

#[tokio::test]
async fn deleting_a_role_already_gone_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/servers/srv/roles/r1"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "type": "NotFound" })))
        .mount(&server)
        .await;

    let err = client(&server).delete_role("srv", "r1").await.unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotFound)),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_role_icon_is_uploaded_then_saved_by_its_id() {
    use stoat_gpui::api::models::Role;
    use stoat_gpui::profile_edit::ProfileImage;
    use stoat_gpui::roles::RoleForm;

    let autumn = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/icons"))
        .and(header("x-session-token", "tok"))
        .and(header_regex(
            "content-type",
            "^multipart/form-data; boundary=",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "f-icon" })))
        .expect(1)
        .mount(&autumn)
        .await;
    let api = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/r1"))
        .and(body_json(json!({ "icon": "f-icon" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "r1", "name": "Mods", "rank": 1, "permissions": { "a": 0, "d": 0 },
            "icon": { "_id": "f-icon", "tag": "icons", "filename": "icon.png",
                      "metadata": { "type": "Image", "width": 64, "height": 64 },
                      "content_type": "image/png", "size": 6 }
        })))
        .expect(1)
        .mount(&api)
        .await;

    let role: Role = serde_json::from_value(json!({
        "_id": "r1", "name": "Mods", "rank": 1, "permissions": { "a": 0, "d": 0 }
    }))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("icon.png");
    std::fs::write(&file, b"\x89PNG\r\n").unwrap();
    let mut form = RoleForm::of(&role);
    form.icon = ProfileImage::New(file.clone());
    assert!(form.has_changes(&role));
    assert_eq!(form.changes(&role), None);

    let client = client(&api);
    let path = form.new_file().unwrap().to_path_buf();
    let id = client
        .upload_file(
            &autumn.uri(),
            "icons",
            "icon.png",
            "image/png",
            &path,
            |_, _| {},
        )
        .await
        .unwrap();
    form.icon = ProfileImage::Current(id);
    let edit = form.changes(&role).unwrap();
    assert!(edit.remove.is_empty());
    let saved = client.edit_role("srv", "r1", &edit).await.unwrap();
    assert_eq!(saved.icon.map(|icon| icon.id).as_deref(), Some("f-icon"));
}

#[tokio::test]
async fn removing_a_colour_and_icon_sends_both_in_remove() {
    use stoat_gpui::api::models::FieldsRole;

    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/r1"))
        .and(body_json(json!({ "remove": ["Colour", "Icon"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "r1", "name": "Mods", "rank": 1, "permissions": { "a": 0, "d": 0 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let edit = DataEditRole {
        remove: vec![FieldsRole::Colour, FieldsRole::Icon],
        ..empty_role_edit()
    };
    client(&server).edit_role("srv", "r1", &edit).await.unwrap();
}

#[tokio::test]
async fn a_new_colour_is_sent_as_hex() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/r1"))
        .and(body_json(json!({ "colour": "#3e63dd" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "r1", "name": "Mods", "rank": 1, "colour": "#3e63dd",
            "permissions": { "a": 0, "d": 0 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let edit = DataEditRole {
        colour: Some("#3e63dd".into()),
        ..empty_role_edit()
    };
    let saved = client(&server).edit_role("srv", "r1", &edit).await.unwrap();
    assert_eq!(saved.colour.as_deref(), Some("#3e63dd"));
}

#[tokio::test]
async fn reordering_sends_every_role_highest_first() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/ranks"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "ranks": ["r-a", "r-c", "r-b"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "srv", "owner": "u-me", "name": "Lounge", "channels": [],
            "default_permissions": 0, "approximate_member_count": 1,
            "roles": {
                "r-a": { "_id": "r-a", "name": "A", "rank": 0, "permissions": { "a": 0, "d": 0 } },
                "r-c": { "_id": "r-c", "name": "C", "rank": 1, "permissions": { "a": 0, "d": 0 } },
                "r-b": { "_id": "r-b", "name": "B", "rank": 2, "permissions": { "a": 0, "d": 0 } }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ranks: Vec<String> = ["r-a", "r-c", "r-b"].map(String::from).into();
    let saved = client(&server)
        .edit_role_ranks("srv", &ranks)
        .await
        .unwrap();
    assert_eq!(saved.roles["r-c"].rank, 1);
}

#[tokio::test]
async fn reordering_above_your_role_is_refused_in_words() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/ranks"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "type": "NotElevated" })))
        .mount(&server)
        .await;

    let ranks: Vec<String> = ["r-b", "r-a"].map(String::from).into();
    let err = client(&server)
        .edit_role_ranks("srv", &ranks)
        .await
        .unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotElevated)),
        "{err:?}"
    );
    assert_eq!(
        reorder_error_text(&err),
        "You can only move roles below your highest role, among the places below it."
    );
}

#[tokio::test]
async fn everyones_permissions_are_sent_as_one_value() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/permissions/default"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "permissions": 3_145_728u64 })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "srv", "owner": "u-me", "name": "Lounge", "channels": [],
            "default_permissions": 3_145_728u64, "approximate_member_count": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    let saved = client(&server)
        .set_default_permissions("srv", 3_145_728)
        .await
        .unwrap();
    assert_eq!(saved.default_permissions, 3_145_728);
}

#[tokio::test]
async fn granting_everyone_a_permission_you_lack_is_refused_in_words() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/permissions/default"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "CannotGiveMissingPermissions" })),
        )
        .mount(&server)
        .await;

    let err = client(&server)
        .set_default_permissions("srv", u64::from(u32::MAX))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err.error_type(),
            Some(ErrorType::CannotGiveMissingPermissions)
        ),
        "{err:?}"
    );
    assert_eq!(
        permissions_error_text(&err),
        "You can't grant permissions you don't have."
    );
}

const SEND: u64 = ChannelPermission::SendMessage as u64;
const REACT: u64 = ChannelPermission::React as u64;

/// `srv` as Stoat answers a permissions `PUT`, with `r1` allowing `allow`
/// and denying `deny`.
fn server_with_r1(name: &str, allow: u64, deny: u64) -> serde_json::Value {
    json!({
        "_id": "srv", "owner": "u-me", "name": "Lounge", "channels": [],
        "default_permissions": 0, "approximate_member_count": 1,
        "roles": {
            "r1": { "_id": "r1", "name": name, "rank": 0, "permissions": { "a": allow, "d": deny } }
        }
    })
}

#[tokio::test]
async fn a_roles_permissions_are_sent_as_allow_and_deny() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/permissions/r1"))
        .and(header("x-session-token", "tok"))
        .and(body_json(
            json!({ "permissions": { "allow": REACT, "deny": SEND } }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(server_with_r1("Mods", REACT, SEND)))
        .expect(1)
        .mount(&server)
        .await;

    let permissions = Override {
        allow: REACT,
        deny: SEND,
    };
    let saved = client(&server)
        .set_role_permissions("srv", "r1", &permissions)
        .await
        .unwrap();
    let role = &saved.roles["r1"];
    assert_eq!(
        (role.permissions.a, role.permissions.d),
        (REACT as i64, SEND as i64)
    );
}

#[tokio::test]
async fn a_rename_and_new_permissions_are_a_patch_then_a_put() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/servers/srv/roles/r1"))
        .and(body_json(json!({ "name": "Regulars" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "r1", "name": "Regulars", "rank": 0, "permissions": { "a": 0, "d": 0 }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/permissions/r1"))
        .and(body_json(
            json!({ "permissions": { "allow": REACT, "deny": 0 } }),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(server_with_r1("Regulars", REACT, 0)),
        )
        .expect(1)
        .mount(&server)
        .await;

    let role: Role = serde_json::from_value(json!({
        "_id": "r1", "name": "Mods", "rank": 0, "permissions": { "a": 0, "d": 0 }
    }))
    .unwrap();
    let form = RoleForm {
        name: "Regulars".into(),
        permissions: Override {
            allow: REACT,
            deny: 0,
        },
        ..RoleForm::of(&role)
    };
    // As the role editor's Save sends them: the PATCH first, then the PUT.
    let client = client(&server);
    let patched = client
        .edit_role("srv", "r1", &form.changes(&role).unwrap())
        .await
        .unwrap();
    assert_eq!(patched.name, "Regulars");
    let change = form.permissions_change(&patched).unwrap();
    let saved = client
        .set_role_permissions("srv", "r1", &change)
        .await
        .unwrap();
    let saved = &saved.roles["r1"];
    assert_eq!(saved.name, "Regulars");
    assert_eq!(saved.permissions.a, REACT as i64);
}

#[tokio::test]
async fn role_permission_refusals_read_plainly() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/permissions/r-high"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "type": "NotElevated" })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/permissions/r1"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "type": "CannotGiveMissingPermissions" })),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/servers/srv/permissions/r-gone"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "type": "NotFound" })))
        .mount(&server)
        .await;

    let client = client(&server);
    let allow = Override {
        allow: SEND,
        deny: 0,
    };
    let text = |role: &'static str| {
        let client = &client;
        let allow = &allow;
        async move {
            let err = client
                .set_role_permissions("srv", role, allow)
                .await
                .unwrap_err();
            permissions_error_text(&err)
        }
    };
    assert_eq!(
        text("r-high").await,
        "That role ranks at or above your highest role."
    );
    assert_eq!(
        text("r1").await,
        "You can't grant permissions you don't have."
    );
    assert_eq!(text("r-gone").await, "That role was deleted.");
}
