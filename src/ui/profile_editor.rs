//! The Profile page's form, for one account: avatar, banner, display name,
//! pronouns and bio, saved together with one `PATCH /users/@me` (see
//! [`crate::profile_edit`] for the rules and what's sent). New images are
//! uploaded first, unchanged.
//!
//! The settings window keeps one per account, so unsaved edits survive
//! switching accounts while it's open.

use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::{ApiError, StoatClient};
use crate::draft::decimal_size_label;
use crate::profile_edit::{
    BIO_MAX, DISPLAY_NAME_MAX, DISPLAY_NAME_MIN, FieldCheck, ImageLimits, PRONOUNS_MAX,
    ProfileForm, ProfileImage, check_bio, check_display_name, check_image, check_pronouns,
    save_error_text,
};
use crate::runtime::spawn_io;
use crate::ui::chat::{ChatView, avatar};

/// Shown when the bio and banner couldn't be loaded.
const PROFILE_FAILED_TEXT: &str = "Couldn't load your bio and banner.";

/// Rows the bio field grows between before it scrolls.
const BIO_ROWS: (usize, usize) = (3, 10);

/// The avatar preview's size.
pub(crate) const AVATAR_PREVIEW: f32 = 80.;
/// The banner preview: the web client's 232:100, at 1.5×.
const BANNER_PREVIEW: (f32, f32) = (348., 150.);

/// Where loading the bio and banner (`GET /users/{id}/profile`) is at.
/// Until it's loaded, Save stays disabled, so an empty bio can't overwrite
/// the real one.
enum ProfileLoad {
    Loading { _task: Task<()> },
    Loaded,
    Failed,
}

/// What a Save is doing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Saving {
    Uploading,
    Sending,
}

/// Which image a row edits: where it's uploaded, and its size limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageKind {
    Avatar,
    Banner,
}

impl ImageKind {
    /// The file server's tag (profile banners go in `backgrounds`, as the
    /// web client puts them; `banners` is for servers).
    pub(crate) fn tag(self) -> &'static str {
        match self {
            ImageKind::Avatar => "avatars",
            ImageKind::Banner => "backgrounds",
        }
    }

    pub(crate) fn limit(self, limits: ImageLimits) -> u64 {
        match self {
            ImageKind::Avatar => limits.avatar,
            ImageKind::Banner => limits.background,
        }
    }
}

/// One image in a form, and what's wrong with the last file picked for it.
#[derive(Debug, Clone, Default)]
pub(crate) struct ImageSlot {
    pub image: ProfileImage,
    pub problem: Option<String>,
}

impl ImageSlot {
    fn new(image: ProfileImage) -> Self {
        Self {
            image,
            problem: None,
        }
    }

    /// Take a picked file, or say why it can't be used (keeping the image
    /// as it was, with Save disabled until it's sorted out).
    pub(crate) fn pick(&mut self, path: PathBuf, limit: u64) {
        let checked = std::fs::metadata(&path)
            .map_err(|err| format!("Couldn't read the file ({err})."))
            .and_then(|meta| {
                check_image(&path, meta.len(), limit).map_err(|problem| problem.to_string())
            });
        match checked {
            Ok(_) => *self = Self::new(ProfileImage::New(path)),
            Err(problem) => self.problem = Some(problem),
        }
    }

    pub(crate) fn remove(&mut self) {
        *self = Self::new(ProfileImage::None);
    }
}

