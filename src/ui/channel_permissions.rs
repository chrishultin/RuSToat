//! Channel Settings' Permissions page. For a server channel it's a list
//! (Everyone, roles with an override, roles without), each opening an
//! [`OverrideEditor`] on the same page; for a group it's a
//! [`GroupPermissionsEditor`], what members may do. Who may change what,
//! and what Stoat compares an edit with, is
//! [`crate::channel_permissions`].

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use revolt_permissions::Override;

use crate::api::models::Channel;
use crate::channel_permissions::{
    OverrideRow, OverrideTarget, channel_permissions_error_text, grant_basis, group_lock_note,
    group_permissions, held, may_edit_override, override_lock_notes, override_rows,
    stored_override,
};
use crate::permission_matrix::{
    MatrixContext, Setting, follow_override, may_grant, override_change, setting_enabled,
    with_setting,
};
use crate::runtime::spawn_io;
use crate::ui::chat::ChatView;
use crate::ui::permission_matrix::{Mode, PermissionMatrix};
use crate::ui::server_settings::{colour_dot, lock_note, section_heading};

/// "‹ Permissions" was clicked in an editor.
pub struct BackToList;

/// What the page shows.
enum PageMode {
    /// A server channel's overrides.
    List,
    /// One override's editor.
    Override(Entity<OverrideEditor>),
    /// A group's member permissions.
    Group(Entity<GroupPermissionsEditor>),
}

/// The Permissions page of one channel's or group's settings.
pub struct ChannelPermissionsPage {
    chat: WeakEntity<ChatView>,
    channel: String,
    mode: PageMode,
    _subscriptions: Vec<Subscription>,
}

impl ChannelPermissionsPage {
    pub fn new(chat: &Entity<ChatView>, channel: String, cx: &mut Context<Self>) -> Self {
        let group = matches!(
            chat.read(cx).store().channel(&channel),
            Some(Channel::Group { .. })
        );
        let mode = if group {
            let editor = cx.new(|cx| GroupPermissionsEditor::new(chat, channel.clone(), cx));
            PageMode::Group(editor)
        } else {
            PageMode::List
        };
        Self {
            chat: chat.downgrade(),
            channel,
            mode,
            _subscriptions: vec![cx.observe(chat, |this, _, cx| this.follow(cx))],
        }
    }

    /// Back to the list from an override whose role has gone.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let PageMode::Override(editor) = &self.mode else {
            cx.notify();
            return;
        };
        let gone = match &editor.read(cx).target {
            OverrideTarget::Role(role) => self.chat.upgrade().is_none_or(|chat| {
                let store = chat.read(cx).store();
                let server = match store.channel(&self.channel) {
                    Some(Channel::TextChannel { server, .. }) => server.clone(),
                    _ => return true,
                };
                store
                    .server(&server)
                    .is_none_or(|server| !server.roles.contains_key(role))
            }),
            OverrideTarget::Everyone => false,
        };
        if gone {
            self.mode = PageMode::List;
        }
        cx.notify();
    }

    fn open(&mut self, target: OverrideTarget, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let channel = self.channel.clone();
        let editor = cx.new(|cx| OverrideEditor::new(&chat, channel, target, cx));
        self._subscriptions
            .push(cx.subscribe(&editor, |this, _, _: &BackToList, cx| {
                this.mode = PageMode::List;
                cx.notify();
            }));
        self.mode = PageMode::Override(editor);
        cx.notify();
    }

    fn render_everyone(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        list_row("override-everyone", cx)
            .on_click(cx.listener(|this, _, _, cx| this.open(OverrideTarget::Everyone, cx)))
            .child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(Icon::new(IconName::Users).small()),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Everyone"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Permissions available when no role overrides apply"),
            )
            .child(chevron(cx))
            .into_any_element()
    }

    fn render_role(&self, row: OverrideRow, set: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let summary = if set {
            row.summary()
        } else {
            "No overrides yet".into()
        };
        let role = row.role.clone();
        list_row(SharedString::from(format!("override-{}", row.role)), cx)
            .on_click(
                cx.listener(move |this, _, _, cx| {
                    this.open(OverrideTarget::Role(role.clone()), cx)
                }),
            )
            .child(colour_dot(row.colour.as_deref(), cx))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(row.name),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(summary),
            )
            .child(chevron(cx))
            .into_any_element()
    }

    fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let (set, unset) = self
            .chat
            .upgrade()
            .map(|chat| override_rows(chat.read(cx).store(), &self.channel))
            .unwrap_or_default();
        let muted = cx.theme().muted_foreground;
        let empty = |text: &'static str| div().px_2().text_sm().text_color(muted).child(text);
        let set_rows: Vec<AnyElement> = set
            .into_iter()
            .map(|row| self.render_role(row, true, cx))
            .collect();
        let unset_rows: Vec<AnyElement> = unset
            .into_iter()
            .map(|row| self.render_role(row, false, cx))
            .collect();
        let set_empty = set_rows.is_empty();
        let unset_empty = unset_rows.is_empty();
        v_flex()
            .w_full()
            .gap_4()
            .child(self.render_everyone(cx))
            .child(
                v_flex()
                    .gap_1()
                    .child(section_heading("Role overrides"))
                    .when(set_empty, |el| {
                        el.child(empty("No role has an override here."))
                    })
                    .children(set_rows),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(section_heading("Roles without overrides"))
                    .when(unset_empty, |el| {
                        el.child(empty("Every role has an override here."))
                    })
                    .children(unset_rows),
            )
            .into_any_element()
    }
}

