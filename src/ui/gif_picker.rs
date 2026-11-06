//! The message box's GIF picker ([`GifBrowser`]), backed by Gifbox: a
//! search field over category tiles ("Trending" first), or over a query's
//! results. Tiles show stills; only the hovered one plays, since every GIF
//! drawn keeps all its frames in memory. Picking one hands its page URL to
//! [`OnGifPick`].

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::ApiError;
use crate::api::gifbox::{GIF_LIMIT, GifCategory, GifResult, GifboxClient, category_poster};
use crate::runtime::spawn_io;

/// What picking a GIF does, given its page URL (what's sent).
pub type OnGifPick = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// The picker's width (as Stoat's web client's).
const WIDTH: f32 = 400.;
/// The tiles' area is this tall; it scrolls.
const BODY_HEIGHT: f32 = 340.;
/// Space between tiles.
const GAP: f32 = 8.;
/// Two tiles a row.
const TILE_WIDTH: f32 = (WIDTH - GAP) / 2.;
/// A category tile's height.
const CATEGORY_HEIGHT: f32 = 72.;
/// A result tile's height.
const RESULT_HEIGHT: f32 = 120.;
/// Typing settles for this long before searching (Gifbox rate-limits).
const DEBOUNCE: Duration = Duration::from_millis(250);

/// Categories and trending GIFs, kept for the session so reopening the
/// picker doesn't fetch them again. One per account.
#[derive(Default)]
pub struct GifCache {
    categories: Option<Rc<Vec<GifCategory>>>,
    trending: Option<Rc<Vec<GifResult>>>,
}

/// What the picker is showing the results of.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GifQuery {
    Trending,
    /// Lower-cased, as Stoat's web client sends it.
    Search(String),
}

enum Load<T> {
    Loading,
    Ready(T),
    Failed(SharedString),
}

pub struct GifBrowser {
    client: GifboxClient,
    cache: Rc<RefCell<GifCache>>,
    search: Entity<InputState>,
    /// The search text as last handled (trimmed, lower-cased), so a change
    /// the picker made itself isn't searched again.
    typed: String,
    /// `None`: the categories page.
    query: Option<GifQuery>,
    categories: Load<Rc<Vec<GifCategory>>>,
    results: Load<Rc<Vec<GifResult>>>,
    /// The result under the mouse, which plays.
    hovered: Option<usize>,
    /// Counts result requests, so an answer for an old one is dropped.
    seq: u64,
    request: Option<Task<()>>,
    categories_request: Option<Task<()>>,
    /// The search waiting for typing to settle.
    debounce: Option<Task<()>>,
    on_pick: OnGifPick,
    _search: Subscription,
}

