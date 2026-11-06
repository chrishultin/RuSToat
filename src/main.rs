use std::sync::Arc;

use gpui_kit::*;
use stoat_gpui::accounts::{AccountsFile, FileAccounts, MemoryAccounts};
use stoat_gpui::session::{MigratingStore, TokenStore};
use stoat_gpui::{runtime, ui};

actions!(stoat, [Quit, LogOut, AddAccount]);

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

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        // No shortcut for Log Out: ⇧⌘Q is macOS's own "Log Out <user>".
        cx.set_menus(vec![Menu {
            name: "Stoat".into(),
            items: vec![
                MenuItem::action("Add Account…", AddAccount),
                MenuItem::action("Log Out", LogOut),
                MenuItem::separator(),
                MenuItem::action("Quit Stoat", Quit),
            ],
            disabled: false,
        }]);

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
            }
        })
        .detach();

        cx.set_global(MainWindow {
            store,
            accounts,
            current: None,
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
        cx.activate(true);
        Ok(())
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

    /// Run `f` on the main window's content, if it's open.
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
