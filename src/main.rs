use std::sync::Arc;

use gpui_kit::*;
use stoat_gpui::accounts::{AccountsFile, FileAccounts, MemoryAccounts};
use stoat_gpui::session::{MigratingStore, TokenStore};
use stoat_gpui::{runtime, ui};

actions!(stoat, [Quit, LogOut, AddAccount]);

// ⌘1 to ⌘9: show the nth account in the switcher. Unit actions, since an
// action with a field needs GPUI's derive under the crate name `gpui`.
actions!(
    stoat,
    [
        SwitchAccount1,
        SwitchAccount2,
        SwitchAccount3,
        SwitchAccount4,
        SwitchAccount5,
        SwitchAccount6,
        SwitchAccount7,
        SwitchAccount8,
        SwitchAccount9
    ]
);

/// Each ⌘-digit's action, for the nth account (from 0).
fn switch_account_action(index: usize) -> Option<Box<dyn Action>> {
    Some(match index {
        0 => Box::new(SwitchAccount1),
        1 => Box::new(SwitchAccount2),
        2 => Box::new(SwitchAccount3),
        3 => Box::new(SwitchAccount4),
        4 => Box::new(SwitchAccount5),
        5 => Box::new(SwitchAccount6),
        6 => Box::new(SwitchAccount7),
        7 => Box::new(SwitchAccount8),
        8 => Box::new(SwitchAccount9),
        _ => return None,
    })
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
                MenuItem::action("Log Out", LogOut),
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

    // Start the IO runtime up front so a failure shows up at launch, not on
    // the first network call.
    runtime::runtime();

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
    // Clicking the Dock icon while no window is showing. A minimized window
    // comes back on its own; a closed one is opened again.
    app.on_reopen(|cx| {
        if !MainWindow::is_open(cx) {
            log::info!("Dock clicked with no window; opening one");
            if let Err(err) = MainWindow::open(cx) {
                log::error!("couldn't open the main window: {err:#}");
            }
        }
    });
    app.run(|cx| {
        gpui_kit::init(cx);
        // After gpui_kit's bindings, so the chat shortcuts win over theirs.
        ui::chat::init(cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        // No shortcut for Log Out: ⇧⌘Q is macOS's own "Log Out <user>".
        set_menus(&[], cx);

        cx.bind_keys([
            KeyBinding::new("cmd-1", SwitchAccount1, None),
            KeyBinding::new("cmd-2", SwitchAccount2, None),
            KeyBinding::new("cmd-3", SwitchAccount3, None),
            KeyBinding::new("cmd-4", SwitchAccount4, None),
            KeyBinding::new("cmd-5", SwitchAccount5, None),
            KeyBinding::new("cmd-6", SwitchAccount6, None),
            KeyBinding::new("cmd-7", SwitchAccount7, None),
            KeyBinding::new("cmd-8", SwitchAccount8, None),
            KeyBinding::new("cmd-9", SwitchAccount9, None),
        ]);
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
        // Without a window, Add Account… opens one, which restores the saved
        // accounts and offers the + for another.
        cx.on_action(|_: &AddAccount, cx| {
            if MainWindow::is_open(cx) {
                MainWindow::update(cx, |root, window, cx| root.add_account(window, cx));
            } else if let Err(err) = MainWindow::open(cx) {
                log::error!("couldn't open the main window: {err:#}");
            }
        });
        // Closing the window drops every account's connection with it, so
        // nothing keeps the Dock badge up to date: clear it rather than leave
        // a stale count. Reopening the window counts again.
        cx.on_window_closed(|cx, id| {
            if MainWindow::window_id(cx) == Some(id) {
                log::info!("main window closed");
                stoat_gpui::dock::set_badge(0);
                // No accounts to switch to until it's opened again.
                MainWindow::sync_menus(Vec::new(), cx);
            }
        })
        .detach();

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
