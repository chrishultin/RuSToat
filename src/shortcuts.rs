//! The keyboard shortcuts the Settings window lists. The descriptions are
//! written here; the keys never are: the page reads them from the live
//! keymap, so the list can't drift from what the keys actually do. A test
//! checks that every action listed here is bound.

use gpui_kit::Action;

use crate::ui::actions::{OpenSettings, Quit, SwitchAccount1, SwitchAccount9};
use crate::ui::chat::{
    JumpToPresent, MarkServerRead, NextChannel, NextServer, NextUnread, PrevChannel, PrevServer,
    PrevUnread,
};

/// Where a shortcut is listed on the page, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Navigation,
    Reading,
    Accounts,
    App,
}

impl Group {
    pub const ALL: [Group; 4] = [
        Group::Navigation,
        Group::Reading,
        Group::Accounts,
        Group::App,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Navigation => "Navigation",
            Group::Reading => "Reading",
            Group::Accounts => "Accounts",
            Group::App => "App",
        }
    }
}

/// Builds an action, to look up its keys.
pub type MakeAction = fn() -> Box<dyn Action>;

/// The keys a row shows.
#[derive(Clone, Copy)]
pub enum Keys {
    /// One action's keys.
    One(MakeAction),
    /// A run of actions, shown as the first's keys to the last's (⌘1–⌘9).
    Range(MakeAction, MakeAction),
}

/// One row on the Keyboard Shortcuts page.
#[derive(Clone, Copy)]
pub struct Shortcut {
    pub group: Group,
    pub description: &'static str,
    pub keys: Keys,
}

impl Shortcut {
    /// The actions whose keys the row shows.
    pub fn actions(&self) -> Vec<Box<dyn Action>> {
        match self.keys {
            Keys::One(action) => vec![action()],
            Keys::Range(first, last) => vec![first(), last()],
        }
    }
}

const fn row(group: Group, description: &'static str, keys: Keys) -> Shortcut {
    Shortcut {
        group,
        description,
        keys,
    }
}

/// Every shortcut, grouped and in the page's order.
pub const SHORTCUTS: &[Shortcut] = &[
    row(
        Group::Navigation,
        "Previous channel",
        Keys::One(|| Box::new(PrevChannel)),
    ),
    row(
        Group::Navigation,
        "Next channel",
        Keys::One(|| Box::new(NextChannel)),
    ),
    row(
        Group::Navigation,
        "Previous unread channel",
        Keys::One(|| Box::new(PrevUnread)),
    ),
    row(
        Group::Navigation,
        "Next unread channel",
        Keys::One(|| Box::new(NextUnread)),
    ),
    row(
        Group::Navigation,
        "Previous server",
        Keys::One(|| Box::new(PrevServer)),
    ),
    row(
        Group::Navigation,
        "Next server",
        Keys::One(|| Box::new(NextServer)),
    ),
    row(
        Group::Reading,
        "Jump to the newest message and mark the channel read",
        Keys::One(|| Box::new(JumpToPresent)),
    ),
    row(
        Group::Reading,
        "Mark the server read",
        Keys::One(|| Box::new(MarkServerRead)),
    ),
    row(
        Group::Accounts,
        "Switch to the first to ninth account",
        Keys::Range(|| Box::new(SwitchAccount1), || Box::new(SwitchAccount9)),
    ),
    row(Group::App, "Settings", Keys::One(|| Box::new(OpenSettings))),
    row(Group::App, "Quit RuSToat", Keys::One(|| Box::new(Quit))),
];

/// The shortcuts in `group`, in order.
pub fn in_group(group: Group) -> impl Iterator<Item = &'static Shortcut> {
    SHORTCUTS.iter().filter(move |s| s.group == group)
}
