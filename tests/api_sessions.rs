//! The account's sessions against a mock Stoat API: listing, renaming and
//! logging them out.

use serde_json::json;
use stoat_gpui::api::StoatClient;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

#[tokio::test]
async fn listing_sessions_gets_every_one() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth/session/all"))
        .and(header("x-session-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "_id": "01K00000000000000000000000", "name": "RuSToat" },
            { "_id": "01J00000000000000000000000", "name": "Stoat for Web (Firefox on Linux)" }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let sessions = client(&server).fetch_sessions().await.unwrap();
    let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["RuSToat", "Stoat for Web (Firefox on Linux)"]);
    assert_eq!(sessions[0].id, "01K00000000000000000000000");
}

#[tokio::test]
async fn renaming_a_session_sends_only_its_friendly_name() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/auth/session/s1"))
        .and(header("x-session-token", "tok"))
        .and(body_json(json!({ "friendly_name": "Work Mac" })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "_id": "s1", "name": "Work Mac" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let session = client(&server)
        .rename_session("s1", "Work Mac")
        .await
        .unwrap();
    assert_eq!(
        (session.id.as_str(), session.name.as_str()),
        ("s1", "Work Mac")
    );
}

#[tokio::test]
async fn logging_out_a_session_sends_the_ticket() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/auth/session/s2"))
        .and(header("x-session-token", "tok"))
        .and(header("x-mfa-ticket", "tok-ticket"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .revoke_session("tok-ticket", "s2")
        .await
        .unwrap();
}

#[tokio::test]
async fn logging_out_the_others_spares_this_session() {
    use wiremock::matchers::query_param;
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/auth/session/all"))
        .and(query_param("revoke_self", "false"))
        .and(header("x-mfa-ticket", "tok-ticket"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .revoke_other_sessions("tok-ticket")
        .await
        .unwrap();
}

/// A ticket Stoat no longer takes is an API error, not the end of the
/// session, so nothing signs the account out.
#[tokio::test]
async fn a_refused_ticket_is_not_an_ended_session() {
    use revolt_result::ErrorType;
    use stoat_gpui::api::ApiError;
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/auth/session/s2"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "type": "InvalidToken" })))
        .mount(&server)
        .await;

    let err = client(&server)
        .revoke_session("used-ticket", "s2")
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApiError::Api(e) if matches!(e.error_type, ErrorType::InvalidToken)),
        "got {err:?}"
    );
    assert_eq!(
        stoat_gpui::security::action_error_text(&err, "Couldn't log the session out."),
        "That confirmation expired. Try again."
    );
}
