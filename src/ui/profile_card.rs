//! The profile card: a person's banner, avatar and presence, names,
//! pronouns, status, roles, mutual friends, servers and groups, join dates
//! and bio, in a small floating card, and the full lists of mutuals. What it
//! shows comes from [`crate::profile::profile_card`], the fetched profile
//! and [`crate::profile::mutuals`]; `ChatView` decides where it goes and
//! what its buttons do.

use std::rc::Rc;
use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::profile::{MutualFriend, MutualKind, MutualPlace, Mutuals, ProfileCard, overflow_text};
use crate::store::PresenceKind;
use crate::ui::chat::{avatar, hex_colour};
use crate::ui::markup::{MarkupPlugin, code_block_actions};
use crate::ui::math::MathPlugin;

/// Width of the card.
pub const CARD_WIDTH: f32 = 340.;
/// Tallest the card gets; a long bio scrolls inside it.
const CARD_MAX_HEIGHT: f32 = 560.;
const BANNER_HEIGHT: f32 = 110.;
/// The strip at the top when there's no banner.
const NO_BANNER_HEIGHT: f32 = 36.;
const CARD_AVATAR: f32 = 48.;
/// A mutuals row shows this many icons, then "+N".
const MUTUALS_SHOWN: usize = 8;
const MUTUAL_ICON: f32 = 24.;

/// The list of mutuals' width.
pub const MUTUALS_DIALOG_WIDTH: f32 = 360.;
/// Tallest the list gets before it scrolls.
const MUTUALS_LIST_HEIGHT: f32 = 400.;
const MUTUALS_LIST_ICON: f32 = 32.;

/// Everything the card draws.
pub struct CardContent {
    pub card: ProfileCard,
    pub banner_url: Option<String>,
    /// The bio as prepared Markdown, with the key its parse is cached by.
    pub bio: Option<CardBio>,
    /// What the user shares with them; empty rows aren't shown.
    pub mutuals: Mutuals,
}

pub struct CardBio {
    pub render_key: String,
    pub markdown: String,
}

/// What a link click in the bio does.
pub type OnLink = Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync>;

/// One of the card's two rows of mutuals, and the list it opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutualList {
    Friends,
    Places,
}

impl MutualList {
    /// The row's heading and the list's title.
    pub fn title(self) -> &'static str {
        match self {
            Self::Friends => "Mutual friends",
            Self::Places => "Mutual servers and groups",
        }
    }
}

/// What clicking a row of mutuals does: open its list.
pub type OnMutuals = Rc<dyn Fn(MutualList, &mut Window, &mut App)>;

/// What clicking someone named on the card does (a bot's owner): open
/// their card.
pub type OnUser = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// Something chosen from a list of mutuals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutualPick {
    /// A friend: their card.
    Friend(String),
    Server(String),
    Group(String),
}

/// What choosing from a list of mutuals does.
pub type OnMutualPick = Rc<dyn Fn(MutualPick, &mut Window, &mut App)>;

