//! The signed-in UI: server rail, channel list, and the selected channel.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::events::{self, Connection, ConnectionEvent, ServerEvent};
use crate::api::{StoatClient, file_url};
use crate::runtime::spawn_io;
use crate::store::{
    ChannelEntry, ChannelKind, ChatStore, MessageRow, RowOp, ServerEntry, TimelineRow, diff_rows,
    display_name,
};

/// How many messages to load when a channel is opened.
pub const HISTORY_PAGE: usize = 50;

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

/// A loaded channel's display rows and its own scroll state, so switching
/// back to a channel keeps its position.
struct Pane {
    rows: Rc<Vec<TimelineRow>>,
    scroller: Entity<MessageScrollerState>,
    /// Watches scrolling, to load older messages at the top.
    _scroll: Subscription,
}

pub struct ChatView {
    client: StoatClient,
    store: ChatStore,
    has_ready: bool,
    status: Status,
    place: Place,
    /// Last channel picked in each place, so switching servers comes back to it.
    selected: HashMap<Place, String>,
    /// File server base URL for icons, from `GET /`. `None` until loaded.
    file_server: Option<String>,
    panes: HashMap<String, Pane>,
    /// History requests in flight, by channel. Results always land in their
    /// own channel, so a slow one can't overwrite the channel being viewed.
    loading: HashMap<String, Task<()>>,
    load_errors: HashMap<String, SharedString>,
    /// Older-page requests in flight, and the last failure, by channel.
    loading_older: HashMap<String, Task<()>>,
    older_errors: HashMap<String, SharedString>,
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

