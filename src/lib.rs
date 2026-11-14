//! Proof-of-concept Stoat Chat client built with GPUI.
//!
//! The library holds everything testable (networking, state, views), and
//! `main.rs` only boots the app. Integration tests in `tests/` can import
//! from here.
//!
//! The `desktop` feature (on by default) adds the GPUI views, voice and the
//! macOS integrations. Without it, the networking, state and logic modules
//! build on their own, for the iOS app (`ios/`).

pub mod account_edit;
pub mod accounts;
pub mod api;
#[cfg(feature = "desktop")]
pub mod assets;
pub mod attachment;
#[cfg(feature = "desktop")]
pub mod banners;
pub mod bots;
pub mod channel_order;
pub mod channel_permissions;
pub mod channel_settings;
pub mod complete;
pub mod custom_emoji;
#[cfg(feature = "desktop")]
pub mod dock;
pub mod draft;
pub mod embed;
pub mod emoji;
pub mod file_actions;
pub mod friends;
pub mod groups;
#[cfg(feature = "desktop")]
pub mod http;
pub mod invites;
pub mod links;
pub mod live;
#[cfg(feature = "desktop")]
pub mod login_item;
pub mod markdown;
pub mod media_player;
pub mod math;
pub mod moderation;
pub mod nav;
pub mod media;
pub mod notify;
pub mod permission_matrix;
pub mod permissions;
pub mod places;
pub mod playback;
pub mod prefs;
pub mod profile;
pub mod profile_edit;
pub mod reports;
pub mod roles;
pub mod rail;
pub mod runtime;
pub mod safety;
pub mod search;
pub mod security;
pub mod server_edit;
pub mod server_settings;
pub mod session;
#[cfg(feature = "desktop")]
pub mod shortcuts;
pub mod slowmode;
#[cfg(feature = "desktop")]
pub mod sound;
pub mod status;
pub mod store;
#[cfg(feature = "desktop")]
pub mod themes;
pub mod time;
pub mod typing;
pub mod video_window;
#[cfg(feature = "desktop")]
pub mod ui;
#[cfg(feature = "desktop")]
pub mod voice;
pub mod webhooks;
#[cfg(feature = "desktop")]
pub mod window_hide;
