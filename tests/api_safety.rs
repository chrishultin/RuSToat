//! Reports against a mock Stoat API: `POST /safety/report` for a message,
//! a user and a server, and who may be reported.

use revolt_result::ErrorType;
use serde_json::json;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{ContentReportReason, UserReportReason};
use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::reports::{Reason, ReportTarget, can_report, report_body, report_error_text};
use stoat_gpui::store::ChatStore;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> StoatClient {
    StoatClient::new(server.uri()).with_token("tok")
}

/// Expect one report with exactly `body`.
async fn expect_report(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/safety/report"))
        .and(header("x-session-token", "tok"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_message_report_sends_its_id_and_reason() {
    let server = MockServer::start().await;
    expect_report(
        &server,
        json!({
            "content": { "type": "Message", "id": "m1", "report_reason": "SpamAbuse" },
            "additional_context": ""
        }),
    )
    .await;

    let target = ReportTarget::Message {
        id: "m1".into(),
        author: "u-ada".into(),
    };
    let reason = Reason::Content(ContentReportReason::SpamAbuse);
    let body = report_body(&target, &reason, "").unwrap();
    client(&server).report(&body).await.unwrap();
}

#[tokio::test]
async fn a_user_report_carries_its_message() {
    let server = MockServer::start().await;
    expect_report(
        &server,
        json!({
            "content": {
                "type": "User", "id": "u-ada", "report_reason": "Impersonation",
                "message_id": "m1"
            },
            "additional_context": "Pretends to be staff"
        }),
    )
    .await;

    let target = ReportTarget::User {
        id: "u-ada".into(),
        message: Some("m1".into()),
    };
    let reason = Reason::User(UserReportReason::Impersonation);
    let body = report_body(&target, &reason, " Pretends to be staff ").unwrap();
    client(&server).report(&body).await.unwrap();
}

#[tokio::test]
async fn a_user_report_without_a_message_leaves_it_out() {
    let server = MockServer::start().await;
    expect_report(
        &server,
        json!({
            "content": { "type": "User", "id": "u-ada", "report_reason": "NoneSpecified" },
            "additional_context": "Other reason"
        }),
    )
    .await;

    let target = ReportTarget::User {
        id: "u-ada".into(),
        message: None,
    };
    let reason = Reason::User(UserReportReason::NoneSpecified);
    let body = report_body(&target, &reason, "Other reason").unwrap();
    client(&server).report(&body).await.unwrap();
}

#[tokio::test]
async fn a_server_report_sends_its_id_and_reason() {
    let server = MockServer::start().await;
    expect_report(
        &server,
        json!({
            "content": { "type": "Server", "id": "srv", "report_reason": "ScamsFraud" },
            "additional_context": ""
        }),
    )
    .await;

    let target = ReportTarget::Server { id: "srv".into() };
    let reason = Reason::Content(ContentReportReason::ScamsFraud);
    let body = report_body(&target, &reason, "").unwrap();
    client(&server).report(&body).await.unwrap();
}

#[tokio::test]
async fn reporting_yourself_is_refused_in_words() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/safety/report"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "type": "CannotReportYourself" })),
        )
        .mount(&server)
        .await;

    let target = ReportTarget::Server { id: "srv".into() };
    let reason = Reason::Content(ContentReportReason::Raid);
    let err = client(&server)
        .report(&report_body(&target, &reason, "").unwrap())
        .await
        .unwrap_err();
    assert!(matches!(&err, ApiError::Api(_)), "{err:?}");
    assert!(matches!(
        err.error_type(),
        Some(ErrorType::CannotReportYourself)
    ));
    assert_eq!(report_error_text(&err), "You can't report yourself.");
}

fn store() -> ChatStore {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let frame = std::fs::read_to_string(path).unwrap();
    let Some(ServerEvent::Ready(ready)) = parse_frame(&frame).pop() else {
        panic!("ready fixture");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

#[test]
fn nobody_reports_themselves_their_messages_or_their_servers() {
    let store = store();
    let message = |author: &str| ReportTarget::Message {
        id: "m1".into(),
        author: author.into(),
    };
    let user = |id: &str| ReportTarget::User {
        id: id.into(),
        message: None,
    };
    let server = |id: &str| ReportTarget::Server { id: id.into() };
    assert!(can_report(&store, &message("u-ada")));
    assert!(!can_report(&store, &message("u-me")));
    assert!(can_report(&store, &user("u-ada")));
    assert!(!can_report(&store, &user("u-me")));
    // Alpha is Ada's; Beta is the user's own.
    assert!(can_report(&store, &server("srv-alpha")));
    assert!(!can_report(&store, &server("srv-beta")));
    assert!(!can_report(&store, &server("srv-unknown")));
}
