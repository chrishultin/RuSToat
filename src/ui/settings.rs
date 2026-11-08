//! The settings window: a sidebar of sections, each a [`SettingPage`].
//!
//! A section is a function returning its page ([`PageFn`]), so a later
//! section is one more function in the list the view is created with.

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::setting::{
    SelectIndex, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex};
use gpui_kit::*;

use crate::login_item::{self, LoginItem};
use crate::shortcuts::{self, Group, Keys, Shortcut};
use crate::themes::{self, Appearance, Palette};
use crate::ui::AppRoot;
use crate::ui::account_details::AccountDetailsView;
use crate::ui::account_page::AccountSecurityView;
use crate::ui::app_prefs::AppPrefs;
use crate::ui::profile_editor::{ProfileEditor, render_server_identities};
use crate::ui::sessions_page::SessionsView;

/// Builds one section of the window, each time the window renders.
pub type PageFn = fn(&SettingsView, &mut App) -> SettingPage;

/// The settings window's content.
pub struct SettingsView {
    /// The main window's content, for sections about the account on screen.
    root: WeakEntity<AppRoot>,
    pages: Vec<PageFn>,
    /// The Profile page's form for each account it has shown, by account
    /// ID, so unsaved edits survive switching accounts.
    profile_editors: HashMap<String, Entity<ProfileEditor>>,
    /// The Sessions page for each account it has shown, by account ID.
    sessions_views: HashMap<String, Entity<SessionsView>>,
    /// The Account page for each account it has shown, by account ID.
    account_views: HashMap<String, Entity<AccountSecurityView>>,
    /// The Account page's details for each account it has shown, by
    /// account ID.
    details_views: HashMap<String, Entity<AccountDetailsView>>,
    /// Bumped to go back to the first page (Profile): a new ID gives the
    /// sidebar new state, which starts there.
    page_reset: usize,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(
        root: WeakEntity<AppRoot>,
        pages: Vec<PageFn>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![
            // Show what the sections read from the preferences, whichever
            // window changed them.
            cx.observe_global::<AppPrefs>(|_, cx| cx.notify()),
            // Follow macOS's light/dark setting even with the main window
            // closed.
            cx.observe_window_appearance(window, |_, _, cx| themes::apply_current(cx)),
        ];
        // The Profile page follows the account on screen.
        if let Some(root) = root.upgrade() {
            subscriptions.push(cx.observe(&root, |_, _, cx| cx.notify()));
        }
        Self {
            root,
            pages,
            profile_editors: HashMap::new(),
            sessions_views: HashMap::new(),
            account_views: HashMap::new(),
            details_views: HashMap::new(),
            page_reset: 0,
            _subscriptions: subscriptions,
        }
    }

    /// Go to the Profile page ("Edit profile…" with the window open).
    pub fn show_profile(&mut self, cx: &mut Context<Self>) {
        self.page_reset += 1;
        cx.notify();
    }

