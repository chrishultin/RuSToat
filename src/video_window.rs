//! The window a video attachment plays in: a native window holding AVKit's
//! player view, with its own controls. No GPUI. There's one per account at
//! most: playing another video replaces what it shows.
//!
//! The chat view keeps it while it's open and polls it, as it does audio:
//! when the user closes it, playback stops. ⌘W and Escape close it (the
//! app's menus have no Close item).
//!
//! On other platforms nothing opens: [`VideoWindow::open`] fails.

#[cfg(target_os = "macos")]
pub use mac::VideoWindow;

#[cfg(not(target_os = "macos"))]
pub use other::VideoWindow;

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSBackingStoreType, NSEvent, NSEventModifierFlags, NSResponder, NSScreen, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_av_kit::AVPlayerView;
    use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};

    use crate::media_player::{Player, PlayerSnapshot};
    use crate::playback::window_size;

    define_class!(
        // SAFETY: NSWindow may be subclassed. Nothing is added but two
        // overrides with the superclass's signatures, and no `Drop`.
        #[unsafe(super(NSWindow, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "RuSToatVideoWindow"]
        struct PlayerWindow;

        impl PlayerWindow {
            /// ⌘W closes it, as in any Mac app.
            #[unsafe(method(performKeyEquivalent:))]
            fn perform_key_equivalent(&self, event: &NSEvent) -> Bool {
                let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
                let w = event
                    .charactersIgnoringModifiers()
                    .is_some_and(|keys| keys.to_string().eq_ignore_ascii_case("w"));
                if command && w {
                    self.performClose(None);
                    return Bool::YES;
                }
                // SAFETY: the superclass's own method, with its arguments.
                unsafe { msg_send![super(self), performKeyEquivalent: event] }
            }

            /// So does Escape.
            #[unsafe(method(cancelOperation:))]
            fn cancel_operation(&self, _sender: Option<&AnyObject>) {
                self.performClose(None);
            }
        }
    );

    impl PlayerWindow {
        fn new(mtm: MainThreadMarker, size: NSSize) -> Retained<Self> {
            let style = NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable;
            let rect = NSRect::new(NSPoint::new(0.0, 0.0), size);
            let this = Self::alloc(mtm).set_ivars(());
            // SAFETY: NSWindow's designated initialiser, with valid values.
            unsafe {
                msg_send![
                    super(this),
                    initWithContentRect: rect,
                    styleMask: style,
                    backing: NSBackingStoreType::Buffered,
                    defer: false,
                ]
            }
        }
    }

    /// An open player window and what it plays. Dropping it closes it.
    pub struct VideoWindow {
        window: Retained<PlayerWindow>,
        view: Retained<AVPlayerView>,
        player: Player,
    }

    impl VideoWindow {
        /// Open a window titled `title`, sized for a video of `original`
        /// pixels (zero if unknown), playing `url`. Fails off the main
        /// thread or for a URL that doesn't parse.
        pub fn open(url: &str, title: &str, original: (usize, usize)) -> Result<Self, String> {
            let mtm = MainThreadMarker::new().ok_or("windows live on the main thread")?;
            let player = Player::open(url)?;
            let window = PlayerWindow::new(mtm, NSSize::new(640.0, 360.0));
            // SAFETY: kept alive by `VideoWindow`, so AppKit mustn't free it
            // when it's closed.
            unsafe { window.setReleasedWhenClosed(false) };
            // SAFETY: a plain constructor, on the main thread.
            let view = unsafe { AVPlayerView::new(mtm) };
            // SAFETY: a live player for a live view.
            unsafe { view.setPlayer(Some(player.av_player())) };
            window.setContentView(Some(&view));
            let this = Self {
                window,
                view,
                player,
            };
            this.show(title, original, mtm);
            Ok(this)
        }

        /// Play `url` in this window instead, retitled and resized, and
        /// bring it forward.
        pub fn replace(
            &mut self,
            url: &str,
            title: &str,
            original: (usize, usize),
        ) -> Result<(), String> {
            let mtm = MainThreadMarker::new().ok_or("windows live on the main thread")?;
            let player = Player::open(url)?;
            // SAFETY: a live player for a live view.
            unsafe { self.view.setPlayer(Some(player.av_player())) };
            // The old player pauses as it's dropped.
            self.player = player;
            self.show(title, original, mtm);
            Ok(())
        }

        /// Bring it forward, playing.
        pub fn bring_forward(&self) {
            self.window.makeKeyAndOrderFront(None);
            self.player.play();
        }

        /// Whether it's still open (minimised counts as open).
        pub fn is_open(&self) -> bool {
            self.window.isVisible() || self.window.isMiniaturized()
        }

        /// How its player is doing (for failures).
        pub fn snapshot(&self) -> PlayerSnapshot {
            self.player.snapshot()
        }

        /// Title it, size it for the video within the screen, centre it,
        /// and play.
        fn show(&self, title: &str, original: (usize, usize), mtm: MainThreadMarker) {
            self.window.setTitle(&NSString::from_str(title));
            let screen = NSScreen::mainScreen(mtm)
                .map(|screen| screen.visibleFrame().size)
                .map_or((1280.0, 800.0), |size| (size.width, size.height));
            let (width, height) = window_size(original, screen);
            self.window.setContentSize(NSSize::new(width, height));
            self.window.center();
            self.bring_forward();
        }
    }

    impl Drop for VideoWindow {
        fn drop(&mut self) {
            self.player.pause();
            // SAFETY: a live view; it lets go of the player.
            unsafe { self.view.setPlayer(None) };
            self.window.close();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod other {
    use crate::media_player::PlayerSnapshot;

    /// Never made: nothing plays here.
    pub struct VideoWindow(());

    impl VideoWindow {
        pub fn open(_url: &str, _title: &str, _original: (usize, usize)) -> Result<Self, String> {
            Err("playback needs macOS".into())
        }

        pub fn replace(
            &mut self,
            _url: &str,
            _title: &str,
            _original: (usize, usize),
        ) -> Result<(), String> {
            Err("playback needs macOS".into())
        }

        pub fn bring_forward(&self) {}

        pub fn is_open(&self) -> bool {
            false
        }

        pub fn snapshot(&self) -> PlayerSnapshot {
            PlayerSnapshot::LOADING
        }
    }
}
