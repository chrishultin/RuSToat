//! "Join a server": paste an invite link or code, see what it leads to,
//! then join it, or open it if the user is already in. And "Invite
//! people": create an invite to a channel and copy its link. The rules for
//! codes and links, and the error texts, are in [`crate::invites`].

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::StoatClient;
use crate::api::models::Channel;
use crate::invites::{
    Expiry, InviteHosts, InvitePreview, MAX_USES_OPTIONS, already_joined, create_error_text,
    expires_at, invite_code, invite_summary, join_error_text, max_uses_label, parse_input,
};
use crate::links::invite_link;
use crate::runtime::spawn_io;
use crate::ui::chat::{ChatView, avatar};

/// The dialog's width (as Add friend's).
pub const JOIN_DIALOG_WIDTH: f32 = 420.;
/// The invite's icon in the preview.
const PREVIEW_ICON: f32 = 48.;

/// What the dialog is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    LookingUp,
    Joining,
}

/// The Join dialog's content, for one account.
pub struct JoinDialog {
    chat: WeakEntity<ChatView>,
    client: StoatClient,
    hosts: InviteHosts,
    /// "@ada#1234": who joins.
    account: Option<String>,
    input: Entity<InputState>,
    /// The code last looked up; a lookup that comes back for another
    /// (the field was edited meanwhile) is dropped.
    looked_up: Option<String>,
    preview: Option<InvitePreview>,
    busy: Option<Busy>,
    error: Option<String>,
    _input: Subscription,
}

impl JoinDialog {
    /// The dialog for `chat`'s account, with `code` filled in and looked up
    /// when it's given (a clicked invite link).
    pub fn new(
        chat: &ChatView,
        weak: WeakEntity<ChatView>,
        code: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let hosts = chat.invite_hosts();
        let placeholder = hosts.placeholder();
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(placeholder);
            if let Some(code) = &code {
                input.set_value(code.clone(), window, cx);
            }
            input
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.edited(cx),
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => {}
            });
        let account = chat
            .store()
            .user(chat.store().me())
            .map(|me| format!("@{}#{}", me.username, me.discriminator));
        let mut this = Self {
            chat: weak,
            client: chat.client().clone(),
            hosts,
            account,
            input,
            looked_up: None,
            preview: None,
            busy: None,
            error: None,
            _input: subscription,
        };
        if code.is_some() {
            this.look_up(window, cx);
        }
        this
    }

    /// The field, to focus when the dialog opens.
    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// The field changed: what was looked up no longer applies.
    fn edited(&mut self, cx: &mut Context<Self>) {
        let typed = self.input.read(cx).value().to_string();
        let same = self
            .looked_up
            .as_deref()
            .is_some_and(|code| parse_input(&typed, &self.hosts).is_ok_and(|typed| typed == code));
        if same || self.busy == Some(Busy::Joining) {
            return;
        }
        self.looked_up = None;
        self.preview = None;
        self.error = None;
        self.busy = None;
        cx.notify();
    }

    /// Enter: look the invite up, or join it once it's shown.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.is_some() {
            self.join(window, cx);
        } else {
            self.look_up(window, cx);
        }
    }

    /// Whether the user is already in what `preview` leads to.
    fn is_member(&self, preview: &InvitePreview, cx: &App) -> bool {
        self.chat
            .upgrade()
            .is_some_and(|chat| chat.read(cx).is_invite_member(preview))
    }

    /// Look up what's typed: Stoat says what the invite leads to.
    fn look_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let typed = self.input.read(cx).value().to_string();
        let code = match parse_input(&typed, &self.hosts) {
            Ok(code) => code,
            Err(text) => {
                self.preview = None;
                self.error = Some(text.into());
                cx.notify();
                return;
            }
        };
        self.looked_up = Some(code.clone());
        self.preview = None;
        self.error = None;
        self.busy = Some(Busy::LookingUp);
        cx.notify();
        let request = spawn_io({
            let (client, code) = (self.client.clone(), code.clone());
            async move { client.fetch_invite(&code).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if this.looked_up.as_deref() != Some(code.as_str()) {
                    return;
                }
                this.busy = None;
                match result {
                    Ok(Ok(invite)) => this.preview = Some(InvitePreview::new(invite)),
                    Ok(Err(err)) => {
                        log::info!("couldn't look up invite {code}: {err:?}");
                        this.error = Some(join_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("invite lookup task failed: {err:?}");
                        this.error = Some("Couldn't reach Stoat. Try again.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Join what's shown, or open it if the user is already in. Success
    /// closes the dialog and shows it; a refusal says why and keeps it.
    fn join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.preview.clone() else {
            return;
        };
        if self.busy.is_some() {
            return;
        }
        if self.is_member(&preview, cx) {
            self.open(&preview, window, cx);
            return;
        }
        self.busy = Some(Busy::Joining);
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let (client, code) = (self.client.clone(), preview.code.clone());
            async move { client.join_invite(&code).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(Ok(joined)) => {
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.invite_joined(&preview, joined, cx));
                        }
                        window.close_dialog(cx);
                        window.push_notification(
                            Notification::success(format!("Joined {}.", preview.name)),
                            cx,
                        );
                    }
                    Ok(Err(err)) if already_joined(&err) => this.open(&preview, window, cx),
                    Ok(Err(err)) => {
                        log::info!("couldn't join invite {}: {err:?}", preview.code);
                        this.error = Some(join_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("invite join task failed: {err:?}");
                        this.error = Some("Couldn't reach Stoat. Try again.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Show what `preview` leads to, which the user is already in.
    fn open(&mut self, preview: &InvitePreview, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.upgrade() {
            chat.update(cx, |chat, cx| chat.open_invite(preview, cx));
        }
        window.close_dialog(cx);
    }
}

impl Render for JoinDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let looking_up = self.busy == Some(Busy::LookingUp);
        let joining = self.busy == Some(Busy::Joining);
        let member = self
            .preview
            .as_ref()
            .is_some_and(|preview| self.is_member(preview, cx));
        let icon_url = self.preview.as_ref().and_then(|preview| {
            let chat = self.chat.upgrade()?;
            chat.read(cx).store().media().file(preview.icon.as_ref()?)
        });
        let preview = self
            .preview
            .as_ref()
            .map(|preview| render_preview(preview, icon_url, cx).into_any_element());
        let entity = cx.entity().downgrade();
        let look_up = {
            let entity = entity.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                entity.update(cx, |this, cx| this.look_up(window, cx)).ok();
            }
        };
        let join = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            entity.update(cx, |this, cx| this.join(window, cx)).ok();
        };

        v_flex()
            .gap_3()
            .children(self.account.clone().map(|account| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("Joining as {account}"))
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Paste an invite link or code."),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&self.input).disabled(joining)),
                            )
                            .child(
                                Button::new("look-up-invite")
                                    .outline()
                                    .label("Look up")
                                    .loading(looking_up)
                                    .disabled(self.busy.is_some())
                                    .on_click(look_up),
                            ),
                    )
                    .children(
                        self.error
                            .clone()
                            .map(|text| div().text_xs().text_color(danger).child(text)),
                    ),
            )
            .children(preview)
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-join")
                            .outline()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("join-invite")
                            .primary()
                            .label(if member { "Open" } else { "Join" })
                            .loading(joining)
                            .disabled(self.preview.is_none() || self.busy.is_some())
                            .on_click(join),
                    ),
            )
    }
}

