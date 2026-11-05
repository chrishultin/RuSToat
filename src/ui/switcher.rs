//! The account switcher: a column left of the server rail with one button per
//! account and a **+** to add another, like Slack's workspaces. Plain
//! rendering; [`super::AppRoot`] owns the accounts and handles the clicks.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::ui::chat::{MenuAction, avatar, rail_button};

/// How an account's session is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountStatus {
    /// Checking its saved session.
    Loading,
    SignedIn,
    /// Its saved session couldn't be checked (offline, server error).
    Offline,
    /// Stoat ended its session; it needs logging in again.
    SignedOut,
}

/// One account's button.
pub struct SwitcherItem {
    pub id: String,
    pub name: String,
    /// The instance's host, e.g. `api.stoat.chat`.
    pub host: String,
    pub avatar: Option<String>,
    pub status: AccountStatus,
    pub selected: bool,
    /// Something unread (shown only for accounts not on screen).
    pub unread: bool,
    pub mentions: usize,
}

/// Something done to an account by ID: show it, log out of it.
pub type AccountAction = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// The switcher column. `adding` highlights **+** while the add-account
/// screen is up.
pub fn render_switcher(
    items: Vec<SwitcherItem>,
    adding: bool,
    on_select: AccountAction,
    on_log_out: AccountAction,
    on_add: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let buttons: Vec<_> = items
        .into_iter()
        .map(|item| {
            let tooltip = match item.status {
                AccountStatus::Offline => format!("{} — {} (offline)", item.name, item.host),
                AccountStatus::SignedOut => format!("{} — {} (signed out)", item.name, item.host),
                AccountStatus::Loading | AccountStatus::SignedIn => {
                    format!("{} — {}", item.name, item.host)
                }
            };
            let dim = matches!(
                item.status,
                AccountStatus::Offline | AccountStatus::SignedOut
            );
            let content = avatar(
                SharedString::from(format!("switcher-avatar-{}", item.id)),
                item.avatar,
                &item.name,
                48.,
                theme.secondary,
                theme.secondary_foreground,
            );
            let log_out: MenuAction = {
                let (id, on_log_out) = (item.id.clone(), on_log_out.clone());
                Rc::new(move |_, window, cx| on_log_out(&id, window, cx))
            };
            let on_click = {
                let (id, on_select) = (item.id.clone(), on_select.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| on_select(&id, window, cx)
            };
            rail_button(
                SharedString::from(format!("switcher-{}", item.id)),
                content,
                tooltip,
                item.selected,
                item.unread && !item.selected,
                if item.selected { 0 } else { item.mentions },
                Some(("Log Out", log_out)),
                on_click,
                cx,
            )
            .when(dim, |el| el.opacity(0.5))
        })
        .collect();

    let add = rail_button(
        "switcher-add",
        Icon::new(IconName::Plus).into_any_element(),
        "Add an account",
        adding,
        false,
        0,
        None,
        on_add,
        cx,
    );

    v_flex()
        .id("account-switcher")
        .w(px(64.))
        .h_full()
        .flex_none()
        .items_center()
        .gap_2()
        .py_3()
        .bg(theme.sidebar)
        .border_r_1()
        .border_color(theme.sidebar_border)
        .overflow_y_scroll()
        .children(buttons)
        .child(add)
}

/// The host of an instance URL, for labels: `https://api.stoat.chat` →
/// `api.stoat.chat`. Falls back to the URL itself.
pub fn host_of(instance: &str) -> String {
    url::Url::parse(instance)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| instance.to_string())
}
