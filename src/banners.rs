//! macOS notification banners: whether they can be shown at all.
//!
//! GPUI posts them (`cx.show_system_notification`), but only when running
//! from an app bundle (`scripts/bundle.sh`). Outside one it logs `system
//! notifications disabled` and drops them, and macOS drops them too when the
//! user has denied permission. RuSToat checks both, so it can fall back to
//! in-app toasts instead of losing notifications.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once macOS says the user turned banners off for RuSToat.
static DENIED: AtomicBool = AtomicBool::new(false);

/// The app bundle's identifier, when running from one.
pub fn bundle_identifier() -> Option<&'static str> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(read_bundle_identifier).as_deref()
}

/// Whether banners can be shown now: running bundled, and not denied.
/// Until macOS has answered the permission request this is optimistic.
pub fn available() -> bool {
    bundle_identifier().is_some() && !DENIED.load(Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
fn read_bundle_identifier() -> Option<String> {
    objc2_foundation::NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|id| id.to_string())
}

#[cfg(not(target_os = "macos"))]
fn read_bundle_identifier() -> Option<String> {
    None
}

/// Ask macOS for permission to show banners. The first time, macOS prompts
/// the user; after that it answers from their saved choice without asking.
/// The answer is logged and remembered for [`available`]. Does nothing when
/// not bundled, where the notification center isn't usable at all.
///
/// GPUI would ask on the first banner, but that banner is lost while the
/// prompt is up, so ask before any arrive.
#[cfg(target_os = "macos")]
pub fn request_permission() {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::NSError;
    use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};

    let Some(id) = bundle_identifier() else {
        log::info!("banners: not running from an app bundle; using in-app toasts");
        return;
    };
    // Runs on an arbitrary thread; only touches the atomic and the log.
    let completion = RcBlock::new(|granted: Bool, error: *mut NSError| {
        // SAFETY: when non-null, `error` is a valid `NSError` for the
        // duration of the callback.
        if let Some(error) = unsafe { error.as_ref() } {
            log::warn!(
                "banners: permission request failed ({}); using in-app toasts",
                error.localizedDescription()
            );
            DENIED.store(true, Ordering::Relaxed);
        } else if granted.as_bool() {
            log::info!("banners: allowed");
            DENIED.store(false, Ordering::Relaxed);
        } else {
            log::info!(
                "banners: turned off in System Settings > Notifications; using in-app toasts"
            );
            DENIED.store(true, Ordering::Relaxed);
        }
    });
    log::debug!("banners: bundled as {id}; asking for permission");
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &completion,
        );
}

#[cfg(not(target_os = "macos"))]
pub fn request_permission() {}