/// Ask for one image file, and hand its path to `picked`.
pub(crate) fn prompt_for_image<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    picked: impl FnOnce(&mut V, PathBuf, &mut Context<V>) + 'static,
) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Choose".into()),
    });
    cx.spawn_in(window, async move |this, cx| {
        let Ok(Ok(Some(mut paths))) = paths.await else {
            return;
        };
        let Some(path) = paths.pop() else {
            return;
        };
        this.update(cx, |this, cx| {
            picked(this, path, cx);
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// Upload a picked image as it is, for `kind`. Returns its file ID.
pub(crate) async fn upload_image(
    client: StoatClient,
    file_server: String,
    kind: ImageKind,
    path: PathBuf,
) -> Result<String, ApiError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
    let content_type = check_image(&path, 0, u64::MAX).unwrap_or("application/octet-stream");
    client
        .upload_file(
            &file_server,
            kind.tag(),
            &name,
            content_type,
            &path,
            |_, _| {},
        )
        .await
}

/// What an image preview draws.
pub(crate) enum Preview {
    /// An image on Stoat (or the default avatar).
    Url(String),
    /// A picked file, not uploaded yet.
    File(PathBuf),
    /// Nothing: the banner's empty strip.
    Empty,
}

impl Preview {
    /// The preview for `image`: a file on the file server by its ID, a
    /// picked file, or else `fallback`.
    pub(crate) fn of(
        image: &ProfileImage,
        kind: ImageKind,
        media: &crate::media::MediaUrls,
        fallback: Preview,
    ) -> Preview {
        match image {
            ProfileImage::New(path) => Preview::File(path.clone()),
            ProfileImage::Current(id) => media
                .file_by_id(kind.tag(), id)
                .map_or(fallback, Preview::Url),
            ProfileImage::None => fallback,
        }
    }
}

/// An avatar preview: a centre-cropped circle, as every client shows it.
pub(crate) fn avatar_preview(
    id: &'static str,
    preview: Preview,
    name: &str,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    match preview {
        Preview::Url(url) => avatar(
            id,
            Some(url),
            name,
            AVATAR_PREVIEW,
            theme.secondary,
            theme.secondary_foreground,
        ),
        Preview::File(path) => {
            let label = file_label(&path);
            img(path)
                .id(id)
                .size(px(AVATAR_PREVIEW))
                .flex_none()
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || unpreviewable(label.clone()))
                .into_any_element()
        }
        Preview::Empty => avatar(
            id,
            None,
            name,
            AVATAR_PREVIEW,
            theme.secondary,
            theme.secondary_foreground,
        ),
    }
}

/// A banner preview: a centre-cropped 232:100 strip.
fn banner_preview(preview: Preview, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (w, h) = BANNER_PREVIEW;
    let frame = |el: Div| {
        el.w(px(w))
            .h(px(h))
            .flex_none()
            .rounded_md()
            .overflow_hidden()
    };
    let image = |source: ImageSource, fallback: SharedString| {
        img(source)
            .id("profile-banner-preview")
            .size_full()
            .object_fit(ObjectFit::Cover)
            .with_fallback(move || unpreviewable(fallback.clone()))
            .into_any_element()
    };
    match preview {
        Preview::Url(url) => frame(div())
            .child(image(SharedUri::from(url).into(), "Banner".into()))
            .into_any_element(),
        Preview::File(path) => {
            let label = file_label(&path);
            frame(div())
                .child(image(path.into(), label))
                .into_any_element()
        }
        Preview::Empty => frame(div())
            .bg(theme.secondary)
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child("No banner")
            .into_any_element(),
    }
}

/// A picked file's name, for a preview that can't draw it.
fn file_label(path: &Path) -> SharedString {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .into()
}

/// Shown in place of an image that can't be drawn here (it's uploaded all
/// the same).
fn unpreviewable(label: SharedString) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_1()
        .text_xs()
        .text_center()
        .child(label)
        .into_any_element()
}

/// A row for one image: its preview, Change… and Remove, and what's wrong
/// with the last file picked, or the types and size allowed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_image_row(
    id: &'static str,
    label: &'static str,
    preview: AnyElement,
    slot: &ImageSlot,
    limit: u64,
    disabled: bool,
    on_change: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_remove: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let note = match &slot.problem {
        Some(problem) => div().text_color(theme.danger).child(problem.clone()),
        None => div().text_color(theme.muted_foreground).child(format!(
            "PNG, JPEG, GIF or WebP, up to {}.",
            decimal_size_label(limit)
        )),
    };
    v_flex()
        .gap_1()
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
        .child(
            h_flex().gap_4().items_center().child(preview).child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new(SharedString::from(format!("{id}-change")))
                                    .label("Change…")
                                    .outline()
                                    .small()
                                    .disabled(disabled)
                                    .on_click(on_change),
                            )
                            .child(
                                Button::new(SharedString::from(format!("{id}-remove")))
                                    .label("Remove")
                                    .ghost()
                                    .small()
                                    .disabled(disabled || slot.image == ProfileImage::None)
                                    .on_click(on_remove),
                            ),
                    )
                    .child(note.text_xs()),
            ),
        )
        .into_any_element()
}

