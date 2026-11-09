//! A server's roles against a mock Stoat API, from Server Settings' Roles
//! page: `POST /servers/{id}/roles` (create one), `PATCH
//! /servers/{id}/roles/{role}` (edit one) and `DELETE
//! /servers/{id}/roles/{role}` (delete one).

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::StoatClient;
use stoat_gpui::api::models::DataEditRole;
use stoat_gpui::roles::{create_error_text, delete_error_text, edit_error_text, empty_role_edit};
use wiremock::matchers::{body_json, body_string, header, method, path};
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
