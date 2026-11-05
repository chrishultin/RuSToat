//! The emoji pickers. The reaction picker: a grid of emoji in a popover. The common Unicode
//! emoji come first, then custom emoji (see [`ChatStore::reaction_choices`]),
//! or only the ones a message allows. Emoji the user has already reacted
//! with are highlighted, and picking one again takes it back. The message
//! box's picker ([`EmojiBrowser`]) has every emoji, with search, and the
//! default skin tone beside the search field.
//!
//! [`ChatStore::reaction_choices`]: crate::store::ChatStore::reaction_choices

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};

use crate::emoji::{
    CustomEmoji, EmojiChoice, PickerRow, RecentEmoji, picker_rows, picker_sections, toned,
};
use crate::prefs::Tone;
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
    let muted = cx.theme().muted_foreground;
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
                custom_face(format!("{id}-pick-{ix}-img"), url),
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
        emoji_cell(
            format!("{id}-pick-{ix}"),
            face,
            tooltip,
            chosen.contains(&choice.key),
            cx,
        )
        .on_click(move |_, window, cx| on_pick(&key, window, cx))
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

/// One emoji's square, as both pickers draw it, highlighted if `chosen`.
/// Callers add what clicking it does.
fn emoji_cell(
    id: String,
    face: AnyElement,
    tooltip: SharedString,
    chosen: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (hover, selected) = (theme.secondary, theme.primary.opacity(0.2));
    div()
        .id(SharedString::from(id))
        .size(px(CELL))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .when(chosen, |el| el.bg(selected))
        .hover(move |style| style.bg(hover))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(face)
}

/// A custom emoji's picture in its square, pulsing while it loads.
fn custom_face(id: String, url: &str) -> AnyElement {
    img(SharedUri::from(url.to_string()))
        .id(SharedString::from(id))
        .size(px(CUSTOM_EMOJI_PX))
        .object_fit(ObjectFit::Contain)
        .with_loading(|| {
            Skeleton::new()
                .size(px(CUSTOM_EMOJI_PX))
                .rounded_sm()
                .into_any_element()
        })
        .into_any_element()
}

/// What picking in the emoji browser does: the emoji, and whether Shift
/// was held (to keep the browser open).
pub type OnBrowsePick = Rc<dyn Fn(&EmojiChoice, bool, &mut Window, &mut App)>;

/// What choosing a skin tone in the emoji browser does.
pub type OnTone = Rc<dyn Fn(Tone, &mut Window, &mut App)>;

/// A skin tone swatch's square, beside the search field.
const SWATCH: f32 = 22.;
/// The hand the swatches show, in each tone.
const SWATCH_EMOJI: &str = "✋";

/// The browser's list is this tall; it scrolls.
const BROWSER_HEIGHT: f32 = 9. * CELL;

/// The message box's emoji picker: a search field over a virtualized list
/// of every emoji (recently used, each category, then custom emoji), in
/// rows of [`COLUMNS`] with a header row per section.
pub struct EmojiBrowser {
    search: Entity<InputState>,
    recent: RecentEmoji,
    custom: Vec<CustomEmoji>,
    rows: Rc<Vec<PickerRow>>,
    scroll: UniformListScrollHandle,
    /// The default skin tone: emoji are drawn in it, and its swatch is lit.
    tone: Tone,
    on_pick: OnBrowsePick,
    on_tone: OnTone,
    _search: Subscription,
}

impl EmojiBrowser {
    /// The browser, its search field focused.
    pub fn new(
        recent: RecentEmoji,
        custom: Vec<CustomEmoji>,
        tone: Tone,
        on_pick: OnBrowsePick,
        on_tone: OnTone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search emoji"));
        let subscription = cx.subscribe_in(
            &search,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.rebuild(cx);
                    this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                    cx.notify();
                }
                // Enter picks the first result.
                InputEvent::PressEnter { .. } => {
                    let first = this.rows.iter().find_map(|row| match row {
                        PickerRow::Emoji(emoji) => emoji.first().cloned(),
                        PickerRow::Header(_) => None,
                    });
                    if let Some(first) = first {
                        (this.on_pick)(&first, false, window, cx);
                    }
                }
                _ => {}
            },
        );
        search.update(cx, |search, cx| search.focus(window, cx));
        let mut browser = Self {
            search,
            recent,
            custom,
            rows: Rc::default(),
            scroll: UniformListScrollHandle::new(),
            tone,
            on_pick,
            on_tone,
            _search: subscription,
        };
        browser.rebuild(cx);
        browser
    }

    /// Show emoji in `tone` from now on.
    pub fn set_tone(&mut self, tone: Tone, cx: &mut Context<Self>) {
        if self.tone != tone {
            self.tone = tone;
            cx.notify();
        }
    }

    fn rebuild(&mut self, cx: &App) {
        let query = self.search.read(cx).value();
        let sections = picker_sections(&query, &self.recent, &self.custom);
        self.rows = Rc::new(picker_rows(sections, COLUMNS));
    }
}