impl GifBrowser {
    /// The picker on its categories page, its search field focused.
    pub fn new(
        client: GifboxClient,
        cache: Rc<RefCell<GifCache>>,
        on_pick: OnGifPick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search Gifbox"));
        let subscription = cx.subscribe_in(
            &search,
            window,
            |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::Change => this.on_search_change(cx),
                // Enter searches without waiting.
                InputEvent::PressEnter { .. }
                    if !this.typed.is_empty() && this.debounce.is_some() =>
                {
                    this.debounce = None;
                    this.run(GifQuery::Search(this.typed.clone()), cx);
                }
                _ => {}
            },
        );
        search.update(cx, |search, cx| search.focus(window, cx));
        let mut browser = Self {
            client,
            cache,
            search,
            typed: String::new(),
            query: None,
            categories: Load::Loading,
            results: Load::Loading,
            hovered: None,
            seq: 0,
            request: None,
            categories_request: None,
            debounce: None,
            on_pick,
            _search: subscription,
        };
        browser.load_categories(cx);
        browser
    }

    /// The categories, from the cache or else Gifbox.
    fn load_categories(&mut self, cx: &mut Context<Self>) {
        if let Some(categories) = self.cache.borrow().categories.clone() {
            self.categories = Load::Ready(categories);
            return;
        }
        self.categories = Load::Loading;
        let client = self.client.clone();
        let request = spawn_io(async move { client.categories().await });
        self.categories_request = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.categories = match result {
                    Ok(Ok(categories)) => {
                        let categories = Rc::new(categories);
                        this.cache.borrow_mut().categories = Some(categories.clone());
                        Load::Ready(categories)
                    }
                    Ok(Err(err)) => {
                        log::warn!("Gifbox categories failed: {err:?}");
                        Load::Failed(gif_error_text(&err))
                    }
                    Err(err) => {
                        log::warn!("Gifbox categories task failed: {err:?}");
                        Load::Failed("Couldn't load GIFs.".into())
                    }
                };
                cx.notify();
            })
            .ok();
        }));
    }

    /// The search text changed: back to the categories when it's empty,
    /// otherwise search for it once typing settles.
    fn on_search_change(&mut self, cx: &mut Context<Self>) {
        let typed = self.search.read(cx).value().trim().to_lowercase();
        if typed == self.typed {
            return;
        }
        self.typed = typed.clone();
        if typed.is_empty() {
            self.debounce = None;
            self.show_categories(cx);
            return;
        }
        self.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            this.update(cx, |this, cx| {
                this.debounce = None;
                this.run(GifQuery::Search(typed), cx);
            })
            .ok();
        }));
    }

    fn show_categories(&mut self, cx: &mut Context<Self>) {
        self.query = None;
        self.request = None;
        self.seq += 1;
        self.hovered = None;
        if matches!(self.categories, Load::Failed(_)) {
            self.load_categories(cx);
        }
        cx.notify();
    }

    /// Show `query`'s results: trending from the cache if it's there,
    /// otherwise asked for. An answer lands only if nothing replaced it.
    fn run(&mut self, query: GifQuery, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        self.hovered = None;
        self.query = Some(query.clone());
        if query == GifQuery::Trending
            && let Some(trending) = self.cache.borrow().trending.clone()
        {
            self.results = Load::Ready(trending);
            self.request = None;
            cx.notify();
            return;
        }
        self.results = Load::Loading;
        let client = self.client.clone();
        let request = spawn_io({
            let query = query.clone();
            async move {
                match &query {
                    GifQuery::Trending => client.trending(GIF_LIMIT).await,
                    GifQuery::Search(text) => client.search(text, GIF_LIMIT).await,
                }
            }
        });
        self.request = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if this.seq != seq {
                    return;
                }
                this.results = match result {
                    Ok(Ok(page)) => {
                        let results = Rc::new(page.results);
                        if query == GifQuery::Trending {
                            this.cache.borrow_mut().trending = Some(results.clone());
                        }
                        Load::Ready(results)
                    }
                    Ok(Err(err)) => {
                        log::warn!("Gifbox {query:?} failed: {err:?}");
                        Load::Failed(gif_error_text(&err))
                    }
                    Err(err) => {
                        log::warn!("Gifbox task failed: {err:?}");
                        Load::Failed("Couldn't load GIFs.".into())
                    }
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// A category tile: search for its title (shown in the field).
    fn pick_category(&mut self, title: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.typed = title.trim().to_lowercase();
        self.debounce = None;
        self.search.update(cx, |search, cx| {
            search.set_value(title.to_string(), window, cx)
        });
        self.run(GifQuery::Search(self.typed.clone()), cx);
    }

    /// Back to the categories, clearing the search.
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.typed.clear();
        self.debounce = None;
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.show_categories(cx);
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        match self.query.clone() {
            Some(query) => self.run(query, cx),
            None => {
                self.load_categories(cx);
                cx.notify();
            }
        }
    }

    fn set_hovered(&mut self, ix: usize, hovered: bool, cx: &mut Context<Self>) {
        let now = match (hovered, self.hovered) {
            (true, _) => Some(ix),
            (false, Some(current)) if current == ix => None,
            (false, current) => current,
        };
        if now != self.hovered {
            self.hovered = now;
            cx.notify();
        }
    }
}

/// What a failed Gifbox request says.
fn gif_error_text(err: &ApiError) -> SharedString {
    match err {
        ApiError::RateLimited { .. } => "Too many searches, try again in a moment.".into(),
        ApiError::Network(_) => "Couldn't reach Gifbox.".into(),
        _ => "Couldn't load GIFs.".into(),
    }
}

