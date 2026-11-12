//! Downloading files against a mock file server: the body, the cap, and
//! failures.

use stoat_gpui::api::{ApiError, StoatClient};
use stoat_gpui::file_actions::download_failure;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client() -> StoatClient {
    StoatClient::new("http://unused.invalid").with_token("tok")
}

#[tokio::test]
async fn downloading_returns_the_body_without_the_session_token() {
    let autumn = MockServer::start().await;
    let body: Vec<u8> = (0..300_000u32).map(|n| n as u8).collect();
    Mock::given(method("GET"))
        .and(path("/attachments/f1"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .expect(1)
        .mount(&autumn)
        .await;

    let bytes = client()
        .download(&format!("{}/attachments/f1", autumn.uri()), None)
        .await
        .unwrap();
    assert_eq!(bytes, body);

    // The file server and proxy are public: the token stays with the API.
    let requests: Vec<Request> = autumn.received_requests().await.unwrap();
    assert!(requests[0].headers.get("x-session-token").is_none());
}

#[tokio::test]
async fn a_capped_download_stops_at_the_cap() {
    let autumn = MockServer::start().await;
    let body: Vec<u8> = (0..120_000u32).map(|n| n as u8).collect();
    Mock::given(method("GET"))
        .and(path("/attachments/log"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&autumn)
        .await;
    let url = format!("{}/attachments/log", autumn.uri());

    let bytes = client().download(&url, Some(50_000)).await.unwrap();
    assert_eq!(bytes, body[..50_000]);
    // A cap above the size gives it all.
    let bytes = client().download(&url, Some(500_000)).await.unwrap();
    assert_eq!(bytes, body);
}

#[tokio::test]
async fn a_missing_file_reads_plainly() {
    let autumn = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&autumn)
        .await;

    let err = client()
        .download(&format!("{}/attachments/gone", autumn.uri()), None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApiError::Unexpected { status: 404, .. }),
        "{err:?}"
    );
    assert_eq!(download_failure(&err), "it's no longer there");
}

#[tokio::test]
async fn an_unreachable_server_reads_plainly() {
    let err = client()
        .download("http://127.0.0.1:9/attachments/f1", None)
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Network(_)), "{err:?}");
    assert_eq!(download_failure(&err), "couldn't reach the server");
}
