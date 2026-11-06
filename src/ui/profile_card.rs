//! The profile card: a person's banner, avatar and presence, names,
//! pronouns, status, roles, join dates and bio, in a small floating card.
//! What it shows comes from [`crate::profile::profile_card`] and the
//! fetched profile; `ChatView` decides where it goes and what its buttons do.

use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::profile::ProfileCard;
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

/// Everything the card draws.
pub struct CardContent {
    pub card: ProfileCard,
    pub banner_url: Option<String>,
    /// The bio as prepared Markdown, with the key its parse is cached by.
    pub bio: Option<CardBio>,
}

pub struct CardBio {
    pub render_key: String,
    pub markdown: String,
}

/// What a link click in the bio does.
pub type OnLink = Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync>;

/// The card itself. `actions` go in a row under the names (chunks of the
/// card that act on the person: Message, friend requests, the menu).
pub fn render_profile_card(
    content: &CardContent,
    actions: Vec<AnyElement>,
    on_link: OnLink,
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
        );

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

/// "5 Nov 2026", in local time.
fn date_label(when: DateTime<Utc>) -> String {
    when.with_timezone(&Local).format("%-d %b %Y").to_string()
}
