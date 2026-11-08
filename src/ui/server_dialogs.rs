//! "Create a server": name it, and Stoat makes it with one text channel.
//! The naming rules and the error texts are in [`crate::server_edit`].

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::StoatClient;
use crate::invites::is_official_app;
use crate::runtime::spawn_io;
use crate::server_edit::{AUP_URL, NAME_MAX, create_server_error_text, valid_name};
use crate::ui::chat::ChatView;

/// The dialog's width (as Join a server's).
pub const CREATE_SERVER_DIALOG_WIDTH: f32 = 420.;

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
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let typed = self.input.read(cx).value();
        let count = typed.trim().chars().count();
        let valid = valid_name(&typed).is_some();
        let entity = cx.entity().downgrade();

        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(muted)
                            .child("Server name"),
                    )
                    .child(Input::new(&self.input).disabled(self.creating))
                    .child(
                        div()
                            .text_xs()
                            .text_right()
                            .text_color(if count > NAME_MAX { danger } else { muted })
                            .child(format!("{count}/{NAME_MAX}")),
                    )
                    .children(
                        self.error
                            .clone()
                            .map(|text| div().text_xs().text_color(danger).child(text)),
                    ),
            )
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