/// One account's profile form.
pub struct ProfileEditor {
    /// The account's chat: its client, store and cached profiles.
    chat: WeakEntity<ChatView>,
    me: String,
    display_name: Entity<InputState>,
    pronouns: Entity<InputState>,
    bio: Entity<TextareaState>,
    avatar: ImageSlot,
    banner: ImageSlot,
    /// What Stoat has, which the form was filled with (Reset goes back to it).
    loaded: ProfileForm,
    /// The display name, pronouns and avatar are loaded (from the account's
    /// store).
    user_loaded: bool,
    profile: ProfileLoad,
    saving: Option<Saving>,
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
            avatar: ImageSlot::default(),
            banner: ImageSlot::default(),
            loaded: ProfileForm::default(),
            user_loaded: false,
            profile: ProfileLoad::Failed,
            saving: None,
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

    /// Whether this is the form for `chat` (an account signed in again has
    /// a new one).
    pub fn is_for(&self, chat: &Entity<ChatView>) -> bool {
        self.chat.entity_id() == chat.entity_id()
    }

    /// Fill the display name, pronouns and avatar from the account's store,
    /// once it knows the user. Returns whether it did.
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
        let avatar = user.avatar.as_ref().map_or(ProfileImage::None, |file| {
            ProfileImage::Current(file.id.clone())
        });
        self.loaded.display_name = name.clone();
        self.loaded.pronouns = pronouns.clone();
        self.loaded.avatar = avatar.clone();
        self.avatar = ImageSlot::new(avatar);
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

    /// Fetch the bio and banner, which aren't part of the user.
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
                        let banner = profile
                            .background
                            .as_ref()
                            .map_or(ProfileImage::None, |file| {
                                ProfileImage::Current(file.id.clone())
                            });
                        this.loaded.bio = bio.clone();
                        this.loaded.banner = banner.clone();
                        this.banner = ImageSlot::new(banner);
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

    /// The form as edited.
    fn edited(&self, cx: &App) -> ProfileForm {
        ProfileForm {
            display_name: self.display_name.read(cx).value().to_string(),
            pronouns: self.pronouns.read(cx).value().to_string(),
            bio: self.bio.read(cx).value().to_string(),
            avatar: self.avatar.image.clone(),
            banner: self.banner.image.clone(),
        }
    }

    fn loaded(&self) -> bool {
        self.user_loaded && matches!(self.profile, ProfileLoad::Loaded)
    }

    /// Whether Save can send the form: loaded, every field and image fine,
    /// something changed, and not already saving.
    fn can_save(&self, edited: &ProfileForm) -> bool {
        self.loaded()
            && self.saving.is_none()
            && edited.is_valid()
            && self.avatar.problem.is_none()
            && self.banner.problem.is_none()
            && ProfileForm::has_changes(&self.loaded, edited)
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
        self.avatar = ImageSlot::new(loaded.avatar);
        self.banner = ImageSlot::new(loaded.banner);
        cx.notify();
    }

    fn slot(&mut self, kind: ImageKind) -> &mut ImageSlot {
        match kind {
            ImageKind::Avatar => &mut self.avatar,
            ImageKind::Banner => &mut self.banner,
        }
    }

    /// Change… on an image row.
    fn pick_image(&mut self, kind: ImageKind, window: &mut Window, cx: &mut Context<Self>) {
        let limits = self
            .chat
            .upgrade()
            .map(|chat| chat.read(cx).image_limits())
            .unwrap_or_default();
        prompt_for_image(window, cx, move |this: &mut Self, path, _| {
            this.slot(kind).pick(path, kind.limit(limits));
        });
    }

