//! "Change identity on {server}": the user's nickname, avatar and pronouns
//! on one server, each falling back to their global profile when cleared.
//! Saved with one `PATCH /servers/{server}/members/{user}` (see
//! [`crate::profile_edit::IdentityForm`]), after uploading a new avatar.
//!
//! Nickname and avatar need Change Nickname and Change Avatar there; the
//! pronouns need nothing, as Stoat decides.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::ApiError;
use crate::profile_edit::{
    IdentityForm, NICKNAME_MAX, PRONOUNS_MAX, ProfileImage, check_nickname, check_pronouns,
    identity_error_text,
};
use crate::runtime::spawn_io;
use crate::store::ChatStore;
use crate::ui::chat::ChatView;
use crate::ui::profile_editor::{
    ImageKind, ImageSlot, Preview, avatar_preview, field, prompt_for_image, render_image_row,
    upload_image,
};

/// The dialog's width.
pub const IDENTITY_DIALOG_WIDTH: f32 = 440.;

/// What a Save is doing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Saving {
    Uploading,
    Sending,
}

/// The identity form for one server.
pub struct IdentityEditor {
    chat: WeakEntity<ChatView>,
    server: String,
    me: String,
    nickname: Entity<InputState>,
    pronouns: Entity<InputState>,
    avatar: ImageSlot,
    /// The membership as it was when the dialog opened (Stoat's answer
    /// after a save).
    loaded: IdentityForm,
    saving: Option<Saving>,
    _inputs: [Subscription; 2],
}

impl IdentityEditor {
    /// The form for the user's membership of `server`, filled from
    /// `store`, the account's. The placeholders are what shows when a
    /// field is blank: the global display name and pronouns.
    ///
    /// It takes the store rather than reading `chat`, which is being
    /// updated while it opens the dialog.
    pub fn new(
        chat: WeakEntity<ChatView>,
        store: &ChatStore,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (me, loaded, name, pronouns) = {
            let me = store.me().to_string();
            let member = store.member(&server, &me);
            let loaded = IdentityForm {
                nickname: member.and_then(|m| m.nickname.clone()).unwrap_or_default(),
                avatar: member
                    .and_then(|m| m.avatar.as_ref())
                    .map_or(ProfileImage::None, |file| {
                        ProfileImage::Current(file.id.clone())
                    }),
                pronouns: member.and_then(|m| m.pronouns.clone()).unwrap_or_default(),
            };
            let user = store.user(&me);
            (
                me,
                loaded,
                user.map(crate::store::display_name)
                    .unwrap_or_default()
                    .to_string(),
                user.and_then(|u| u.pronouns.clone()).unwrap_or_default(),
            )
        };
        let nickname = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(name);
            input.set_value(loaded.nickname.clone(), window, cx);
            input
        });
        let pronouns = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(pronouns);
            input.set_value(loaded.pronouns.clone(), window, cx);
            input
        });
        let inputs = [
            cx.subscribe(&nickname, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&pronouns, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        Self {
            chat,
            server,
            me,
            nickname,
            pronouns,
            avatar: ImageSlot {
                image: loaded.avatar.clone(),
                problem: None,
            },
            loaded,
            saving: None,
            _inputs: inputs,
        }
    }

    /// The nickname field, to focus when the dialog opens.
    pub fn nickname_input(&self) -> &Entity<InputState> {
        &self.nickname
    }

    /// Whether the user may change their nickname and avatar here.
    fn permissions(&self, cx: &App) -> (bool, bool) {
        let Some(chat) = self.chat.upgrade() else {
            return (false, false);
        };
        let store = chat.read(cx).store();
        (
            crate::permissions::can_change_nickname(store, &self.me, &self.server),
            crate::permissions::can_change_avatar(store, &self.me, &self.server),
        )
    }

    fn edited(&self, cx: &App) -> IdentityForm {
        IdentityForm {
            nickname: self.nickname.read(cx).value().to_string(),
            avatar: self.avatar.image.clone(),
            pronouns: self.pronouns.read(cx).value().to_string(),
        }
    }

    fn can_save(&self, edited: &IdentityForm) -> bool {
        self.saving.is_none()
            && edited.is_valid()
            && self.avatar.problem.is_none()
            && IdentityForm::has_changes(&self.loaded, edited)
    }

    fn pick_avatar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let limits = self
            .chat
            .upgrade()
            .map(|chat| chat.read(cx).image_limits())
            .unwrap_or_default();
        prompt_for_image(window, cx, move |this: &mut Self, path, _| {
            this.avatar.pick(path, ImageKind::Avatar.limit(limits));
        });
    }

    /// Upload a new avatar if there is one, then send what changed. Stoat's
    /// answer updates the account at once and closes the dialog; a failure
    /// says why and keeps it open.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edited = self.edited(cx);
        if !self.can_save(&edited) {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let (client, file_server) = {
            let chat = chat.read(cx);
            let file_server = chat.store().media().file_server().map(str::to_string);
            (chat.client().clone(), file_server)
        };
        let upload = edited.avatar.new_file().map(|path| path.to_path_buf());
        let upload = match (upload, file_server) {
            (None, _) => None,
            (Some(path), Some(server)) => Some((path, server)),
            (Some(_), None) => {
                window.push_notification(
                    Notification::error("Couldn't upload: Stoat's file server isn't known yet."),
                    cx,
                );
                return;
            }
        };
        self.saving = Some(if upload.is_some() {
            Saving::Uploading
        } else {
            Saving::Sending
        });
        cx.notify();
        let (server, me) = (self.server.clone(), self.me.clone());
        cx.spawn_in(window, async move |this, cx| {
            let mut edited = edited;
            if let Some((path, file_server)) = upload {
                let uploaded = spawn_io(upload_image(
                    client.clone(),
                    file_server,
                    ImageKind::Avatar,
                    path,
                ));
                match uploaded.await {
                    Ok(Ok(id)) => edited.avatar = ProfileImage::Current(id),
                    failed => {
                        let failed = failed.map(|result| result.map(|_| ()));
                        this.update_in(cx, |this, window, cx| this.save_failed(failed, window, cx))
                            .ok();
                        return;
                    }
                }
            }
            let Ok(edit) = this.update(cx, |this, cx| {
                this.saving = Some(Saving::Sending);
                cx.notify();
                IdentityForm::changes(&this.loaded, &edited)
            }) else {
                return;
            };
            let request = spawn_io(async move { client.edit_member(&server, &me, &edit).await });
            let result = request.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(member)) => {
                    this.saving = None;
                    if let Some(chat) = this.chat.upgrade() {
                        chat.update(cx, |chat, cx| chat.identity_saved(member, cx));
                    }
                    window.close_dialog(cx);
                }
                failed => this.save_failed(failed.map(|result| result.map(|_| ())), window, cx),
            })
            .ok();
        })
        .detach();
    }

    fn save_failed(
        &mut self,
        result: Result<Result<(), ApiError>, impl std::fmt::Debug>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving = None;
        let text = match result {
            Ok(Err(err)) => {
                log::warn!("couldn't change identity on {}: {err:?}", self.server);
                identity_error_text(&err)
            }
            Ok(Ok(())) => return,
            Err(err) => {
                log::warn!("identity save task failed: {err:?}");
                "Couldn't change your identity on this server.".into()
            }
        };
        window.push_notification(Notification::error(text), cx);
        cx.notify();
    }
}

