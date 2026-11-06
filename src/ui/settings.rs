//! The settings window: a sidebar of sections, each a [`SettingPage`].
//!
//! A section is a function returning its page ([`PageFn`]), so a later
//! section is one more function in the list the view is created with.

use gpui_kit::assets::IconName;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::setting::{SettingGroup, SettingItem, SettingPage, Settings};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;

use crate::shortcuts::{self, Group, Keys, Shortcut};
use crate::ui::AppRoot;

/// Builds one section of the window, each time the window renders.
pub type PageFn = fn(&SettingsView, &mut App) -> SettingPage;

/// The settings window's content.
pub struct SettingsView {
    /// The main window's content, for sections about the account on screen.
    root: WeakEntity<AppRoot>,
    pages: Vec<PageFn>,
}

impl SettingsView {
    pub fn new(root: WeakEntity<AppRoot>, pages: Vec<PageFn>) -> Self {
        Self { root, pages }
    }

    /// The app's own sections, in sidebar order.
    pub fn app_pages() -> Vec<PageFn> {
        vec![shortcuts_page]
    }

    /// The main window's content, if it's still open.
    pub fn root(&self) -> Option<Entity<AppRoot>> {
        self.root.upgrade()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pages: Vec<SettingPage> = self.pages.iter().map(|page| page(self, cx)).collect();
        div()
            .size_full()
            .bg(cx.theme().background)
            .child(Settings::new("settings").pages(pages))
    }
}

/// Keyboard Shortcuts: every shortcut, with its keys read from the keymap.
fn shortcuts_page(_: &SettingsView, _: &mut App) -> SettingPage {
    SettingPage::new("Keyboard Shortcuts")
        .icon(IconName::Keyboard)
        .resettable(false)
        .groups(Group::ALL.into_iter().map(|group| {
            SettingGroup::new()
                .title(group.label())
                .items(shortcuts::in_group(group).map(|shortcut| {
                    let shortcut = *shortcut;
                    // Custom rows only match a search by their keywords.
                    SettingItem::render(move |_, _, cx| render_shortcut(&shortcut, cx))
                        .keywords([shortcut.description, group.label()])
                }))
        }))
}

fn render_shortcut(shortcut: &Shortcut, cx: &App) -> impl IntoElement + use<> {
    let keys = match shortcut.keys {
        Keys::One(action) => keystroke_for(&*action(), cx).map(|key| vec![Kbd::new(key)]),
        Keys::Range(first, last) => keystroke_for(&*first(), cx)
            .zip(keystroke_for(&*last(), cx))
            .map(|(first, last)| vec![Kbd::new(first), Kbd::new(last)]),
    };
    let muted = cx.theme().muted_foreground;
    h_flex()
        .w_full()
        .justify_between()
        .gap_3()
        .child(div().text_sm().child(shortcut.description))
        .child(match keys {
            Some(keys) => h_flex()
                .gap_1()
                .text_color(muted)
                .children(with_dashes(keys))
                .into_any_element(),
            None => div()
                .text_sm()
                .text_color(muted)
                .child("Not bound")
                .into_any_element(),
        })
}

/// One key, or a range of two with a dash between.
fn with_dashes(keys: Vec<Kbd>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for (ix, key) in keys.into_iter().enumerate() {
        if ix > 0 {
            out.push("–".into_any_element());
        }
        out.push(key.into_any_element());
    }
    out
}

/// The keystroke that triggers `action`: the binding added last, which is
/// the one that wins.
fn keystroke_for(action: &dyn Action, cx: &App) -> Option<Keystroke> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).next_back()?;
    Some(binding.keystrokes().first()?.as_keystroke().clone())
}