/// A clickable row of the list, for `id`.
fn list_row(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let hover = cx.theme().secondary;
    h_flex()
        .id(id)
        .w_full()
        .gap_3()
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .hover(move |el| el.bg(hover))
}

/// The "opens something" chevron at a row's end.
fn chevron(cx: &App) -> AnyElement {
    div()
        .flex_none()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(IconName::ChevronRight).small())
        .into_any_element()
}

impl Render for ChannelPermissionsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.mode {
            PageMode::List => self.render_list(cx),
            PageMode::Override(editor) => editor.clone().into_any_element(),
            PageMode::Group(editor) => editor.clone().into_any_element(),
        }
    }
}

/// An editor's bar at the top of the page: "‹ Permissions" on the left,
/// and `actions` (Reset and Save) on the right, if the user may save.
fn editor_bar<V: EventEmitter<BackToList>>(
    actions: Option<[Button; 2]>,
    cx: &Context<V>,
) -> AnyElement {
    h_flex()
        .w_full()
        .gap_2()
        .child(
            Button::new("permissions-back")
                .label("Permissions")
                .icon(IconName::ChevronLeft)
                .ghost()
                .small()
                .on_click(cx.listener(|_, _, _, cx| cx.emit(BackToList))),
        )
        .child(div().flex_1())
        .children(actions.into_iter().flatten())
        .into_any_element()
}

/// One override in a server channel: Everyone's, or a role's, as Deny /
/// Neutral / Allow for each permission that means something in a channel,
/// saved with one `PUT`. Which options are enabled is Stoat's grant rule,
/// against [`grant_basis`] and the user's permissions in the channel.
/// Untouched permissions follow changes made elsewhere.
pub struct OverrideEditor {
    chat: WeakEntity<ChatView>,
    channel: String,
    target: OverrideTarget,
    /// The stored override the matrix was last filled with.
    filled: Override,
    /// The matrix as edited.
    edited: Override,
    saving: bool,
    _chat: Subscription,
}

impl EventEmitter<BackToList> for OverrideEditor {}

/// What the user may do with an override, now.
struct OverrideRights {
    /// They may change it at all.
    editable: bool,
    /// The permissions they hold in the channel.
    held: u64,
    /// What Stoat compares an edit with.
    basis: Override,
}