impl Render for IdentityEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let edited = self.edited(cx);
        let saving = self.saving.is_some();
        let (can_nickname, can_avatar) = self.permissions(cx);
        let can_save = self.can_save(&edited);
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);

        let avatar = self.chat.upgrade().map(|chat| {
            let chat = chat.read(cx);
            let store = chat.store();
            let name = store
                .user(&self.me)
                .map(crate::store::display_name)
                .unwrap_or_default();
            // Blank, the global avatar shows.
            let global = Preview::Url(store.user_avatar_url(&self.me));
            let preview = avatar_preview(
                "identity-avatar-preview",
                Preview::of(&self.avatar.image, ImageKind::Avatar, store.media(), global),
                name,
                cx,
            );
            render_image_row(
                "identity-avatar",
                "Server avatar",
                preview,
                &self.avatar,
                ImageKind::Avatar.limit(chat.image_limits()),
                !can_avatar || saving,
                cx.listener(|this, _, window, cx| this.pick_avatar(window, cx)),
                cx.listener(|this, _, _, cx| {
                    this.avatar.remove();
                    cx.notify();
                }),
                cx,
            )
        });
        let nickname = field(
            "Nickname",
            Input::new(&self.nickname)
                .disabled(!can_nickname || saving)
                .into_any_element(),
            edited.nickname.trim().chars().count(),
            NICKNAME_MAX,
            None,
            cx,
        );
        let pronouns = field(
            "Pronouns",
            Input::new(&self.pronouns)
                .disabled(saving)
                .into_any_element(),
            edited.pronouns.trim().chars().count(),
            PRONOUNS_MAX,
            None,
            cx,
        );
        let note = |text: &'static str| div().text_xs().text_color(muted).child(text);

        v_flex()
            .gap_4()
            .children(avatar)
            .when(!can_avatar, |this| {
                this.child(note("You can't change your avatar on this server."))
            })
            .child(nickname)
            .when(!can_nickname, |this| {
                this.child(note("You can't change your nickname on this server."))
            })
            .child(pronouns)
            .child(note(
                "Leave a field empty to use your profile's. Only this server sees these.",
            ))
            .when(
                !check_nickname(&edited.nickname).is_ok()
                    || !check_pronouns(&edited.pronouns).is_ok(),
                |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(danger)
                            .child("Shorten the fields in red to save."),
                    )
                },
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("identity-cancel")
                            .label("Cancel")
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("identity-save")
                            .label(match self.saving {
                                Some(Saving::Uploading) => "Uploading…",
                                _ => "Save",
                            })
                            .primary()
                            .loading(saving)
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}