    /// The Profile page's form for the account on screen, made the first
    /// time it's shown. A form whose account has since signed out and in
    /// again is replaced, since its chat has gone.
    fn sync_profile_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, chat)) = self.root().and_then(|root| root.read(cx).shown_chat()) else {
            return;
        };
        if self
            .profile_editors
            .get(&id)
            .is_some_and(|editor| editor.read(cx).is_for(&chat))
        {
            return;
        }
        let editor = cx.new(|cx| ProfileEditor::new(chat, window, cx));
        self.profile_editors.insert(id, editor);
    }

    /// The Sessions page for the account on screen, made the first time
    /// it's shown, and again when the account signs in afresh.
    fn sync_sessions_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root() else {
            return;
        };
        let Some((id, chat)) = root.read(cx).shown_chat() else {
            return;
        };
        let session = root.read(cx).shown_session_id();
        if self
            .sessions_views
            .get(&id)
            .is_some_and(|view| view.read(cx).is_for(&chat, session.as_deref()))
        {
            return;
        }
        let view = cx.new(|cx| SessionsView::new(chat, session, window, cx));
        self.sessions_views.insert(id, view);
    }

    /// The Account page for the account on screen, made the first time it's
    /// shown, and again when the account signs in afresh.
    fn sync_account_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, chat)) = self.root().and_then(|root| root.read(cx).shown_chat()) else {
            return;
        };
        if self
            .account_views
            .get(&id)
            .is_some_and(|view| view.read(cx).is_for(&chat))
        {
            return;
        }
        let view = cx.new(|cx| AccountSecurityView::new(chat, window, cx));
        self.account_views.insert(id, view);
    }

    /// The Account page's details for the account on screen, made the
    /// first time they're shown, and again when the account signs in afresh.
    fn sync_details_view(&mut self, cx: &mut Context<Self>) {
        let Some((id, chat)) = self.root().and_then(|root| root.read(cx).shown_chat()) else {
            return;
        };
        if self
            .details_views
            .get(&id)
            .is_some_and(|view| view.read(cx).is_for(&chat))
        {
            return;
        }
        let view = cx.new(|_| AccountDetailsView::new(chat));
        self.details_views.insert(id, view);
    }

    /// The Account page's details for the account on screen, if it's
    /// signed in.
    fn shown_details_view(&self, cx: &App) -> Option<Entity<AccountDetailsView>> {
        let (id, chat) = self.root()?.read(cx).shown_chat()?;
        let view = self.details_views.get(&id)?;
        view.read(cx).is_for(&chat).then(|| view.clone())
    }

    /// The Account page for the account on screen, if it's signed in.
    fn shown_account_view(&self, cx: &App) -> Option<Entity<AccountSecurityView>> {
        let (id, chat) = self.root()?.read(cx).shown_chat()?;
        let view = self.account_views.get(&id)?;
        view.read(cx).is_for(&chat).then(|| view.clone())
    }

    /// The Sessions page for the account on screen, if it's signed in.
    fn shown_sessions_view(&self, cx: &App) -> Option<Entity<SessionsView>> {
        let root = self.root()?;
        let (id, chat) = root.read(cx).shown_chat()?;
        let session = root.read(cx).shown_session_id();
        let view = self.sessions_views.get(&id)?;
        view.read(cx)
            .is_for(&chat, session.as_deref())
            .then(|| view.clone())
    }

    /// The Profile page's form for the account on screen, if it's signed in.
    fn shown_profile_editor(&self, cx: &App) -> Option<Entity<ProfileEditor>> {
        let (id, chat) = self.root()?.read(cx).shown_chat()?;
        let editor = self.profile_editors.get(&id)?;
        editor.read(cx).is_for(&chat).then(|| editor.clone())
    }

    /// The app's own sections, in sidebar order.
    pub fn app_pages() -> Vec<PageFn> {
        vec![
            profile_page,
            account_page,
            sessions_page,
            appearance_page,
            notifications_page,
            app_page,
            shortcuts_page,
        ]
    }

    /// The main window's content, if it's still open.
    pub fn root(&self) -> Option<Entity<AppRoot>> {
        self.root.upgrade()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Inputs need the window, which a page function doesn't have.
        self.sync_profile_editor(window, cx);
        self.sync_sessions_view(window, cx);
        self.sync_account_view(window, cx);
        self.sync_details_view(cx);
        let pages: Vec<SettingPage> = self.pages.iter().map(|page| page(self, cx)).collect();
        div().size_full().bg(cx.theme().background).child(
            Settings::new(("settings", self.page_reset))
                .default_selected_index(SelectIndex::default())
                .pages(pages),
        )
    }
}

/// Profile: the account on screen's avatar, banner, display name, pronouns
/// and bio, and its identity on each server.
fn profile_page(view: &SettingsView, cx: &mut App) -> SettingPage {
    let page = SettingPage::new("Profile")
        .icon(IconName::CircleUser)
        .resettable(false);
    let item = match view.shown_profile_editor(cx) {
        Some(editor) => SettingItem::render(move |_, _, _| editor.clone()),
        None => SettingItem::render(|_, _, cx| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Sign in to edit your profile.")
        }),
    };
    // Custom rows only match a search by their keywords.
    let page = page.group(SettingGroup::new().item(item.keywords([
        "profile",
        "display name",
        "pronouns",
        "bio",
        "avatar",
        "banner",
    ])));
    let Some((_, chat)) = view.root().and_then(|root| root.read(cx).shown_chat()) else {
        return page;
    };
    page.group(
        SettingGroup::new()
            .title("Server identities")
            .description("A nickname, avatar and pronouns for one server.")
            .item(
                SettingItem::render(move |_, _, cx| render_server_identities(&chat, cx))
                    .keywords(["server identities", "nickname", "server avatar"]),
            ),
    )
}