impl Render for GifBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let header = h_flex()
            .gap_1()
            .when(self.query.is_some(), |el| {
                el.child(
                    Button::new("gif-back")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowLeft)
                        .tooltip("Back to categories")
                        .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))),
                )
            })
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search)
                        .small()
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).small().text_color(muted)),
                ),
            );
        let body = match &self.query {
            None => match &self.categories {
                // Trending is there even while the categories load.
                Load::Ready(categories) => self.render_categories(categories, cx),
                Load::Loading => self.render_categories(&Rc::default(), cx),
                Load::Failed(error) => self.render_note(error.clone(), true, cx),
            },
            Some(_) => match &self.results {
                Load::Loading => self.render_note("Loading GIFs…".into(), false, cx),
                Load::Failed(error) => self.render_note(error.clone(), true, cx),
                Load::Ready(results) if results.is_empty() => {
                    self.render_note("No GIFs found".into(), false, cx)
                }
                Load::Ready(results) => self.render_results(results.clone(), cx),
            },
        };
        v_flex().w(px(WIDTH)).gap_2().child(header).child(
            div()
                .id("gif-body")
                .h(px(BODY_HEIGHT))
                .overflow_y_scroll()
                .child(body),
        )
    }
}

impl GifBrowser {
    /// A message in the middle of the body, with Retry for an error.
    fn render_note(&self, text: SharedString, retry: bool, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .h(px(BODY_HEIGHT))
            .gap_2()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text)
            .when(retry, |el| {
                el.child(
                    Button::new("gif-retry")
                        .xsmall()
                        .label("Retry")
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                )
            })
            .into_any_element()
    }

    /// "Trending", then a tile per category: its still, its title over it.
    fn render_categories(
        &self,
        categories: &Rc<Vec<GifCategory>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (accent, accent_foreground, muted_bg) =
            (theme.primary, theme.primary_foreground, theme.muted);
        let trending = div()
            .id("gif-category-trending")
            .w(px(TILE_WIDTH))
            .h(px(CATEGORY_HEIGHT))
            .rounded_md()
            .overflow_hidden()
            .cursor_pointer()
            .bg(accent)
            .text_color(accent_foreground)
            .flex()
            .items_center()
            .justify_center()
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|el| el.opacity(0.85))
            .on_click(cx.listener(|this, _, _, cx| {
                this.debounce = None;
                this.run(GifQuery::Trending, cx);
            }))
            .child("Trending");
        let tiles = categories.iter().enumerate().map(|(ix, category)| {
            let title = category.title.clone();
            div()
                .id(("gif-category", ix))
                .relative()
                .w(px(TILE_WIDTH))
                .h(px(CATEGORY_HEIGHT))
                .rounded_md()
                .overflow_hidden()
                .cursor_pointer()
                .bg(muted_bg)
                .hover(|el| el.opacity(0.85))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.pick_category(&title, window, cx)),
                )
                .child(
                    img(SharedUri::from(category_poster(&category.image)))
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(black().opacity(0.45))
                        .text_color(white())
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(category.title.clone()),
                )
        });
        div()
            .flex()
            .flex_wrap()
            .gap(px(GAP))
            .child(trending)
            .children(tiles)
            .into_any_element()
    }

    /// A tile per GIF: its still, or its animation while hovered.
    fn render_results(&self, results: Rc<Vec<GifResult>>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted_bg, muted) = (theme.muted, theme.muted_foreground);
        let tiles = results.iter().enumerate().map(|(ix, gif)| {
            let hovered = self.hovered == Some(ix);
            let shown = hovered
                .then(|| gif.animated())
                .flatten()
                .or_else(|| gif.still());
            let face = match shown {
                Some(media) => img(SharedUri::from(media.url.clone()))
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                // No still to show: its ID, so it can still be picked.
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_2()
                    .text_xs()
                    .text_color(muted)
                    .truncate()
                    .child(gif.id.clone())
                    .into_any_element(),
            };
            let (url, on_pick) = (gif.url.clone(), self.on_pick.clone());
            div()
                .id(("gif", ix))
                .w(px(TILE_WIDTH))
                .h(px(RESULT_HEIGHT))
                .rounded_md()
                .overflow_hidden()
                .cursor_pointer()
                .bg(muted_bg)
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.set_hovered(ix, *hovered, cx)
                }))
                .on_click(move |_, window, cx| on_pick(&url, window, cx))
                .child(face)
        });
        div()
            .flex()
            .flex_wrap()
            .gap(px(GAP))
            .children(tiles)
            .into_any_element()
    }
}
