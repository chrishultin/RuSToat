//! Proof-of-concept Stoat Chat client built with GPUI.
//!
//! The library holds everything testable (networking, state, views), and
//! `main.rs` only boots the app. Integration tests in `tests/` can import
//! from here.

pub mod accounts;
pub mod api;
pub mod assets;
pub mod attachment;
pub mod banners;
pub mod complete;
pub mod dock;
pub mod draft;
pub mod embed;
pub mod emoji;
pub mod friends;
pub mod http;
pub mod links;
pub mod login_item;
pub mod markdown;
pub mod math;
pub mod nav;
pub mod media;
pub mod notify;
pub mod permissions;
pub mod places;
pub mod prefs;
pub mod profile;
pub mod runtime;
pub mod safety;
pub mod search;
pub mod session;
pub mod shortcuts;
pub mod slowmode;
pub mod store;
pub mod themes;
pub mod time;
pub mod typing;
pub mod ui;
pub mod window_hide;
