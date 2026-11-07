//! The Profile page's form, for one account: display name, pronouns and
//! bio, saved together with one `PATCH /users/@me` (see
//! [`crate::profile_edit`] for the rules and what's sent).
//!
//! The settings window keeps one per account, so unsaved edits survive
//! switching accounts while it's open.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::profile_edit::{
    BIO_MAX, DISPLAY_NAME_MAX, DISPLAY_NAME_MIN, FieldCheck, PRONOUNS_MAX, ProfileForm, check_bio,
    check_display_name, check_pronouns, is_empty_edit, save_error_text,
};
use crate::runtime::spawn_io;
use crate::ui::chat::ChatView;

/// Shown when the bio and banner couldn't be loaded.
const PROFILE_FAILED_TEXT: &str = "Couldn't load your bio and banner.";

/// Rows the bio field grows between before it scrolls.
const BIO_ROWS: (usize, usize) = (3, 10);

/// Where loading the bio and banner (`GET /users/{id}/profile`) is at.
/// Until it's loaded, Save stays disabled, so an empty bio can't overwrite
/// the real one.
enum ProfileLoad {
    Loading { _task: Task<()> },
    Loaded,
    Failed,
}

/// One account's profile form.
pub struct ProfileEditor {
    /// The account's chat: its client, store and cached profiles.
    chat: WeakEntity<ChatView>,
    me: String,
    display_name: Entity<InputState>,
    pronouns: Entity<InputState>,
    bio: Entity<TextareaState>,
    /// What Stoat has, which the form was filled with (Reset goes back to it).
    loaded: ProfileForm,
    /// The display name and pronouns are loaded (from the account's store).
    user_loaded: bool,
    profile: ProfileLoad,
    saving: bool,
    _inputs: [Subscription; 3],
    /// Watches the chat until the user's account has loaded.
    _waiting: Option<Subscription>,
}

