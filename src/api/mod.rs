//! Stoat REST API access.
//!
//! Request and response types come straight from the Stoat backend's own
//! `revolt-models` crate (re-exported here as [`models`]), so they match the
//! server exactly. This module adds only the HTTP plumbing and client-side
//! error handling.

pub mod client;
pub mod error;
pub mod events;

pub use client::{DEFAULT_API_URL, RootConfig, SESSION_FRIENDLY_NAME, StoatClient, file_url};
pub use error::ApiError;

/// Stoat's API types (`revolt_models::v0`): `User`, `Session`, `DataLogin`, …
pub use revolt_models::v0 as models;
