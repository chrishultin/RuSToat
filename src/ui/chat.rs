//! The signed-in UI: server rail, channel list, and the selected channel.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::events::{self, Connection, ConnectionEvent, ServerEvent};
use crate::api::{StoatClient, file_url};
use crate::runtime::spawn_io;
use crate::store::{ChannelEntry, ChannelKind, ChatStore, ServerEntry, display_name};

pub enum ChatEvent {
    LogOutRequested,
    /// The server no longer accepts our session.
    SessionRejected,
}

/// Which list the channel column shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Place {
    Home,
    Server(String),
}

enum Status {
    Connecting {
        attempt: u32,
    },
    Connected,
    Reconnecting {
        reason: SharedString,
        retry_in: Duration,
    },
}

pub struct ChatView {
    store: ChatStore,
    has_ready: bool,
    status: Status,
    place: Place,
    /// Last channel picked in each place, so switching servers comes back to it.
    selected: HashMap<Place, String>,
    /// File server base URL for icons, from `GET /`. `None` until loaded.
    file_server: Option<String>,
    _connection: Connection,
    _events: Task<()>,
    _root: Task<()>,
}

impl EventEmitter<ChatEvent> for ChatView {}

impl ChatView {
    /// `client` must carry the session token. `me` is the logged-in user's ID.
    pub fn new(client: StoatClient, me: String, cx: &mut Context<Self>) -> Self {
        let root = spawn_io({
            let client = client.clone();
            async move { client.root_config().await }
        });
        let root = cx.spawn(async move |this, cx| match root.await {
            Ok(Ok(root)) => {
                let file_server = root.file_server().map(str::to_string);
                log::debug!("file server: {file_server:?}");
                this.update(cx, |this, cx| {
                    this.file_server = file_server;
                    cx.notify();
                })
                .ok();
            }
            Ok(Err(err)) => log::warn!("couldn't load API config (no server icons): {err}"),
            Err(err) => log::warn!("API config task failed: {err:?}"),
        });

        let (connection, mut events) = events::connect(client);
        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |this, cx| this.on_connection_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            store: ChatStore::new(me),
            has_ready: false,
            status: Status::Connecting { attempt: 1 },
            place: Place::Home,
            selected: HashMap::new(),
            file_server: None,
            _connection: connection,
            _events: events,
            _root: root,
        }
    }

    fn on_connection_event(&mut self, event: ConnectionEvent, cx: &mut Context<Self>) {
        match event {
            ConnectionEvent::Connecting { attempt } => self.status = Status::Connecting { attempt },
            ConnectionEvent::Connected => self.status = Status::Connected,
            ConnectionEvent::Event(ServerEvent::Ready(ready)) => {
                self.store.apply_ready(*ready);
                self.has_ready = true;
                self.status = Status::Connected;
                self.drop_stale_selection();
            }
            // Message events arrive in chunk 6.
            ConnectionEvent::Event(_) => return,
            ConnectionEvent::Disconnected { reason, retry_in } => {
                self.status = Status::Reconnecting {
                    reason: reason.into(),
                    retry_in,
                }
            }
            ConnectionEvent::SessionRejected => {
                cx.emit(ChatEvent::SessionRejected);
                return;
            }
        }
        cx.notify();
    }

    /// After a (re)connect, forget selections that point at things that are gone.
    fn drop_stale_selection(&mut self) {
        if let Place::Server(id) = &self.place
            && self.store.server(id).is_none()
        {
            self.place = Place::Home;
        }
        self.selected
            .retain(|_, channel| self.store.channel(channel).is_some());
    }

    fn select_place(&mut self, place: Place, cx: &mut Context<Self>) {
        self.place = place;
        cx.notify();
    }

    fn select_channel(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected.insert(self.place.clone(), id);
        cx.notify();
    }

    fn sections(&self) -> Vec<(Option<String>, Vec<ChannelEntry>)> {
        match &self.place {
            Place::Home => vec![(None, self.store.home_channels())],
            Place::Server(id) => self
                .store
                .server_channels(id)
                .into_iter()
                .map(|s| (s.title, s.channels))
                .collect(),
        }
    }

    /// The picked channel for this place, or else its first channel.
    fn current_channel(&self) -> Option<ChannelEntry> {
        if let Some(id) = self.selected.get(&self.place)
            && let Some(entry) = self.store.channel_entry(id)
        {
            return Some(entry);
        }
        self.sections()
            .into_iter()
            .flat_map(|(_, channels)| channels)
            .next()
    }

    fn status_line(&self) -> SharedString {
        match &self.status {
            Status::Connected => "Online".into(),
            Status::Connecting { attempt: 1 } => "Connecting…".into(),
            Status::Connecting { .. } => "Reconnecting…".into(),
            Status::Reconnecting { retry_in, .. } => {
                format!("Offline · retrying in {}s", retry_in.as_secs().max(1)).into()
            }
        }
    }

    fn render_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let home = rail_button(
            "rail-home",
            Icon::new(IconName::House).into_any_element(),
            "Direct Messages",
            self.place == Place::Home,
            cx.listener(|this, _, _, cx| this.select_place(Place::Home, cx)),
            cx,
        );
        let servers: Vec<_> = self
            .store
            .servers()
            .into_iter()
            .map(|server| {
                let place = Place::Server(server.id.clone());
                let selected = self.place == place;
                let icon = server_icon(&server, self.file_server.as_deref(), selected);
                rail_button(
                    SharedString::from(format!("rail-{}", server.id)),
                    icon,
                    server.name,
                    selected,
                    cx.listener(move |this, _, _, cx| this.select_place(place.clone(), cx)),
                    cx,
                )
            })
            .collect();

        v_flex()
            .id("server-rail")
            .w(px(72.))
            .h_full()
            .flex_none()
            .items_center()
            .gap_2()
            .py_3()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .overflow_y_scroll()
            .child(home)
            .child(
                div()
                    .w(px(32.))
                    .h(px(1.))
                    .flex_none()
                    .bg(theme.sidebar_border),
            )
            .children(servers)
    }

    fn render_channel_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title: SharedString = match &self.place {
            Place::Home => "Direct Messages".into(),
            Place::Server(id) => self
                .store
                .server(id)
                .map(|s| s.name.clone())
                .unwrap_or_default()
                .into(),
        };
        let current = self.current_channel().map(|c| c.id);
        let sections = self.sections();
        let empty = sections.iter().all(|(_, channels)| channels.is_empty());
        let muted = cx.theme().muted_foreground;

        let rows: Vec<_> = sections
            .into_iter()
            .map(|(title, channels)| {
                v_flex()
                    .gap_0p5()
                    .when_some(title, |el, title| {
                        el.child(
                            div()
                                .px_2()
                                .pt_3()
                                .pb_1()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(muted)
                                .child(title.to_uppercase()),
                        )
                    })
                    .children(channels.into_iter().map(|channel| {
                        let selected = current.as_deref() == Some(channel.id.as_str());
                        self.render_channel_row(channel, selected, cx)
                    }))
            })
            .collect();

        let theme = cx.theme();
        v_flex()
            .w(px(240.))
            .h_full()
            .flex_none()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex()
                    .h(px(48.))
                    .flex_none()
                    .px_4()
                    .border_b_1()
                    .border_color(theme.sidebar_border)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().truncate().child(title)),
            )
            .child(
                v_flex()
                    .id("channel-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_0p5()
                    .when(!self.has_ready, |el| {
                        el.child(div().p_2().text_sm().text_color(muted).child("Loading…"))
                    })
                    .when(self.has_ready && empty, |el| {
                        el.child(
                            div()
                                .p_2()
                                .text_sm()
                                .text_color(muted)
                                .child("Nothing here yet."),
                        )
                    })
                    .children(rows),
            )
            .child(self.render_user_panel(cx))
    }

    fn render_channel_row(
        &self,
        channel: ChannelEntry,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let hover_bg = theme.sidebar_accent;
        let hover_fg = theme.sidebar_accent_foreground;
        let id = channel.id.clone();
        h_flex()
            .id(SharedString::from(format!("channel-{}", channel.id)))
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .when(selected, |el| {
                el.bg(theme.sidebar_accent)
                    .text_color(theme.sidebar_accent_foreground)
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!selected, |el| {
                el.text_color(theme.muted_foreground)
                    .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.select_channel(id.clone(), cx)))
            .child(Icon::new(channel_icon(channel.kind)).small())
            .child(div().truncate().child(channel.name))
    }

    fn render_user_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let name: SharedString = self
            .store
            .user(self.store.me())
            .map(|u| display_name(u).to_string())
            .unwrap_or_else(|| "…".into())
            .into();
        let online = matches!(self.status, Status::Connected);
        h_flex()
            .h(px(56.))
            .flex_none()
            .px_3()
            .gap_2()
            .justify_between()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(name),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .when(!online, |el| {
                                el.child(Icon::new(IconName::WifiOff).xsmall())
                            })
                            .child(self.status_line()),
                    ),
            )
            .child(
                Button::new("log-out")
                    .ghost()
                    .small()
                    .label("Log out")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ChatEvent::LogOutRequested))),
            )
    }

    fn render_main(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let channel = self.current_channel();
        let banner = match (&self.status, self.has_ready) {
            (Status::Reconnecting { reason, retry_in }, _) => Some(format!(
                "Connection lost ({reason}). Retrying in {}s…",
                retry_in.as_secs().max(1)
            )),
            (Status::Connecting { attempt }, true) if *attempt > 1 => {
                Some("Reconnecting…".to_string())
            }
            _ => None,
        };

        let header = h_flex()
            .h(px(48.))
            .flex_none()
            .px_4()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .font_weight(FontWeight::SEMIBOLD)
            .map(|el| match &channel {
                Some(channel) => el
                    .child(Icon::new(channel_icon(channel.kind)).text_color(theme.muted_foreground))
                    .child(div().truncate().child(channel.name.clone())),
                None => el.child(div().text_color(theme.muted_foreground).child(
                    if self.has_ready {
                        "No channel selected"
                    } else {
                        "Connecting to Stoat…"
                    },
                )),
            });

        let body_text: SharedString = match &channel {
            Some(channel) => format!("Messages in {} will show up here.", channel.name).into(),
            None if self.has_ready => "Pick a conversation from the sidebar.".into(),
            None => "Loading your servers and conversations…".into(),
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .when_some(banner, |el, banner| {
                el.child(
                    div()
                        .px_4()
                        .py_1()
                        .flex_none()
                        .text_sm()
                        .bg(theme.warning)
                        .text_color(theme.warning_foreground)
                        .child(banner),
                )
            })
            .child(header)
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(body_text),
            )
    }
}

