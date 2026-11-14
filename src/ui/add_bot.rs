//! The Add bot dialog, in the main window: the bot's avatar, name and
//! description (fetched as the public bot), a server or group to add it to,
//! and for a server, the permissions it asks for that the user may give.
//! Opened from a bot's page in My Bots, or a `/bot/{id}` link. What it
//! offers and sends is [`crate::bots`].

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::models::PublicBot;
use crate::bots::{
    DESTINATION_FILTER_FROM, Destination, DestinationKind, added_text, bot_error_text,
    destinations, filter_destinations, grantable, invite_permissions, requested,
    ungrantable_reason,
};
use crate::runtime::spawn_io;
use crate::ui::chat::{ChatView, avatar};

/// The dialog's width.
pub const ADD_BOT_DIALOG_WIDTH: f32 = 440.;

/// How big the bot's avatar shows.
const BOT_AVATAR: f32 = 56.;

/// How big a server's or group's icon shows in the list.
const PLACE_ICON: f32 = 24.;

/// How tall the list of places may grow before it scrolls.
const LIST_MAX_HEIGHT: f32 = 220.;

/// Fetching the bot.
enum Load {
    Loading {
        _task: Task<()>,
    },
    Loaded(Box<PublicBot>),
    /// Why it couldn't be fetched.
    Failed(String),
}

/// Add a bot to a server or group.
pub struct AddBotDialog {
    chat: WeakEntity<ChatView>,
    bot: String,
    load: Load,
    filter: Entity<InputState>,
    /// The place picked, by ID.
    selected: Option<String>,
    /// The requested permissions ticked (for a server).
    ticked: u64,
    adding: bool,
    error: Option<String>,
    _filter: Subscription,
}