/// Account: the account on screen's login details and two-factor
/// authentication.
fn account_page(view: &SettingsView, cx: &mut App) -> SettingPage {
    let page = SettingPage::new("Account")
        .icon(IconName::KeyRound)
        .resettable(false);
    let (Some(details), Some(security)) =
        (view.shown_details_view(cx), view.shown_account_view(cx))
    else {
        return page.group(
            SettingGroup::new().item(
                signed_out_item("Sign in to manage your account.").keywords(ACCOUNT_KEYWORDS),
            ),
        );
    };
    page.group(SettingGroup::new().title("Account details").item(
        SettingItem::render(move |_, _, _| details.clone()).keywords([
            "account", "username", "email", "password", "change", "login",
        ]),
    ))
    .group(SettingGroup::new().title("Two-factor authentication").item(
        SettingItem::render(move |_, _, _| security.clone()).keywords([
            "account",
            "two-factor",
            "2fa",
            "mfa",
            "authenticator",
            "totp",
            "recovery codes",
            "security",
        ]),
    ))
}

/// What finds the Account page while no account is signed in.
const ACCOUNT_KEYWORDS: [&str; 7] = [
    "account",
    "username",
    "email",
    "password",
    "two-factor",
    "2fa",
    "security",
];

/// Sessions: where the account on screen is signed in.
fn sessions_page(view: &SettingsView, cx: &mut App) -> SettingPage {
    let page = SettingPage::new("Sessions")
        .icon(IconName::MonitorSmartphone)
        .resettable(false);
    let item = match view.shown_sessions_view(cx) {
        Some(sessions) => SettingItem::render(move |_, _, _| sessions.clone()),
        None => signed_out_item("Sign in to manage your sessions."),
    };
    page.group(
        SettingGroup::new().item(item.keywords(["sessions", "devices", "log out", "sign out"])),
    )
}

/// A page's only row when no account is signed in on screen.
fn signed_out_item(text: &'static str) -> SettingItem {
    SettingItem::render(move |_, _, cx| {
        div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    })
}

/// Appearance: the colour palette, and light, dark or following macOS.
fn appearance_page(_: &SettingsView, _: &mut App) -> SettingPage {
    let palettes = Palette::ALL
        .iter()
        .map(|p| (p.label().into(), p.label().into()))
        .collect();
    let appearances = Appearance::ALL
        .iter()
        .map(|a| (a.label().into(), a.label().into()))
        .collect();
    SettingPage::new("Appearance")
        .icon(IconName::Palette)
        .group(
            SettingGroup::new()
                .item(
                    SettingItem::new(
                        "Palette",
                        SettingField::dropdown(
                            palettes,
                            |cx| AppPrefs::get(cx).palette.label().into(),
                            |label, cx| {
                                if let Some(palette) = Palette::from_label(&label) {
                                    AppPrefs::update(cx, |prefs| prefs.palette = palette);
                                }
                            },
                        )
                        .default_value(Palette::default().label()),
                    )
                    .description(
                        "The colours of every window. Each has a light and a dark version.",
                    ),
                )
                .item(
                    SettingItem::new(
                        "Appearance",
                        SettingField::dropdown(
                            appearances,
                            |cx| AppPrefs::get(cx).appearance.label().into(),
                            |label, cx| {
                                if let Some(appearance) = Appearance::from_label(&label) {
                                    AppPrefs::update(cx, |prefs| prefs.appearance = appearance);
                                }
                            },
                        )
                        .default_value(Appearance::default().label()),
                    )
                    .description("Light or dark, or System to follow macOS."),
                ),
        )
}

/// Notifications: whether a message notice plays a sound (with a preview),
/// and whether calls play theirs.
fn notifications_page(_: &SettingsView, _: &mut App) -> SettingPage {
    SettingPage::new("Notifications")
        .icon(IconName::Bell)
        .group(
            SettingGroup::new()
                .title("Sounds")
                .item(
                    SettingItem::new(
                        "Notification sounds",
                        SettingField::switch(
                            |cx| AppPrefs::get(cx).notification_sounds,
                            |on, cx| AppPrefs::update(cx, |prefs| prefs.notification_sounds = on),
                        )
                        .default_value(true),
                    )
                    .description("Play a sound when a message notifies you."),
                )
                .item(
                    SettingItem::new(
                        "Call sounds",
                        SettingField::switch(
                            |cx| AppPrefs::get(cx).call_sounds,
                            |on, cx| AppPrefs::update(cx, |prefs| prefs.call_sounds = on),
                        )
                        .default_value(true),
                    )
                    .description(
                        "Play a sound when you or others join or leave your call, \
                         and when you mute or deafen.",
                    ),
                )
                .item(
                    SettingItem::new(
                        "Preview",
                        SettingField::render(|_, _, _| {
                            Button::new("preview-sound")
                                .label("Play")
                                .icon(IconName::Play)
                                .outline()
                                .small()
                                .on_click(|_, _, _| crate::sound::preview_message_sound())
                        }),
                    )
                    .description("macOS's Glass sound."),
                ),
        )
}