/// The card itself. `actions` go in a row under the names (chunks of the
/// card that act on the person: Message, friend requests, the menu).
/// Clicking a row of mutuals calls `on_mutuals`; clicking a bot's owner
/// calls `on_user`.
pub fn render_profile_card(
    content: &CardContent,
    actions: Vec<AnyElement>,
    on_link: OnLink,
    on_mutuals: OnMutuals,
    on_user: OnUser,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let card = &content.card;
    let muted = theme.muted_foreground;
    let presence = match card.presence {
        PresenceKind::Online => theme.success,
        PresenceKind::Idle => theme.warning,
        PresenceKind::Focus => theme.info,
        PresenceKind::Busy => theme.danger,
        PresenceKind::Offline => muted,
    };

    let banner = match &content.banner_url {
        Some(url) => img(SharedUri::from(url.clone()))
            .id("profile-card-banner")
            .w_full()
            .h(px(BANNER_HEIGHT))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .w_full()
            .h(px(NO_BANNER_HEIGHT))
            .bg(theme.secondary)
            .into_any_element(),
    };

    let tag = card.username_tag.clone();
    let names = v_flex()
        .min_w_0()
        .flex_1()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .truncate()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(card.name.clone()),
                )
                .when(card.bot, |el| {
                    el.child(
                        div()
                            .flex_none()
                            .px_1()
                            .rounded_sm()
                            .bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Bot"),
                    )
                }),
        )
        .child(
            div()
                .id("profile-card-username")
                .truncate()
                .text_sm()
                .text_color(muted)
                .cursor_pointer()
                .hover(|el| el.text_color(theme.foreground))
                .tooltip(|window, cx| Tooltip::new("Copy username").build(window, cx))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(tag.clone()));
                })
                .child(card.username_tag.clone()),
        )
        .children(card.owner.as_ref().map(|owner| {
            let line = div()
                .id("profile-card-owner")
                .truncate()
                .text_xs()
                .text_color(muted)
                .child(format!("Owned by {}", owner.name));
            if !owner.known {
                return line;
            }
            let id = owner.id.clone();
            line.cursor_pointer()
                .hover(|el| el.text_color(theme.foreground).underline())
                .on_click(move |_, window, cx| on_user(id.clone(), window, cx))
        }));

    let header = h_flex()
        .px_4()
        .pt_3()
        .gap_3()
        .items_center()
        .child(
            div()
                .relative()
                .size(px(CARD_AVATAR))
                .flex_none()
                .child(avatar(
                    format!("profile-card-avatar-{}", card.user_id),
                    Some(card.avatar_url.clone()),
                    &card.name,
                    CARD_AVATAR,
                    theme.secondary,
                    theme.secondary_foreground,
                ))
                .child(
                    div()
                        .absolute()
                        .right(px(-2.))
                        .bottom(px(-2.))
                        .size(px(14.))
                        .rounded_full()
                        .border_2()
                        .border_color(theme.popover)
                        .bg(presence),
                ),
        )
        .child(names);

    let section = |title: &'static str| {
        v_flex().gap_1().child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child(title.to_uppercase()),
        )
    };

    let mut sections: Vec<AnyElement> = Vec::new();
    if let Some(until) = card.timed_out_until {
        let until = crate::time::format_until(
            until.with_timezone(&Local).naive_local(),
            Local::now().naive_local(),
        );
        sections.push(
            h_flex()
                .gap_2()
                .items_center()
                .text_sm()
                .text_color(theme.danger)
                .child(Icon::new(IconName::Clock).text_color(theme.danger))
                .child(format!("Timed out until {until}"))
                .into_any_element(),
        );
    }
    if let Some(status) = &card.status {
        sections.push(
            section("Status")
                .child(div().text_sm().child(status.clone()))
                .into_any_element(),
        );
    }
    if let Some(pronouns) = &card.pronouns {
        sections.push(
            section("Pronouns")
                .child(div().text_sm().child(pronouns.clone()))
                .into_any_element(),
        );
    }
    if !card.roles.is_empty() {
        let chips = card.roles.iter().map(|role| {
            let dot = role.colour.as_deref().and_then(hex_colour).unwrap_or(muted);
            h_flex()
                .gap_1()
                .items_center()
                .px_2()
                .py(px(2.))
                .rounded_md()
                .bg(theme.secondary)
                .text_xs()
                .child(div().size(px(8.)).flex_none().rounded_full().bg(dot))
                .child(role.name.clone())
        });
        sections.push(
            section("Roles")
                .child(h_flex().flex_wrap().gap_1().children(chips))
                .into_any_element(),
        );
    }
    let mutuals = &content.mutuals;
    for (list, count) in [
        (MutualList::Friends, mutuals.friends.len()),
        (MutualList::Places, mutuals.places.len()),
    ] {
        if count == 0 {
            continue;
        }
        let icons: Vec<AnyElement> = match list {
            MutualList::Friends => mutuals
                .friends
                .iter()
                .take(MUTUALS_SHOWN)
                .map(|friend| {
                    named(
                        format!("card-mutual-{}", friend.user_id),
                        &friend.name,
                        friend_icon(friend, "card", MUTUAL_ICON, cx),
                    )
                })
                .collect(),
            MutualList::Places => mutuals
                .places
                .iter()
                .take(MUTUALS_SHOWN)
                .map(|place| {
                    named(
                        format!("card-mutual-{}", place.id),
                        &place.name,
                        place_icon(place, "card", MUTUAL_ICON, cx),
                    )
                })
                .collect(),
        };
        let more = count.saturating_sub(MUTUALS_SHOWN);
        let on_mutuals = on_mutuals.clone();
        // The whole row opens the list; its hover reaches a little past the
        // icons, without moving them.
        sections.push(
            section(list.title())
                .id(SharedString::from(format!("profile-card-{list:?}")))
                .mx_neg_2()
                .my_neg_1()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .hover(|el| el.bg(theme.secondary))
                .on_click(move |_, window, cx| on_mutuals(list, window, cx))
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .children(icons)
                        .when(more > 0, |el| {
                            el.child(
                                div()
                                    .pl_1()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(muted)
                                    .child(overflow_text(more)),
                            )
                        }),
                )
                .into_any_element(),
        );
    }
    let mut since: Vec<(String, DateTime<Utc>)> = Vec::new();
    if let Some(created) = card.created {
        since.push(("Stoat".into(), created));
    }
    if let Some((server, joined)) = &card.joined_server {
        since.push((server.clone(), *joined));
    }
    if !since.is_empty() {
        sections.push(
            section("Member since")
                .children(since.into_iter().map(|(place, when)| {
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(div().text_color(muted).truncate().child(place))
                        .child(div().flex_none().child(date_label(when)))
                }))
                .into_any_element(),
        );
    }
    if let Some(bio) = &content.bio {
        sections.push(
            section("About me")
                .child(
                    TextView::markdown(
                        SharedString::from(bio.render_key.clone()),
                        bio.markdown.clone(),
                    )
                    .plugin(MathPlugin)
                    .plugin(MarkupPlugin::default())
                    .code_block_actions(code_block_actions)
                    .text_sm()
                    .selectable(true)
                    .on_link_click(move |url, _, window, cx| on_link(url, window, cx)),
                )
                .into_any_element(),
        );
    }

    v_flex()
        .w(px(CARD_WIDTH))
        .max_h(px(CARD_MAX_HEIGHT))
        .overflow_hidden()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow_lg()
        .child(div().flex_none().child(banner))
        .child(div().flex_none().child(header))
        .when(!actions.is_empty(), |el| {
            el.child(h_flex().flex_none().px_4().pt_3().gap_2().children(actions))
        })
        .child(
            v_flex()
                .id("profile-card-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_4()
                .pt_3()
                .pb_4()
                .gap_3()
                .children(sections),
        )
}