    /// Send what changed: upload any new images first, then one `PATCH`
    /// with their IDs, only if every upload worked. Stoat's answer updates
    /// the account at once; a failure says why in a toast, and the form
    /// keeps what was edited.
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
        let uploads: Vec<(ImageKind, PathBuf)> = [
            (ImageKind::Avatar, edited.avatar.new_file()),
            (ImageKind::Banner, edited.banner.new_file()),
        ]
        .into_iter()
        .filter_map(|(kind, path)| Some((kind, path?.to_path_buf())))
        .collect();
        let file_server = match (file_server, uploads.is_empty()) {
            (Some(server), _) => server,
            (None, true) => String::new(),
            (None, false) => {
                window.push_notification(
                    Notification::error("Couldn't upload: Stoat's file server isn't known yet."),
                    cx,
                );
                return;
            }
        };
        self.saving = Some(if uploads.is_empty() {
            Saving::Sending
        } else {
            Saving::Uploading
        });
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let mut edited = edited;
            for (kind, path) in uploads {
                let upload = spawn_io(upload_image(
                    client.clone(),
                    file_server.clone(),
                    kind,
                    path,
                ));
                let id = match upload.await {
                    Ok(Ok(id)) => id,
                    failed => {
                        let failed = failed.map(|result| result.map(|_| ()));
                        this.update_in(cx, |this, window, cx| this.save_failed(failed, window, cx))
                            .ok();
                        return;
                    }
                };
                match kind {
                    ImageKind::Avatar => edited.avatar = ProfileImage::Current(id),
                    ImageKind::Banner => edited.banner = ProfileImage::Current(id),
                }
            }
            let Ok(edit) = this.update(cx, |this, cx| {
                this.saving = Some(Saving::Sending);
                cx.notify();
                ProfileForm::changes(&this.loaded, &edited)
            }) else {
                return;
            };
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
            let request = spawn_io({
                let client = client.clone();
                async move { client.edit_user(&edit).await }
            });
            let result = request.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(me)) => this.saved(me, edited, bio, window, cx),
                failed => this.save_failed(failed.map(|result| result.map(|_| ())), window, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Stoat took the change: it's what's loaded now, and the account shows
    /// it at once.
    fn saved(
        &mut self,
        me: crate::api::models::User,
        sent: ProfileForm,
        bio: Option<Option<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving = None;
        let banner_changed = sent.banner != self.loaded.banner;
        self.loaded = ProfileForm {
            display_name: me.display_name.clone().unwrap_or_default(),
            pronouns: me.pronouns.clone().unwrap_or_default(),
            bio: match &bio {
                Some(bio) => bio.clone().unwrap_or_default(),
                None => self.loaded.bio.clone(),
            },
            avatar: me.avatar.as_ref().map_or(ProfileImage::None, |file| {
                ProfileImage::Current(file.id.clone())
            }),
            banner: sent.banner,
        };
        self.reset(window, cx);
        if let Some(chat) = self.chat.upgrade() {
            chat.update(cx, |chat, cx| {
                chat.own_profile_saved(me, bio, banner_changed, cx)
            });
        }
        window.push_notification(Notification::success("Profile saved."), cx);
    }

    /// An upload or the `PATCH` failed: say why, and keep what was edited.
    fn save_failed(
        &mut self,
        result: Result<Result<(), ApiError>, impl std::fmt::Debug>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving = None;
        let text = match result {
            Ok(Err(err)) => {
                log::warn!("couldn't save the profile: {err:?}");
                save_error_text(&err)
            }
            Ok(Ok(())) => return,
            Err(err) => {
                log::warn!("profile save task failed: {err:?}");
                "Couldn't save your profile.".into()
            }
        };
        window.push_notification(Notification::error(text), cx);
        cx.notify();
    }

    /// `@username#1234`, for the line saying which account is edited.
    fn handle(&self, cx: &App) -> Option<String> {
        let chat = self.chat.upgrade()?;
        let user = chat.read(cx).store().user(&self.me)?;
        Some(format!("@{}#{}", user.username, user.discriminator))
    }

    fn render_images(&self, cx: &mut Context<Self>) -> Option<[AnyElement; 2]> {
        let chat = self.chat.upgrade()?;
        let chat = chat.read(cx);
        let store = chat.store();
        let limits = chat.image_limits();
        let media = store.media();
        let name = store
            .user(&self.me)
            .map(crate::store::display_name)
            .unwrap_or_default();
        let default_avatar = Preview::Url(media.default_avatar(&self.me));
        let avatar = avatar_preview(
            "profile-avatar-preview",
            Preview::of(&self.avatar.image, ImageKind::Avatar, media, default_avatar),
            name,
            cx,
        );
        let banner = banner_preview(
            Preview::of(&self.banner.image, ImageKind::Banner, media, Preview::Empty),
            cx,
        );
        Some([
            self.image_row(ImageKind::Avatar, avatar, limits, cx),
            self.image_row(ImageKind::Banner, banner, limits, cx),
        ])
    }

    /// The avatar's or banner's row. The banner waits for the profile to
    /// load, as the bio does.
    fn image_row(
        &self,
        kind: ImageKind,
        preview: AnyElement,
        limits: ImageLimits,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (id, label, slot, loaded) = match kind {
            ImageKind::Avatar => ("profile-avatar", "Avatar", &self.avatar, self.user_loaded),
            ImageKind::Banner => (
                "profile-banner",
                "Banner",
                &self.banner,
                matches!(self.profile, ProfileLoad::Loaded),
            ),
        };
        render_image_row(
            id,
            label,
            preview,
            slot,
            kind.limit(limits),
            !loaded || self.saving.is_some(),
            cx.listener(move |this, _, window, cx| this.pick_image(kind, window, cx)),
            cx.listener(move |this, _, _, cx| {
                this.slot(kind).remove();
                cx.notify();
            }),
            cx,
        )
    }
}

