//! Opening RuSToat at login, as one of macOS's own login items
//! (`SMAppService.mainAppService`, macOS 13 and later). macOS keeps the
//! setting, so RuSToat doesn't: Settings reads [`status`] each time it shows
//! the switch.
//!
//! It registers the running app bundle, so it needs one: under a plain
//! `cargo run` it's [`LoginItem::Unavailable`].

/// Whether RuSToat opens at login.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginItem {
    On,
    Off,
    /// Registered, but the user has to allow it in System Settings ›
    /// General › Login Items first.
    NeedsApproval,
    /// Can't be changed from here, for the reason given.
    Unavailable(Unavailable),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// Not running from the app bundle.
    NotBundled,
    /// Before macOS 13.
    OldMacOs,
    /// macOS reported a status this version doesn't know.
    UnknownStatus,
}

impl Unavailable {
    /// Shown under the greyed-out switch.
    pub fn note(self) -> &'static str {
        match self {
            Unavailable::NotBundled => "Only available in the RuSToat app bundle.",
            Unavailable::OldMacOs => "Needs macOS 13 or later.",
            Unavailable::UnknownStatus => "macOS didn't say whether RuSToat opens at login.",
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use anyhow::anyhow;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    use super::{LoginItem, Unavailable};

    /// The state macOS reports, as a [`LoginItem`].
    pub fn from_status(status: SMAppServiceStatus) -> LoginItem {
        match status {
            SMAppServiceStatus::Enabled => LoginItem::On,
            SMAppServiceStatus::RequiresApproval => LoginItem::NeedsApproval,
            // NotFound is what macOS reports for an app that has never been
            // registered; registering it works.
            SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound => LoginItem::Off,
            _ => LoginItem::Unavailable(Unavailable::UnknownStatus),
        }
    }

    /// Why it can't be used here, if it can't.
    fn unavailable() -> Option<Unavailable> {
        if crate::banners::bundle_identifier().is_none() {
            Some(Unavailable::NotBundled)
        } else if !objc2::available!(macos = 13.0) {
            Some(Unavailable::OldMacOs)
        } else {
            None
        }
    }

    pub fn status() -> LoginItem {
        if let Some(reason) = unavailable() {
            return LoginItem::Unavailable(reason);
        }
        // SAFETY: macOS 13 or later, checked above.
        from_status(unsafe { SMAppService::mainAppService().status() })
    }

    pub fn set(on: bool) -> anyhow::Result<()> {
        if let Some(reason) = unavailable() {
            return Err(anyhow!("{}", reason.note()));
        }
        // SAFETY: macOS 13 or later, checked above.
        let result = unsafe {
            let service = SMAppService::mainAppService();
            if on {
                service.registerAndReturnError()
            } else {
                service.unregisterAndReturnError()
            }
        };
        result.map_err(|err| anyhow!("{}", err.localizedDescription()))
    }

    pub fn open_system_settings() {
        if unavailable().is_none() {
            // SAFETY: macOS 13 or later, checked above.
            unsafe { SMAppService::openSystemSettingsLoginItems() };
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_status_maps() {
            assert_eq!(from_status(SMAppServiceStatus::Enabled), LoginItem::On);
            assert_eq!(
                from_status(SMAppServiceStatus::NotRegistered),
                LoginItem::Off
            );
            assert_eq!(from_status(SMAppServiceStatus::NotFound), LoginItem::Off);
            assert_eq!(
                from_status(SMAppServiceStatus::RequiresApproval),
                LoginItem::NeedsApproval
            );
            assert_eq!(
                from_status(SMAppServiceStatus(42)),
                LoginItem::Unavailable(Unavailable::UnknownStatus)
            );
        }

        /// `cargo test` doesn't run from the app bundle.
        #[test]
        fn unavailable_outside_the_bundle() {
            assert_eq!(status(), LoginItem::Unavailable(Unavailable::NotBundled));
            assert!(set(true).is_err());
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::{from_status, open_system_settings, set, status};

#[cfg(not(target_os = "macos"))]
pub fn status() -> LoginItem {
    LoginItem::Unavailable(Unavailable::NotBundled)
}

#[cfg(not(target_os = "macos"))]
pub fn set(_on: bool) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("{}", Unavailable::NotBundled.note()))
}

#[cfg(not(target_os = "macos"))]
pub fn open_system_settings() {}
