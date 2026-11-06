//! The Dock icon's badge: how many things need the user's attention.
//!
//! AppKit's `NSDockTile` badge works for a plain `cargo run` binary too (no
//! app bundle needed), unlike system notifications. In the app bundle,
//! macOS shows it only if notification badges are allowed, which
//! [`crate::banners::request_permission`] asks for.

/// The badge text for `count`: nothing at 0, "99+" past 99.
pub fn badge_label(count: usize) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99+".to_string()),
    }
}

/// Show `count` on the Dock icon (or clear it at 0). Must be called on the
/// main thread, where GPUI runs UI code; anywhere else it does nothing.
#[cfg(target_os = "macos")]
pub fn set_badge(count: usize) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("dock badge: not on the main thread; skipping");
        return;
    };
    let label = badge_label(count).map(|label| NSString::from_str(&label));
    NSApplication::sharedApplication(mtm)
        .dockTile()
        .setBadgeLabel(label.as_deref());
}

#[cfg(not(target_os = "macos"))]
pub fn set_badge(_count: usize) {}

#[cfg(test)]
mod tests {
    use super::badge_label;

    #[test]
    fn labels() {
        assert_eq!(badge_label(0), None);
        assert_eq!(badge_label(1).as_deref(), Some("1"));
        assert_eq!(badge_label(99).as_deref(), Some("99"));
        assert_eq!(badge_label(100).as_deref(), Some("99+"));
    }
}
