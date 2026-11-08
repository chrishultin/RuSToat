use std::sync::Arc;

use gpui_kit::*;
use stoat_gpui::accounts::{AccountsFile, FileAccounts, MemoryAccounts};
use stoat_gpui::prefs::{Appearance, Palette};
use stoat_gpui::session::{MigratingStore, TokenStore};
use stoat_gpui::ui::actions::*;
use stoat_gpui::ui::app_prefs::AppPrefs;
use stoat_gpui::ui::settings::SettingsView;
use stoat_gpui::{runtime, ui};

/// The theme the preferences ask for.
fn theme_choice(cx: &App) -> (Palette, Appearance) {
    let prefs = AppPrefs::get(cx);
    (prefs.palette, prefs.appearance)
}

/// Handle ⌘(index + 1): show that account, if the window is open.
fn on_switch_account<A: Action>(index: usize, cx: &mut App) {
    cx.on_action(move |_: &A, cx| {
        MainWindow::update_root(cx, |root, cx| root.show_account_at(index, cx));
    });
}

/// The menu bar. The Accounts menu lists `accounts` (names, in the
/// switcher's order) with their ⌘-digit shortcuts.
fn set_menus(accounts: &[String], cx: &mut App) {
    let switch = accounts.iter().enumerate().filter_map(|(ix, name)| {
        let action = switch_account_action(ix)?;
        Some(MenuItem::Action {
            name: name.clone().into(),
            action,
            os_action: None,
            checked: false,
            disabled: false,
        })
    });
    cx.set_menus(vec![
        Menu {
            name: "Stoat".into(),
            items: vec![
                MenuItem::action("Add Account…", AddAccount),
                MenuItem::action("Create Server…", CreateServer),
                MenuItem::action("Join Server…", JoinServer),
                MenuItem::action("Log Out", LogOut),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit Stoat", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "Accounts".into(),
            items: switch.collect(),
            disabled: accounts.is_empty(),
        },
    ]);
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("stoat_gpui=info"))
        .init();

    // rustls has two crypto providers in this build (reqwest's aws-lc-rs and
    // livekit's ring), so it won't pick one by itself, and the first TLS
    // connection would panic. Use aws-lc-rs, as before livekit came along.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Start the IO runtime up front so a failure shows up at launch, not on
    // the first network call.
    runtime::runtime();

    // Voice spike: check that WebRTC links and starts, and name the audio
    // devices it sees. Off the main thread; only when debug logging is on.
    if log::log_enabled!(log::Level::Debug) {
        runtime::runtime().spawn_blocking(|| match stoat_gpui::voice::webrtc_ready() {
            Ok((inputs, outputs)) => {
                log::debug!("webrtc ready: inputs {inputs:?}, outputs {outputs:?}")
            }
            Err(err) => log::warn!("webrtc audio didn't start: {err}"),
        });
    }

    // Pasted images left behind by RuSToat processes that have since quit
    // (a day without changes); in the background, so launch isn't held up.
    std::thread::spawn(|| {
        let removed = stoat_gpui::draft::sweep_paste_dirs(
            &std::env::temp_dir(),
            std::process::id(),
            std::time::SystemTime::now(),
            std::time::Duration::from_secs(24 * 60 * 60),
        );
        if !removed.is_empty() {
            log::info!("removed {} old pasted-image folder(s)", removed.len());
        }
    });

    let store: Arc<dyn TokenStore> = Arc::new(MigratingStore::keychain());
    let accounts: Arc<dyn AccountsFile> = match FileAccounts::in_app_dir() {
        Some(file) => {
            log::debug!("account list: {}", file.path().display());
            Arc::new(file)
        }
        None => {
            log::warn!("no home directory; accounts won't be remembered");
            Arc::new(MemoryAccounts::default())
        }
    };

    let app = gpui_kit::application()
        .with_assets(stoat_gpui::assets::AppAssets)
        .with_http_client(stoat_gpui::http::ReqwestHttpClient::new());
    // Clicking the Dock icon while no window is showing: show the hidden
    // window again, or open a new one if it was closed. A minimized window
    // comes back on its own.
    app.on_reopen(MainWindow::show);
    app.run(|cx| {
        gpui_kit::init(cx);
        // Emoji newer than the system's emoji font (the `emojis` crate can be
        // a Unicode version ahead) stay out of the picker and completions.
        match stoat_gpui::emoji::hide_undrawable_by_system_font() {
            Some(0) => {}
            Some(hidden) => log::info!("hiding {hidden} emoji the system font can't draw"),
            None => log::warn!("no system emoji font found; showing every emoji"),
        }
        // The saved palette before any window opens, so there's no flash of
        // the default theme.
        stoat_gpui::themes::register(cx);
        AppPrefs::init(cx);
        stoat_gpui::themes::apply_current(cx);
        // And again whenever Settings changes the palette or appearance.
        let mut shown = theme_choice(cx);
        cx.observe_global::<AppPrefs>(move |cx| {
            let choice = theme_choice(cx);
            if choice != shown {
                shown = choice;
                stoat_gpui::themes::apply_current(cx);
            }
        })
        .detach();
        // After gpui_kit's bindings, so the chat shortcuts win over theirs.
        ui::chat::init(cx);

        cx.bind_keys(app_key_bindings());
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &OpenSettings, cx| SettingsWindow::open_or_focus(false, cx));
        cx.on_action(|_: &OpenProfileSettings, cx| SettingsWindow::open_or_focus(true, cx));
        set_menus(&[], cx);

        on_switch_account::<SwitchAccount1>(0, cx);
        on_switch_account::<SwitchAccount2>(1, cx);
        on_switch_account::<SwitchAccount3>(2, cx);
        on_switch_account::<SwitchAccount4>(3, cx);
        on_switch_account::<SwitchAccount5>(4, cx);
        on_switch_account::<SwitchAccount6>(5, cx);
        on_switch_account::<SwitchAccount7>(6, cx);
        on_switch_account::<SwitchAccount8>(7, cx);
        on_switch_account::<SwitchAccount9>(8, cx);

        // Handled at the app level so the menu items work wherever focus is.
        // Log Out applies to the account on screen, so without a window it
        // does nothing.
        cx.on_action(|_: &LogOut, cx| {
            MainWindow::update(cx, |root, window, cx| root.log_out_current(window, cx));
        });
        // Create Server… and Join Server… act for the account on screen;
        // without one (the login view, or no window) they do nothing.
        cx.on_action(|_: &CreateServer, cx| {
            MainWindow::update(cx, |root, window, cx| {
                if let Some((_, chat)) = root.shown_chat() {
                    window.activate_window();
                    chat.update(cx, |chat, cx| chat.open_create_server(window, cx));
                }
            });
        });
        cx.on_action(|_: &JoinServer, cx| {
            MainWindow::update(cx, |root, window, cx| {
                if let Some((_, chat)) = root.shown_chat() {
                    window.activate_window();
                    chat.update(cx, |chat, cx| chat.open_join_dialog(None, window, cx));
                }
            });
        });
        // Without a window, Add Account… opens one, which restores the saved
        // accounts and offers the + for another.
        cx.on_action(|_: &AddAccount, cx| {
            if MainWindow::is_open(cx) {
                MainWindow::update(cx, |root, window, cx| {
                    window.activate_window();
                    root.add_account(window, cx);
                });
                cx.activate(true);
            } else if let Err(err) = MainWindow::open(cx) {
                log::error!("couldn't open the main window: {err:#}");
            }
        });
        // Really closing the window (with Keep running off) drops every
        // account's connection with it, so nothing keeps the Dock badge up to
        // date: clear it rather than leave a stale count. Reopening the
        // window counts again. A hidden window never gets here.
        cx.on_window_closed(|cx, id| {
            if MainWindow::window_id(cx) == Some(id) {
                log::info!("main window closed");
                stoat_gpui::dock::set_badge(0);
                // No accounts to switch to until it's opened again.
                MainWindow::sync_menus(Vec::new(), cx);
            }
        })
        .detach();

        cx.set_global(SettingsWindow(None));
        cx.set_global(MainWindow {
            store,
            accounts,
            current: None,
            menu_accounts: Vec::new(),
        });
        MainWindow::open(cx).expect("failed to open the main window");
        log::info!("Stoat window opened");
    });
}

