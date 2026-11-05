//! The reaction picker: a grid of emoji in a popover. The common Unicode
//! emoji come first, then custom emoji (see [`ChatStore::reaction_choices`]),
//! or only the ones a message allows. Emoji the user has already reacted
//! with are highlighted, and picking one again takes it back.
//!
//! [`ChatStore::reaction_choices`]: crate::store::ChatStore::reaction_choices

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::store::{PickerEmoji, ReactionEmoji};

/// Emoji per row.
const COLUMNS: usize = 8;
/// One emoji's square.
const CELL: f32 = 34.;
/// Custom emoji inside their square.
const CUSTOM_EMOJI_PX: f32 = 24.;
/// Taller than this, the grid scrolls (a server can have a lot of emoji).
const MAX_HEIGHT: f32 = 6. * CELL;

/// What picking an emoji does, given its key.
pub type OnPick = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// The grid. `id` is unique per message; `note` is shown above it; `chosen`
/// are the keys the user has already reacted with.
pub fn emoji_grid(
    id: &str,
    choices: &[PickerEmoji],
    note: Option<&'static str>,
    chosen: &HashSet<String>,
    on_pick: OnPick,
    cx: &App,
) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let (hover, selected, muted) = (
        theme.secondary,
        theme.primary.opacity(0.2),
        theme.muted_foreground,
    );
    let cells = choices.iter().enumerate().map(|(ix, choice)| {
        let key = choice.key.clone();
        let on_pick = on_pick.clone();
        let (face, tooltip): (AnyElement, SharedString) = match &choice.emoji {
            ReactionEmoji::Unicode(emoji) => (
                div().text_xl().child(emoji.clone()).into_any_element(),
                emoji.clone().into(),
            ),
            ReactionEmoji::Custom {
                url: Some(url),
                name,
                ..
            } => (
                img(SharedUri::from(url.clone()))
                    .id(SharedString::from(format!("{id}-pick-{ix}-img")))
                    .size(px(CUSTOM_EMOJI_PX))
                    .object_fit(ObjectFit::Contain)
                    .with_loading(|| {
                        Skeleton::new()
                            .size(px(CUSTOM_EMOJI_PX))
                            .rounded_sm()
                            .into_any_element()
                    })
                    .into_any_element(),
                name.clone().into(),
            ),
            ReactionEmoji::Custom {
                url: None, name, ..
            } => (
                div()
                    .text_xs()
                    .text_color(muted)
                    .truncate()
                    .child(name.clone())
                    .into_any_element(),
                name.clone().into(),
            ),
        };
        div()
            .id(SharedString::from(format!("{id}-pick-{ix}")))
            .size(px(CELL))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .when(chosen.contains(&choice.key), |el| el.bg(selected))
            .hover(move |style| style.bg(hover))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_click(move |_, window, cx| on_pick(&key, window, cx))
            .child(face)
    });
    let grid = div()
        .id(SharedString::from(format!("{id}-picker")))
        .w(px(COLUMNS as f32 * CELL))
        .max_h(px(MAX_HEIGHT))
        .overflow_y_scroll()
        .flex()
        .flex_wrap()
        .children(cells);
    div()
        .flex()
        .flex_col()
        .gap_2()
        .when_some(note, |el, note| {
            el.child(
                div()
                    .w(px(COLUMNS as f32 * CELL))
                    .text_xs()
                    .text_color(muted)
                    .child(note),
            )
        })
        .child(grid)
}
