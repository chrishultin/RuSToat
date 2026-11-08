//! "Create a server": name it, and Stoat makes it with one text channel.
//! "Create channel": name it and pick text or voice, and it's made in the
//! category it was asked for from. "Create a new category" and "Rename
//! category": a name, and the server's categories are sent again with it.
//! The naming rules, the category edits and the error texts are in
//! [`crate::server_edit`].

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{ActiveTheme, Disableable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::StoatClient;
use crate::invites::is_official_app;
use crate::runtime::spawn_io;
use crate::server_edit::{
    AUP_URL, NAME_MAX, categories_error_text, create_channel_error_text, create_server_error_text,
    new_category_id, valid_name, with_category_added, with_category_renamed,
};
use crate::ui::chat::ChatView;

/// The dialog's width (as Join a server's).
pub const CREATE_SERVER_DIALOG_WIDTH: f32 = 420.;

/// Create channel's width (the same).
pub const CREATE_CHANNEL_DIALOG_WIDTH: f32 = 420.;

/// Create and Rename category's width (the same).
pub const NAME_DIALOG_WIDTH: f32 = 420.;

/// The line under the name on the official instance, and its link.
const AUP_TEXT: &str = "By creating this server, you agree to the Acceptable Use Policy.";
const AUP_LINK: &str = "Acceptable Use Policy";

/// The Create server dialog's content, for one account.
pub struct CreateServerDialog {
    chat: WeakEntity<ChatView>,
    client: StoatClient,
    /// On the official instance, whose Acceptable Use Policy applies.
    official: bool,
    input: Entity<InputState>,
    creating: bool,
    error: Option<String>,
    _input: Subscription,
}

impl CreateServerDialog {
    /// The dialog for `chat`'s account.
    pub fn new(
        chat: &ChatView,
        weak: WeakEntity<ChatView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.create(window, cx),
                _ => {}
            });
        Self {
            chat: weak,
            client: chat.client().clone(),
            official: chat.web_app().is_some_and(is_official_app),
            input,
            creating: false,
            error: None,
            _input: subscription,
        }
    }

    /// The field, to focus when the dialog opens.
    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Create the server with the typed name. Success closes the dialog
    /// and opens the server; a refusal says why and keeps it.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating {
            return;
        }
        let Some(name) = valid_name(&self.input.read(cx).value()) else {
            return;
        };
        self.creating = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let client = self.client.clone();
            async move { client.create_server(&name).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match result {
                    Ok(Ok(created)) => {
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.server_created(created, cx));
                        }
                        window.close_dialog(cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't create a server: {err:?}");
                        this.error = Some(create_server_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("server create task failed: {err:?}");
                        this.error = Some("Couldn't reach Stoat. Try again.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// "By creating this server, you agree to the Acceptable Use Policy.",
    /// with the policy's name as a link.
    fn render_aup(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let start = AUP_TEXT.find(AUP_LINK).unwrap_or(0);
        let link = start..start + AUP_LINK.len();
        let text = StyledText::new(AUP_TEXT).with_highlights([(
            link.clone(),
            HighlightStyle {
                color: Some(theme.link),
                ..Default::default()
            },
        )]);
        div().text_xs().text_color(theme.muted_foreground).child(
            InteractiveText::new("create-server-aup", text)
                .on_click(vec![link], |_, _, cx| cx.open_url(AUP_URL)),
        )
    }
}

impl Render for CreateServerDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let valid = valid_name(&self.input.read(cx).value()).is_some();
        let entity = cx.entity().downgrade();

        v_flex()
            .gap_3()
            .child(name_field(
                "Server name",
                &self.input,
                self.creating,
                self.error.clone(),
                cx,
            ))
            .when(self.official, |this| this.child(self.render_aup(cx)))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-create-server")
                            .outline()
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("create-server-submit")
                            .primary()
                            .label("Create")
                            .loading(self.creating)
                            .disabled(self.creating || !valid)
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| this.create(window, cx)).ok();
                            }),
                    ),
            )
    }
}

/// A name field as these dialogs show it: `label` above the field, a
/// "{n}/32" counter under it (red past 32), and `error` below that.
fn name_field(
    label: &'static str,
    input: &Entity<InputState>,
    busy: bool,
    error: Option<String>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let (muted, danger) = (theme.muted_foreground, theme.danger);
    let count = input.read(cx).value().trim().chars().count();
    v_flex()
        .gap_1()
        .child(field_label(label, cx))
        .child(Input::new(input).disabled(busy))
        .child(
            div()
                .text_xs()
                .text_right()
                .text_color(if count > NAME_MAX { danger } else { muted })
                .child(format!("{count}/{NAME_MAX}")),
        )
        .children(error.map(|text| div().text_xs().text_color(danger).child(text)))
}

