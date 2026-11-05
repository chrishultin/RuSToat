//! Opt-in checks that the live media endpoints behave as `MediaUrls` assumes.
//! Run with `cargo test --test live_media -- --ignored`.

use stoat_gpui::api::{DEFAULT_API_URL, StoatClient};
use stoat_gpui::media::MediaUrls;

async fn media() -> MediaUrls {
    let root = StoatClient::new(DEFAULT_API_URL)
        .root_config()
        .await
        .unwrap();
    MediaUrls::from_root(DEFAULT_API_URL, &root)
}

async fn assert_image(url: &str) {
    let response = reqwest::get(url).await.unwrap();
    let kind = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        response.status().is_success(),
        "{url}: {}",
        response.status()
    );
    assert!(kind.starts_with("image/"), "{url}: content-type {kind}");
}

#[tokio::test]
#[ignore = "talks to the live Stoat services"]
async fn default_avatar_needs_no_auth() {
    // Generated from the ID, so any well-formed ID works.
    assert_image(&media().await.default_avatar("01JC0000000000000000000000")).await;
}

#[tokio::test]
#[ignore = "talks to the live Stoat services"]
async fn proxy_serves_external_images() {
    let url = media()
        .await
        .external("https://www.rust-lang.org/logos/rust-logo-256x256.png")
        .expect("live config has a proxy");
    assert_image(&url).await;
}