        let (connection, mut events) = events::connect(client.clone());
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
            client,
            store: ChatStore::new(me),
            has_ready: false,
            status: Status::Connecting { attempt: 1 },
            place: Place::Home,
            selected: HashMap::new(),
            file_server: None,
            panes: HashMap::new(),
            loading: HashMap::new(),
            load_errors: HashMap::new(),
            loading_older: HashMap::new(),
            older_errors: HashMap::new(),
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
                let reconnect = self.has_ready;
                self.store.apply_ready(*ready);
                self.has_ready = true;
                self.status = Status::Connected;
                self.drop_stale_selection();
                if reconnect {
                    // Events missed while offline would leave gaps, so drop
                    // loaded history and reload what's on screen.
                    log::info!("reconnected: reloading message history");
                    self.store.clear_histories();
                    self.panes.clear();
                    self.loading.clear();
                    self.loading_older.clear();
                    self.older_errors.clear();
                }
                self.ensure_current_loaded(cx);
            }
            ConnectionEvent::Event(ServerEvent::Message(message)) => {
                let channel = message.channel.clone();
                log::debug!("live: new message {} in {channel}", message.id);
                if self.store.apply_message(*message) {
                    self.sync_pane(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::MessageUpdate {
                id,
                channel,
                data,
                clear,
            }) => {
                log::debug!("live: edit {id} in {channel}");
                if self.store.apply_update(&channel, &id, *data, &clear) {
                    self.sync_pane(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::MessageDelete { id, channel }) => {
                log::debug!("live: delete {id} in {channel}");
                if self.store.apply_delete(&channel, &[id]) {
                    self.sync_pane(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::BulkMessageDelete { channel, ids }) => {
                if self.store.apply_delete(&channel, &ids) {
                    self.sync_pane(&channel, cx);
                }
            }
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
        self.ensure_current_loaded(cx);
        cx.notify();
    }

    fn select_channel(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected.insert(self.place.clone(), id);
        self.ensure_current_loaded(cx);
        cx.notify();
    }

    /// Start loading the current channel's history if it isn't loaded or loading.
    fn ensure_current_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.has_ready {
            return;
        }
        if let Some(channel) = self.current_channel()
            && self.store.history(&channel.id).is_none()
            && !self.loading.contains_key(&channel.id)
        {
            self.load_history(channel.id, cx);
        }
    }

    fn load_history(&mut self, channel: String, cx: &mut Context<Self>) {
        self.load_errors.remove(&channel);
        let client = self.client.clone();
        let request = spawn_io({
            let channel = channel.clone();
            async move {
                client
                    .fetch_messages(&channel, None, HISTORY_PAGE as i64)
                    .await
            }
        });
        let task = cx.spawn({
            let channel = channel.clone();
            async move |this, cx| {
                let result = request.await;
                this.update(cx, |this, cx| {
                    this.loading.remove(&channel);
                    match result {
                        Ok(Ok(page)) => {
                            this.store.set_history(&channel, page, HISTORY_PAGE);
                            log::debug!(
                                "loaded {} messages for {channel}",
                                this.store.history(&channel).map_or(0, |h| h.messages.len())
                            );
                            this.sync_pane(&channel, cx);
                        }
                        Ok(Err(err)) => {
                            log::warn!("loading messages for {channel} failed: {err:?}");
                            this.load_errors.insert(channel, err.to_string().into());
                        }
                        Err(err) => {
                            log::warn!("message load task failed: {err:?}");
                            this.load_errors
                                .insert(channel, "Something went wrong loading messages.".into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        });
        self.loading.insert(channel, task);
    }

    /// Bring a loaded channel's rows up to date. Existing panes get the
    /// minimal scroller operations (`diff_rows`), so live messages keep the
    /// view at the bottom and older pages slide in above without a jump.
    fn sync_pane(&mut self, channel: &str, cx: &mut Context<Self>) {
        let rows = Rc::new(self.store.timeline(channel));
        if let Some(pane) = self.panes.get_mut(channel) {
            let ops = diff_rows(&pane.rows, &rows);
            pane.rows = rows;
            pane.scroller.update(cx, |scroller, cx| {
                for op in ops {
                    let applied = match op {
                        RowOp::Reset(count) => {
                            scroller.reset(count, cx);
                            scroller.scroll_to_end(cx);
                            true
                        }
                        RowOp::Prepend(count) => scroller.prepend(count, cx),
                        RowOp::Append(count) => scroller.append(count, cx),
                        RowOp::Remove { at, count } => scroller.splice(at..at + count, 0, cx),
                        RowOp::Insert { at, count } => scroller.splice(at..at, count, cx),
                        RowOp::Remeasure { at } => scroller.remeasure_items(at..at + 1, cx),
                    };
                    if !applied {
                        log::warn!("scroller rejected {op:?}; resetting");
                    }
                }
            });
            return;
        }

        let scroller = cx.new(|cx| MessageScrollerState::new(rows.len(), cx));
        let watched = channel.to_string();
        let scroll = cx.observe(&scroller, move |this, _, cx| {
            this.maybe_load_older(&watched, cx)
        });
        self.panes.insert(
            channel.to_string(),
            Pane {
                rows,
                scroller,
                _scroll: scroll,
            },
        );
    }

    /// Load the page before the oldest loaded message, if the reader is at
    /// the top of `channel` and there's more to load.
    fn maybe_load_older(&mut self, channel: &str, cx: &mut Context<Self>) {
        if self.loading_older.contains_key(channel) || self.older_errors.contains_key(channel) {
            return;
        }
        let Some(history) = self.store.history(channel) else {
            return;
        };
        if history.exhausted || history.messages.is_empty() {
            return;
        }
        let Some(pane) = self.panes.get(channel) else {
            return;
        };
        let at_top = {
            let scroller = pane.scroller.read(cx);
            scroller.item_count() > 0 && scroller.logical_scroll_top().item_ix == 0
        };
        if at_top {
            self.load_older(channel.to_string(), cx);
        }
    }

    fn load_older(&mut self, channel: String, cx: &mut Context<Self>) {
        let Some(before) = self.store.oldest_message_id(&channel).map(str::to_string) else {
            return;
        };
        self.older_errors.remove(&channel);
        let client = self.client.clone();
        let request = spawn_io({
            let channel = channel.clone();
            async move {
                client
                    .fetch_messages(&channel, Some(&before), HISTORY_PAGE as i64)
                    .await
            }
        });
        let task = cx.spawn({
            let channel = channel.clone();
            async move |this, cx| {
                let result = request.await;
                this.update(cx, |this, cx| {
                    this.loading_older.remove(&channel);
                    match result {
                        Ok(Ok(page)) => {
                            if this.store.prepend_page(&channel, page, HISTORY_PAGE) {
                                log::debug!(
                                    "loaded older messages for {channel} ({} total)",
                                    this.store.history(&channel).map_or(0, |h| h.messages.len())
                                );
                                this.sync_pane(&channel, cx);
                            }
                        }
                        Ok(Err(err)) => {
                            log::warn!("loading older messages for {channel} failed: {err:?}");
                            this.older_errors.insert(channel, err.to_string().into());
                        }
                        Err(err) => {
                            log::warn!("older-messages task failed: {err:?}");
                            this.older_errors
                                .insert(channel, "Couldn't load older messages.".into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        });
        self.loading_older.insert(channel, task);
        cx.notify();
    }

    fn retry_older(&mut self, channel: String, cx: &mut Context<Self>) {
        self.older_errors.remove(&channel);
        self.load_older(channel, cx);
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

        let colors = RowColors {
            muted: theme.muted_foreground,
            system: theme.muted_foreground,
            danger: theme.danger,
        };
        let placeholder = |text: SharedString| {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(colors.muted)
                .child(text)
                .into_any_element()
        };
        let body = match &channel {
            None if self.has_ready => placeholder("Pick a conversation from the sidebar.".into()),
            None => placeholder("Loading your servers and conversations…".into()),
            Some(channel) => {
                if let Some(pane) = self.panes.get(&channel.id) {
                    if pane.rows.is_empty() {
                        placeholder("No messages yet.".into())
                    } else {
                        let rows = pane.rows.clone();
                        let older = if self.loading_older.contains_key(&channel.id) {
                            Some(
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .text_xs()
                                    .bg(theme.secondary)
                                    .text_color(theme.secondary_foreground)
                                    .child("Loading older messages…")
                                    .into_any_element(),
                            )
                        } else {
                            self.older_errors.get(&channel.id).map(|error| {
                                let id = channel.id.clone();
                                h_flex()
                                    .gap_2()
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .text_xs()
                                    .bg(theme.secondary)
                                    .text_color(colors.danger)
                                    .child(error.clone())
                                    .child(
                                        Button::new("retry-older")
                                            .xsmall()
                                            .label("Retry")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.retry_older(id.clone(), cx)
                                            })),
                                    )
                                    .into_any_element()
                            })
                        };
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(
                                MessageScroller::new(
                                    SharedString::from(format!("messages-{}", channel.id)),
                                    pane.scroller.clone(),
                                    move |ix, _window, _cx| render_row(&rows[ix], colors),
                                )
                                .flex_1(),
                            )
                            .when_some(older, |el, pill| {
                                el.child(
                                    div()
                                        .absolute()
                                        .top_2()
                                        .left_0()
                                        .right_0()
                                        .flex()
                                        .justify_center()
                                        .child(pill),
                                )
                            })
                            .into_any_element()
                    }
                } else if let Some(error) = self.load_errors.get(&channel.id) {
                    let id = channel.id.clone();
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_sm()
                        .child(div().text_color(colors.danger).child(error.clone()))
                        .child(Button::new("retry-history").label("Retry").on_click(
                            cx.listener(move |this, _, _, cx| this.load_history(id.clone(), cx)),
                        ))
                        .into_any_element()
                } else {
                    placeholder("Loading messages…".into())
                }
            }
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
            .child(body)
    }
}

#[derive(Clone, Copy)]
struct RowColors {
    muted: Hsla,
    system: Hsla,
    danger: Hsla,
}

fn render_row(row: &TimelineRow, colors: RowColors) -> AnyElement {
    match row {
        TimelineRow::StartOfChannel { name } => v_flex()
            .px_4()
            .pt_6()
            .pb_2()
            .gap_1()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child(format!("This is the start of {name}")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted)
                    .child("There are no older messages."),
            )
            .into_any_element(),
        TimelineRow::Message(message) if message.system => render_system_row(message, colors),
        TimelineRow::Message(message) => render_message_row(message, colors),
    }
}

fn render_system_row(message: &MessageRow, colors: RowColors) -> AnyElement {
    h_flex()
        .px_4()
        .py_1()
        .gap_2()
        .text_sm()
        .text_color(colors.system)
        .child(div().child("→"))
        .child(
            div()
                .italic()
                .child(message.body.clone().unwrap_or_default()),
        )
        .child(div().text_xs().child(message.time.clone()))
        .into_any_element()
}

fn render_message_row(message: &MessageRow, colors: RowColors) -> AnyElement {
    v_flex()
        .w_full()
        .min_w_0()
        .px_4()
        .when(!message.continuation, |el| el.pt_3())
        .pb_0p5()
        .when(!message.continuation, |el| {
            el.child(
                h_flex()
                    .gap_2()
                    .items_baseline()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(message.author.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted)
                            .child(message.time.clone()),
                    ),
            )
        })
        .when_some(message.body.clone(), |el, body| {
            el.child(
                div()
                    .w_full()
                    .text_sm()
                    .child(body)
                    .when(message.edited, |el| {
                        el.child(div().text_xs().text_color(colors.muted).child("(edited)"))
                    }),
            )
        })
        .children(message.attachments.iter().map(|name| {
            div()
                .text_sm()
                .text_color(colors.muted)
                .child(format!("[attachment: {name}]"))
        }))
        .into_any_element()
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // After layout, check whether the open channel is scrolled to its top
        // (or too short to scroll) and has more history to load.
        if let Some(channel) = self.current_channel().map(|c| c.id) {
            cx.defer_in(window, move |this, _, cx| {
                this.maybe_load_older(&channel, cx)
            });
        }
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