fn channel_icon(kind: ChannelKind) -> IconName {
    match kind {
        ChannelKind::Text { voice: false } => IconName::Hash,
        ChannelKind::Text { voice: true } => IconName::Volume2,
        ChannelKind::DirectMessage => IconName::AtSign,
        ChannelKind::Group => IconName::Users,
        ChannelKind::SavedMessages => IconName::Bookmark,
    }
}

/// The server's uploaded icon, or its initials while that loads, if it fails,
/// or if there isn't one.
fn server_icon(server: &ServerEntry, file_server: Option<&str>, selected: bool) -> AnyElement {
    let initials = server.initials.clone();
    match (file_server, &server.icon) {
        (Some(file_server), Some(icon)) => img(SharedUri::from(file_url(file_server, icon)))
            .size(px(48.))
            .object_fit(ObjectFit::Cover)
            .map(|img| {
                if selected {
                    img.rounded_xl()
                } else {
                    img.rounded_full()
                }
            })
            .with_fallback(move || div().child(initials.clone()).into_any_element())
            .into_any_element(),
        _ => div().child(initials).into_any_element(),
    }
}

/// A rail button, with a marker on the left edge when selected (an icon image
/// covers the button's own highlight colour).
fn rail_button(
    id: impl Into<ElementId>,
    content: AnyElement,
    tooltip: impl Into<SharedString>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let tooltip: SharedString = tooltip.into();
    let hover_bg = theme.primary_hover;
    let hover_fg = theme.primary_foreground;
    let marker = theme.foreground;
    let button = div()
        .id(id)
        .size(px(48.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .when(selected, |el| {
            el.rounded_xl()
                .bg(theme.primary)
                .text_color(theme.primary_foreground)
        })
        .when(!selected, |el| {
            el.rounded_full()
                .bg(theme.secondary)
                .text_color(theme.secondary_foreground)
                .hover(move |style| style.rounded_xl().bg(hover_bg).text_color(hover_fg))
        })
        .on_click(on_click)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(content);

    div()
        .relative()
        .w_full()
        .flex()
        .justify_center()
        .child(button)
        .when(selected, |el| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(8.))
                    .w(px(4.))
                    .h(px(32.))
                    .rounded_r_md()
                    .bg(marker),
            )
        })
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.render_rail(cx))
            .child(self.render_channel_list(cx))
            .child(self.render_main(cx))
    }
}