impl AddBotDialog {
    pub fn new(
        chat: &ChatView,
        weak: WeakEntity<ChatView>,
        bot: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Find a server or group"));
        let subscription = cx.subscribe(&filter, |_, _, _: &InputEvent, cx| cx.notify());
        let request = spawn_io({
            let (client, bot) = (chat.client().clone(), bot.clone());
            async move { client.fetch_public_bot(&bot).await }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.load = match result {
                    Ok(Ok(public)) => Load::Loaded(Box::new(public)),
                    Ok(Err(err)) => {
                        log::warn!("couldn't fetch bot {}: {err}", this.bot);
                        Load::Failed(bot_error_text(&err, None))
                    }
                    Err(err) => {
                        log::warn!("bot fetch task failed: {err:?}");
                        Load::Failed("Couldn't load the bot.".into())
                    }
                };
                cx.notify();
            })
            .ok();
        });
        Self {
            chat: weak,
            bot,
            load: Load::Loading { _task: task },
            filter,
            selected: None,
            ticked: 0,
            adding: false,
            error: None,
            _filter: subscription,
        }
    }

    /// The bits the bot asks for that the matrix knows.
    fn requested_bits(&self) -> u64 {
        match &self.load {
            Load::Loaded(public) => requested(public.default_permissions.unwrap_or(0) as u64)
                .iter()
                .fold(0, |bits, entry| bits | entry.bit()),
            _ => 0,
        }
    }

    /// Where the bot can go now.
    fn destinations(&self, cx: &App) -> Vec<Destination> {
        self.chat
            .upgrade()
            .map(|chat| destinations(chat.read(cx).store(), &self.bot))
            .unwrap_or_default()
    }

    /// The place picked, if it's still one the bot can go.
    fn selected(&self, cx: &App) -> Option<Destination> {
        let id = self.selected.as_ref()?;
        self.destinations(cx)
            .into_iter()
            .find(|place| &place.id == id)
    }

    /// What the user may give the bot in the picked server.
    fn grantable(&self, cx: &App) -> u64 {
        match (self.selected(cx), self.chat.upgrade()) {
            (Some(place), Some(chat)) if place.kind == DestinationKind::Server => {
                grantable(chat.read(cx).store(), &place.id, self.requested_bits())
            }
            _ => 0,
        }
    }

    /// Pick a place: in a server, every requested permission the user may
    /// give starts ticked.
    fn select(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.error = None;
        self.ticked = self.grantable(cx);
        cx.notify();
    }

    fn toggle(&mut self, bit: u64, on: bool, cx: &mut Context<Self>) {
        if on {
            self.ticked |= bit & self.grantable(cx);
        } else {
            self.ticked &= !bit;
        }
        cx.notify();
    }

    /// Add the bot to the picked place, with what's ticked and grantable.
    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.adding {
            return;
        }
        let (Some(place), Some(chat), Load::Loaded(public)) =
            (self.selected(cx), self.chat.upgrade(), &self.load)
        else {
            return;
        };
        let name = public.username.clone();
        let permissions = match place.kind {
            DestinationKind::Server => invite_permissions(self.ticked, self.grantable(cx)),
            DestinationKind::Group => None,
        };
        self.adding = true;
        self.error = None;
        cx.notify();
        let request = spawn_io({
            let (client, bot, destination) = (
                chat.read(cx).client().clone(),
                self.bot.clone(),
                place.invite(),
            );
            async move { client.invite_bot(&bot, &destination, permissions).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.adding = false;
                match result {
                    Ok(Ok(())) => {
                        log::info!("added bot {} to {}", this.bot, place.id);
                        window.close_dialog(cx);
                        window.push_notification(
                            Notification::success(added_text(&name, &place.name)),
                            cx,
                        );
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't add bot {} to {}: {err}", this.bot, place.id);
                        this.error = Some(bot_error_text(&err, None));
                    }
                    Err(err) => {
                        log::warn!("bot invite task failed: {err:?}");
                        this.error = Some("Couldn't add the bot.".into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_bot(&self, public: &PublicBot, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let url = self.chat.upgrade().map(|chat| {
            let media = chat.read(cx).store().media();
            (!public.avatar.is_empty())
                .then(|| media.file_by_id("avatars", &public.avatar))
                .flatten()
                .unwrap_or_else(|| media.default_avatar(&public.id))
        });
        let description = public.description.trim();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(avatar(
                        format!("add-bot-avatar-{}", public.id),
                        url,
                        &public.username,
                        BOT_AVATAR,
                        theme.secondary,
                        theme.secondary_foreground,
                    ))
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(public.username.clone()),
                    ),
            )
            .when(!description.is_empty(), |el| {
                el.child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Description provided by the bot's owner"),
                        )
                        .child(div().text_sm().child(description.to_string())),
                )
            })
            .into_any_element()
    }

    fn render_place(&self, place: &Destination, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let picked = self.selected.as_ref() == Some(&place.id);
        let id = place.id.clone();
        h_flex()
            .id(SharedString::from(format!("add-bot-to-{}", place.id)))
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .when(picked, |el| el.bg(theme.accent))
            .when(!picked, |el| el.hover(|el| el.bg(theme.secondary)))
            .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
            .child(avatar(
                format!("add-bot-place-{}", place.id),
                place.icon.clone(),
                &place.name,
                PLACE_ICON,
                theme.secondary,
                theme.secondary_foreground,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .child(place.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(match place.kind {
                        DestinationKind::Server => "Server",
                        DestinationKind::Group => "Group",
                    }),
            )
            .into_any_element()
    }

    /// The requested permissions, for the picked server: each ticked and
    /// enabled only if the user may give it.
    fn render_permissions(&self, server: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let chat = self.chat.upgrade()?;
        let bits = self.requested_bits();
        if bits == 0 {
            return None;
        }
        let muted = cx.theme().muted_foreground;
        let rows: Vec<AnyElement> = requested(bits)
            .into_iter()
            .map(|entry| {
                let bit = entry.bit();
                let reason = ungrantable_reason(chat.read(cx).store(), server, bit);
                let view = cx.entity().downgrade();
                Checkbox::new(SharedString::from(format!("add-bot-permission-{bit}")))
                    .label(entry.title)
                    .checked(self.ticked & bit != 0)
                    .disabled(reason.is_some() || self.adding)
                    .when_some(reason, |el, reason| el.tooltip(reason))
                    .on_click(move |on: &bool, _, cx| {
                        let on = *on;
                        view.update(cx, |this, cx| this.toggle(bit, on, cx)).ok();
                    })
                    .into_any_element()
            })
            .collect();
        Some(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Requested permissions"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("It gets a role of its own with the ones ticked."),
                )
                .children(rows)
                .into_any_element(),
        )
    }
}

impl Render for AddBotDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let public = match &self.load {
            Load::Loading { .. } => {
                return v_flex()
                    .gap_3()
                    .child(div().text_sm().text_color(muted).child("Loading…"))
                    .child(close_row(cx))
                    .into_any_element();
            }
            Load::Failed(text) => {
                return v_flex()
                    .gap_3()
                    .child(div().text_sm().text_color(danger).child(text.clone()))
                    .child(close_row(cx))
                    .into_any_element();
            }
            Load::Loaded(public) => (**public).clone(),
        };
        let places = self.destinations(cx);
        let bot = self.render_bot(&public, cx);
        if places.is_empty() {
            return v_flex()
                .gap_4()
                .child(bot)
                .child(div().text_sm().text_color(muted).child(
                    "There's nowhere you can add it: you need Manage Server in a server, \
                     or Invite Others in a group.",
                ))
                .child(close_row(cx))
                .into_any_element();
        }
        let filtering = places.len() > DESTINATION_FILTER_FROM;
        let filter = self.filter.read(cx).value().to_string();
        let shown: Vec<Destination> =
            filter_destinations(&places, if filtering { &filter } else { "" })
                .into_iter()
                .cloned()
                .collect();
        let rows: Vec<AnyElement> = shown
            .iter()
            .map(|place| self.render_place(place, cx))
            .collect();
        let selected = self.selected(cx);
        let permissions = selected
            .as_ref()
            .filter(|place| place.kind == DestinationKind::Server)
            .and_then(|place| self.render_permissions(&place.id, cx));
        let none_given = selected
            .as_ref()
            .is_some_and(|place| place.kind == DestinationKind::Server)
            && self.requested_bits() == 0;
        v_flex()
            .gap_4()
            .child(bot)
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Add it to"),
                    )
                    .when(filtering, |el| {
                        el.child(Input::new(&self.filter).small().disabled(self.adding))
                    })
                    .child(
                        v_flex()
                            .id("add-bot-places")
                            .max_h(px(LIST_MAX_HEIGHT))
                            .overflow_y_scroll()
                            .gap_1()
                            .children(rows)
                            .when(shown.is_empty(), |el| {
                                el.child(
                                    div()
                                        .px_2()
                                        .text_sm()
                                        .text_color(muted)
                                        .child("Nothing matches."),
                                )
                            }),
                    ),
            )
            .children(permissions)
            .when(none_given, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("The bot will not be granted any permissions."),
                )
            })
            .children(
                self.error
                    .clone()
                    .map(|text| div().text_sm().text_color(danger).child(text)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("add-bot-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.adding)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("add-bot-ok")
                            .primary()
                            .label("Add")
                            .loading(self.adding)
                            .disabled(self.adding || selected.is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

/// Only Close, when there's nothing to do.
fn close_row(_cx: &App) -> AnyElement {
    h_flex()
        .justify_end()
        .child(
            Button::new("add-bot-close")
                .outline()
                .label("Close")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .into_any_element()
}
