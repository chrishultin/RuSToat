//! Groups against a mock Stoat API: `POST /channels/create` (create a
//! group), `PUT` and `DELETE /channels/{id}/recipients/{user}` (add and
//! remove a member), `PATCH /channels/{id}` (hand it on) and `DELETE
//! /channels/{id}` (leave it).

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::models::Channel;
use stoat_gpui::api::{ApiError, StoatClient};
use wiremock::matchers::{body_json, body_string, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn creating_a_group_sends_only_its_name_and_people() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/create"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "name": "Trip", "users": ["u1", "u2"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "Group", "_id": "g1", "name": "Trip", "owner": "u-me",
            "recipients": ["u-me", "u1", "u2"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let users = ["u1".to_string(), "u2".to_string()];
    let channel = client(&server).create_group("Trip", &users).await.unwrap();
    let Channel::Group {
        id,
        name,
        owner,
        recipients,
        ..
    } = &channel
    else {
        panic!("a group: {channel:?}");
    };
    assert_eq!((id.as_str(), name.as_str()), ("g1", "Trip"));
    assert_eq!(owner, "u-me");
    assert_eq!(recipients, &["u-me", "u1", "u2"]);
}

#[tokio::test]
async fn a_group_of_one_sends_no_people() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/create"))
        .and(body_json(json!({ "name": "Notes", "users": [] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "Group", "_id": "g2", "name": "Notes", "owner": "u-me",
            "recipients": ["u-me"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let channel = client(&server).create_group("Notes", &[]).await.unwrap();
    assert!(matches!(channel, Channel::Group { .. }), "{channel:?}");
}

#[tokio::test]
async fn creating_a_group_with_a_stranger_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/create"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "type": "NotFriends" })))
        .mount(&server)
        .await;

    let users = ["u-stranger".to_string()];
    let err = client(&server)
        .create_group("Trip", &users)
        .await
        .unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotFriends)),
        "{err:?}"
    );
}

#[tokio::test]
async fn too_large_a_group_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/create"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(json!({ "type": "GroupTooLarge", "max": 50 })),
        )
        .mount(&server)
        .await;

    let err = client(&server).create_group("Trip", &[]).await.unwrap_err();
    assert!(
        matches!(err.error_type(), Some(ErrorType::GroupTooLarge { max: 50 })),
        "{err:?}"
    );
}

#[tokio::test]
async fn adding_a_member_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/channels/g1/recipients/u1"))
        .and(header("x-session-token", "tok"))
        .and(body_string(""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server).add_group_member("g1", "u1").await.unwrap();
}

#[tokio::test]
async fn removing_a_member_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/channels/g1/recipients/u1"))
        .and(header("x-session-token", "tok"))
        .and(body_string(""))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .remove_group_member("g1", "u1")
        .await
        .unwrap();
}

#[tokio::test]
async fn adding_someone_already_in_the_group_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/channels/g1/recipients/u1"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({ "type": "AlreadyInGroup" })))
        .mount(&server)
        .await;

    let err = client(&server)
        .add_group_member("g1", "u1")
        .await
        .unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(
        matches!(err.error_type(), Some(ErrorType::AlreadyInGroup)),
        "{err:?}"
    );
}

#[tokio::test]
async fn adding_a_stranger_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/channels/g1/recipients/u-stranger"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "type": "NotFriends" })))
        .mount(&server)
        .await;

    let err = client(&server)
        .add_group_member("g1", "u-stranger")
        .await
        .unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotFriends)),
        "{err:?}"
    );
}

#[tokio::test]
async fn leaving_a_group_says_whether_silently() {
    let server = MockServer::start().await;
    for silently in ["true", "false"] {
        Mock::given(method("DELETE"))
            .and(path("/channels/g1"))
            .and(header("x-session-token", "tok"))
            .and(query_param("leave_silently", silently))
            .and(body_string(""))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
    }

    client(&server).leave_channel("g1", true).await.unwrap();
    client(&server).leave_channel("g1", false).await.unwrap();
}

#[tokio::test]
async fn handing_a_group_on_sends_only_its_new_owner() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/channels/g1"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "owner": "u2" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "channel_type": "Group", "_id": "g1", "name": "Trip", "owner": "u2",
            "recipients": ["u-me", "u2"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let channel = client(&server).transfer_group("g1", "u2").await.unwrap();
    let Channel::Group { owner, .. } = &channel else {
        panic!("a group: {channel:?}");
    };
    assert_eq!(owner, "u2");
}

#[tokio::test]
async fn handing_on_a_group_the_user_doesnt_own_is_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/channels/g1"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "type": "NotOwner" })))
        .mount(&server)
        .await;

    let err = client(&server)
        .transfer_group("g1", "u2")
        .await
        .unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(
        matches!(err.error_type(), Some(ErrorType::NotOwner)),
        "{err:?}"
    );
}