impl Render for EmojiBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (rows, on_pick, tone) = (self.rows.clone(), self.on_pick.clone(), self.tone);
        let muted = cx.theme().muted_foreground;
        let list = if rows.is_empty() {
            div()
                .h(px(BROWSER_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child("No emoji found")
                .into_any_element()
        } else {
            uniform_list("emoji-browser", rows.len(), move |range, _, cx| {
                range
                    .map(|ix| browser_row(ix, &rows[ix], tone, &on_pick, cx))
                    .collect()
            })
            .h(px(BROWSER_HEIGHT))
            .track_scroll(&self.scroll)
            .into_any_element()
        };
        v_flex()
            .w(px(COLUMNS as f32 * CELL))
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.search)
                                .small()
                                .cleanable(true)
                                .prefix(Icon::new(IconName::Search).small().text_color(muted)),
                        ),
                    )
                    .child(tone_swatches(self.tone, &self.on_tone, cx)),
            )
            .child(list)
    }
}

/// A ✋ in each skin tone, the chosen one highlighted. Clicking one makes
/// it the default.
fn tone_swatches(chosen: Tone, on_tone: &OnTone, cx: &App) -> Div {
    let theme = cx.theme();
    let (hover, selected, ring) = (
        theme.secondary,
        theme.primary.opacity(0.2),
        theme.primary,
    );
    h_flex().children(Tone::ALL.into_iter().map(|tone| {
        let on_tone = on_tone.clone();
        let label: SharedString = tone.label().into();
        div()
            .id(SharedString::from(format!("skin-tone-{tone:?}")))
            .size(px(SWATCH))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .when(tone == chosen, |el| el.bg(selected).border_1().border_color(ring))
            .hover(move |style| style.bg(hover))
            .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            .on_click(move |_, window, cx| on_tone(tone, window, cx))
            .child(toned(SWATCH_EMOJI, tone))
    }))
}

/// Row `ix` of the browser: a section header, or a row of emoji.
fn browser_row(
    ix: usize,
    row: &PickerRow,
    tone: Tone,
    on_pick: &OnBrowsePick,
    cx: &App,
) -> AnyElement {
    match row {
        PickerRow::Header(title) => div()
            .h(px(CELL))
            .px_1()
            .flex()
            .items_end()
            .pb_1()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(cx.theme().muted_foreground)
            .child(title.clone())
            .into_any_element(),
        PickerRow::Emoji(emoji) => div()
            .h(px(CELL))
            .flex()
            .children(emoji.iter().enumerate().map(|(col, choice)| {
                let id = format!("browse-{ix}-{col}");
                let (face, tooltip): (AnyElement, SharedString) = match choice {
                    EmojiChoice::Unicode { emoji, shortcode } => (
                        div().text_xl().child(toned(emoji, tone)).into_any_element(),
                        format!(":{shortcode}:").into(),
                    ),
                    EmojiChoice::Custom(custom) => (
                        match &custom.url {
                            Some(url) => custom_face(format!("{id}-img"), url),
                            None => div().text_xs().truncate().child(custom.name.clone()).into_any_element(),
                        },
                        format!(":{}:", custom.name).into(),
                    ),
                };
                let (choice, on_pick) = (choice.clone(), on_pick.clone());
                emoji_cell(id, face, tooltip, false, cx).on_click(move |event, window, cx| {
                    on_pick(&choice, event.modifiers().shift, window, cx)
                })
            }))
            .into_any_element(),
    }
}