impl Render for ProfileEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let edited = self.edited(cx);
        let saving = self.saving.is_some();
        let can_edit = self.user_loaded && !saving;
        let bio_ready = matches!(self.profile, ProfileLoad::Loaded) && !saving;
        let changed = ProfileForm::has_changes(&self.loaded, &edited);
        let can_save = self.can_save(&edited);
        let muted = cx.theme().muted_foreground;
        let editing = match self.handle(cx) {
            Some(handle) => format!("Editing {handle}"),
            None => "Loading your account…".to_string(),
        };
        let images = self.render_images(cx);

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
            .children(images.into_iter().flatten())
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
                            .disabled(!changed || saving)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    )
                    .child(
                        Button::new("profile-save")
                            .label(match self.saving {
                                Some(Saving::Uploading) => "Uploading…",
                                _ => "Save",
                            })
                            .primary()
                            .small()
                            .loading(saving)
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
pub(crate) fn field(
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

/// The Profile page's "Server identities": the account's servers, each
/// opening its identity dialog in the main window.
pub fn render_server_identities(chat: &Entity<ChatView>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let view = chat.read(cx);
    let media = view.store().media();
    let servers = view.store().servers();
    if servers.is_empty() {
        return div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child("You aren't in any servers.")
            .into_any_element();
    }
    v_flex()
        .w_full()
        .gap_1()
        .children(servers.into_iter().map(|server| {
            let icon = server_icon(&server, media, cx);
            let (chat, id) = (chat.downgrade(), server.id.clone());
            h_flex()
                .id(SharedString::from(format!("identity-{}", server.id)))
                .w_full()
                .gap_3()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .hover(|el| el.bg(theme.secondary))
                .child(icon)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .child(server.name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("Edit identity…"),
                )
                .on_click(move |_, _, cx| {
                    open_identity_in_main_window(chat.clone(), id.clone(), cx)
                })
        }))
        .into_any_element()
}

/// A server's icon in the identities list, or its initials.
fn server_icon(
    server: &crate::store::ServerEntry,
    media: &crate::media::MediaUrls,
    cx: &App,
) -> AnyElement {
    const SIZE: f32 = 28.;
    let theme = cx.theme();
    let (bg, fg) = (theme.secondary, theme.secondary_foreground);
    let initials = server.initials.clone();
    let placeholder = move || {
        div()
            .size(px(SIZE))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .text_color(fg)
            .text_xs()
            .child(initials.clone())
            .into_any_element()
    };
    match server.icon.as_ref().and_then(|icon| media.file(icon)) {
        Some(url) => img(SharedUri::from(url))
            .id(SharedString::from(format!("identity-icon-{}", server.id)))
            .size(px(SIZE))
            .flex_none()
            .rounded_full()
            .object_fit(ObjectFit::Cover)
            .with_fallback(placeholder)
            .into_any_element(),
        None => placeholder(),
    }
}

/// Open `server`'s identity dialog in the account's own window (the main
/// window), and bring it forward. Deferred: the click arrives while GPUI
/// is updating the settings window.
fn open_identity_in_main_window(chat: WeakEntity<ChatView>, server: String, cx: &mut App) {
    cx.defer(move |cx| {
        let Some(chat) = chat.upgrade() else {
            return;
        };
        let handle = chat.read(cx).window_handle();
        handle
            .update(cx, |_, window, cx| {
                window.activate_window();
                chat.update(cx, |chat, cx| chat.open_identity_editor(server, window, cx));
            })
            .ok();
        cx.activate(true);
    });
}
