//! The app's preferences, shared by every window: loaded from `prefs.json`
//! once, and saved on every change. Observe it with
//! `cx.observe_global::<AppPrefs>`.

use gpui_kit::{App, BorrowAppContext, Global};

use crate::prefs::{Prefs, PrefsFile};

pub struct AppPrefs {
    prefs: Prefs,
    /// `None` without a home directory: preferences then last until quit.
    file: Option<PrefsFile>,
}

impl Global for AppPrefs {}

impl AppPrefs {
    /// Load the saved preferences. Call once, at launch.
    pub fn init(cx: &mut App) {
        let file = PrefsFile::in_app_dir();
        if file.is_none() {
            log::warn!("no home directory; preferences won't be remembered");
        }
        let prefs = file.as_ref().map(PrefsFile::load).unwrap_or_default();
        cx.set_global(Self { prefs, file });
    }

    pub fn get(cx: &App) -> &Prefs {
        &cx.global::<Self>().prefs
    }

    /// Change the preferences. If anything changed, save them and notify
    /// observers.
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Prefs)) {
        let mut prefs = Self::get(cx).clone();
        f(&mut prefs);
        if prefs == *Self::get(cx) {
            return;
        }
        cx.update_global::<Self, _>(|this, _| {
            this.prefs = prefs;
            if let Some(file) = &this.file
                && let Err(err) = file.save(&this.prefs)
            {
                log::warn!("couldn't save preferences: {err:#}");
            }
        });
    }
}
