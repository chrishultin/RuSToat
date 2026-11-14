//! RuSToat's core for the iPhone app.
//!
//! A thin layer over `stoat-gpui` (built without its `desktop` feature),
//! exported to Swift with UniFFI's proc-macros. Swift gets plain records and
//! a flat error; everything else stays in the shared library.

uniffi::setup_scaffolding!();

/// An error Swift can show as it is: `message` is user-facing text.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{message}")]
    Failed { message: String },
}

impl CoreError {
    pub fn failed(message: impl std::fmt::Display) -> Self {
        Self::Failed {
            message: message.to_string(),
        }
    }
}

/// This core's version, the `stoat-ffi` crate's.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Tidy up a typed server address, as the login screen's Server field does
/// (see `stoat_gpui::api::instance::normalize_instance`).
#[uniffi::export]
pub fn normalize_instance(input: String) -> Result<String, CoreError> {
    stoat_gpui::api::instance::normalize_instance(&input).map_err(CoreError::failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_the_crate() {
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains(&format!("version = \"{}\"", core_version())));
    }

    #[test]
    fn addresses_are_normalized_as_on_desktop() {
        assert_eq!(
            normalize_instance("stoat.example.com".into()).unwrap(),
            "https://stoat.example.com"
        );
        let err = normalize_instance("  ".into()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Enter the address of a Stoat server, like stoat.example.com."
        );
    }
}
