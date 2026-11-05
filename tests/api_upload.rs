//! Attachments against a mock file server (Autumn) and API: the multipart
//! upload, its progress, refusals, the limits from `GET /`, and sending the
//! uploaded IDs.

use std::sync::{Arc, Mutex};

use serde_json::json;
use stoat_gpui::api::{ApiError, RootConfig, StoatClient};
use stoat_gpui::draft::AttachmentLimits;
use wiremock::matchers::{body_json, header, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "rustoat-1793700000000-0-0123456789abcdef";

fn client(api: &str) -> StoatClient {
    StoatClient::new(api).with_token("tok")
}

fn fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[tokio::test]
async fn uploading_posts_the_file_as_multipart_and_returns_its_id() {
    let autumn = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/attachments"))
        .and(header("x-session-token", "tok"))
        .and(header_regex("content-type", "^multipart/form-data; boundary="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "f-cat" })))
        .expect(1)
        .mount(&autumn)
        .await;

    let bytes: Vec<u8> = (0..200_000u32).map(|n| n as u8).collect();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let id = client("http://unused.invalid")
        .upload_attachment(&format!("{}/", autumn.uri()), "cat.png", "image/png", bytes.clone(), {
            let progress = progress.clone();
            move |sent, total| progress.lock().unwrap().push((sent, total))
        })
        .await
        .unwrap();
    assert_eq!(id, "f-cat");

    // Progress rises to the whole file, in more than one step.
    let progress = progress.lock().unwrap().clone();
    assert!(progress.len() > 1, "{progress:?}");
    assert!(progress.windows(2).all(|w| w[0].0 < w[1].0));
    assert_eq!(progress.last(), Some(&(200_000, 200_000)));

    // One part, `file`, with the name, type and exact bytes.
    let request = &autumn.received_requests().await.unwrap()[0];
    let body = &request.body;
    let head = String::from_utf8_lossy(&body[..body.len().min(400)]).to_string();
    assert!(
        head.contains(r#"Content-Disposition: form-data; name="file"; filename="cat.png""#),
        "{head}"
    );
    assert!(head.contains("Content-Type: image/png"), "{head}");
    let start = body.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(&body[start..start + bytes.len()], bytes.as_slice());
}

#[tokio::test]
async fn a_refused_upload_is_an_api_error() {
    let autumn = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/attachments"))
        .respond_with(ResponseTemplate::new(413).set_body_string("Payload Too Large"))
        .mount(&autumn)
        .await;
    let err = client("http://unused.invalid")
        .upload_attachment(&autumn.uri(), "big.bin", "application/octet-stream", vec![0; 10], |_, _| {})
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Unexpected { status: 413, .. }), "{err:?}");

    let autumn = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/attachments"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "type": "FileTypeNotAllowed" })),
        )
        .mount(&autumn)
        .await;
    let err = client("http://unused.invalid")
        .upload_attachment(&autumn.uri(), "x.exe", "application/octet-stream", vec![0; 10], |_, _| {})
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Api(_)), "{err:?}");
}

#[tokio::test]
async fn an_unreachable_file_server_is_a_network_error() {
    let err = client("http://unused.invalid")
        .upload_attachment("http://127.0.0.1:1", "a.txt", "text/plain", vec![1], |_, _| {})
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Network(_)), "{err:?}");
}

#[test]
fn root_config_gives_the_attachment_limits() {
    let root: RootConfig = serde_json::from_value(fixture("root.json")).unwrap();
    assert_eq!(root.attachment_size_limit(), 20_000_000);
    assert_eq!(root.max_attachments(), 5);

    // The stricter of the two account limits.
    let mut value = fixture("root.json");
    value["features"]["limits"]["new_user"]["file_upload_size_limits"]["attachments"] =
        json!(8_000_000);
    value["features"]["limits"]["default"]["message_attachments"] = json!(10);
    let root: RootConfig = serde_json::from_value(value).unwrap();
    assert_eq!(
        AttachmentLimits::from_root(&root),
        AttachmentLimits {
            max_bytes: 8_000_000,
            max_count: 5
        }
    );
}

#[test]
fn root_config_without_limits_falls_back_to_stoats_defaults() {
    let mut value = fixture("root.json");
    value["features"].as_object_mut().unwrap().remove("limits");
    let root: RootConfig = serde_json::from_value(value).unwrap();
    assert_eq!(AttachmentLimits::from_root(&root), AttachmentLimits::default());

    let mut value = fixture("root.json");
    value["features"]["limits"] = json!({ "default": {}, "new_user": { "bots": 2 } });
    let root: RootConfig = serde_json::from_value(value).unwrap();
    assert_eq!(AttachmentLimits::from_root(&root), AttachmentLimits::default());
}

#[tokio::test]
async fn sending_files_without_text_leaves_content_out() {
    let api = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .and(header("Idempotency-Key", KEY))
        .and(body_json(json!({ "attachments": ["f-cat", "f-pdf"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "01JC0000000000000000000009", "nonce": KEY, "channel": "ch-general",
            "author": "u-me"
        })))
        .expect(1)
        .mount(&api)
        .await;
    let ids = ["f-cat".to_string(), "f-pdf".to_string()];
    let message = client(&api.uri())
        .send_message("ch-general", "", &ids, &[], KEY)
        .await
        .unwrap();
    assert_eq!(message.nonce.as_deref(), Some(KEY));
}

#[tokio::test]
async fn sending_files_with_a_caption_sends_both() {
    let api = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .and(body_json(json!({ "content": "look", "attachments": ["f-cat"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "_id": "01JC0000000000000000000009", "nonce": KEY, "channel": "ch-general",
            "author": "u-me", "content": "look"
        })))
        .expect(1)
        .mount(&api)
        .await;
    client(&api.uri())
        .send_message("ch-general", "look", &["f-cat".to_string()], &[], KEY)
        .await
        .unwrap();
}

#[tokio::test]
async fn too_many_attachments_is_refused_with_the_max() {
    let api = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/channels/ch-general/messages"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({ "type": "TooManyAttachments", "max": 5 })),
        )
        .mount(&api)
        .await;
    let ids: Vec<String> = (0..6).map(|n| format!("f{n}")).collect();
    let err = client(&api.uri())
        .send_message("ch-general", "", &ids, &[], KEY)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err.error_type(),
            Some(revolt_result::ErrorType::TooManyAttachments { max: 5 })
        ),
        "{err:?}"
    );
}