impl ProfileEditor {
    pub fn new(chat: Entity<ChatView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let display_name = cx.new(|cx| InputState::new(window, cx));
        let pronouns = cx.new(|cx| InputState::new(window, cx).placeholder("they/them"));
        let bio = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(BIO_ROWS.0, BIO_ROWS.1)
                .placeholder("Tell people about yourself")
        });
        // Counters and Save follow what's typed.
        let inputs = [
            cx.subscribe(&display_name, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&pronouns, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&bio, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        let me = chat.read(cx).store().me().to_string();
        let mut this = Self {
            chat: chat.downgrade(),
            me,
            display_name,
            pronouns,
            bio,
            loaded: ProfileForm::default(),
            user_loaded: false,
            profile: ProfileLoad::Failed,
            saving: false,
            _inputs: inputs,
            _waiting: None,
        };
        if !this.fill_user(window, cx) {
            this._waiting = Some(cx.observe_in(&chat, window, |this, _, window, cx| {
                if this.fill_user(window, cx) {
                    this._waiting = None;
                }
            }));
        }
        this.load_profile(window, cx);
        this
    }

    /// Fill the display name and pronouns from the account's store, once
    /// it knows the user. Returns whether it did.
    fn fill_user(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(chat) = self.chat.upgrade() else {
            return false;
        };
        let Some(user) = chat.read(cx).store().user(&self.me) else {
            return false;
        };
        let (name, pronouns, username) = (
            user.display_name.clone().unwrap_or_default(),
            user.pronouns.clone().unwrap_or_default(),
            user.username.clone(),
        );
        self.loaded.display_name = name.clone();
        self.loaded.pronouns = pronouns.clone();
        self.display_name.update(cx, |input, cx| {
            input.set_placeholder(username, window, cx);
            input.set_value(name, window, cx);
        });
        self.pronouns
            .update(cx, |input, cx| input.set_value(pronouns, window, cx));
        self.user_loaded = true;
        cx.notify();
        true
    }

    /// Fetch the bio (and banner), which aren't part of the user.
    fn load_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let request = spawn_io({
            let (client, me) = (chat.read(cx).client().clone(), self.me.clone());
            async move { client.fetch_profile(&me).await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(Ok(profile)) => {
                        let bio = profile.content.clone().unwrap_or_default();
                        this.loaded.bio = bio.clone();
                        this.bio
                            .update(cx, |input, cx| input.set_value(bio, window, cx));
                        if let Some(chat) = this.chat.upgrade() {
                            let me = this.me.clone();
                            chat.update(cx, |chat, _| chat.cache_profile(me, profile));
                        }
                        this.profile = ProfileLoad::Loaded;
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't load the user's own profile: {err}");
                        this.profile = ProfileLoad::Failed;
                    }
                    Err(err) => {
                        log::warn!("own profile task failed: {err:?}");
                        this.profile = ProfileLoad::Failed;
                    }
                }
                cx.notify();
            })
            .ok();
        });
        self.profile = ProfileLoad::Loading { _task: task };
        cx.notify();
    }

    /// The form as typed.
    fn edited(&self, cx: &App) -> ProfileForm {
        ProfileForm {
            display_name: self.display_name.read(cx).value().to_string(),
            pronouns: self.pronouns.read(cx).value().to_string(),
            bio: self.bio.read(cx).value().to_string(),
        }
    }

    fn loaded(&self) -> bool {
        self.user_loaded && matches!(self.profile, ProfileLoad::Loaded)
    }

    /// Whether anything typed differs from what Stoat has.
    fn has_changes(&self, cx: &App) -> bool {
        !is_empty_edit(&ProfileForm::changes(&self.loaded, &self.edited(cx)))
    }

    /// Put the form back to what was loaded.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let loaded = self.loaded.clone();
        self.display_name.update(cx, |input, cx| {
            input.set_value(loaded.display_name, window, cx)
        });
        self.pronouns
            .update(cx, |input, cx| input.set_value(loaded.pronouns, window, cx));
        self.bio
            .update(cx, |input, cx| input.set_value(loaded.bio, window, cx));
        cx.notify();
    }

    /// Send what changed. Stoat's answer updates the account at once; a
    /// failure says why in a toast, and the form keeps what was typed.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edited = self.edited(cx);
        if self.saving || !self.loaded() || !edited.is_valid() {
            return;
        }
        let edit = ProfileForm::changes(&self.loaded, &edited);
        if is_empty_edit(&edit) {
            return;
        }
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        // The bio as Stoat will have it, if it changes.
        let bio = match &edit.profile {
            Some(profile) if profile.content.is_some() => Some(profile.content.clone()),
            _ if edit
                .remove
                .contains(&crate::api::models::FieldsUser::ProfileContent) =>
            {
                Some(None)
            }
            _ => None,
        };
        self.saving = true;
        cx.notify();
        let request = spawn_io({
            let client = chat.read(cx).client().clone();
            async move { client.edit_user(&edit).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(Ok(me)) => {
                        this.loaded.display_name = me.display_name.clone().unwrap_or_default();
                        this.loaded.pronouns = me.pronouns.clone().unwrap_or_default();
                        if let Some(bio) = &bio {
                            this.loaded.bio = bio.clone().unwrap_or_default();
                        }
                        this.reset(window, cx);
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.own_profile_saved(me, bio, cx));
                        }
                        window.push_notification(Notification::success("Profile saved."), cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't save the profile: {err:?}");
                        window.push_notification(Notification::error(save_error_text(&err)), cx);
                    }
                    Err(err) => {
                        log::warn!("profile save task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't save your profile."),
                            cx,
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Whether this is the form for `chat` (an account signed in again has
    /// a new one).
    pub fn is_for(&self, chat: &Entity<ChatView>) -> bool {
        self.chat.entity_id() == chat.entity_id()
    }

    /// `@username#1234`, for the line saying which account is edited.
    fn handle(&self, cx: &App) -> Option<String> {
        let chat = self.chat.upgrade()?;
        let user = chat.read(cx).store().user(&self.me)?;
        Some(format!("@{}#{}", user.username, user.discriminator))
    }
}

impl Render for ProfileEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let edited = self.edited(cx);
        let can_edit = self.user_loaded && !self.saving;
        let bio_ready = matches!(self.profile, ProfileLoad::Loaded) && !self.saving;
        let changed = self.has_changes(cx);
        let can_save = self.loaded() && !self.saving && edited.is_valid() && changed;
        let muted = cx.theme().muted_foreground;
        let editing = match self.handle(cx) {
            Some(handle) => format!("Editing {handle}"),
            None => "Loading your account…".to_string(),
        };

        let display_name = field(
            "Display name",
            Input::new(&self.display_name)
                .disabled(!can_edit)
                .into_any_element(),
            edited.display_name.trim().chars().count(),
            DISPLAY_NAME_MAX,
            problem(check_display_name(&edited.display_name)),
            cx,
        );
        let pronouns = field(
            "Pronouns",
            Input::new(&self.pronouns)
                .disabled(!can_edit)
                .into_any_element(),
            edited.pronouns.trim().chars().count(),
            PRONOUNS_MAX,
            problem(check_pronouns(&edited.pronouns)),
            cx,
        );
        let bio = field(
            "Bio",
            Textarea::new(&self.bio)
                .disabled(!bio_ready)
                .into_any_element(),
            edited.bio.chars().count(),
            BIO_MAX,
            problem(check_bio(&edited.bio)),
            cx,
        );

        v_flex()
            .w_full()
            .gap_4()
            .child(div().text_sm().text_color(muted).child(editing))
            .child(display_name)
            .child(pronouns)
            .child(bio)
            .when(matches!(self.profile, ProfileLoad::Failed), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(PROFILE_FAILED_TEXT)
                        .child(
                            Button::new("profile-retry")
                                .label("Retry")
                                .outline()
                                .xsmall()
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.load_profile(window, cx)
                                    }),
                                ),
                        ),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Your username can't be changed here yet."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("profile-reset")
                            .label("Reset")
                            .outline()
                            .small()
                            .disabled(!changed || self.saving)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    )
                    .child(
                        Button::new("profile-save")
                            .label("Save")
                            .primary()
                            .small()
                            .loading(self.saving)
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}

/// What's wrong with a field, under it.
fn problem(check: FieldCheck<'_>) -> Option<String> {
    match check {
        FieldCheck::Ok(_) | FieldCheck::Clear => None,
        // Only a display name has a minimum above one.
        FieldCheck::TooShort(_) => Some(format!("At least {DISPLAY_NAME_MIN} characters.")),
        // The count turns red.
        FieldCheck::TooLong(_) => None,
        FieldCheck::Invalid(reason) => Some(reason.to_string()),
    }
}

/// A labelled field with its character count, red when over `max`, and
/// what's wrong with it underneath.
fn field(
    label: &'static str,
    input: AnyElement,
    count: usize,
    max: usize,
    problem: Option<String>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
                .child(
                    div()
                        .text_xs()
                        .text_color(if count > max {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(format!("{count}/{max}")),
                ),
        )
        .child(input)
        .children(problem.map(|problem| div().text_xs().text_color(theme.danger).child(problem)))
        .into_any_element()
}
