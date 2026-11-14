//! A fake Stoat for the controller tests: the REST API (wiremock) and the
//! events websocket (a local tokio-tungstenite server), plus a listener
//! that records what the controller tells Swift.

#![allow(dead_code)] // Each test file uses a different part.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use stoat_ffi::controller::{AccountController, Change, ConnectionStatus, ControllerListener};
use stoat_gpui::api::StoatClient;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const ME: &str = "u-me";
pub const TOKEN: &str = "test-token";

pub fn fixture_text(name: &str) -> String {
    let path = format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

pub fn fixture(name: &str) -> Value {
    serde_json::from_str(&fixture_text(name)).unwrap()
}

/// The events service: each connection gets `Authenticated`, then the
/// fixture's `Ready`, then whatever [`Events::send`] pushes.
pub struct Events {
    pub url: String,
    pub connections: Arc<AtomicUsize>,
    frames: broadcast::Sender<String>,
    drop_all: broadcast::Sender<()>,
}

impl Events {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicUsize::new(0));
        let (frames, _) = broadcast::channel::<String>(64);
        let (drop_all, _) = broadcast::channel::<()>(4);
        let ready = fixture_text("ready.json");
        tokio::spawn({
            let connections = connections.clone();
            let frames = frames.clone();
            let drop_all = drop_all.clone();
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    let mut frames = frames.subscribe();
                    let mut drop_rx = drop_all.subscribe();
                    let ready = ready.clone();
                    connections.fetch_add(1, Ordering::SeqCst);
                    tokio::spawn(async move {
                        // The first frame is `Authenticate`.
                        let Some(Ok(WsMessage::Text(auth))) = socket.next().await else {
                            return;
                        };
                        assert!(auth.contains(TOKEN), "{auth}");
                        let authenticated = json!({ "type": "Authenticated" }).to_string();
                        socket.send(WsMessage::Text(authenticated.into())).await.unwrap();
                        socket.send(WsMessage::Text(ready.into())).await.unwrap();
                        loop {
                            tokio::select! {
                                frame = frames.recv() => {
                                    let Ok(frame) = frame else { return };
                                    if socket.send(WsMessage::Text(frame.into())).await.is_err() {
                                        return;
                                    }
                                }
                                _ = drop_rx.recv() => return,
                                incoming = socket.next() => match incoming {
                                    Some(Ok(_)) => {}
                                    _ => return,
                                },
                            }
                        }
                    });
                }
            }
        });
        Self {
            url,
            connections,
            frames,
            drop_all,
        }
    }

    /// Send a frame to every open connection.
    pub fn send(&self, frame: Value) {
        let _ = self.frames.send(frame.to_string());
    }

    /// Close every open connection, as a dropped network would.
    pub fn drop_connections(&self) {
        let _ = self.drop_all.send(());
    }
}

/// The REST API, with `GET /` pointing at `events`.
pub async fn api(events: &Events) -> MockServer {
    let server = MockServer::start().await;
    let mut root = fixture("root.json");
    root["ws"] = json!(events.url);
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sync/settings/fetch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    server
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    Changed(Change),
    Status(ConnectionStatus),
}

/// Records what the controller tells Swift.
#[derive(Default, Clone)]
pub struct Recorder(Arc<Mutex<Vec<Heard>>>);

impl ControllerListener for Recorder {
    fn changed(&self, change: Change) {
        self.0.lock().unwrap().push(Heard::Changed(change));
    }

    fn status(&self, status: ConnectionStatus) {
        self.0.lock().unwrap().push(Heard::Status(status));
    }
}

impl Recorder {
    pub fn heard(&self) -> Vec<Heard> {
        self.0.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    /// Wait (up to 5 s) until something heard matches.
    pub async fn wait_for(&self, what: &str, pred: impl Fn(&Heard) -> bool) {
        for _ in 0..500 {
            if self.heard().iter().any(&pred) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("never heard {what}; heard {:#?}", self.heard());
    }

    pub async fn wait_for_status(&self, status: ConnectionStatus) {
        let what = format!("{status:?}");
        self.wait_for(&what, |h| *h == Heard::Status(status.clone()))
            .await;
    }
}

/// A connected controller, after its first `Ready`.
pub async fn connected(api: &MockServer) -> (Arc<AccountController>, Recorder) {
    let recorder = Recorder::default();
    let client = StoatClient::new(api.uri()).with_token(TOKEN);
    let controller = AccountController::for_client(client, ME.into(), Box::new(recorder.clone()));
    controller.connect();
    recorder.wait_for_status(ConnectionStatus::Connected).await;
    recorder
        .wait_for("Ready", |h| matches!(h, Heard::Changed(c) if c.ready))
        .await;
    (controller, recorder)
}
