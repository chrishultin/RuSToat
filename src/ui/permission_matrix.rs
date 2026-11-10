//! The permission matrix: every permission under its group's heading, each
//! with its title, what it does, and a control. In [`Mode::Values`] (the
//! server's default permissions) that's a checkbox; in [`Mode::Overrides`]
//! (a role, later a channel's overrides) it's Deny / Neutral / Allow. The
//! caller says which choices the user may make, and hears each change. The
//! list and the rules are [`crate::permission_matrix`].

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use revolt_permissions::Override;

use crate::permission_matrix::{PERMISSIONS, PermissionEntry, Setting, setting_of};

/// What the matrix shows.
#[derive(Debug, Clone)]
pub enum Mode {
    /// Permission bits, on or off: a checkbox each. On is
    /// [`Setting::Allow`], off [`Setting::Neutral`].
    Values(u64),
    /// Allow and deny bits: Deny / Neutral / Allow each.
    Overrides(Override),
}

impl Mode {
    /// The setting shown for `bit`.
    fn setting(&self, bit: u64) -> Setting {
        match self {
            Mode::Values(value) if value & bit != 0 => Setting::Allow,
            Mode::Values(_) => Setting::Neutral,
            Mode::Overrides(value) => setting_of(value, bit),
        }
    }
}

/// Which choices the user may make: `(bit, setting)`.
type Enabled = Rc<dyn Fn(u64, Setting) -> bool>;
/// A choice made: `(bit, setting)`.
type OnChange = Rc<dyn Fn(u64, Setting, &mut Window, &mut App)>;

/// The tooltip of a choice the user may not make, as Stoat's grant rule
/// refuses it.
const UNHELD_VALUE: &str = "You don't have this permission.";
const UNHELD_OVERRIDE: &str = "You don't have this permission, so you can only make it stricter.";

/// The matrix, drawn afresh each time from the caller's values.
#[derive(IntoElement)]
pub struct PermissionMatrix {
    id: SharedString,
    mode: Mode,
    enabled: Enabled,
    on_change: Option<OnChange>,
    read_only: bool,
}

impl PermissionMatrix {
    /// A matrix of `mode`'s values. Element IDs start with `id`, so two
    /// matrices on one page don't collide. Every choice is enabled until
    /// [`Self::enabled`] says otherwise.
    pub fn new(id: impl Into<SharedString>, mode: Mode) -> Self {
        Self {
            id: id.into(),
            mode,
            enabled: Rc::new(|_, _| true),
            on_change: None,
            read_only: false,
        }
    }

    /// Which choices the user may make; the others are greyed out, with a
    /// tooltip saying why.
    pub fn enabled(mut self, enabled: impl Fn(u64, Setting) -> bool + 'static) -> Self {
        self.enabled = Rc::new(enabled);
        self
    }

    /// Each choice made, among the enabled ones.
    pub fn on_change(
        mut self,
        on_change: impl Fn(u64, Setting, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(on_change));
        self
    }

    /// Grey out everything, with no tooltip about permissions (the page
    /// says why it can't be changed).
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Whether the user may choose `setting` for `bit` now.
    fn allows(&self, bit: u64, setting: Setting) -> bool {
        !self.read_only && (self.enabled)(bit, setting)
    }

    /// Choose `setting` for `bit`, if that's allowed.
    fn change(&self, bit: u64, setting: Setting) -> impl Fn(&mut Window, &mut App) + 'static {
        let allowed = self.allows(bit, setting);
        let on_change = self.on_change.clone();
        move |window, cx| {
            if let Some(on_change) = on_change.as_ref().filter(|_| allowed) {
                on_change(bit, setting, window, cx);
            }
        }
    }

    fn render_checkbox(&self, entry: &PermissionEntry) -> AnyElement {
        let bit = entry.bit();
        let checked = self.mode.setting(bit) == Setting::Allow;
        let next = if checked {
            Setting::Neutral
        } else {
            Setting::Allow
        };
        let allowed = self.allows(bit, next);
        let change = self.change(bit, next);
        Checkbox::new(SharedString::from(format!("{}-{bit}", self.id)))
            .checked(checked)
            .disabled(!allowed)
            .when(!allowed && !self.read_only, |this| {
                this.tooltip(UNHELD_VALUE)
            })
            .on_click(move |_, window, cx| change(window, cx))
            .into_any_element()
    }

    fn render_override(&self, entry: &PermissionEntry) -> AnyElement {
        let bit = entry.bit();
        let current = self.mode.setting(bit);
        let buttons = Setting::ALL.map(|setting| {
            let allowed = self.allows(bit, setting);
            let change = self.change(bit, setting);
            let icon = match setting {
                Setting::Deny => IconName::Close,
                Setting::Neutral => IconName::Minus,
                Setting::Allow => IconName::Check,
            };
            let tooltip = if allowed || self.read_only {
                setting.label().to_string()
            } else {
                format!("{}: {UNHELD_OVERRIDE}", setting.label())
            };
            let id = format!("{}-{bit}-{}", self.id, setting.label());
            let button = Button::new(SharedString::from(id))
                .icon(icon)
                .selected(setting == current)
                .disabled(!allowed)
                .tooltip(tooltip)
                .on_click(move |_, window, cx| change(window, cx));
            // The chosen setting in its colour: red to deny, green to
            // allow.
            match setting {
                Setting::Deny if setting == current => button.danger(),
                Setting::Allow if setting == current => button.success(),
                _ => button,
            }
        });
        // No group `on_click`: each button handles its own, so a disabled
        // one does nothing and the keyboard works.
        ButtonGroup::new(SharedString::from(format!("{}-{bit}", self.id)))
            .outline()
            .xsmall()
            .children(buttons)
            .into_any_element()
    }

    fn render_row(&self, entry: &PermissionEntry, cx: &App) -> AnyElement {
        let control = match self.mode {
            Mode::Values(_) => self.render_checkbox(entry),
            Mode::Overrides(_) => self.render_override(entry),
        };
        h_flex()
            .w_full()
            .gap_3()
            .py_1()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(entry.title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(entry.server_description),
                    ),
            )
            .child(div().flex_none().child(control))
            .into_any_element()
    }
}

impl RenderOnce for PermissionMatrix {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut matrix = v_flex().w_full().gap_1();
        for entry in &PERMISSIONS {
            if let Some(heading) = entry.heading {
                matrix = matrix.child(
                    div()
                        .pt_3()
                        .pb_1()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().muted_foreground)
                        .child(heading.to_uppercase()),
                );
            }
            matrix = matrix.child(self.render_row(entry, cx));
        }
        matrix
    }
}
