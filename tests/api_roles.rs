//! A server's roles against a mock Stoat API, from Server Settings' Roles
//! page: `POST /servers/{id}/roles` (create one).

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::StoatClient;
use stoat_gpui::roles::create_error_text;
use wiremock::matchers::{body_json, header, method, path};
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
