//! Stoat REST API access.
//!
//! Request and response types come straight from the Stoat backend's own
//! `revolt-models` crate (re-exported here as [`models`]), so they match the
//! server exactly. This module adds only the HTTP plumbing and client-side
//! error handling.

pub mod client;
pub mod error;
pub mod events;
pub mod gifbox;
pub mod instance;

pub use client::{
    DEFAULT_API_URL, DEFAULT_ATTACHMENT_SIZE_LIMIT, DEFAULT_AVATAR_SIZE_LIMIT, DEFAULT_BACKGROUND_SIZE_LIMIT,
    DEFAULT_BANNER_SIZE_LIMIT, DEFAULT_ICON_SIZE_LIMIT, DEFAULT_MAX_ATTACHMENTS, RootConfig, SESSION_FRIENDLY_NAME, StoatClient, file_url,
};
pub use error::ApiError;
pub use instance::{InstanceError, normalize_instance, resolve_instance};

/// Stoat's API types (`revolt_models::v0`): `User`, `Session`, `DataLogin`, …
pub use revolt_models::v0 as models;
