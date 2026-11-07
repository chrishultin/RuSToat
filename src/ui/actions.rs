//! The app-level actions: the menu bar's, and the shortcuts that work
//! wherever focus is. `main.rs` handles them; they're declared here so the
//! Keyboard Shortcuts page and its tests can name them.

use gpui_kit::*;

actions!(
    stoat,
    [
        /// Quit RuSToat (⌘Q).
        Quit,
        /// Log out of the account on screen.
        LogOut,
        /// Sign in to another account.
        AddAccount,
        /// Open the settings window, or bring it forward (⌘,).
        OpenSettings,
        /// Open the settings window on the Profile page ("Edit profile…").
        OpenProfileSettings
    ]
);

// ⌘1 to ⌘9: show the nth account in the switcher. Unit actions, since an
// action with a field needs GPUI's derive under the crate name `gpui`.
actions!(
    stoat,
    [
        SwitchAccount1,
        SwitchAccount2,
        SwitchAccount3,
        SwitchAccount4,
        SwitchAccount5,
        SwitchAccount6,
        SwitchAccount7,
        SwitchAccount8,
        SwitchAccount9
    ]
);

/// Each ⌘-digit's action, for the nth account (from 0).
pub fn switch_account_action(index: usize) -> Option<Box<dyn Action>> {
    Some(match index {
        0 => Box::new(SwitchAccount1),
        1 => Box::new(SwitchAccount2),
        2 => Box::new(SwitchAccount3),
        3 => Box::new(SwitchAccount4),
        4 => Box::new(SwitchAccount5),
        5 => Box::new(SwitchAccount6),
        6 => Box::new(SwitchAccount7),
        7 => Box::new(SwitchAccount8),
        8 => Box::new(SwitchAccount9),
        _ => return None,
    })
}

/// The app-level shortcuts, with no key context, so they work in every
/// window. No shortcut for Log Out: ⇧⌘Q is macOS's own "Log Out <user>".
pub fn app_key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-1", SwitchAccount1, None),
        KeyBinding::new("cmd-2", SwitchAccount2, None),
        KeyBinding::new("cmd-3", SwitchAccount3, None),
        KeyBinding::new("cmd-4", SwitchAccount4, None),
        KeyBinding::new("cmd-5", SwitchAccount5, None),
        KeyBinding::new("cmd-6", SwitchAccount6, None),
        KeyBinding::new("cmd-7", SwitchAccount7, None),
        KeyBinding::new("cmd-8", SwitchAccount8, None),
        KeyBinding::new("cmd-9", SwitchAccount9, None),
    ]
}
