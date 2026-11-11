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

    /// The section listing `channel`, and where in it.
    fn find_mut(&mut self, channel: &str) -> Option<(&mut Vec<String>, usize)> {
        let sections = std::iter::once(&mut self.top)
            .chain(self.categories.iter_mut().map(|c| &mut c.channels));
        for list in sections {
            if let Some(at) = list.iter().position(|id| id == channel) {
                return Some((list, at));
            }
        }
        None
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
    move_channel(
        server,
        channel,
        &ChannelDrop::EndOf(section.map(str::to_string)),
    )
}

/// Where a dragged channel would land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelDrop {
    /// Just before this channel, in its section.
    Before(String),
    /// Just after this channel, in its section.
    After(String),
    /// Last in this section (`None` is the top section).
    EndOf(Option<String>),
    /// First in this category.
    StartOf(String),
}

/// What a dragged channel is over in the channel list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelHover<'a> {
    /// A channel's row.
    Channel(&'a str),
    /// A category's heading. `above` is the section shown before it
    /// (`None` is the top section, even when it isn't shown).
    Heading {
        category: &'a str,
        above: Option<&'a str>,
    },
}

/// Where a channel lands over `hovered`, `fraction` of the way down it:
/// over a row, the top half is before that channel and the bottom half
/// after it; over a heading, the top half is the end of the section above
/// and the bottom half the start of that category. Whether that changes
/// anything is [`move_channel`]'s to say.
pub fn channel_target(hovered: ChannelHover, fraction: f32) -> ChannelDrop {
    let top = fraction < 0.5;
    match hovered {
        ChannelHover::Channel(id) if top => ChannelDrop::Before(id.to_string()),
        ChannelHover::Channel(id) => ChannelDrop::After(id.to_string()),
        ChannelHover::Heading { above, .. } if top => ChannelDrop::EndOf(above.map(str::to_string)),
        ChannelHover::Heading { category, .. } => ChannelDrop::StartOf(category.to_string()),
    }
}

/// The categories that put `channel` at `target`, with everything else
/// where it was: a drop beside a channel is placed by that channel's ID,
/// so channels the user can't see around it don't move. `None` if nothing
/// would change, or the channel, its neighbour or the category is unknown.
pub fn move_channel(server: &Server, channel: &str, target: &ChannelDrop) -> Option<Vec<Category>> {
    let before = layout(server);
    let mut after = before.clone();
    if !after.take(channel) {
        return None;
    }
    match target {
        ChannelDrop::Before(id) | ChannelDrop::After(id) => {
            let (list, at) = after.find_mut(id)?;
            let at = if matches!(target, ChannelDrop::After(_)) {
                at + 1
            } else {
                at
            };
            list.insert(at, channel.to_string());
        }
        ChannelDrop::EndOf(section) => after
            .section_mut(section.as_deref())?
            .push(channel.to_string()),
        ChannelDrop::StartOf(category) => after
            .section_mut(Some(category))?
            .insert(0, channel.to_string()),
    }
    (after != before).then(|| after.into_categories(server))
}

/// Where a dragged category would land: just before or after another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryDrop {
    Before(String),
    After(String),
}

/// Where the category `dragged` lands when it's over `hovered`'s section
/// (`None` is the top section), `fraction` of the way down it: the top
/// half drops before that category and the bottom half after it. Over the
/// top section, which always comes first, it goes before the first
/// category. `shown` is the categories shown, in order. `None` when that's
/// where it is already.
pub fn category_target(
    shown: &[String],
    dragged: &str,
    hovered: Option<&str>,
    fraction: f32,
) -> Option<CategoryDrop> {
    let target = match hovered {
        None => CategoryDrop::Before(shown.first()?.clone()),
        Some(id) if id == dragged => return None,
        Some(id) if fraction < 0.5 => CategoryDrop::Before(id.to_string()),
        Some(id) => CategoryDrop::After(id.to_string()),
    };
    let moved = reordered(shown, dragged, &target)?;
    (moved != shown).then_some(target)
}

/// `order` with `dragged` moved to `target`. `None` if either is missing,
/// or the target is `dragged` itself.
fn reordered(order: &[String], dragged: &str, target: &CategoryDrop) -> Option<Vec<String>> {
    let anchor = match target {
        CategoryDrop::Before(id) | CategoryDrop::After(id) => id,
    };
    if anchor == dragged || !order.iter().any(|id| id == dragged) {
        return None;
    }
    let mut moved: Vec<String> = order.iter().filter(|id| *id != dragged).cloned().collect();
    let at = moved.iter().position(|id| id == anchor)?;
    let at = match target {
        CategoryDrop::Before(_) => at,
        CategoryDrop::After(_) => at + 1,
    };
    moved.insert(at, dragged.to_string());
    Some(moved)
}

/// The categories that put category `dragged` at `target`, with every
/// channel where it was. The top section stays first (and isn't written
/// just for this). `None` if nothing would change, or either category is
/// unknown.
pub fn move_category(
    server: &Server,
    dragged: &str,
    target: &CategoryDrop,
) -> Option<Vec<Category>> {
    let before = layout(server);
    let order: Vec<String> = before.categories.iter().map(|c| c.id.clone()).collect();
    let moved = reordered(&order, dragged, target)?;
    if moved == order {
        return None;
    }
    let mut after = before;
    after.categories = moved
        .iter()
        .filter_map(|id| after.categories.iter().find(|c| &c.id == id).cloned())
        .collect();
    Some(after.into_categories(server))
}

/// Keep each channel where it's first listed, and drop any later copy:
/// Stoat refuses a list with a channel twice.
pub(crate) fn listed_once(categories: &mut [Category]) {
    let mut seen = HashSet::new();
    for c in categories {
        c.channels.retain(|id| seen.insert(id.clone()));
    }
}
