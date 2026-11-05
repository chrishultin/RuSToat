//! Opt-in checks against the live events service. No login needed: the
//! websocket handshake happens before authentication.
//! Run with `cargo test --test live_events -- --ignored`.

use stoat_gpui::api::StoatClient;
use stoat_gpui::api::events::events_url;

#[tokio::test]
#[ignore = "talks to the live Stoat events service"]
async fn live_events_handshake_succeeds() {
    let root = StoatClient::new(stoat_gpui::api::DEFAULT_API_URL)
        .root_config()
        .await
        .unwrap();
    let url = events_url(&root.ws);
    let (mut socket, response) = tokio_tungstenite::connect_async(&url)
        .await
        .unwrap_or_else(|e| panic!("handshake to {url} failed: {e}"));
    assert_eq!(response.status().as_u16(), 101);
    socket.close(None).await.ok();
}