/// The whole of one list of mutuals, for its dialog: each friend, server or
/// group with its icon and name, scrolling past [`MUTUALS_LIST_HEIGHT`].
/// Choosing one calls `on_pick`.
pub fn render_mutuals_list(
    list: MutualList,
    mutuals: &Mutuals,
    on_pick: OnMutualPick,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let row = |key: String, icon: AnyElement, name: &str, pick: MutualPick| {
        let on_pick = on_pick.clone();
        h_flex()
            .id(SharedString::from(key))
            .flex_none()
            .px_2()
            .py_1()
            .gap_2()
            .items_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|el| el.bg(theme.secondary))
            .on_click(move |_, window, cx| on_pick(pick.clone(), window, cx))
            .child(icon)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .child(name.to_string()),
            )
            .into_any_element()
    };
    let rows: Vec<AnyElement> = match list {
        MutualList::Friends => mutuals
            .friends
            .iter()
            .map(|friend| {
                row(
                    format!("mutual-friend-{}", friend.user_id),
                    friend_icon(friend, "list", MUTUALS_LIST_ICON, cx),
                    &friend.name,
                    MutualPick::Friend(friend.user_id.clone()),
                )
            })
            .collect(),
        MutualList::Places => mutuals
            .places
            .iter()
            .map(|place| {
                let pick = match place.kind {
                    MutualKind::Server => MutualPick::Server(place.id.clone()),
                    MutualKind::Group => MutualPick::Group(place.id.clone()),
                };
                row(
                    format!("mutual-place-{}", place.id),
                    place_icon(place, "list", MUTUALS_LIST_ICON, cx),
                    &place.name,
                    pick,
                )
            })
            .collect(),
    };
    v_flex()
        .id("mutuals-list")
        .max_h(px(MUTUALS_LIST_HEIGHT))
        .overflow_y_scroll()
        .children(rows)
}

/// A friend's avatar. `key` keeps its loading state apart from the same
/// avatar elsewhere on screen.
fn friend_icon(friend: &MutualFriend, key: &str, size: f32, cx: &App) -> AnyElement {
    let theme = cx.theme();
    avatar(
        format!("mutual-{key}-{}", friend.user_id),
        Some(friend.avatar_url.clone()),
        &friend.name,
        size,
        theme.secondary,
        theme.secondary_foreground,
    )
}

/// A server's or group's icon, round. Without one, a server shows its
/// initial and a group the group glyph, as in Home's list.
fn place_icon(place: &MutualPlace, key: &str, size: f32, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let glyph = Icon::new(IconName::Users);
    let glyph = if size < MUTUALS_LIST_ICON {
        glyph.xsmall()
    } else {
        glyph.small()
    };
    match (&place.icon_url, place.kind) {
        (None, MutualKind::Group) => div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.secondary)
            .text_color(theme.secondary_foreground)
            .child(glyph)
            .into_any_element(),
        (url, _) => avatar(
            format!("mutual-{key}-{}", place.id),
            url.clone(),
            &place.name,
            size,
            theme.secondary,
            theme.secondary_foreground,
        ),
    }
}

/// `icon` with `name` as its tooltip.
fn named(id: String, name: &str, icon: AnyElement) -> AnyElement {
    let name = SharedString::from(name.to_string());
    div()
        .id(SharedString::from(id))
        .flex_none()
        .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
        .child(icon)
        .into_any_element()
}

/// "5 Nov 2026", in local time.
fn date_label(when: DateTime<Utc>) -> String {
    when.with_timezone(&Local).format("%-d %b %Y").to_string()
}
