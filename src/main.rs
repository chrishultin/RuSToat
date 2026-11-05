use gpui_kit::*;
use stoat_gpui::{runtime, ui};

actions!(stoat, [Quit]);

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("stoat_gpui=info"))
        .init();

    // Start the IO runtime up front so a failure shows up at launch, not on
    // the first network call.
    runtime::runtime();

    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus(vec![Menu {
            name: "Stoat".into(),
            items: vec![MenuItem::action("Quit Stoat", Quit)],
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

        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| ui::AppRoot::new(window, cx))
        })
        .expect("failed to open the main window");

        cx.activate(true);
        log::info!("Stoat window opened");
    });
}