/// The main window, and what it needs to be opened again after it's closed
/// (from the Dock, or Add Account…).
struct MainWindow {
    store: Arc<dyn TokenStore>,
    accounts: Arc<dyn AccountsFile>,
    /// The window opened last, and its content. The content is held weakly so
    /// it goes when the window closes, along with every account's connection.
    current: Option<(AnyWindowHandle, WeakEntity<ui::AppRoot>)>,
    /// The account names the Accounts menu lists now.
    menu_accounts: Vec<String>,
}

impl Global for MainWindow {}

impl MainWindow {
    /// Open the main window and bring the app forward. Every saved account
    /// is restored from the Keychain, as at launch.
    fn open(cx: &mut App) -> anyhow::Result<()> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1100.), px(720.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Stoat".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let this = cx.global::<Self>();
        let (store, accounts) = (this.store.clone(), this.accounts.clone());
        let (window, root) = gpui_kit::open_window(options, cx, |window, cx| {
            // With Keep running on, the close button hides the window, and
            // every account stays connected on its tasks. ⌘Q still quits.
            window.on_window_should_close(cx, |window, cx| {
                if !AppPrefs::get(cx).keep_running {
                    return true;
                }
                log::info!("main window hidden; still running");
                if !stoat_gpui::window_hide::hide(window) {
                    cx.hide();
                }
                false
            });
            cx.new(|cx| ui::AppRoot::new(store, accounts, window, cx))
        })?;
        cx.global_mut::<Self>().current = Some((window, root.downgrade()));
        // Keep the Accounts menu in step with the switcher.
        cx.observe(&root, |root, cx| {
            let names = root.read(cx).account_names(cx);
            MainWindow::sync_menus(names, cx);
        })
        .detach();
        cx.activate(true);
        Ok(())
    }

    /// Bring the main window forward: show it if it's hidden, or open it if
    /// it was closed.
    fn show(cx: &mut App) {
        if MainWindow::is_open(cx) {
            MainWindow::update(cx, |_, window, _| window.activate_window());
            cx.activate(true);
        } else {
            log::info!("no main window; opening one");
            if let Err(err) = MainWindow::open(cx) {
                log::error!("couldn't open the main window: {err:#}");
            }
        }
    }

    /// Rebuild the menu bar if the accounts it lists have changed.
    fn sync_menus(names: Vec<String>, cx: &mut App) {
        if cx.global::<Self>().menu_accounts == names {
            return;
        }
        set_menus(&names, cx);
        cx.global_mut::<Self>().menu_accounts = names;
    }

    /// Whether the main window is still open (perhaps minimized).
    fn is_open(cx: &App) -> bool {
        MainWindow::window_id(cx).is_some_and(|id| cx.windows().iter().any(|w| w.window_id() == id))
    }

    fn window_id(cx: &App) -> Option<WindowId> {
        cx.try_global::<Self>()?
            .current
            .as_ref()
            .map(|(window, _)| window.window_id())
    }

    /// Run `f` on the main window's content, if it's open, without its
    /// window. A key binding's action arrives while GPUI is updating the
    /// window, which [`Self::update`] can't enter again; this can.
    fn update_root(cx: &mut App, f: impl FnOnce(&mut ui::AppRoot, &mut Context<ui::AppRoot>)) {
        let root = cx
            .try_global::<Self>()
            .and_then(|this| this.current.as_ref()?.1.upgrade());
        if let Some(root) = root {
            root.update(cx, f);
        }
    }

    /// Run `f` on the main window's content, if it's open. Not from a key
    /// binding's action (see [`Self::update_root`]).
    fn update(
        cx: &mut App,
        f: impl FnOnce(&mut ui::AppRoot, &mut Window, &mut Context<ui::AppRoot>),
    ) {
        let Some((window, root)) = cx
            .try_global::<Self>()
            .and_then(|this| this.current.clone())
        else {
            return;
        };
        window
            .update(cx, |_, window, cx| {
                root.update(cx, |root, cx| f(root, window, cx))
            })
            .ok();
    }
}