/// App: how RuSToat runs.
fn app_page(_: &SettingsView, _: &mut App) -> SettingPage {
    // macOS holds this one, so it's read afresh each time the page renders
    // (including when the window comes forward, after System Settings).
    let login = login_item::status();
    let login_note = match login {
        LoginItem::On | LoginItem::Off => "Starts RuSToat when you log in to your Mac.",
        LoginItem::NeedsApproval => "Approve RuSToat in System Settings › Login Items.",
        LoginItem::Unavailable(reason) => reason.note(),
    };
    let approve = (login == LoginItem::NeedsApproval).then(|| {
        SettingItem::new(
            "Login Items",
            SettingField::render(|_, _, _| {
                Button::new("open-login-items")
                    .label("Open System Settings")
                    .outline()
                    .small()
                    .on_click(|_, _, _| login_item::open_system_settings())
            }),
        )
    });
    SettingPage::new("App").icon(IconName::Settings).group(
        SettingGroup::new()
            .item(
                SettingItem::new(
                    "Keep running when the window is closed",
                    SettingField::switch(
                        |cx| AppPrefs::get(cx).keep_running,
                        |on, cx| AppPrefs::update(cx, |prefs| prefs.keep_running = on),
                    )
                    .default_value(true),
                )
                .description("Stay connected and keep notifying. Quit with ⌘Q."),
            )
            .item(
                SettingItem::new(
                    "Open RuSToat at login",
                    SettingField::switch(
                        |_| {
                            matches!(
                                login_item::status(),
                                LoginItem::On | LoginItem::NeedsApproval
                            )
                        },
                        |on, cx| {
                            if let Err(err) = login_item::set(on) {
                                log::warn!("login item: {err:#}");
                                show_error(format!("Couldn't change the login item: {err}"), cx);
                            }
                            cx.refresh_windows();
                        },
                    ),
                )
                .description(login_note)
                .disabled(matches!(login, LoginItem::Unavailable(_))),
            )
            .items(approve),
    )
}

/// Show `message` as an error toast in the window in front. Deferred: a
/// setting's change handler runs while GPUI is updating that window.
fn show_error(message: String, cx: &mut App) {
    cx.defer(move |cx| {
        if let Some(window) = cx.active_window() {
            window
                .update(cx, |_, window, cx| {
                    window.push_notification(Notification::error(message), cx)
                })
                .ok();
        }
    });
}

/// Keyboard Shortcuts: every shortcut, with its keys read from the keymap.
fn shortcuts_page(_: &SettingsView, _: &mut App) -> SettingPage {
    SettingPage::new("Keyboard Shortcuts")
        .icon(IconName::Keyboard)
        .resettable(false)
        .groups(Group::ALL.into_iter().map(|group| {
            SettingGroup::new()
                .title(group.label())
                .items(shortcuts::in_group(group).map(|shortcut| {
                    let shortcut = *shortcut;
                    // Custom rows only match a search by their keywords.
                    SettingItem::render(move |_, _, cx| render_shortcut(&shortcut, cx))
                        .keywords([shortcut.description, group.label()])
                }))
        }))
}

fn render_shortcut(shortcut: &Shortcut, cx: &App) -> impl IntoElement + use<> {
    let keys = match shortcut.keys {
        Keys::One(action) => keystroke_for(&*action(), cx).map(|key| vec![Kbd::new(key)]),
        Keys::Range(first, last) => keystroke_for(&*first(), cx)
            .zip(keystroke_for(&*last(), cx))
            .map(|(first, last)| vec![Kbd::new(first), Kbd::new(last)]),
    };
    let muted = cx.theme().muted_foreground;
    h_flex()
        .w_full()
        .justify_between()
        .gap_3()
        .child(div().text_sm().child(shortcut.description))
        .child(match keys {
            Some(keys) => h_flex()
                .gap_1()
                .text_color(muted)
                .children(with_dashes(keys))
                .into_any_element(),
            None => div()
                .text_sm()
                .text_color(muted)
                .child("Not bound")
                .into_any_element(),
        })
}

/// One key, or a range of two with a dash between.
fn with_dashes(keys: Vec<Kbd>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for (ix, key) in keys.into_iter().enumerate() {
        if ix > 0 {
            out.push("–".into_any_element());
        }
        out.push(key.into_any_element());
    }
    out
}

/// The keystroke that triggers `action`: the binding added last, which is
/// the one that wins.
fn keystroke_for(action: &dyn Action, cx: &App) -> Option<Keystroke> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).next_back()?;
    Some(binding.keystrokes().first()?.as_keystroke().clone())
}
