//! Proof-of-concept Stoat Chat client built with GPUI.
//!
//! The library holds everything testable (networking, state, views), and
//! `main.rs` only boots the app. Integration tests in `tests/` can import
//! from here.

pub mod api;
pub mod runtime;
pub mod session;
pub mod ui;
