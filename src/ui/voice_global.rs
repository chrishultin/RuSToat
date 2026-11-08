//! The app's one voice call. Every account has its own [`ChatView`], but
//! only one of them may be in a call (or joining one) at a time: joining
//! from one account leaves the call in another.

use gpui_kit::{App, Global, WeakEntity};

use crate::ui::chat::ChatView;

/// The account view that's in a call, or joining one. A view that was
/// dropped (logged out) leaves a handle that no longer upgrades, which is
/// the same as none.
#[derive(Default)]
pub struct ActiveVoice(Option<WeakEntity<ChatView>>);

impl Global for ActiveVoice {}

impl ActiveVoice {
    /// The view holding the call, if any.
    pub fn holder(cx: &App) -> Option<WeakEntity<ChatView>> {
        cx.try_global::<Self>().and_then(|this| this.0.clone())
    }

    /// `view` now holds the call.
    pub fn set(view: WeakEntity<ChatView>, cx: &mut App) {
        cx.set_global(Self(Some(view)));
    }

    /// `view`'s call ended: nobody holds one now. Does nothing if another
    /// view took over since.
    pub fn clear(view: &WeakEntity<ChatView>, cx: &mut App) {
        if Self::holder(cx).as_ref() == Some(view) {
            cx.set_global(Self(None));
        }
    }
}