impl OverrideEditor {
    fn new(
        chat: &Entity<ChatView>,
        channel: String,
        target: OverrideTarget,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            chat: chat.downgrade(),
            channel,
            target,
            filled: Override::default(),
            edited: Override::default(),
            saving: false,
            _chat: cx.observe(chat, |this, _, cx| this.follow(cx)),
        };
        this.reset(cx);
        this
    }

    /// The override as Stoat stores it now.
    fn stored(&self, cx: &App) -> Override {
        self.chat
            .upgrade()
            .and_then(|chat| {
                let store = chat.read(cx).store();
                Some(stored_override(store.channel(&self.channel)?, &self.target))
            })
            .unwrap_or_default()
    }

    fn rights(&self, cx: &App) -> OverrideRights {
        let Some(chat) = self.chat.upgrade() else {
            return OverrideRights {
                editable: false,
                held: 0,
                basis: Override::default(),
            };
        };
        let store = chat.read(cx).store();
        let basis = grant_basis(store, &self.channel, &self.target);
        OverrideRights {
            editable: basis.is_some() && may_edit_override(store, &self.channel, &self.target),
            held: held(store, &self.channel),
            basis: basis.unwrap_or_default(),
        }
    }

    /// Whether Save can send the matrix: the user may change it, Stoat
    /// would take it, something changed, and not already saving.
    fn can_save(&self, cx: &App) -> bool {
        let rights = self.rights(cx);
        !self.saving
            && rights.editable
            && may_grant(rights.held, &rights.basis, &self.edited)
            && override_change(&self.stored(cx), &self.edited).is_some()
    }

    /// Fill the matrix with the stored override.
    fn reset(&mut self, cx: &mut Context<Self>) {
        self.filled = self.stored(cx);
        self.edited = self.filled.clone();
        cx.notify();
    }

    /// The store changed: untouched permissions take the new value, edited
    /// ones keep theirs.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let now = self.stored(cx);
        if now != self.filled {
            self.edited = follow_override(&self.filled, &self.edited, &now);
            self.filled = now;
        }
        cx.notify();
    }

    fn set(&mut self, bit: u64, setting: Setting, cx: &mut Context<Self>) {
        self.edited = with_setting(self.edited.clone(), bit, setting);
        cx.notify();
    }

    /// Send the new override. Stoat's answer shows at once, in the main
    /// window too; a refusal says why in a toast, and the matrix keeps the
    /// edit.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A role's override can't be edited yet (no grant basis).
        if !self.can_save(cx) || self.target != OverrideTarget::Everyone {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let permissions = self.edited.clone();
        self.saving = true;
        cx.notify();
        let client = chat.read(cx).client().clone();
        let channel = self.channel.clone();
        let request = spawn_io(async move {
            client
                .set_channel_default_permissions(&channel, &permissions)
                .await
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.saving = false;
                let text = match result {
                    Ok(Ok(saved)) => {
                        log::info!("saved an override in channel {}", this.channel);
                        this.chat
                            .update(cx, |chat, cx| chat.channel_permissions_saved(&saved, cx))
                            .ok();
                        this.reset(cx);
                        window.push_notification(Notification::success("Saved."), cx);
                        return;
                    }
                    Ok(Err(err)) => {
                        log::warn!(
                            "couldn't save an override in channel {}: {err:?}",
                            this.channel
                        );
                        channel_permissions_error_text(&err)
                    }
                    Err(err) => {
                        log::warn!("override save task failed: {err:?}");
                        "Couldn't save the permissions.".into()
                    }
                };
                window.push_notification(Notification::error(text), cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The role's name, or "Everyone".
    fn title(&self, cx: &App) -> String {
        let OverrideTarget::Role(role) = &self.target else {
            return "Everyone".into();
        };
        self.chat
            .upgrade()
            .and_then(|chat| {
                let store = chat.read(cx).store();
                let Some(Channel::TextChannel { server, .. }) = store.channel(&self.channel) else {
                    return None;
                };
                Some(store.server(server)?.roles.get(role)?.name.clone())
            })
            .unwrap_or_default()
    }
}

impl Render for OverrideEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notes = self
            .chat
            .upgrade()
            .map(|chat| override_lock_notes(chat.read(cx).store(), &self.channel, &self.target))
            .unwrap_or_default();
        let OverrideRights {
            editable,
            held,
            basis,
        } = self.rights(cx);
        let changed = self.edited != self.filled;
        let can_save = self.can_save(cx);
        let actions = editable.then(|| {
            [
                Button::new("override-reset")
                    .label("Reset")
                    .outline()
                    .small()
                    .disabled(!changed || self.saving)
                    .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                Button::new("override-save")
                    .label("Save")
                    .primary()
                    .small()
                    .loading(self.saving)
                    .disabled(!can_save)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            ]
        });
        let explanation = match self.target {
            // Stoat applies the server's default permissions, then this,
            // then roles' server-wide permissions, then role overrides here.
            OverrideTarget::Everyone => {
                "Changes the server's default permissions in this channel. Roles' own \
                 permissions still apply over it, and role overrides here come last."
            }
            OverrideTarget::Role(_) => {
                "What members with this role may do in this channel. It comes last, over \
                 Everyone's override here and the role's server-wide permissions."
            }
        };
        let view = cx.entity().downgrade();
        let matrix = PermissionMatrix::new("override", Mode::Overrides(self.edited.clone()))
            .context(MatrixContext::Channel)
            .read_only(!editable || self.saving)
            .enabled(move |bit, setting| setting_enabled(held, &basis, bit, setting))
            .on_change(move |bit, setting, _, cx| {
                view.update(cx, |this, cx| this.set(bit, setting, cx)).ok();
            });
        v_flex()
            .w_full()
            .gap_3()
            .child(editor_bar(actions, cx))
            .child(section_heading_owned(self.title(cx)))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(explanation),
            )
            .children(notes.into_iter().map(|note| lock_note(note, cx)))
            .child(matrix)
    }
}

/// A heading with text decided at run time.
fn section_heading_owned(text: String) -> Div {
    div()
        .text_base()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// A group's member permissions: what members other than the owner may do
/// in it, each on or off.
pub struct GroupPermissionsEditor {
    chat: WeakEntity<ChatView>,
    channel: String,
    _chat: Subscription,
}

impl EventEmitter<BackToList> for GroupPermissionsEditor {}

impl GroupPermissionsEditor {
    fn new(chat: &Entity<ChatView>, channel: String, cx: &mut Context<Self>) -> Self {
        Self {
            chat: chat.downgrade(),
            channel,
            _chat: cx.observe(chat, |_, _, cx| cx.notify()),
        }
    }

    /// The group's member permissions as Stoat stores them now.
    fn stored(&self, cx: &App) -> u64 {
        self.chat
            .upgrade()
            .and_then(|chat| {
                Some(group_permissions(
                    chat.read(cx).store().channel(&self.channel)?,
                ))
            })
            .unwrap_or_default()
    }
}

impl Render for GroupPermissionsEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let note = self
            .chat
            .upgrade()
            .and_then(|chat| group_lock_note(chat.read(cx).store(), &self.channel));
        let matrix = PermissionMatrix::new("group", Mode::Values(self.stored(cx)))
            .context(MatrixContext::Group)
            .read_only(true);
        v_flex()
            .w_full()
            .gap_3()
            .child(section_heading("Members"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "What members may do in this group. Its owner may always do \
                         everything.",
                    ),
            )
            .children(note.map(|note| lock_note(note, cx)))
            .child(matrix)
    }
}