/// What an invite leads to: its icon, name, members and channel, and who
/// made it.
fn render_preview(preview: &InvitePreview, icon_url: Option<String>, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .gap_3()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .child(avatar(
            format!("invite-preview-{}", preview.code),
            icon_url,
            &preview.name,
            PREVIEW_ICON,
            theme.muted,
            theme.muted_foreground,
        ))
        .child(
            v_flex()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(preview.name.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(preview.detail()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(format!("Invited by {}", preview.inviter)),
                ),
        )
}

/// The Create invite dialog's width.
pub const CREATE_DIALOG_WIDTH: f32 = 420.;

/// A created invite, as the dialog shows it.
struct Created {
    code: String,
    /// The link to share. `None` when the instance doesn't say where its
    /// web client is: the code alone works in the Join dialog.
    link: Option<String>,
    summary: String,
}

/// "Invite people to {server}": how long a new invite to one channel lasts
/// and how often it works, then its link to copy.
pub struct CreateInviteDialog {
    client: StoatClient,
    channel: String,
    channel_name: String,
    /// The web client's address, for the link.
    app: Option<String>,
    expiries: Vec<Expiry>,
    expiry: Expiry,
    max_uses: Option<u64>,
    creating: bool,
    created: Option<Created>,
    error: Option<String>,
}

impl CreateInviteDialog {
    /// The dialog for `channel` in `chat`'s account. Its choices start at
    /// what Stoat's web client always makes: never expiring, no limit.
    pub fn new(chat: &ChatView, channel: String, max_days: Option<u32>) -> Self {
        let channel_name = match chat.store().channel(&channel) {
            Some(Channel::TextChannel { name, .. }) => name.clone(),
            _ => "channel".into(),
        };
        Self {
            client: chat.client().clone(),
            channel,
            channel_name,
            app: chat.web_app().map(str::to_string),
            expiries: Expiry::options(max_days),
            expiry: Expiry::Never,
            max_uses: None,
            creating: false,
            created: None,
            error: None,
        }
    }

    /// Create the invite with the chosen options.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating || self.created.is_some() {
            return;
        }
        self.creating = true;
        self.error = None;
        cx.notify();
        let (expiry, max_uses) = (self.expiry, self.max_uses);
        let expires = expires_at(expiry, chrono::Utc::now());
        let request = spawn_io({
            let (client, channel) = (self.client.clone(), self.channel.clone());
            async move {
                client
                    .create_invite(&channel, max_uses, expires.as_deref())
                    .await
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.creating = false;
                match result {
                    Ok(Ok(invite)) => {
                        let code = invite_code(&invite).to_string();
                        log::info!("created invite {code} to {}", this.channel);
                        this.created = Some(Created {
                            link: this.app.as_deref().map(|app| invite_link(app, &code)),
                            code,
                            summary: invite_summary(expiry, max_uses),
                        });
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't create an invite to {}: {err:?}", this.channel);
                        let had_expiry = expiry != Expiry::Never;
                        this.error = Some(create_error_text(&err, had_expiry));
                    }
                    Err(err) => {
                        log::warn!("invite create task failed: {err:?}");
                        this.error = Some("Couldn't reach Stoat. Try again.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A choice and its menu: "Expire after: [Never ▾]".
    fn picker(
        &self,
        id: &'static str,
        label: &'static str,
        value: String,
        menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .justify_between()
            .gap_3()
            .child(div().text_sm().child(label))
            .child(
                Button::new(id)
                    .outline()
                    .small()
                    .label(value)
                    .icon(IconName::ChevronDown)
                    .disabled(self.creating)
                    .dropdown_menu(menu),
            )
            .text_color(cx.theme().foreground)
    }

    fn expiry_menu(
        &self,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let (entity, expiries, chosen) =
            (cx.entity().downgrade(), self.expiries.clone(), self.expiry);
        move |menu, _, _| {
            expiries.iter().fold(menu, |menu, &expiry| {
                let entity = entity.clone();
                menu.item(
                    PopupMenuItem::new(expiry.label())
                        .checked(expiry == chosen)
                        .on_click(move |_, _, cx| {
                            entity
                                .update(cx, |this, cx| {
                                    this.expiry = expiry;
                                    cx.notify();
                                })
                                .ok();
                        }),
                )
            })
        }
    }

    fn max_uses_menu(
        &self,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let (entity, chosen) = (cx.entity().downgrade(), self.max_uses);
        move |menu, _, _| {
            MAX_USES_OPTIONS.iter().fold(menu, |menu, &uses| {
                let entity = entity.clone();
                menu.item(
                    PopupMenuItem::new(max_uses_label(uses))
                        .checked(uses == chosen)
                        .on_click(move |_, _, cx| {
                            entity
                                .update(cx, |this, cx| {
                                    this.max_uses = uses;
                                    cx.notify();
                                })
                                .ok();
                        }),
                )
            })
        }
    }

    fn render_created(&self, created: &Created, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (copied, copy_label, toast) = match &created.link {
            Some(link) => (link.clone(), "Copy link", "Copied the invite link."),
            None => (created.code.clone(), "Copy code", "Copied the invite code."),
        };
        v_flex()
            .gap_3()
            .child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary)
                    .font_family("Menlo")
                    .text_sm()
                    .child(copied.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(created.summary.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("copy-invite")
                            .outline()
                            .icon(IconName::Copy)
                            .label(copy_label)
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                                window.push_notification(Notification::success(toast), cx);
                            }),
                    )
                    .child(
                        Button::new("invite-done")
                            .primary()
                            .label("Done")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .into_any_element()
    }
}

impl Render for CreateInviteDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let heading = div()
            .text_sm()
            .text_color(muted)
            .child(format!("To #{}", self.channel_name));
        if let Some(created) = &self.created {
            return v_flex()
                .gap_3()
                .child(heading)
                .child(self.render_created(created, cx));
        }
        let entity = cx.entity().downgrade();
        v_flex()
            .gap_3()
            .child(heading)
            .child(self.picker(
                "invite-expiry",
                "Expire after",
                self.expiry.label().into(),
                self.expiry_menu(cx),
                cx,
            ))
            .child(self.picker(
                "invite-max-uses",
                "Max uses",
                max_uses_label(self.max_uses),
                self.max_uses_menu(cx),
                cx,
            ))
            .children(
                self.error
                    .clone()
                    .map(|text| div().text_xs().text_color(danger).child(text)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-invite")
                            .outline()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("create-invite-submit")
                            .primary()
                            .label("Create invite")
                            .loading(self.creating)
                            .disabled(self.creating)
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| this.create(window, cx)).ok();
                            }),
                    ),
            )
    }
}
