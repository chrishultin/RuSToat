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

    gpui_kit::application()
        .with_assets(stoat_gpui::assets::AppAssets)
        .with_http_client(stoat_gpui::http::ReqwestHttpClient::new())
        .run(|cx| {
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
            let (window, root) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| ui::AppRoot::new(store, accounts, window, cx))
            })
            .expect("failed to open the main window");

            // Handled at the app level so the menu items work wherever focus is.
            // Log Out applies to the account on screen.
            cx.on_action({
                let root = root.clone();
                move |_: &LogOut, cx| {
                    window
                        .update(cx, |_, window, cx| {
                            root.update(cx, |root, cx| root.log_out_current(window, cx))
                        })
                        .ok();
                }
            });
            cx.on_action(move |_: &AddAccount, cx| {
                window
                    .update(cx, |_, window, cx| {
                        root.update(cx, |root, cx| root.add_account(window, cx))
                    })
                    .ok();
            });

            cx.activate(true);
            log::info!("Stoat window opened");
        });
}
