//! The Friends page, in the main pane while Home's Friends row is picked:
//! a tab bar (Online, All, Pending, Blocked) over the tab's lists. What's in
//! the lists is [`crate::friends`]'s; this only draws them.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::models::RelationshipStatus;
use crate::friends::{FriendLists, FriendRow, FriendsTab};
use crate::ui::chat::{MemberColors, People, avatar, mention_badge};

/// Height of every row of the page's list (titles too, so it can be a
/// `uniform_list`).
const ROW_HEIGHT: f32 = 48.;

/// Size of a person's avatar.
const AVATAR: f32 = 36.;

/// One row of the page's list.
enum PageRow {
    /// "INCOMING — 2"
    Title {
        title: &'static str,
        count: usize,
    },
    Person(FriendRow),
    /// Under a title with nobody in its list.
    Nobody,
}

/// What a tab shows, as rows: each list's title, then its people (or a
/// note that there are none).
fn page_rows(lists: &FriendLists, tab: FriendsTab) -> Vec<PageRow> {
    let mut rows = Vec::new();
    for (title, people) in lists.sections(tab) {
        rows.push(PageRow::Title {
            title,
            count: people.len(),
        });
        if people.is_empty() {
            rows.push(PageRow::Nobody);
        }
        rows.extend(people.iter().cloned().map(PageRow::Person));
    }
    rows
}

/// The Friends page. `incoming` is the badge on the Pending tab; `on_tab`
/// is called with the tab clicked.
pub(crate) fn render_friends_page(
    lists: &FriendLists,
    tab: FriendsTab,
    incoming: usize,
    colors: MemberColors,
    on_tab: impl Fn(FriendsTab, &mut Window, &mut App) + 'static,
    people: People,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let header = h_flex()
        .h(px(48.))
        .flex_none()
        .px_4()
        .gap_2()
        .border_b_1()
        .border_color(theme.border)
        .font_weight(FontWeight::SEMIBOLD)
        .child(Icon::new(IconName::Users).text_color(theme.muted_foreground))
        .child("Friends");
    let tabs = TabBar::new("friends-tabs")
        .underline()
        .selected_index(FriendsTab::ALL.iter().position(|t| *t == tab).unwrap_or(0))
        .children(FriendsTab::ALL.map(|t| {
            Tab::new()
                .label(t.label())
                .when(t == FriendsTab::Pending && incoming > 0, |tab| {
                    tab.suffix(mention_badge(incoming, cx))
                })
        }))
        .on_click(move |ix, window, cx| {
            if let Some(tab) = FriendsTab::ALL.get(*ix) {
                on_tab(*tab, window, cx)
            }
        });
    let rows = Rc::new(page_rows(lists, tab));
    let count = rows.len();
    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(header)
        .child(div().px_4().flex_none().child(tabs))
        .child(
            div().flex_1().min_h_0().px_2().child(
                uniform_list(
                    SharedString::from(format!("friends-{tab:?}")),
                    count,
                    move |range, _, _| {
                        range
                            .map(|ix| render_page_row(&rows[ix], colors, &people))
                            .collect::<Vec<_>>()
                    },
                )
                .size_full(),
            ),
        )
        .into_any_element()
}

fn render_page_row(row: &PageRow, colors: MemberColors, people: &People) -> AnyElement {
    match row {
        PageRow::Title { title, count } => h_flex()
            .h(px(ROW_HEIGHT))
            .px_2()
            .pb_1()
            .items_end()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(colors.muted)
            .child(format!("{} — {count}", title.to_uppercase()))
            .into_any_element(),
        PageRow::Nobody => h_flex()
            .h(px(ROW_HEIGHT))
            .px_2()
            .text_sm()
            .text_color(colors.muted)
            .child("Nobody here right now!")
            .into_any_element(),
        PageRow::Person(person) => render_person(person, colors, people),
    }
}

/// A person: their avatar (with a presence dot, for friends), name and
/// status. Clicking opens their card, right-clicking their menu.
fn render_person(person: &FriendRow, colors: MemberColors, people: &People) -> AnyElement {
    let friend = person.relationship == RelationshipStatus::Friend;
    let (open, user) = (people.clone(), person.user_id.clone());
    h_flex()
        .id(SharedString::from(format!("friend-{}", person.user_id)))
        .h(px(ROW_HEIGHT))
        .px_2()
        .gap_3()
        .items_center()
        .rounded_md()
        .cursor_pointer()
        .hover(|el| el.bg(colors.hover))
        .on_click(move |_, window, cx| open.open_card(user.clone(), window, cx))
        .child(
            div()
                .relative()
                .size(px(AVATAR))
                .flex_none()
                .child(avatar(
                    format!("friend-avatar-{}", person.user_id),
                    Some(person.avatar_url.clone()),
                    &person.name,
                    AVATAR,
                    colors.avatar_bg,
                    colors.avatar_fg,
                ))
                .when(friend, |el| {
                    el.child(
                        div()
                            .absolute()
                            .right(px(-2.))
                            .bottom(px(-2.))
                            .size(px(12.))
                            .rounded_full()
                            .border_2()
                            .border_color(colors.ring)
                            .bg(colors.presence(person.presence)),
                    )
                }),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .truncate()
                        .text_color(colors.foreground)
                        .child(person.name.clone()),
                )
                // Only friends' presence is meaningful (as the dot).
                .when(friend && !person.status.is_empty(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(colors.muted)
                            .child(person.status.clone()),
                    )
                }),
        )
        .context_menu(people.menu(&person.user_id, true))
        .into_any_element()
}