/// The settings window and its content, while it's open. There's only
/// ever one.
struct SettingsWindow(
    Option<(
        WindowHandle<gpui_kit::component::Root>,
        WeakEntity<SettingsView>,
    )>,
);

impl Global for SettingsWindow {}

impl SettingsWindow {
    /// Open the settings window, or bring it forward if it's already open.
    /// With `profile`, on the Profile page. A new window starts there anyway,
    /// as it's the first page.
    fn open_or_focus(profile: bool, cx: &mut App) {
        // Deferred: from a key binding, GPUI is still updating the window
        // the key was pressed in, which may be this one.
        cx.defer(move |cx| {
            if let Some((window, view)) = cx.global::<Self>().0.clone()
                && window
                    .update(cx, |_, window, cx| {
                        window.activate_window();
                        if profile {
                            view.update(cx, |view, cx| view.show_profile(cx)).ok();
                        }
                    })
                    .is_ok()
            {
                cx.activate(true);
                return;
            }
            match Self::open(cx) {
                Ok(opened) => cx.global_mut::<Self>().0 = Some(opened),
                Err(err) => log::error!("couldn't open the settings window: {err:#}"),
            }
        });
    }

    fn open(
        cx: &mut App,
    ) -> anyhow::Result<(
        WindowHandle<gpui_kit::component::Root>,
        WeakEntity<SettingsView>,
    )> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(760.), px(560.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Settings".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let root = cx
            .try_global::<MainWindow>()
            .and_then(|main| Some(main.current.as_ref()?.1.clone()))
            .unwrap_or_else(WeakEntity::new_invalid);
        let (window, view) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| SettingsView::new(root, SettingsView::app_pages(), window, cx))
        })?;
        cx.activate(true);
        let window = window
            .downcast()
            .ok_or_else(|| anyhow::anyhow!("the settings window isn't a Root"))?;
        Ok((window, view.downgrade()))
    }
}
