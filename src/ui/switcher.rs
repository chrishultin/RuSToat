//! The account switcher: a column left of the server rail with one button per
//! account and a **+** to add another, like Slack's workspaces. Accounts can be
//! dragged into another order. Plain rendering; [`super::AppRoot`] owns the
//! accounts and handles the clicks and drags.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::ui::chat::{
    DRAGGED_OPACITY, DragPreview, DropMark, MenuBuilder, avatar, drag_face, rail_button, rail_icon,
    rail_slot, with_drop_mark,
};

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
    /// It's being dragged.
    pub dragged: bool,
    /// Where the drag would land beside it: above or below.
    pub(crate) mark: Option<DropMark>,
}

/// Something done to an account by ID: show it, log out of it.
pub type AccountAction = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// What an account carries while it's dragged in the switcher.
#[derive(Clone)]
pub(crate) struct AccountDrag {
    id: String,
}

/// A drag in the switcher, as it goes.
pub(crate) enum SwitcherDrag<'a> {
    /// The account `id` started being dragged.
    Started(&'a str),
    /// The drag moved, over the switcher (`true`) or off it. The switcher
    /// sees this before its accounts do.
    Moved(bool),
    /// The drag moved over the account `hovered`: it would land above or
    /// below it, as `mark` says.
    Over { hovered: &'a str, mark: DropMark },
    /// Dropped on the switcher.
    Dropped,
    /// Let go off the switcher: nothing happens.
    Ended,
}

/// Follows a drag in the switcher.
pub(crate) type DragAction = Rc<dyn Fn(SwitcherDrag, &mut Window, &mut App)>;

/// The switcher column. `adding` highlights **+** while the add-account
/// screen is up. Dragging an account (after a few pixels, which cancels the
/// click) shows a line where it would land, as in the server rail.
pub(crate) fn render_switcher(
    items: Vec<SwitcherItem>,
    adding: bool,
    on_select: AccountAction,
    on_log_out: AccountAction,
    on_add: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_drag: DragAction,
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
                item.avatar.clone(),
                &item.name,
                48.,
                theme.secondary,
                theme.secondary_foreground,
            );
            let preview: Rc<dyn Fn(&App) -> AnyElement> = {
                let id = SharedString::from(format!("switcher-drag-avatar-{}", item.id));
                let (url, name) = (item.avatar, item.name.clone());
                Rc::new(move |cx| {
                    let theme = cx.theme();
                    let (bg, fg) = (theme.secondary, theme.secondary_foreground);
                    let face = avatar(id.clone(), url.clone(), &name, 48., bg, fg);
                    drag_face(face, None, cx)
                })
            };
            let log_out: MenuBuilder = {
                let (id, on_log_out) = (item.id.clone(), on_log_out.clone());
                Rc::new(move |menu, _, _| {
                    let (id, on_log_out) = (id.clone(), on_log_out.clone());
                    menu.item(
                        PopupMenuItem::new("Log Out")
                            .on_click(move |_, window, cx| on_log_out(&id, window, cx)),
                    )
                })
            };
            let on_click = {
                let (id, on_select) = (item.id.clone(), on_select.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| on_select(&id, window, cx)
            };
            // A drag starts after a few pixels and cancels the click; the
            // right-click menu is unaffected.
            let start_drag = {
                let on_drag = on_drag.clone();
                move |drag: &AccountDrag, _, window: &mut Window, cx: &mut App| {
                    on_drag(SwitcherDrag::Started(&drag.id), window, cx);
                    DragPreview::new(preview.clone(), cx)
                }
            };
            let id = SharedString::from(format!("switcher-{}", item.id));
            let drag = AccountDrag {
                id: item.id.clone(),
            };
            let face = rail_icon(id, content, tooltip, item.selected, cx)
                .on_click(on_click)
                .on_drag(drag, start_drag)
                .context_menu(move |popup, window, cx| log_out(popup, window, cx));
            let slot = rail_slot(
                face.into_any_element(),
                item.selected,
                item.unread && !item.selected,
                if item.selected { 0 } else { item.mentions },
                cx,
            )
            .when(dim, |el| el.opacity(0.5))
            .when(item.dragged, |el| el.opacity(DRAGGED_OPACITY));
            // Over the top half of an account, a drop lands above it;
            // over the bottom half, below.
            let hovered = item.id;
            let on_drag = on_drag.clone();
            with_drop_mark(slot, item.mark, cx).on_drag_move(
                move |event: &DragMoveEvent<AccountDrag>, window, cx| {
                    let position = event.event.position;
                    if !event.bounds.contains(&position) {
                        return;
                    }
                    let fraction = (position.y - event.bounds.origin.y) / event.bounds.size.height;
                    let mark = match fraction < 0.5 {
                        true => DropMark::Above,
                        false => DropMark::Below,
                    };
                    let over = SwitcherDrag::Over {
                        hovered: &hovered,
                        mark,
                    };
                    on_drag(over, window, cx);
                },
            )
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
        // Seen before the accounts see it.
        .on_drag_move({
            let on_drag = on_drag.clone();
            move |event: &DragMoveEvent<AccountDrag>, window, cx| {
                let over = event.bounds.contains(&event.event.position);
                on_drag(SwitcherDrag::Moved(over), window, cx);
            }
        })
        .on_drop({
            let on_drag = on_drag.clone();
            move |_: &AccountDrag, window, cx| on_drag(SwitcherDrag::Dropped, window, cx)
        })
        .on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
            on_drag(SwitcherDrag::Ended, window, cx)
        })
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

/// What to add to notice titles so they say which account they're for, given
/// each signed-in account's (host, name). With one account, nothing. Otherwise
/// the host, or `host · name` when several accounts share that host.
pub fn notice_labels(accounts: &[(String, String)]) -> Vec<Option<String>> {
    if accounts.len() < 2 {
        return vec![None; accounts.len()];
    }
    accounts
        .iter()
        .map(|(host, name)| {
            let shared = accounts.iter().filter(|(h, _)| h == host).count() > 1;
            Some(if shared {
                format!("{host} · {name}")
            } else {
                host.clone()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in GPUI's own `test` attribute.
    use super::{host_of, notice_labels};

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(h, n)| (h.to_string(), n.to_string()))
            .collect()
    }

    #[test]
    fn one_account_needs_no_label() {
        assert_eq!(notice_labels(&pairs(&[("api.stoat.chat", "Ann")])), [None]);
        assert!(notice_labels(&[]).is_empty());
    }

    #[test]
    fn different_hosts_are_labelled_by_host() {
        assert_eq!(
            notice_labels(&pairs(&[
                ("api.stoat.chat", "Ann"),
                ("stoat.example.com", "Ann")
            ])),
            [
                Some("api.stoat.chat".to_string()),
                Some("stoat.example.com".to_string())
            ]
        );
    }

    #[test]
    fn a_shared_host_adds_the_name() {
        assert_eq!(
            notice_labels(&pairs(&[
                ("api.stoat.chat", "Ann"),
                ("api.stoat.chat", "Bo"),
                ("stoat.example.com", "Ann")
            ])),
            [
                Some("api.stoat.chat · Ann".to_string()),
                Some("api.stoat.chat · Bo".to_string()),
                Some("stoat.example.com".to_string())
            ]
        );
    }

    #[test]
    fn host_of_strips_scheme_and_path() {
        assert_eq!(
            host_of("https://stoat.example.com/api"),
            "stoat.example.com"
        );
        assert_eq!(host_of("not a url"), "not a url");
    }
}
