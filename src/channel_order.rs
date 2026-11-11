//! The order of a server's channels and categories, and the categories a
//! move sends. Plain data with no GPUI, so it's unit-testable.
//!
//! Stoat only stores an order through a server's `categories`: the
//! channels no category lists keep the server's own order, which no
//! request changes. The web client gets around that with a category whose
//! ID is [`DEFAULT_CATEGORY`]: its channels, then every uncategorized one,
//! are the top section, which has no heading and always comes first. Any
//! reorder there writes it. RuSToat draws it the same way, and writes it
//! only when an order needs it (see [`Layout::into_categories`]).

use std::collections::HashSet;

use crate::api::models::{Category, Server};

/// The web client's category for the top section.
pub const DEFAULT_CATEGORY: &str = "default";

/// [`DEFAULT_CATEGORY`]'s title, as the web client writes it. It's never
/// shown.
pub const DEFAULT_TITLE: &str = "Default";

/// A server's channels in order, by section: every ID a category lists
/// (even channels the user can't see, or deleted ones), so a move keeps
/// them where they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The top section: [`DEFAULT_CATEGORY`]'s channels, then every
    /// channel no category lists, in the server's order.
    pub top: Vec<String>,
    /// The other categories, in order.
    pub categories: Vec<Category>,
}

/// `server`'s channels by section.
pub fn layout(server: &Server) -> Layout {
    let categories = server.categories.as_deref().unwrap_or_default();
    let listed: HashSet<&str> = categories
        .iter()
        .flat_map(|c| c.channels.iter().map(String::as_str))
        .collect();
    let mut top: Vec<String> = categories
        .iter()
        .filter(|c| c.id == DEFAULT_CATEGORY)
        .flat_map(|c| c.channels.iter().cloned())
        .collect();
    top.extend(
        server
            .channels
            .iter()
            .filter(|id| !listed.contains(id.as_str()))
            .cloned(),
    );
    Layout {
        top,
        categories: categories
            .iter()
            .filter(|c| c.id != DEFAULT_CATEGORY)
            .cloned()
            .collect(),
    }
}

impl Layout {
    /// The categories to send for this layout of `server`. The top section
    /// becomes [`DEFAULT_CATEGORY`], first, when `server` already has one,
    /// or when its order isn't the one Stoat shows without it (the
    /// server's own order of the channels left out of every category).
    /// Each channel is listed once.
    pub fn into_categories(self, server: &Server) -> Vec<Category> {
        let stored = server.categories.as_deref().unwrap_or_default();
        let existing = stored.iter().find(|c| c.id == DEFAULT_CATEGORY);
        let listed: HashSet<&str> = self
            .categories
            .iter()
            .flat_map(|c| c.channels.iter().map(String::as_str))
            .collect();
        let natural: Vec<&String> = server
            .channels
            .iter()
            .filter(|id| !listed.contains(id.as_str()))
            .collect();
        let on_server: Vec<&String> = self
            .top
            .iter()
            .filter(|id| server.channels.contains(id))
            .collect();
        let mut categories = Vec::with_capacity(self.categories.len() + 1);
        if existing.is_some() || on_server != natural {
            categories.push(Category {
                id: DEFAULT_CATEGORY.to_string(),
                title: existing.map_or_else(|| DEFAULT_TITLE.to_string(), |c| c.title.clone()),
                channels: self.top,
            });
        }
        categories.extend(self.categories);
        listed_once(&mut categories);
        categories
    }

    /// Take `channel` out of whichever section has it. Returns false if
    /// none does.
    fn take(&mut self, channel: &str) -> bool {
        let before = self.len();
        self.top.retain(|id| id != channel);
        for category in &mut self.categories {
            category.channels.retain(|id| id != channel);
        }
        self.len() != before
    }

    /// The channels of `section` (`None` is the top section), if it exists.
    fn section_mut(&mut self, section: Option<&str>) -> Option<&mut Vec<String>> {
        match section {
            None => Some(&mut self.top),
            Some(id) => self
                .categories
                .iter_mut()
                .find(|c| c.id == id)
                .map(|c| &mut c.channels),
        }
    }

    fn len(&self) -> usize {
        self.top.len()
            + self
                .categories
                .iter()
                .map(|c| c.channels.len())
                .sum::<usize>()
    }
}

/// The categories that put `channel` at the end of `section` (`None` is
/// the top section), with everything else where it was. `None` if nothing
/// would change, or there's no such channel or category.
pub fn move_to_end(server: &Server, channel: &str, section: Option<&str>) -> Option<Vec<Category>> {
    let before = layout(server);
    let mut after = before.clone();
    if !after.take(channel) {
        return None;
    }
    after.section_mut(section)?.push(channel.to_string());
    (after != before).then(|| after.into_categories(server))
}

/// Keep each channel where it's first listed, and drop any later copy:
/// Stoat refuses a list with a channel twice.
pub(crate) fn listed_once(categories: &mut [Category]) {
    let mut seen = HashSet::new();
    for c in categories {
        c.channels.retain(|id| seen.insert(id.clone()));
    }
}