/// A field's label, above it.
fn field_label(label: &'static str, cx: &App) -> Div {
    div()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

/// The Create channel dialog's content, for one server of one account.
pub struct CreateChannelDialog {
    chat: WeakEntity<ChatView>,
    client: StoatClient,
    server: String,
    /// The category it goes in, as its ID and title; `None` leaves it
    /// uncategorized.
    category: Option<(String, String)>,
    input: Entity<InputState>,
    /// Voice Channel is chosen, rather than Text Channel.
    voice: bool,
    creating: bool,
    error: Option<String>,
    _input: Subscription,
}

impl CreateChannelDialog {
    /// The dialog for a channel in `server`, put in `category` (ID and
    /// title) if there's one.
    pub fn new(
        chat: &ChatView,
        weak: WeakEntity<ChatView>,
        server: String,
        category: Option<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.create(window, cx),
                _ => {}
            });
        Self {
            chat: weak,
            client: chat.client().clone(),
            server,
            category,
            input,
            voice: false,
            creating: false,
            error: None,
            _input: subscription,
        }
    }

    /// The field, to focus when the dialog opens.
    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Create the channel with the typed name. Success closes the dialog
    /// and opens the channel (moving it into its category); a refusal says
    /// why and keeps it.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating {
            return;
        }
        let Some(name) = valid_name(&self.input.read(cx).value()) else {
            return;
        };
        self.creating = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let (client, server, voice) = (self.client.clone(), self.server.clone(), self.voice);
            async move { client.create_channel(&server, &name, voice).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match result {
                    Ok(Ok(channel)) => {
                        let category = this.category.clone();
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| {
                                chat.channel_created(channel, category, window, cx)
                            });
                        }
                        window.close_dialog(cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't create a channel in {}: {err:?}", this.server);
                        this.error = Some(create_channel_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("channel create task failed: {err:?}");
                        this.error = Some("Couldn't reach Stoat. Try again.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for CreateChannelDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let valid = valid_name(&self.input.read(cx).value()).is_some();
        let entity = cx.entity().downgrade();
        let kinds = entity.clone();

        v_flex()
            .gap_3()
            .when_some(self.category.as_ref(), |this, (_, title)| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(format!("in {}", title.to_uppercase())),
                )
            })
            .child(name_field(
                "Channel name",
                &self.input,
                self.creating,
                self.error.clone(),
                cx,
            ))
            .child(
                v_flex()
                    .gap_1()
                    .child(field_label("Channel type", cx))
                    .child(
                        RadioGroup::vertical("create-channel-type")
                            .flex_none()
                            .child(Radio::new("text").label("Text Channel"))
                            .child(Radio::new("voice").label("Voice Channel"))
                            .selected_index(Some(usize::from(self.voice)))
                            .disabled(self.creating)
                            .on_change(move |index, _, cx| {
                                kinds
                                    .update(cx, |this, cx| {
                                        this.voice = *index == 1;
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-create-channel")
                            .outline()
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("create-channel-submit")
                            .primary()
                            .label("Create")
                            .loading(self.creating)
                            .disabled(self.creating || !valid)
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| this.create(window, cx)).ok();
                            }),
                    ),
            )
    }
}

/// The content of "Create a new category" and "Rename category", for one
/// server of one account: a name, and the server's categories sent again
/// with it.
pub struct NameDialog {
    chat: WeakEntity<ChatView>,
    client: StoatClient,
    server: String,
    /// The category being renamed; `None` creates one.
    renaming: Option<String>,
    input: Entity<InputState>,
    saving: bool,
    error: Option<String>,
    _input: Subscription,
}

impl NameDialog {
    /// The dialog for a category of `server`: a new one, or `renaming`
    /// (its ID and current title, which the field starts with).
    pub fn new(
        chat: &ChatView,
        weak: WeakEntity<ChatView>,
        server: String,
        renaming: Option<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (renaming, title) = match renaming {
            Some((id, title)) => (Some(id), title),
            None => (None, String::new()),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(title));
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.save(window, cx),
                _ => {}
            });
        Self {
            chat: weak,
            client: chat.client().clone(),
            server,
            renaming,
            input,
            saving: false,
            error: None,
            _input: subscription,
        }
    }

    /// The field, to focus when the dialog opens.
    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Send the categories as they are now, with the new one appended or
    /// this one renamed. Success shows them at once and closes the dialog;
    /// a refusal says why and keeps it.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(name) = valid_name(&self.input.read(cx).value()) else {
            return;
        };
        let Some(chat) = self.chat.upgrade() else {
            return;
        };
        let id = self.renaming.clone().unwrap_or_else(new_category_id);
        let edited = {
            let store = chat.read(cx).store();
            let categories = store
                .server(&self.server)
                .and_then(|s| s.categories.as_deref())
                .unwrap_or_default();
            match self.renaming {
                None => Some(with_category_added(categories, &id, &name)),
                Some(_) => with_category_renamed(categories, &id, &name),
            }
        };
        let Some(edited) = edited else {
            self.error = Some("This category has been deleted.".into());
            cx.notify();
            return;
        };
        self.saving = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let (client, server) = (self.client.clone(), self.server.clone());
            async move { client.edit_categories(&server, &edited).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(Ok(categories)) => {
                        let server = this.server.clone();
                        log::debug!("saved category {id} of server {server}");
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| {
                                chat.categories_changed(&server, categories, cx)
                            });
                        }
                        window.close_dialog(cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't edit the categories of {}: {err:?}", this.server);
                        this.error = Some(categories_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("category edit task failed: {err:?}");
                        this.error = Some("Couldn't reach Stoat. Try again.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for NameDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let valid = valid_name(&self.input.read(cx).value()).is_some();
        let entity = cx.entity().downgrade();
        let (label, action) = match self.renaming {
            Some(_) => ("New name", "Rename"),
            None => ("Name", "Create"),
        };

        v_flex()
            .gap_3()
            .child(name_field(
                label,
                &self.input,
                self.saving,
                self.error.clone(),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-name-dialog")
                            .outline()
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("name-dialog-submit")
                            .primary()
                            .label(action)
                            .loading(self.saving)
                            .disabled(self.saving || !valid)
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| this.save(window, cx)).ok();
                            }),
                    ),
            )
    }
}
