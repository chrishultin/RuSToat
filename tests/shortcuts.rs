//! The Keyboard Shortcuts page lists what the keys actually do: every row's
//! action is bound by the same functions the app binds its keys with.

use std::collections::HashSet;

use gpui_kit::{AsKeystroke, Keymap, Keystroke};
use stoat_gpui::shortcuts::{Group, SHORTCUTS, in_group};
use stoat_gpui::ui::{actions, chat};

/// The keymap the app builds at launch, less gpui-component's own bindings.
fn app_keymap() -> Keymap {
    let mut keymap = Keymap::default();
    keymap.add_bindings(chat::key_bindings());
    keymap.add_bindings(actions::app_key_bindings());
    keymap
}

#[test]
fn every_listed_shortcut_is_bound() {
    let keymap = app_keymap();
    for shortcut in SHORTCUTS {
        for action in shortcut.actions() {
            assert!(
                keymap.bindings_for_action(&*action).next().is_some(),
                "{:?} ({}) has no key",
                action.name(),
                shortcut.description
            );
        }
    }
}

#[test]
fn descriptions_are_unique() {
    let mut seen = HashSet::new();
    for shortcut in SHORTCUTS {
        assert!(
            seen.insert(shortcut.description),
            "{} is listed twice",
            shortcut.description
        );
    }
}

#[test]
fn every_group_has_shortcuts_and_every_shortcut_a_group() {
    for group in Group::ALL {
        assert!(in_group(group).next().is_some(), "{group:?} is empty");
    }
    let listed: usize = Group::ALL.iter().map(|g| in_group(*g).count()).sum();
    assert_eq!(listed, SHORTCUTS.len());
}

#[test]
fn the_account_range_runs_from_cmd_1_to_cmd_9() {
    let keymap = app_keymap();
    let keys = |action: &dyn gpui_kit::Action| {
        let binding = keymap.bindings_for_action(action).next_back().unwrap();
        binding.keystrokes()[0].as_keystroke().clone()
    };
    let key = |source| Keystroke::parse(source).unwrap();
    let accounts = in_group(Group::Accounts).next().unwrap();
    let range = accounts.actions();
    assert_eq!(keys(&*range[0]), key("cmd-1"));
    assert_eq!(keys(&*range[1]), key("cmd-9"));
    // ⌘, opens Settings.
    let settings = &actions::OpenSettings;
    assert_eq!(keys(settings), key("cmd-,"));
}
