//! RuSToat's core for the iPhone app.
//!
//! A thin layer over `stoat-gpui` (built without its `desktop` feature),
//! exported to Swift with UniFFI's proc-macros. Swift gets plain records and
//! a flat error; everything else stays in the shared library.

uniffi::setup_scaffolding!();

pub mod controller;
pub mod session;

/// An error Swift can show as it is: `message` is user-facing text.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{message}")]
    Failed { message: String },
}

/// A Swift callback (such as the Keychain vault) threw something it didn't declare.
impl From<uniffi::UnexpectedUniFFICallbackError> for CoreError {
    fn from(err: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::failed(err.reason)
    }
}

impl CoreError {
    pub fn failed(message: impl std::fmt::Display) -> Self {
        Self::Failed {
            message: message.to_string(),
        }
    }
}

/// Send the core's logs to stderr, which Xcode's console shows. `RUST_LOG`
/// (set in the scheme) overrides the default of `info`. Safe to call twice.
#[uniffi::export]
pub fn init_logging() {
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("stoat_gpui=info,stoat_ffi=info"),
    )
    .try_init();
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
