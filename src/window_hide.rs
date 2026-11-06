//! Hiding one window without closing it, for "Keep running when the window
//! is closed". GPUI can hide the whole app (`cx.hide()`, as ⌘H does) but not
//! a single window, so this reaches the window's `NSWindow` through its raw
//! handle and orders it out. `window.activate_window()` brings it back.

use gpui_kit::Window;

/// Hide `window`. `false` if it couldn't be reached, so the caller can fall
/// back to hiding the app.
#[cfg(target_os = "macos")]
pub fn hide(window: &Window) -> bool {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = match HasWindowHandle::window_handle(window) {
        Ok(handle) => handle.as_raw(),
        Err(err) => {
            log::warn!("hiding the window: no handle: {err}");
            return false;
        }
    };
    let RawWindowHandle::AppKit(handle) = handle else {
        log::warn!("hiding the window: not an AppKit window");
        return false;
    };
    // SAFETY: GPUI's AppKit handle points at the window's content view, which
    // lives as long as `window` does.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(ns_window) = view.window() else {
        log::warn!("hiding the window: its view has no window");
        return false;
    };
    ns_window.orderOut(None);
    true
}

#[cfg(not(target_os = "macos"))]
pub fn hide(_window: &Window) -> bool {
    false
}
