//! The signed-in UI: server rail, channel list, and the selected channel.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use gpui_kit::assets::IconName;
use gpui_kit::component::bubble::{Bubble, BubbleContent, BubbleReactions, BubbleVariant};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::message::{
    Message as MessageLayout, MessageAlignment, MessageAvatar, MessageContent, MessageFooter,
    MessageHeader,
};
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::ApiError;
use crate::api::StoatClient;
use crate::api::models::ReplyIntent;
use crate::api::events::{self, Connection, ConnectionEvent, ServerEvent};
use crate::attachment::{AttachmentView, fit_into};
use crate::complete::{Query, QueryKind, Suggestion, TokenRef, rank, rank_people};
use crate::draft::{AttachmentLimits, DraftAttachment, ReplyTarget};
use crate::embed::{Description, EmbedCard, EmbedImage, EmbedView};
use crate::markdown::{JUMBO_TEXT_PX, is_external_link};
use crate::media::MediaUrls;
use crate::notify::{self, Delivery, Notice, Viewing};
use crate::places::{Place, Places, PlacesFile};
use crate::runtime::spawn_io;
use crate::typing::{OutgoingTyping, TypingSignal, TypingState, typing_label};
use crate::store::{
    ChannelEntry, ChannelKind, ChatStore, MessageRow, PendingAttachment, PendingMessage,
    PendingState, PickerEmoji,
    ReactionEmoji, ReplyPreview, ReplyState, RowOp, ServerEntry, TimelineRow, diff_rows,
    display_name,
};
use crate::ui::composer::{Composer, ComposerEvent, file_icon};
use crate::ui::emoji_picker::emoji_grid;
use crate::ui::math::MathPlugin;

/// How many messages to load when a channel is opened.
pub const HISTORY_PAGE: usize = 50;

/// Most reply originals fetched at once.
const REPLY_FETCHES: usize = 4;

pub enum ChatEvent {
    LogOutRequested,
    /// The server no longer accepts our session.
    SessionRejected,
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
    /// Where `place` and `selected` are kept between launches (`None` if
    /// there's nowhere to keep them), and what was last written there.
    places_file: Option<PlacesFile>,
    saved_places: Places,
    /// File server base URL for icons, from `GET /`. `None` until loaded.
    /// Where images come from. Only the API base is known until `GET /` loads.
    media: MediaUrls,
    panes: HashMap<String, Pane>,
    /// History requests in flight, by channel. Results always land in their
    /// own channel, so a slow one can't overwrite the channel being viewed.
    loading: HashMap<String, Task<()>>,
    load_errors: HashMap<String, SharedString>,
    /// Older-page requests in flight, and the last failure, by channel.
    loading_older: HashMap<String, Task<()>>,
    older_errors: HashMap<String, SharedString>,
    /// The one read-acknowledgement waiting to go out: (channel, message, task).
    /// Replacing it (newer message, other channel) cancels the old one.
    pending_ack: Option<(String, String, Task<()>)>,
    /// The count last shown on the Dock icon, to only touch AppKit on change.
    dock_badge: usize,
    /// macOS banners still showing: channel (also the banner's tag) → the
    /// message it's for. Dismissed once the channel is read past it.
    banners: HashMap<String, String>,
    /// The image open in the full-size viewer, over everything else.
    viewer: Option<ViewedImage>,
    /// Focused while the viewer is open, so Esc reaches it.
    viewer_focus: FocusHandle,
    /// The message whose reaction picker is open.
    picker_for: Option<String>,
    /// Reaction changes waiting on the server, as (message, emoji). Another
    /// click on the same one waits, so requests can't cross.
    reacting: HashSet<(String, String)>,
    /// Reply originals being fetched, by message ID (each once).
    reply_fetches: HashSet<String>,
    /// Servers whose full member list was asked for (once per session, for
    /// `@`), and those still loading.
    member_fetches: HashSet<String>,
    members_loading: HashSet<String>,
    /// The message box, under the open channel.
    composer: Entity<Composer>,
    _composer: Subscription,
    /// Others typing, and when to tell Stoat the user is.
    typing: TypingState,
    outgoing_typing: OutgoingTyping,
    /// Wakes up when a typist lapses or the user goes idle.
    typing_tick: Option<Task<()>>,
    connection: Connection,
    _events: Task<()>,
    _root: Task<()>,
    _activation: Subscription,
}

/// Height of the "is typing…" line above the message box.
const TYPING_LINE_HEIGHT: f32 = 16.;

/// How long the newest message must stay on screen before it's marked read,
/// so a burst of live messages sends one acknowledgement, not one each.
const ACK_DELAY: Duration = Duration::from_secs(1);

impl EventEmitter<ChatEvent> for ChatView {}

impl ChatView {
    /// `client` must carry the session token. `me` is the logged-in user's ID.
    pub fn new(
        client: StoatClient,
        me: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Coming back to the window counts as reading what's on screen.
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.maybe_ack_current(window, cx)
        });
        // Banners: ask for permission up front, and make clicking one bring
        // the window forward on its channel (the banner's tag).
        crate::banners::request_permission();
        if crate::banners::available() {
            let view = cx.weak_entity();
            let handle = window.window_handle();
            cx.on_system_notification_response(move |response, cx| {
                let channel = response.tag.to_string();
                log::debug!("banner: clicked {channel}");
                cx.activate(true);
                handle
                    .update(cx, |_, window, cx| {
                        window.activate_window();
                        view.update(cx, |this, cx| this.open_channel(&channel, cx))
                            .ok();
                    })
                    .ok();
            });
        }
        let root = spawn_io({
            let client = client.clone();
            async move { client.root_config().await }
        });
        let api = client.base_url().to_string();
        let root = cx.spawn({
            let api = api.clone();
            async move |this, cx| match root.await {
                Ok(Ok(root)) => {
                    let media = MediaUrls::from_root(&api, &root);
                    let limits = AttachmentLimits::from_root(&root);
                    log::debug!("media: {media:?}, attachments: {limits:?}");
                    this.update(cx, |this, cx| {
                        this.set_media(media, cx);
                        this.composer
                            .update(cx, |composer, _| composer.set_limits(limits));
                    })
                    .ok();
                }
                Ok(Err(err)) => log::warn!("couldn't load API config (no images): {err}"),
                Err(err) => log::warn!("API config task failed: {err:?}"),
            }
        });
        let (connection, mut events) = events::connect(client.clone());
        let events = cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.on_connection_event(event, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        // Until `GET /` loads, only the API base (default avatars) is known.
        let media = MediaUrls::new(api.clone(), None, None);
        // Start where the last launch left off. Anything since deleted is
        // dropped once Ready arrives (`drop_stale_selection`).
        let places_file = PlacesFile::for_account(&api, &me);
        let saved_places = places_file
            .as_ref()
            .map(PlacesFile::load)
            .unwrap_or_default();
        let typing = TypingState::new(me.clone());
        let mut store = ChatStore::new(me);
        store.set_media(media.clone());
        let composer = cx.new(|cx| Composer::new(window, cx));
        let composer_events = cx.subscribe_in(
            &composer,
            window,
            |this, composer, event: &ComposerEvent, window, cx| match event {
                ComposerEvent::Send {
                    text,
                    replies,
                    attachments,
                } => {
                    let Some(channel) = composer.read(cx).channel().map(str::to_string) else {
                        return;
                    };
                    let attachments = attachments.iter().cloned().map(PendingAttachment::new).collect();
                    this.send_message(channel, text.clone(), replies.clone(), attachments, window, cx);
                }
                ComposerEvent::Edit { id, content } => {
                    let Some(channel) = composer.read(cx).channel().map(str::to_string) else {
                        return;
                    };
                    this.edit_message(channel, id.clone(), content.clone(), window, cx);
                }
                ComposerEvent::Input { empty } => {
                    let Some(channel) = composer.read(cx).channel().map(str::to_string) else {
                        return;
                    };
                    let signals =
                        this.outgoing_typing
                            .on_input(&channel, *empty, std::time::Instant::now());
                    this.send_typing(signals, cx);
                }
                ComposerEvent::Complete(query) => {
                    let Some(channel) = composer.read(cx).channel().map(str::to_string) else {
                        return;
                    };
                    this.suggest(&channel, query, true, cx);
                }
                ComposerEvent::EditLast => {
                    let Some(channel) = composer.read(cx).channel().map(str::to_string) else {
                        return;
                    };
                    if let Some(id) = this.store.last_own_message(&channel).map(str::to_string) {
                        this.start_edit(&channel, id, window, cx);
                    }
                }
            },
        );
        Self {
            client,
            store,
            has_ready: false,
            status: Status::Connecting { attempt: 1 },
            place: saved_places.place.clone(),
            selected: saved_places.selected.clone(),
            places_file,
            saved_places,
            media,
            panes: HashMap::new(),
            loading: HashMap::new(),
            load_errors: HashMap::new(),
            loading_older: HashMap::new(),
            older_errors: HashMap::new(),
            pending_ack: None,
            dock_badge: 0,
            banners: HashMap::new(),
            viewer: None,
            viewer_focus: cx.focus_handle(),
            picker_for: None,
            reacting: HashSet::new(),
            reply_fetches: HashSet::new(),
            member_fetches: HashSet::new(),
            members_loading: HashSet::new(),
            composer,
            _composer: composer_events,
            typing,
            outgoing_typing: OutgoingTyping::default(),
            typing_tick: None,
            connection,
            _events: events,
            _root: root,
            _activation: activation,
        }
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                // Sending ends their typing.
                self.typing.stop(&channel, &message.author);
                log::debug!("live: new message {} in {channel}", message.id);
                // Decide before applying: what the user was looking at when it arrived.
                let current = self.current_channel().map(|c| c.id);
                let viewing = Viewing {
                    channel: current.as_deref(),
                    window_active: window.is_window_active(),
                };
                let notice =
                    notify::should_notify(&self.store, &message, viewing, notify::now_ms());
                let message_id = message.id.clone();
                if self.store.apply_message(*message) {
                    self.sync_pane(&channel, cx);
                }
                if let Some(notice) = notice {
                    match notify::delivery(viewing.window_active, crate::banners::available()) {
                        Delivery::Banner => self.show_banner(notice, message_id, cx),
                        Delivery::Toast => self.show_toast(notice, window, cx),
                    }
                }
            }
            ConnectionEvent::Event(ServerEvent::UserSettingsUpdate { notifications }) => {
                match notifications {
                    Some(settings) => {
                        log::debug!("notification settings updated");
                        self.store.set_notification_settings(settings);
                    }
                    None => return,
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
            ConnectionEvent::Event(ServerEvent::MessageReact {
                id,
                channel,
                user,
                emoji,
            }) => {
                if self.store.apply_react(&channel, &id, &user, &emoji, true) {
                    self.sync_pane(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::MessageUnreact {
                id,
                channel,
                user,
                emoji,
            }) => {
                if self.store.apply_react(&channel, &id, &user, &emoji, false) {
                    self.sync_pane(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::MessageRemoveReaction { id, channel, emoji }) => {
                if self.store.apply_remove_reaction(&channel, &id, &emoji) {
                    self.sync_pane(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::MessageAppend {
                id,
                channel,
                embeds,
            }) => {
                log::debug!("live: {} embed(s) for {id} in {channel}", embeds.len());
                if self.store.apply_append(&channel, &id, embeds) {
                    self.sync_pane(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::BulkMessageDelete { channel, ids }) => {
                if self.store.apply_delete(&channel, &ids) {
                    self.sync_pane(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::ChannelAck {
                channel,
                user,
                message_id,
            }) => {
                // Read elsewhere (another of our sessions): clear it here too.
                if user == self.store.me() {
                    self.store.apply_ack(&channel, &message_id);
                } else {
                    return;
                }
            }
            ConnectionEvent::Event(ServerEvent::ChannelCreate(channel)) => {
                log::debug!("live: new channel {}", crate::store::channel_id(&channel));
                self.store.add_channel(*channel);
            }
            ConnectionEvent::Event(ServerEvent::ChannelUpdate { id, data, clear }) => {
                log::debug!("live: channel {id} updated");
                self.store.apply_channel_update(&id, *data, &clear);
            }
            ConnectionEvent::Event(ServerEvent::ChannelStartTyping { channel, user }) => {
                self.typing.start(&channel, &user, std::time::Instant::now());
                self.schedule_typing_tick(cx);
            }
            ConnectionEvent::Event(ServerEvent::ChannelStopTyping { channel, user }) => {
                if !self.typing.stop(&channel, &user) {
                    return;
                }
            }
            ConnectionEvent::Event(ServerEvent::EmojiCreate(emoji)) => {
                self.store.add_emoji(*emoji);
                return;
            }
            ConnectionEvent::Event(ServerEvent::EmojiDelete { id }) => {
                self.store.remove_emoji(&id);
                return;
            }
            ConnectionEvent::Event(_) => return,
            ConnectionEvent::Disconnected { reason, retry_in } => {
                // Their stops may never arrive now.
                self.typing.clear();
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

    /// Image URLs are known: give them to the store and redraw anything loaded.
    fn set_media(&mut self, media: MediaUrls, cx: &mut Context<Self>) {
        self.media = media.clone();
        self.store.set_media(media);
        let loaded: Vec<String> = self.panes.keys().cloned().collect();
        for channel in loaded {
            self.sync_pane(&channel, cx);
        }
        cx.notify();
    }

    fn on_row_action(
        &mut self,
        channel: &str,
        action: RowAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            // "Show image" on a large image: load it after all.
            RowAction::RevealImage(file_id) => {
                self.store.reveal_image(&file_id);
                self.sync_pane(channel, cx);
            }
            RowAction::ViewImage(image) => {
                self.viewer = Some(image);
                window.focus(&self.viewer_focus, cx);
            }
            RowAction::ToggleReaction { message, emoji } => {
                self.picker_for = None;
                self.toggle_reaction(channel.to_string(), message, emoji, window, cx);
            }
            RowAction::Picker(message) => self.picker_for = message,
            RowAction::RetrySend(key) => self.retry_send(channel, &key, window, cx),
            RowAction::DiscardSend(key) => {
                if self.store.discard_pending(channel, &key) {
                    self.sync_pane(channel, cx);
                }
            }
            RowAction::Reply(target) => {
                let added = self
                    .composer
                    .update(cx, |composer, cx| composer.add_reply(target, window, cx));
                if added.is_err() {
                    window.push_notification(Notification::warning(TOO_MANY_REPLIES_TEXT), cx);
                }
            }
            RowAction::Edit(id) => self.start_edit(channel, id, window, cx),
            RowAction::Delete { id, confirmed: true } => {
                self.delete_message(channel.to_string(), id, window, cx)
            }
            RowAction::Delete {
                id,
                confirmed: false,
            } => self.confirm_delete(channel.to_string(), id, window, cx),
            RowAction::CopyText(id) => {
                if let Some(text) = self.store.raw_content(channel, &id) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                }
            }
            RowAction::JumpTo(id) => {
                if let Some(pane) = self.panes.get(channel)
                    && let Some(ix) = pane.rows.iter().position(
                        |row| matches!(row, TimelineRow::Message(m) if m.id == id),
                    )
                {
                    pane.scroller
                        .update(cx, |scroller, cx| scroller.scroll_to_item(ix, cx));
                }
            }
        }
        cx.notify();
    }

    /// Send `content` and `attachments` in `channel`: shown at once as
    /// pending (and scrolled to), then the files are uploaded and it's
    /// posted. Stoat's answer, or the live event if that's first, turns it
    /// into the real message.
    fn send_message(
        &mut self,
        channel: String,
        content: String,
        replies: Vec<ReplyIntent>,
        attachments: Vec<PendingAttachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Sending ends typing (the box is cleared without a change event).
        let stopped = self.outgoing_typing.stop();
        self.send_typing(stopped, cx);
        let sent_ms = notify::now_ms().max(0) as u64;
        let nonce = crate::draft::new_nonce(sent_ms);
        let pending = PendingMessage {
            key: nonce.clone(),
            nonce,
            channel: channel.clone(),
            content,
            replies,
            attachments,
            state: PendingState::Sending,
            sent_ms,
            nonce_used: false,
        };
        self.store.add_pending(pending.clone());
        self.sync_pane(&channel, cx);
        if let Some(pane) = self.panes.get(&channel) {
            pane.scroller.update(cx, |scroller, cx| scroller.scroll_to_end(cx));
        }
        self.post_pending(pending, window, cx);
        cx.notify();
    }

    /// Tell Stoat about the user's typing, and wake up for the idle timeout.
    fn send_typing(
        &mut self,
        signals: impl IntoIterator<Item = TypingSignal>,
        cx: &mut Context<Self>,
    ) {
        for signal in signals {
            log::debug!("typing: {signal:?}");
            match signal {
                TypingSignal::Begin(channel) => self.connection.begin_typing(&channel),
                TypingSignal::End(channel) => self.connection.end_typing(&channel),
            }
        }
        self.schedule_typing_tick(cx);
    }

    /// Wake up when the next typist lapses or the user goes idle, to redraw
    /// the typing line and send `EndTyping`. Replaces the previous wake-up.
    fn schedule_typing_tick(&mut self, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        let next = [self.typing.next_expiry(now), self.outgoing_typing.deadline()]
            .into_iter()
            .flatten()
            .min();
        self.typing_tick = next.map(|at| {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(at.saturating_duration_since(std::time::Instant::now()))
                    .await;
                this.update(cx, |this, cx| {
                    let now = std::time::Instant::now();
                    this.typing.prune(now);
                    let idle = this.outgoing_typing.on_tick(now);
                    this.send_typing(idle, cx);
                    cx.notify();
                })
                .ok();
            })
        });
    }

    /// The "Ada is typing…" line above the message box. Always there, empty
    /// when nobody's typing, so the list above doesn't move.
    fn render_typing_line(&self, channel: &str, cx: &mut Context<Self>) -> Div {
        let names: Vec<String> = self
            .typing
            .typists(channel, std::time::Instant::now())
            .into_iter()
            .map(|user| self.store.name_in(channel, user))
            .collect();
        div()
            .h(px(TYPING_LINE_HEIGHT))
            .flex_none()
            .px_5()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .truncate()
            .children(typing_label(&names))
    }

    /// Answer the message box's `query` with suggestions from what's known
    /// about `channel`. `fetch`: in a server, load its members if needed.
    fn suggest(&mut self, channel: &str, query: &Query, fetch: bool, cx: &mut Context<Self>) {
        let mut loading = false;
        let suggestions = match query.kind {
            QueryKind::Mention => {
                // In a server, everyone can be mentioned: fetch them all the
                // first time, showing who's known meanwhile.
                if let Some(server) = self.store.channel_server(channel).map(str::to_string) {
                    loading = if fetch {
                        self.fetch_members(server, cx)
                    } else {
                        self.members_loading.contains(&server)
                    };
                }
                rank_people(&query.query, &self.store.mention_candidates(channel))
                    .iter()
                    .map(Suggestion::person)
                    .collect()
            }
            QueryKind::Channel => {
                let channels = self.store.channel_candidates(channel);
                rank(&query.query, &channels, |c| vec![c.name.as_str()], |_| false)
                    .iter()
                    .map(|c| Suggestion::channel(&c.id, &c.name))
                    .collect()
            }
            QueryKind::Emoji => Vec::new(),
        };
        self.composer.update(cx, |composer, cx| {
            composer.set_suggestions(query, suggestions, loading, cx)
        });
    }

    /// Fetch `server`'s whole member list, unless it was already asked for
    /// this session. When it arrives, open `@` suggestions there refresh.
    /// Returns whether it's still loading.
    fn fetch_members(&mut self, server: String, cx: &mut Context<Self>) -> bool {
        if !self.member_fetches.insert(server.clone()) {
            return self.members_loading.contains(&server);
        }
        self.members_loading.insert(server.clone());
        let request = spawn_io({
            let (client, server) = (self.client.clone(), server.clone());
            async move { client.fetch_members(&server).await }
        });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.members_loading.remove(&server);
                match result {
                    Ok(Ok(members)) => {
                        log::info!("{} members in {server}", members.members.len());
                        this.store.add_members(members);
                    }
                    // Asked again on the next `@` there.
                    Ok(Err(err)) => {
                        log::warn!("couldn't load members of {server}: {err}");
                        this.member_fetches.remove(&server);
                    }
                    Err(err) => {
                        log::warn!("member task failed: {err:?}");
                        this.member_fetches.remove(&server);
                    }
                }
                // Not fetching again: after a failure, that would loop.
                this.refresh_suggestions(cx);
            })
            .ok();
        })
        .detach();
        true
    }

    /// Suggest again for what the message box is completing (something it
    /// draws on has changed).
    fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        let composer = self.composer.read(cx);
        let (Some(channel), Some(query)) = (
            composer.channel().map(str::to_string),
            composer.completion_query().cloned(),
        ) else {
            return;
        };
        self.suggest(&channel, &query, false, cx);
    }

    /// Edit the user's message `id` in the message box.
    fn start_edit(&mut self, channel: &str, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let stopped = self.outgoing_typing.stop();
        self.send_typing(stopped, cx);
        let content = self.store.raw_content(channel, &id).unwrap_or_default().to_string();
        let store = &self.store;
        let labels = |token: &TokenRef| store.token_label(channel, token);
        self.composer.update(cx, |composer, cx| {
            composer.start_edit(id, &content, &labels, window, cx)
        });
    }

    /// Save an edit: shown at once, then sent. If Stoat refuses, the old
    /// text comes back and a toast says why. The echo (`MessageUpdate`)
    /// brings the same text, so it changes nothing visible.
    fn edit_message(
        &mut self,
        channel: String,
        id: String,
        content: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.store.raw_content(&channel, &id) == Some(content.as_str()) {
            return;
        }
        let Some(edit) = self.store.apply_local_edit(&channel, &id, &content) else {
            return;
        };
        self.sync_pane(&channel, cx);
        let client = self.client.clone();
        let request = spawn_io({
            let (channel, id) = (channel.clone(), id.clone());
            async move { client.edit_message(&channel, &id, &content).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                let error = match result {
                    Ok(Ok(_)) => {
                        log::debug!("edited {id} in {channel}");
                        return;
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't edit {id} in {channel}: {err:?}");
                        edit_error_text(&err)
                    }
                    Err(err) => {
                        log::warn!("edit task failed: {err:?}");
                        "Couldn't save your edit.".into()
                    }
                };
                if this.store.undo_local_edit(&channel, &id, edit) {
                    this.sync_pane(&channel, cx);
                }
                window.push_notification(Notification::error(error), cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Ask before deleting: a dialog showing the message, with Cancel and
    /// Delete (Enter confirms, Esc cancels).
    fn confirm_delete(
        &mut self,
        channel: String,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snippet: SharedString = self
            .store
            .message_snippet(&channel, &id)
            .unwrap_or_default()
            .into();
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let theme = cx.theme();
            let (view, channel, id) = (view.clone(), channel.clone(), id.clone());
            alert
                .confirm()
                .title("Delete message")
                .description("This can't be undone. Hold Shift when choosing Delete to skip this.")
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.secondary)
                        .text_sm()
                        .line_clamp(3)
                        .child(snippet.clone()),
                )
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Cancel")
                .on_ok(move |_, window, cx| {
                    let (channel, id) = (channel.clone(), id.clone());
                    view.update(cx, |this, cx| this.delete_message(channel, id, window, cx))
                        .ok();
                    true
                })
        });
    }

    /// Delete one of the user's messages: gone at once, then sent. If Stoat
    /// refuses, it's put back in place and a toast says why. The echo
    /// (`MessageDelete`) changes nothing by then.
    fn delete_message(
        &mut self,
        channel: String,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Editing it makes no sense any more.
        self.composer
            .update(cx, |composer, cx| composer.stop_editing(&id, window, cx));
        let Some(removed) = self.store.remove_local(&channel, &id) else {
            return;
        };
        self.sync_pane(&channel, cx);
        let client = self.client.clone();
        let request = spawn_io({
            let (channel, id) = (channel.clone(), id.clone());
            async move { client.delete_message(&channel, &id).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                let error = match result {
                    Ok(Ok(())) => {
                        log::debug!("deleted {id} in {channel}");
                        return;
                    }
                    // Already gone (deleted elsewhere): what we wanted.
                    Ok(Err(err)) if matches!(err.error_type(), Some(revolt_result::ErrorType::NotFound)) => {
                        return;
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't delete {id} in {channel}: {err:?}");
                        delete_error_text(&err)
                    }
                    Err(err) => {
                        log::warn!("delete task failed: {err:?}");
                        "Couldn't delete the message.".into()
                    }
                };
                if this.store.restore_local(&channel, removed) {
                    this.sync_pane(&channel, cx);
                }
                window.push_notification(Notification::error(error), cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Try a failed message again (with the same key, unless Stoat has
    /// already seen that one: see [`ChatStore::retry_pending`]).
    fn retry_send(&mut self, channel: &str, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let fresh = || crate::draft::new_nonce(notify::now_ms().max(0) as u64);
        if let Some(pending) = self.store.retry_pending(channel, key, fresh) {
            self.sync_pane(channel, cx);
            self.post_pending(pending, window, cx);
        }
    }

    /// Upload a pending message's files that aren't yet, one at a time,
    /// then `POST` it with their IDs. Success confirms it (unless its echo
    /// already has); failure marks it failed, with the reason.
    fn post_pending(&mut self, pending: PendingMessage, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let file_server = self.media.file_server().map(str::to_string);
        cx.spawn_in(window, async move |this, cx| {
            let PendingMessage { key, channel, .. } = pending.clone();
            let mut ids = Vec::with_capacity(pending.attachments.len());
            for file in &pending.attachments {
                if let Some(id) = file.uploaded_id() {
                    ids.push(id.to_string());
                    continue;
                }
                let uploaded = match &file_server {
                    Some(server) => {
                        upload_file(&this, &client, server, &channel, &key, &file.file, cx).await
                    }
                    None => Err("no file server".to_string()),
                };
                let Ok(id) = uploaded.map_err(|reason| {
                    let reason = format!("Couldn't upload {}: {reason}", file.file.name);
                    // Nothing was posted, so the key is still unused.
                    this.update(cx, |this, cx| {
                        if this.store.fail_pending(&channel, &key, reason, false) {
                            this.sync_pane(&channel, cx);
                        }
                        cx.notify();
                    })
                    .ok();
                }) else {
                    return;
                };
                ids.push(id);
            }

            let result = spawn_io({
                let pending = pending.clone();
                let client = client.clone();
                async move {
                    client
                        .send_message(
                            &pending.channel,
                            &pending.content,
                            &ids,
                            &pending.replies,
                            &pending.nonce,
                        )
                        .await
                }
            })
            .await;
            this.update_in(cx, |this, _, cx| {
                let (reason, answered) = match result {
                    Ok(Ok(message)) => {
                        log::debug!("sent {} in {channel}", message.id);
                        if this.store.apply_message(message) {
                            this.sync_pane(&channel, cx);
                        }
                        cx.notify();
                        return;
                    }
                    // This key got through before (an earlier attempt lost
                    // its answer): the message exists. It shows once its
                    // echo or the next history load brings it.
                    Ok(Err(err)) if is_duplicate(&err) => {
                        log::info!("{key} in {channel} was already sent");
                        if this.store.discard_pending(&channel, &key) {
                            this.sync_pane(&channel, cx);
                        }
                        cx.notify();
                        return;
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't send in {channel}: {err:?}");
                        let answered = !matches!(err, ApiError::Network(_));
                        (send_error_text(&err), answered)
                    }
                    // The request may or may not have reached Stoat.
                    Err(err) => {
                        log::warn!("send task failed: {err:?}");
                        ("Something went wrong sending this.".to_string(), false)
                    }
                };
                if this.store.fail_pending(&channel, &key, reason, answered) {
                    this.sync_pane(&channel, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Point the message box at the open channel (swapping drafts), and
    /// focus it when the channel changes.
    fn sync_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let channel = self.current_channel();
        let id = channel.as_ref().map(|c| c.id.as_str());
        if self.composer.read(cx).channel() == id {
            return;
        }
        // Not typing in the old channel any more.
        let stopped = self.outgoing_typing.stop();
        self.send_typing(stopped, cx);
        let placeholder = channel
            .as_ref()
            .map(composer_placeholder)
            .unwrap_or_default();
        let store = &self.store;
        let labels = |token: &TokenRef| store.token_label(id.unwrap_or_default(), token);
        self.composer.update(cx, |composer, cx| {
            composer.set_channel(id, placeholder.into(), &labels, window, cx);
            if id.is_some() && self.viewer.is_none() {
                composer.focus(window, cx);
            }
        });
    }

    /// Add or remove the user's reaction: shown at once, then sent. If the
    /// server refuses, it's undone and a toast says why. The server echoes a
    /// successful change back as an event, which changes nothing by then.
    fn toggle_reaction(
        &mut self,
        channel: String,
        message: String,
        emoji: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = (message.clone(), emoji.clone());
        if !self.reacting.insert(pending.clone()) {
            return;
        }
        let add = !self.store.has_reacted(&channel, &message, &emoji);
        if add
            && self
                .store
                .would_exceed_reactions(&channel, &message, &emoji)
        {
            self.reacting.remove(&pending);
            window.push_notification(Notification::warning(FULL_REACTIONS_TEXT), cx);
            return;
        }
        let me = self.store.me().to_string();
        if self.store.apply_react(&channel, &message, &me, &emoji, add) {
            self.sync_pane(&channel, cx);
        }
        let client = self.client.clone();
        let request = spawn_io({
            let (channel, message, emoji) = (channel.clone(), message.clone(), emoji.clone());
            async move {
                if add {
                    client.react(&channel, &message, &emoji).await
                } else {
                    client.unreact(&channel, &message, &emoji).await
                }
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.reacting.remove(&pending);
                let error = match result {
                    Ok(Ok(())) => return,
                    Ok(Err(err)) => {
                        log::warn!("couldn't change reaction {emoji} on {message}: {err:?}");
                        reaction_error_text(&err)
                    }
                    Err(err) => {
                        log::warn!("reaction task failed: {err:?}");
                        "Couldn't update your reaction.".into()
                    }
                };
                if this
                    .store
                    .apply_react(&channel, &message, &me, &emoji, !add)
                {
                    this.sync_pane(&channel, cx);
                }
                window.push_notification(Notification::error(error), cx);
            })
            .ok();
        })
        .detach();
    }

    /// Close the full-size viewer, giving focus back to the message box.
    fn close_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.viewer = None;
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
        cx.notify();
    }

    /// The full-size viewer: the image fitted to the window (never enlarged)
    /// on a dimmed backdrop, under a bar with its name and actions. It takes
    /// every click and scroll, so the messages underneath don't react.
    fn render_viewer(
        &self,
        image: &ViewedImage,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let viewport = window.viewport_size();
        let room = |side: Pixels, taken: f32| (f32::from(side) - taken).max(1.) as u32;
        let max_w = room(viewport.width, 2. * VIEWER_MARGIN);
        let max_h = room(viewport.height, 2. * VIEWER_MARGIN + VIEWER_BAR);
        // Unknown dimensions: the whole space, letterboxed.
        let (w, h) =
            fit_into(image.original.0, image.original.1, max_w, max_h).unwrap_or((max_w, max_h));
        let open = image.url.clone();
        let bar = h_flex()
            .h(px(VIEWER_BAR))
            .flex_none()
            .px_4()
            .gap_2()
            .bg(theme.background)
            .text_color(theme.foreground)
            .border_b_1()
            .border_color(theme.border)
            // Clicks on the bar aren't clicks on the backdrop.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(image.name.clone()),
            )
            .child(
                Button::new("viewer-open")
                    .ghost()
                    .small()
                    .icon(IconName::ExternalLink)
                    .label("Open in browser")
                    .on_click(move |_, _, cx| cx.open_url(&open)),
            )
            .child(
                Button::new("viewer-close")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .tooltip("Close (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| this.close_viewer(window, cx))),
            );
        div()
            .id("image-viewer")
            .track_focus(&self.viewer_focus)
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .bg(black().opacity(0.85))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_viewer(window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_viewer(window, cx)),
            )
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(w as f32))
                            .h(px(h as f32))
                            // Clicking the picture itself doesn't close it.
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                img(SharedUri::from(image.url.clone()))
                                    .id("image-viewer-img")
                                    .size_full()
                                    .object_fit(ObjectFit::Contain)
                                    .with_loading(move || image_loading(muted, 0.)),
                            ),
                    ),
            )
    }

    fn select_place(&mut self, place: Place, cx: &mut Context<Self>) {
        self.place = place;
        self.remember_places();
        self.ensure_current_loaded(cx);
        cx.notify();
    }

    /// An in-app toast for a notifying message; one per channel (a newer
    /// message replaces the older toast). Clicking it opens the channel.
    fn show_toast(&mut self, notice: Notice, window: &mut Window, cx: &mut Context<Self>) {
        log::debug!("notify: {} | {}", notice.title, notice.body);
        let view = cx.entity().downgrade();
        let channel = notice.channel.clone();
        let toast = Notification::new()
            .id1::<ChatView>(SharedString::from(notice.tag))
            .title(notice.title)
            .message(notice.body)
            .on_click(move |_, _, cx| {
                view.update(cx, |this, cx| this.open_channel(&channel, cx))
                    .ok();
            });
        window.push_notification(toast, cx);
    }

    /// A macOS banner for a notifying message while RuSToat is in the
    /// background. The channel is the tag, so a newer message replaces the
    /// channel's older banner, and a click opens it (see `new`). Reading the
    /// channel takes the banner away (`dismiss_read_banners`).
    fn show_banner(&mut self, notice: Notice, message_id: String, cx: &mut Context<Self>) {
        log::debug!("banner: {} | {}", notice.title, notice.body);
        cx.show_system_notification(SystemNotification {
            tag: notice.channel.clone().into(),
            title: notice.title.into(),
            body: notice.body.into(),
            actions: Vec::new(),
        });
        self.banners.insert(notice.channel, message_id);
    }

    /// Take away banners for channels read since (here, in another session,
    /// by posting, or with Mark as read).
    fn dismiss_read_banners(&mut self, cx: &mut Context<Self>) {
        let store = &self.store;
        self.banners.retain(|channel, message| {
            let read = store.has_read(channel, message);
            if read {
                log::debug!("banner: dismissing {channel} (read)");
                cx.dismiss_system_notification(channel);
            }
            !read
        });
    }

    /// Show `channel`: switch to its server (or Home) and select it.
    pub fn open_channel(&mut self, channel: &str, cx: &mut Context<Self>) {
        let place = match self.store.channel(channel) {
            Some(crate::api::models::Channel::TextChannel { server, .. }) => {
                Place::Server(server.clone())
            }
            Some(_) => Place::Home,
            None => return,
        };
        self.place = place.clone();
        self.selected.insert(place, channel.to_string());
        self.remember_places();
        self.ensure_current_loaded(cx);
        cx.notify();
    }

    fn select_channel(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected.insert(self.place.clone(), id);
        self.remember_places();
        self.ensure_current_loaded(cx);
        cx.notify();
    }

    /// Save the open place and picked channels for next launch, if they changed.
    fn remember_places(&mut self) {
        let Some(file) = &self.places_file else {
            return;
        };
        let places = Places {
            place: self.place.clone(),
            selected: self.selected.clone(),
        };
        if places == self.saved_places {
            return;
        }
        if let Err(err) = file.save(&places) {
            log::warn!("couldn't save places: {err:#}");
        }
        self.saved_places = places;
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
        // Not loaded yet (a message sent while it loads): the load draws it.
        if self.store.history(channel).is_none() {
            return;
        }
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
            self.fetch_reply_sources(channel, cx);
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
        self.fetch_reply_sources(channel, cx);
    }

    /// Fetch originals that `channel`'s replies need, a few at a time. Each
    /// result re-syncs the channel, which starts the next ones.
    fn fetch_reply_sources(&mut self, channel: &str, cx: &mut Context<Self>) {
        let missing = self.store.missing_reply_sources(channel);
        for id in missing {
            if self.reply_fetches.len() >= REPLY_FETCHES {
                break;
            }
            if !self.reply_fetches.insert(id.clone()) {
                continue;
            }
            let client = self.client.clone();
            let request = spawn_io({
                let (channel, id) = (channel.to_string(), id.clone());
                // Just the message (and a neighbour each side), with its author.
                async move { client.fetch_messages_around(&channel, &id, 1).await }
            });
            let channel = channel.to_string();
            cx.spawn(async move |this, cx| {
                let result = request.await;
                let wait = match &result {
                    Ok(Err(ApiError::RateLimited { retry_after_ms })) => {
                        Some(Duration::from_millis(retry_after_ms.unwrap_or(2_000)))
                    }
                    _ => None,
                };
                if let Some(wait) = wait {
                    log::debug!("reply source {id}: rate limited, waiting {wait:?}");
                    cx.background_executor().timer(wait).await;
                }
                this.update(cx, |this, cx| {
                    this.reply_fetches.remove(&id);
                    match result {
                        Ok(Ok(page)) => this.store.add_reply_sources(page, &[id]),
                        // Tried again below, now the wait is over.
                        Ok(Err(ApiError::RateLimited { .. })) => {}
                        Ok(Err(err)) => {
                            log::warn!("couldn't load reply source {id}: {err:?}");
                            this.store.reply_source_unavailable(&id);
                        }
                        Err(err) => {
                            log::warn!("reply source task failed: {err:?}");
                            this.store.reply_source_unavailable(&id);
                        }
                    }
                    this.sync_pane(&channel, cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
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

    /// Mark the open channel read on Stoat, if the user can actually see its
    /// newest message: the window is active and the list is at the bottom.
    /// Debounced by [`ACK_DELAY`]; at most one acknowledgement is pending.
    fn maybe_ack_current(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.has_ready || !window.is_window_active() {
            return;
        }
        let Some(channel) = self.current_channel().map(|c| c.id) else {
            return;
        };
        // "Not scrolled up" covers following the tail, being scrolled to the
        // end by hand, and content too short to scroll at all.
        let at_bottom = self
            .panes
            .get(&channel)
            .is_some_and(|pane| !pane.scroller.read(cx).is_scrolled_up());
        if !at_bottom {
            return;
        }
        let Some(latest) = self.store.unacked_latest(&channel) else {
            return;
        };
        if matches!(&self.pending_ack, Some((c, m, _)) if *c == channel && *m == latest) {
            return;
        }

        let client = self.client.clone();
        let task = cx.spawn({
            let (channel, latest) = (channel.clone(), latest.clone());
            async move |this, cx| {
                cx.background_executor().timer(ACK_DELAY).await;
                // Clear it here straight away; the request follows.
                if this
                    .update(cx, |this, cx| {
                        this.store.apply_ack(&channel, &latest);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                let request = spawn_io({
                    let (channel, latest) = (channel.clone(), latest.clone());
                    async move { client.ack_message(&channel, &latest).await }
                });
                match request.await {
                    Ok(Ok(())) => log::debug!("acked {channel} up to {latest}"),
                    Ok(Err(err)) => log::warn!("couldn't mark {channel} read: {err:?}"),
                    Err(err) => log::warn!("ack task failed: {err:?}"),
                }
                this.update(cx, |this, _| {
                    if matches!(&this.pending_ack, Some((c, m, _)) if *c == channel && *m == latest)
                    {
                        this.pending_ack = None;
                    }
                })
                .ok();
            }
        });
        self.pending_ack = Some((channel, latest, task));
    }

    /// "Mark as read" on a server: clear it here and tell Stoat.
    fn mark_server_read(&mut self, server: String, cx: &mut Context<Self>) {
        self.store.apply_server_ack(&server);
        cx.notify();
        let client = self.client.clone();
        let request = spawn_io({
            let server = server.clone();
            async move { client.ack_server(&server).await }
        });
        cx.spawn(async move |_, _| match request.await {
            Ok(Ok(())) => log::debug!("marked server {server} read"),
            Ok(Err(err)) => log::warn!("couldn't mark server {server} read: {err:?}"),
            Err(err) => log::warn!("server ack task failed: {err:?}"),
        })
        .detach();
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
        let (home_unread, home_mentions) = self.store.home_summary();
        let home = rail_button(
            "rail-home",
            Icon::new(IconName::House).into_any_element(),
            "Direct Messages",
            self.place == Place::Home,
            home_unread,
            home_mentions,
            None,
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
                let icon = server_icon(&server, &self.media, selected);
                let view = cx.entity().downgrade();
                let server_id = server.id.clone();
                let mark_read: MarkRead = Rc::new(move |_, _, cx| {
                    let server_id = server_id.clone();
                    view.update(cx, |this, cx| this.mark_server_read(server_id, cx))
                        .ok();
                });
                rail_button(
                    SharedString::from(format!("rail-{}", server.id)),
                    icon,
                    server.name,
                    selected,
                    server.unread,
                    server.mentions,
                    Some(mark_read),
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
        let unread_fg = theme.sidebar_foreground;
        let id = channel.id.clone();
        let (unread, mentions) = (channel.unread, channel.mentions);
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
                el.text_color(if unread {
                    unread_fg
                } else {
                    theme.muted_foreground
                })
                .when(unread, |el| el.font_weight(FontWeight::SEMIBOLD))
                .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.select_channel(id.clone(), cx)))
            .child(match channel.avatar.clone() {
                Some(url) => avatar(
                    format!("channel-avatar-{}", channel.id),
                    Some(url),
                    &channel.name,
                    20.,
                    theme.secondary,
                    theme.secondary_foreground,
                ),
                None => Icon::new(channel_icon(channel.kind))
                    .small()
                    .into_any_element(),
            })
            .child(div().flex_1().min_w_0().truncate().child(channel.name))
            .when(mentions > 0, |el| el.child(mention_badge(mentions, cx)))
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
        let me_avatar = avatar(
            "my-avatar",
            Some(self.store.user_avatar_url(self.store.me())),
            &name,
            32.,
            theme.secondary,
            theme.secondary_foreground,
        );
        h_flex()
            .h(px(56.))
            .flex_none()
            .px_3()
            .gap_2()
            .justify_between()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex().gap_2().min_w_0().child(me_avatar).child(
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
            avatar_bg: theme.secondary,
            avatar_fg: theme.secondary_foreground,
            border: theme.border,
            link: theme.link,
            mine: theme.primary,
            mine_bg: theme.primary.opacity(0.15),
            card_bg: theme.background,
            image_bg: theme.secondary,
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
                        let pickers = Rc::new(Pickers {
                            choices: Rc::new(self.store.reaction_choices(&channel.id)),
                            open_for: self.picker_for.clone(),
                        });
                        let view = cx.entity().downgrade();
                        let channel_id = channel.id.clone();
                        let on_action: OnRowAction = Rc::new(move |action, window, cx| {
                            view.update(cx, |this, cx| {
                                this.on_row_action(&channel_id, action, window, cx)
                            })
                            .ok();
                        });
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
                                    move |ix, _window, _cx| {
                                        render_row(&rows[ix], colors, &on_action, &pickers)
                                    },
                                )
                                // The scroller's default rows are spaced for whole
                                // chat turns (`pb_8` under every row, plus `px_3`).
                                // Ours are single messages, which space themselves.
                                .with_row_style(StyleRefinement::default().pb(px(0.)).px(px(0.)))
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
            // Files dropped anywhere on the conversation are attached.
            .relative()
            .group(DROP_GROUP)
            .when(channel.is_some(), |el| {
                el.on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    // The image viewer covers the conversation.
                    if this.viewer.is_some() {
                        return;
                    }
                    let paths = paths.paths().to_vec();
                    this.composer
                        .update(cx, |composer, cx| composer.add_files(paths, window, cx));
                }))
            })
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
            .when_some(channel.as_ref(), |el, channel| {
                el.child(self.render_typing_line(&channel.id, cx))
                    .child(self.composer.clone())
                    .child(render_drop_overlay(cx))
            })
    }
}

/// Group name for the conversation, which files can be dropped on.
const DROP_GROUP: &str = "conversation-drop";

/// "Drop to attach" over the conversation, shown only while files are
/// dragged over it.
fn render_drop_overlay(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .absolute()
        .inset_0()
        .p_3()
        .invisible()
        .group_drag_over::<ExternalPaths>(DROP_GROUP, |style| style.visible())
        .child(
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .rounded_lg()
                .border_2()
                .border_dashed()
                .border_color(theme.primary)
                .bg(theme.background.opacity(0.85))
                .text_color(theme.primary)
                .child(Icon::new(IconName::Paperclip).large())
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Drop to attach")),
        )
}

#[derive(Clone, Copy)]
struct RowColors {
    muted: Hsla,
    system: Hsla,
    danger: Hsla,
    avatar_bg: Hsla,
    avatar_fg: Hsla,
    border: Hsla,
    link: Hsla,
    /// Outline and count of reactions the user is part of.
    mine: Hsla,
    mine_bg: Hsla,
    /// File cards, so they stand out inside a bubble.
    card_bg: Hsla,
    /// Behind an image while it loads, or if it fails.
    image_bg: Hsla,
}

/// Something a message row asks the view to do.
#[derive(Clone)]
enum RowAction {
    /// "Show image": load a large image after all (by file ID).
    RevealImage(String),
    /// Open an inline image in the full-size viewer.
    ViewImage(ViewedImage),
    /// Add the user's `emoji` reaction to `message`, or take it back if
    /// they've already reacted with it.
    ToggleReaction { message: String, emoji: String },
    /// Open the reaction picker on a message (`Some`), or close it.
    Picker(Option<String>),
    /// Send a failed message again (by its pending key).
    RetrySend(String),
    /// Give up on a failed message (by its pending key).
    DiscardSend(String),
    /// Scroll to a loaded message (a reply's original).
    JumpTo(String),
    /// Reply to a message (from the hover toolbar or the right-click menu).
    Reply(ReplyTarget),
    /// Copy a message's text as typed (its Markdown source).
    CopyText(String),
    /// Edit one of the user's messages in the message box.
    Edit(String),
    /// Delete one of the user's messages, after asking (unless Shift was
    /// held: `confirmed`).
    Delete { id: String, confirmed: bool },
}

type OnRowAction = Rc<dyn Fn(RowAction, &mut Window, &mut App)>;

#[derive(Clone)]
struct ViewedImage {
    url: String,
    name: String,
    /// From the upload's metadata; zero if unknown.
    original: (usize, usize),
}

/// Space around the image in the full-size viewer.
const VIEWER_MARGIN: f32 = 32.;
/// Height of the viewer's top bar.
const VIEWER_BAR: f32 = 48.;

/// Matches gpui-component's message avatar slot (`size-8`).
const AVATAR_SIZE: f32 = 32.;

/// A round avatar, falling back to the name's initial while it loads, if it
/// fails, or if there's no image.
/// `id` must be unique among the avatars on screen: the image keeps its
/// loading state under it (without one, the initial never shows while loading).
fn avatar(
    id: impl Into<SharedString>,
    url: Option<String>,
    name: &str,
    size: f32,
    bg: Hsla,
    fg: Hsla,
) -> AnyElement {
    let initial: SharedString = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".into())
        .into();
    let placeholder = move || {
        div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .text_color(fg)
            .text_size(px(size * 0.45))
            .font_weight(FontWeight::SEMIBOLD)
            .child(initial.clone())
            .into_any_element()
    };
    match url {
        Some(url) => img(SharedUri::from(url))
            .id(id.into())
            .size(px(size))
            .flex_none()
            .rounded_full()
            .object_fit(ObjectFit::Cover)
            .with_fallback(placeholder.clone())
            .with_loading(placeholder)
            .into_any_element(),
        None => placeholder(),
    }
}

/// The reaction picker's state for the rows being drawn.
struct Pickers {
    /// What the picker offers in this channel (messages may restrict it).
    choices: Rc<Vec<PickerEmoji>>,
    /// The message whose picker is open.
    open_for: Option<String>,
}

fn render_row(
    row: &TimelineRow,
    colors: RowColors,
    on_action: &OnRowAction,
    pickers: &Pickers,
) -> AnyElement {
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
        TimelineRow::Message(message) => render_message_row(message, colors, on_action, pickers),
    }
}

fn render_system_row(message: &MessageRow, colors: RowColors) -> AnyElement {
    // Centred between the two sides of the conversation.
    h_flex()
        .w_full()
        .justify_center()
        .px_4()
        .py_1p5()
        .gap_2()
        .text_xs()
        .text_color(colors.system)
        .child(
            div()
                .italic()
                .child(message.body.clone().unwrap_or_default()),
        )
        .child(div().child(message.time.clone()))
        .into_any_element()
}

/// Vertical space above the first bubble of a group (between senders).
const GROUP_GAP: f32 = 8.;
/// Vertical space between consecutive bubbles from one sender.
const BUBBLE_GAP: f32 = 2.;
/// Space between a group's name/time header and its first bubble.
const HEADER_GAP: f32 = 2.;

/// Corner radius where a bubble isn't touching a neighbour from its sender.
const BUBBLE_RADIUS: f32 = 16.;
/// Corner radius where consecutive bubbles from one sender meet.
const JOINED_RADIUS: f32 = 4.;

fn render_message_row(
    message: &MessageRow,
    colors: RowColors,
    on_action: &OnRowAction,
    pickers: &Pickers,
) -> AnyElement {
    let first = !message.continuation;
    let last = message.group_end;
    let alignment = if message.mine {
        MessageAlignment::End
    } else {
        MessageAlignment::Start
    };

    // The corners on the sender's side flatten where bubbles from the same
    // person stack, so a group reads as one unit.
    let round = px(BUBBLE_RADIUS);
    let top = px(if first { BUBBLE_RADIUS } else { JOINED_RADIUS });
    let bottom = px(if last { BUBBLE_RADIUS } else { JOINED_RADIUS });
    let surface = if message.mine {
        BubbleContent::new()
            .rounded_tl(round)
            .rounded_bl(round)
            .rounded_tr(top)
            .rounded_br(bottom)
    } else {
        BubbleContent::new()
            .rounded_tr(round)
            .rounded_br(round)
            .rounded_tl(top)
            .rounded_bl(bottom)
    };
    let surface =
        surface
            .when_some(message.body.clone(), |el, body| {
                el.child(
                    // Keyed by `render_key`, so an edit (or a renamed mention)
                    // gets a fresh parse instead of the cached one.
                    TextView::markdown(SharedString::from(message.render_key.clone()), body)
                        // `$$…$$` formatting (colour, size), drawn natively.
                        .plugin(MathPlugin)
                        // Emoji-only messages are drawn big (custom ones are sized
                        // in the Markdown itself).
                        .map(|text| {
                            if message.jumbo {
                                text.text_size(px(JUMBO_TEXT_PX))
                            } else {
                                text.text_sm()
                            }
                        })
                        .selectable(true)
                        .on_link_click(|url, _, _, cx| {
                            // Mentions are internal `stoat:` links: nothing to open yet.
                            if is_external_link(url) {
                                cx.open_url(url);
                            }
                        }),
                )
            })
            .when(!message.attachments.is_empty(), |el| {
                el.child(
                    v_flex()
                        .gap_1()
                        .when(message.body.is_some(), |el| el.mt_1())
                        .children(message.attachments.iter().enumerate().map(
                            |(ix, attachment)| {
                                let id =
                                    SharedString::from(format!("attachment-{}-{ix}", message.id));
                                render_attachment(id, attachment, colors, on_action)
                            },
                        )),
                )
            })
            .when(!message.embeds.is_empty(), |el| {
                el.child(
                    v_flex()
                        .gap_1()
                        .when(
                            message.body.is_some() || !message.attachments.is_empty(),
                            |el| el.mt_1(),
                        )
                        .children(message.embeds.iter().enumerate().map(|(ix, embed)| {
                            let id = SharedString::from(format!("embed-{}-{ix}", message.id));
                            render_embed(id, embed, colors, on_action)
                        })),
                )
            })
            .when(message.edited, |el| {
                el.child(div().text_xs().text_color(colors.muted).child("(edited)"))
            });

    // Big emoji and bare attachments stand on their own: same bubble (so it
    // keeps its width and side), with the surface made invisible. Not
    // `BubbleVariant::Ghost`: gpui-component stretches ghost bubbles to the
    // full row width, which left-aligned emoji-only messages even when they
    // were ours.
    let bare = message.jumbo
        || (message.body.is_none()
            && (!message.attachments.is_empty() || !message.embeds.is_empty()));
    let surface = surface.when(bare, |el| el.bg(transparent_black()).p_0());
    let bubble = Bubble::new()
        .with_variant(if message.mine {
            BubbleVariant::Tinted
        } else {
            BubbleVariant::Secondary
        })
        .content(surface)
        // Nothing to react or reply to until it's on the server.
        .when(message.pending.is_none(), |el| {
            el.reactions(message_toolbar(message, on_action, pickers))
        });

    // The avatar sits beside the last bubble of a group; earlier ones keep
    // the empty slot so the bubbles line up.
    let avatar_slot = if last {
        MessageAvatar::new().child(avatar(
            format!("avatar-{}", message.id),
            message.avatar.clone(),
            &message.author,
            AVATAR_SIZE,
            colors.avatar_bg,
            colors.avatar_fg,
        ))
    } else {
        MessageAvatar::new()
    };

    let header = first.then(|| {
        let name = if message.mine {
            "You".to_string()
        } else {
            message.author.clone()
        };
        MessageHeader::new()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
            .child(div().text_color(colors.muted).child(message.time.clone()))
    });

    let layout = MessageLayout::new()
        .alignment(alignment)
        .avatar_slot(avatar_slot)
        .when_some(header, |el, header| el.header(header))
        .content(
            MessageContent::new()
                .when(!message.replies.is_empty(), |el| {
                    el.gap(px(REPLY_GAP)).children(
                        message
                            .replies
                            .iter()
                            .enumerate()
                            .map(|(ix, reply)| render_reply_preview(message, ix, reply, colors, on_action)),
                    )
                })
                .bubble(bubble),
        )
        .when_some(
            render_reactions(message, colors, on_action),
            |el, footer| el.footer(footer),
        )
        .when_some(
            render_pending_status(message, colors, on_action),
            |el, footer| el.footer(footer),
        )
        // The name/time header sits right above its bubble (default gap is 10 px).
        .with_stack_style(StyleRefinement::default().gap(px(HEADER_GAP)))
        // Reactions sit just under their bubble (default gap is 10 px too).
        .gap(px(REACTIONS_GAP))
        .px_4()
        // Space between groups; bubbles within a group sit close together.
        .pt(px(if first { GROUP_GAP } else { BUBBLE_GAP }));

    let row = div()
        .id(SharedString::from(format!("row-{}", message.key)))
        .relative()
        .group(ROW_GROUP)
        // Muted until Stoat has it.
        .when(message.pending == Some(PendingState::Sending), |el| {
            el.opacity(PENDING_OPACITY)
        })
        .child(layout);
    if message.pending.is_some() {
        return row.into_any_element();
    }
    // Right-click anywhere on the row (text, padding, avatar).
    let (on_action, id, mine) = (on_action.clone(), message.id.clone(), message.mine);
    let reply = reply_target(message);
    row.context_menu(move |menu, _, _| {
        let act = |action: RowAction| {
            let on_action = on_action.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                on_action(action.clone(), window, cx)
            }
        };
        menu.item(
            PopupMenuItem::new("Reply")
                .icon(IconName::Reply)
                .on_click(act(RowAction::Reply(reply.clone()))),
        )
        .item(
            PopupMenuItem::new("Copy text")
                .icon(IconName::Copy)
                .on_click(act(RowAction::CopyText(id.clone()))),
        )
        .when(mine, |menu| {
            let delete = {
                let (on_action, id) = (on_action.clone(), id.clone());
                move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let confirmed = event.modifiers().shift;
                    on_action(RowAction::Delete { id: id.clone(), confirmed }, window, cx)
                }
            };
            menu.separator()
                .item(
                    PopupMenuItem::new("Edit")
                        .icon(IconName::Pencil)
                        .on_click(act(RowAction::Edit(id.clone()))),
                )
                .item(
                    PopupMenuItem::element(|_, cx| {
                        let danger = cx.theme().danger;
                        h_flex()
                            .gap_2()
                            .text_color(danger)
                            .child(Icon::new(IconName::Trash).text_color(danger))
                            .child("Delete message")
                    })
                    .on_click(delete),
                )
        })
    })
    .into_any_element()
}

/// Replying to `message`: pinging its author by default, unless it's ours.
fn reply_target(message: &MessageRow) -> ReplyTarget {
    ReplyTarget {
        id: message.id.clone(),
        author: if message.mine {
            "yourself".into()
        } else {
            message.author.clone()
        },
        mention: !message.mine,
    }
}

/// Shown when replying to a sixth message at once.
const TOO_MANY_REPLIES_TEXT: &str = "You can reply to up to 5 messages at once.";

/// Height of a reply preview line, the same whatever its state, so a row
/// doesn't change height when its original loads.
const REPLY_LINE_HEIGHT: f32 = 18.;
/// Space between reply previews and the bubble.
const REPLY_GAP: f32 = 2.;
/// The original author's avatar in a reply preview.
const REPLY_AVATAR_SIZE: f32 = 14.;

/// "↱ Name: snippet" above a reply. Clicking a loaded original scrolls to it
/// (if it isn't loaded in the list, nothing happens).
fn render_reply_preview(
    message: &MessageRow,
    ix: usize,
    reply: &ReplyPreview,
    colors: RowColors,
    on_action: &OnRowAction,
) -> AnyElement {
    let id = SharedString::from(format!("reply-{}-{ix}", message.id));
    let line = h_flex()
        .id(id.clone())
        .h(px(REPLY_LINE_HEIGHT))
        .max_w_full()
        .min_w_0()
        .gap_1()
        .text_xs()
        .text_color(colors.muted)
        .child(Icon::new(IconName::CornerUpRight).xsmall());
    let line = match reply.state {
        ReplyState::Loaded => {
            let jump = {
                let (on_action, target) = (on_action.clone(), reply.id.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    on_action(RowAction::JumpTo(target.clone()), window, cx)
                }
            };
            let name = if reply.mentions_author {
                format!("@{}", reply.author)
            } else {
                reply.author.clone()
            };
            line.cursor_pointer()
                .on_click(jump)
                .child(avatar(
                    format!("{id}-avatar"),
                    reply.avatar.clone(),
                    &reply.author,
                    REPLY_AVATAR_SIZE,
                    colors.avatar_bg,
                    colors.avatar_fg,
                ))
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(div().min_w_0().truncate().child(reply.snippet.clone()))
        }
        ReplyState::Loading => line.child(div().italic().child("Loading…")),
        ReplyState::Deleted => line.child(div().italic().child("Original message was deleted")),
        ReplyState::Unavailable => {
            line.child(div().italic().child("Couldn't load the original message"))
        }
    };
    line.into_any_element()
}

/// How faint a message is while it's being sent.
const PENDING_OPACITY: f32 = 0.6;

/// Under a message that isn't on the server yet: "Sending…", or why it
/// failed with Retry and Discard. `None` for real messages.
fn render_pending_status(
    message: &MessageRow,
    colors: RowColors,
    on_action: &OnRowAction,
) -> Option<MessageFooter> {
    let status = match message.pending.as_ref()? {
        PendingState::Sending => div()
            .text_xs()
            .text_color(colors.muted)
            .child("Sending…")
            .into_any_element(),
        PendingState::Failed(reason) => {
            let key = message.id.clone();
            let action = |make: fn(String) -> RowAction| {
                let (on_action, key) = (on_action.clone(), key.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    on_action(make(key.clone()), window, cx)
                }
            };
            h_flex()
                .gap_2()
                .text_xs()
                .text_color(colors.danger)
                .child(Icon::new(IconName::CircleAlert).xsmall())
                .child(reason.clone())
                .child(
                    Button::new(SharedString::from(format!("retry-{key}")))
                        .xsmall()
                        .label("Retry")
                        .on_click(action(RowAction::RetrySend)),
                )
                .child(
                    Button::new(SharedString::from(format!("discard-{key}")))
                        .xsmall()
                        .ghost()
                        .label("Discard")
                        .on_click(action(RowAction::DiscardSend)),
                )
                .into_any_element()
        }
    };
    Some(MessageFooter::new().content_inset(false).child(status))
}

/// Group name for hovering a message row.
const ROW_GROUP: &str = "message-row";

/// How far outside the bubble's edge the hover toolbar reaches: two 20 px
/// buttons, a 2 px gap, and 8 px of space from the bubble.
const TOOLBAR_OFFSET: f32 = 50.;

/// The hover toolbar ("add reaction" and Reply), just outside the bubble at
/// its bottom corner (on the side away from the avatar), so the picker opens
/// under the message. It shows while the row is hovered or its picker is
/// open, and takes no space (so rows don't change height).
fn message_toolbar(
    message: &MessageRow,
    on_action: &OnRowAction,
    pickers: &Pickers,
) -> BubbleReactions {
    let open = pickers.open_for.as_deref() == Some(message.id.as_str());
    let choices = Rc::new(message.picker_choices(&pickers.choices));
    let chosen: Rc<HashSet<String>> = Rc::new(
        message
            .reactions
            .iter()
            .filter(|chip| chip.mine)
            .map(|chip| chip.key.clone())
            .collect(),
    );
    let id = message.id.clone();
    let on_open = {
        let (on_action, id) = (on_action.clone(), id.clone());
        move |open: &bool, window: &mut Window, cx: &mut App| {
            on_action(RowAction::Picker(open.then(|| id.clone())), window, cx)
        }
    };
    let on_pick: crate::ui::emoji_picker::OnPick = {
        let (on_action, id) = (on_action.clone(), id.clone());
        Rc::new(move |emoji: &str, window: &mut Window, cx: &mut App| {
            on_action(
                RowAction::ToggleReaction {
                    message: id.clone(),
                    emoji: emoji.to_string(),
                },
                window,
                cx,
            )
        })
    };
    let popover = Popover::new(SharedString::from(format!("react-{id}")))
        // Opens below the button, reaching back under the bubble.
        .anchor(if message.mine {
            Anchor::TopLeft
        } else {
            Anchor::TopRight
        })
        .open(open)
        .on_open_change(on_open)
        .trigger(
            Button::new(SharedString::from(format!("react-{id}-button")))
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .tooltip("Add reaction"),
        )
        .content(move |_, _, cx| {
            let note = choices.full.then_some(FULL_REACTIONS_TEXT);
            emoji_grid(&id, &choices.emoji, note, &chosen, on_pick.clone(), cx)
        });
    let reply = {
        let (on_action, target) = (on_action.clone(), reply_target(message));
        Button::new(SharedString::from(format!("reply-{}-button", message.id)))
            .ghost()
            .xsmall()
            .icon(IconName::Reply)
            .tooltip("Reply")
            .on_click(move |_, window, cx| on_action(RowAction::Reply(target.clone()), window, cx))
    };
    let toolbar = h_flex()
        .gap_0p5()
        .when(!open, |el| {
            el.invisible()
                .group_hover(ROW_GROUP, |style| style.visible())
        })
        .child(popover)
        .child(reply);
    // gpui-component's slot for things on a bubble's edge. Moved fully
    // outside the bubble, level with its bottom, without a pill of its own.
    let outside = px(-TOOLBAR_OFFSET);
    BubbleReactions::new()
        .alignment(if message.mine {
            MessageAlignment::Start
        } else {
            MessageAlignment::End
        })
        .bottom(px(0.))
        .map(|el| {
            if message.mine {
                el.left(outside)
            } else {
                el.right(outside)
            }
        })
        .bg(transparent_black())
        .border_0()
        .p_0()
        .child(toolbar)
}

/// Shown when a message is at [`MAX_REACTIONS`](crate::store::MAX_REACTIONS).
const FULL_REACTIONS_TEXT: &str =
    "This message has as many different reactions as it can (20). You can still join one.";

/// How often a pending message's upload progress is redrawn.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Upload one of the pending message `key`'s files to `file_server`,
/// showing its progress in the row. Returns its ID, or why it failed (for
/// the user).
async fn upload_file(
    this: &WeakEntity<ChatView>,
    client: &StoatClient,
    file_server: &str,
    channel: &str,
    key: &str,
    file: &DraftAttachment,
    cx: &mut AsyncWindowContext,
) -> Result<String, String> {
    use futures::FutureExt;

    let (tx, mut progress) = futures::channel::mpsc::unbounded::<(u64, u64)>();
    let mut upload = spawn_io({
        let (client, file_server, file) = (client.clone(), file_server.to_string(), file.clone());
        async move {
            let path = file.path.clone();
            let bytes = tokio::task::spawn_blocking(move || std::fs::read(path))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("couldn't read it ({e})"))?;
            client
                .upload_attachment(&file_server, &file.name, &file.content_type, bytes, move |sent, total| {
                    tx.unbounded_send((sent, total)).ok();
                })
                .await
                .map_err(|err| upload_error_text(&err))
        }
    })
    .fuse();

    let mut shown: Option<std::time::Instant> = None;
    loop {
        futures::select! {
            update = progress.next() => {
                let Some((sent, total)) = update else { continue };
                let due = shown.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL);
                if !due && sent < total {
                    continue;
                }
                shown = Some(std::time::Instant::now());
                this.update(cx, |this, cx| {
                    if this.store.set_upload_progress(channel, key, file.local_id, sent, total) {
                        this.sync_pane(channel, cx);
                    }
                })
                .ok();
            }
            result = upload => {
                let id = result.map_err(|e| e.to_string())??;
                log::info!("uploaded {} as {id}", file.name);
                this.update(cx, |this, cx| {
                    if this.store.set_uploaded(channel, key, file.local_id, id.clone()) {
                        this.sync_pane(channel, cx);
                    }
                })
                .ok();
                return Ok(id);
            }
        }
    }
}

/// Why an upload failed, after "Couldn't upload {name}: ".
fn upload_error_text(err: &ApiError) -> String {
    match err {
        ApiError::Network(_) => "couldn't reach the file server.".into(),
        ApiError::Unexpected { status: 413, .. } => "it's too big.".into(),
        ApiError::RateLimited { .. } => "uploading too fast. Wait a moment, then retry.".into(),
        _ => match err.error_type() {
            Some(revolt_result::ErrorType::FileTooLarge { .. }) => "it's too big.".into(),
            _ => err.to_string(),
        },
    }
}

/// What a refused reaction's toast says.
fn reaction_error_text(err: &ApiError) -> SharedString {
    use revolt_result::ErrorType;
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => "You can't react here.".into(),
        // The reaction limit (checked before sending, but another client may
        // have just filled it), an emoji the author didn't allow, or a
        // custom emoji that isn't usable here.
        Some(ErrorType::InvalidOperation) => "Stoat didn't accept that reaction. The message \
             may have the most reactions it can, or that emoji isn't allowed here."
            .into(),
        _ => format!("Couldn't update your reaction: {err}").into(),
    }
}

/// The message box's placeholder in `channel`.
fn composer_placeholder(channel: &ChannelEntry) -> String {
    match channel.kind {
        ChannelKind::Text { .. } => format!("Message #{}", channel.name),
        ChannelKind::DirectMessage => format!("Message @{}", channel.name),
        ChannelKind::Group => format!("Message {}", channel.name),
        ChannelKind::SavedMessages => "Save a note".into(),
    }
}

/// What a refused delete's toast says.
fn delete_error_text(err: &ApiError) -> SharedString {
    use revolt_result::ErrorType;
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => "You can't delete this message.".into(),
        _ => format!("Couldn't delete the message: {err}").into(),
    }
}

/// What a refused edit's toast says.
fn edit_error_text(err: &ApiError) -> SharedString {
    use revolt_result::ErrorType;
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. } | ErrorType::CannotEditMessage) => {
            "You can't edit this message.".into()
        }
        _ => format!("Couldn't save your edit: {err}").into(),
    }
}

/// Stoat has seen this send's idempotency key before.
fn is_duplicate(err: &ApiError) -> bool {
    matches!(err.error_type(), Some(revolt_result::ErrorType::DuplicateNonce))
}

/// What a failed message says under it.
fn send_error_text(err: &ApiError) -> String {
    use revolt_result::ErrorType;
    match err {
        ApiError::Network(_) => "Couldn't reach Stoat.".into(),
        ApiError::RateLimited { .. } => "Sending too fast. Wait a moment, then retry.".into(),
        _ => match err.error_type() {
            Some(ErrorType::MissingPermission { .. }) => "You can't send messages here.".into(),
            Some(ErrorType::PayloadTooLarge) => "This message is too long.".into(),
            _ => err.to_string(),
        },
    }
}

/// Space between a bubble and its reactions.
const REACTIONS_GAP: f32 = 4.;
/// Height of a reaction chip.
const REACTION_HEIGHT: f32 = 24.;
/// Custom emoji in a reaction chip.
const REACTION_EMOJI_PX: f32 = 16.;

/// The message's reaction chips (emoji and count, outlined when the user is
/// one of them), wrapping under the bubble. `None` without reactions.
fn render_reactions(
    message: &MessageRow,
    colors: RowColors,
    on_action: &OnRowAction,
) -> Option<MessageFooter> {
    if message.reactions.is_empty() {
        return None;
    }
    let chips = message.reactions.iter().enumerate().map(|(ix, chip)| {
        let (emoji, tooltip): (AnyElement, SharedString) = match &chip.emoji {
            ReactionEmoji::Unicode(emoji) => (
                div().text_sm().child(emoji.clone()).into_any_element(),
                emoji.clone().into(),
            ),
            ReactionEmoji::Custom {
                url: Some(url),
                name,
                ..
            } => (
                img(SharedUri::from(url.clone()))
                    .id(SharedString::from(format!(
                        "reaction-{}-{ix}-emoji",
                        message.id
                    )))
                    .size(px(REACTION_EMOJI_PX))
                    .flex_none()
                    .object_fit(ObjectFit::Contain)
                    .with_loading(|| icon_loading(REACTION_EMOJI_PX, false))
                    .into_any_element(),
                name.clone().into(),
            ),
            ReactionEmoji::Custom {
                url: None, name, ..
            } => (
                div().text_xs().child(name.clone()).into_any_element(),
                name.clone().into(),
            ),
        };
        h_flex()
            .id(SharedString::from(format!("reaction-{}-{ix}", message.id)))
            .h(px(REACTION_HEIGHT))
            .px_2()
            .gap_1()
            .rounded_full()
            .border_1()
            .map(|el| {
                if chip.mine {
                    el.border_color(colors.mine).bg(colors.mine_bg)
                } else {
                    el.border_color(colors.border).bg(colors.card_bg)
                }
            })
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            // Clicking a chip adds the user's reaction, or takes it back.
            .cursor_pointer()
            .on_click({
                let on_action = on_action.clone();
                let toggle = RowAction::ToggleReaction {
                    message: message.id.clone(),
                    emoji: chip.key.clone(),
                };
                move |_, window, cx| on_action(toggle.clone(), window, cx)
            })
            .child(emoji)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if chip.mine { colors.mine } else { colors.muted })
                    .child(chip.count.to_string()),
            )
    });
    Some(
        MessageFooter::new()
            // Line up with the bubble's edge, not its text.
            .content_inset(false)
            .flex_wrap()
            .children(chips),
    )
}

/// Corner radius of inline images and file cards.
const ATTACHMENT_RADIUS: f32 = 8.;
/// File cards' width (they shrink in a narrow bubble).
const FILE_CARD_WIDTH: f32 = 320.;

/// An inline image at its precomputed size (so the row never changes height
/// when it loads), or a file card. `id` is unique within the list.
fn render_attachment(
    id: SharedString,
    attachment: &AttachmentView,
    colors: RowColors,
    on_action: &OnRowAction,
) -> AnyElement {
    match attachment {
        AttachmentView::Image {
            url,
            name,
            width,
            height,
            original,
            thumbhash,
            too_big: false,
            ..
        } => {
            let viewed = ViewedImage {
                url: url.clone(),
                name: name.clone(),
                original: *original,
            };
            let on_action = on_action.clone();
            inline_image(id, url, (*width, *height), thumbhash.clone(), colors)
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    on_action(RowAction::ViewImage(viewed.clone()), window, cx)
                })
                .into_any_element()
        }
        AttachmentView::Image {
            id: file_id,
            name,
            size_label,
            too_big: true,
            ..
        } => {
            let on_action = on_action.clone();
            let file_id = file_id.clone();
            file_card(id.clone(), IconName::FileImage, name, size_label, colors)
                .child(
                    Button::new(SharedString::from(format!("{id}-show")))
                        .xsmall()
                        .label("Show image")
                        .on_click(move |_, window, cx| {
                            on_action(RowAction::RevealImage(file_id.clone()), window, cx)
                        }),
                )
                .into_any_element()
        }
        AttachmentView::File {
            name,
            size_label,
            url,
            kind,
        } => {
            file_card(id, file_icon(*kind), name, size_label, colors)
                .when_some(url.clone(), |card, url| {
                    card.cursor_pointer()
                        .on_click(move |_, _, cx| cx.open_url(&url))
                        .child(Icon::new(IconName::Download).text_color(colors.muted))
                })
                .into_any_element()
        }
        // Still uploading: the local copy, with a progress bar along its
        // bottom edge (over it, so the row's height doesn't change).
        AttachmentView::Local {
            name,
            size_label,
            kind,
            image,
            progress,
        } => match image {
            Some((path, width, height)) => div()
                .id(id.clone())
                .relative()
                .w(px(*width as f32))
                .h(px(*height as f32))
                .flex_none()
                .child(
                    img(path.clone())
                        .id(SharedString::from(format!("{id}-img")))
                        .size_full()
                        .rounded(px(ATTACHMENT_RADIUS))
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(move || {
                            div()
                                .size_full()
                                .rounded(px(ATTACHMENT_RADIUS))
                                .bg(colors.image_bg)
                                .into_any_element()
                        }),
                )
                .child(upload_bar(*progress, colors).bottom(px(6.)).left(px(6.)).right(px(6.)))
                .into_any_element(),
            None => file_card(id, file_icon(*kind), name, size_label, colors)
                .relative()
                .child(upload_bar(*progress, colors).bottom(px(3.)).left(px(8.)).right(px(8.)))
                .into_any_element(),
        },
    }
}

/// How much of a file has uploaded (`progress` in thousandths): a thin
/// bar, positioned by the caller.
fn upload_bar(progress: u16, colors: RowColors) -> Div {
    div()
        .absolute()
        .h(px(3.))
        .rounded_full()
        .bg(colors.border)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(colors.mine)
                .w(relative(f32::from(progress.min(1000)) / 1000.)),
        )
}

/// A picture at exactly its display size (so its row never changes height
/// when it loads), with a backdrop while loading or if it fails. Callers add
/// what clicking it does.
/// `thumbhash`: shown blurred while the image loads, instead of the pulsing box.
fn inline_image(
    id: SharedString,
    url: &str,
    (width, height): (u32, u32),
    thumbhash: Option<Arc<[u8]>>,
    colors: RowColors,
) -> Stateful<Div> {
    let backdrop = move || {
        div()
            .size_full()
            .rounded(px(ATTACHMENT_RADIUS))
            .bg(colors.image_bg)
    };
    div()
        .id(id.clone())
        .relative()
        .w(px(width as f32))
        .h(px(height as f32))
        .flex_none()
        .child(
            img(SharedUri::from(url.to_string()))
                // The image's own ID keeps its loading state between frames;
                // without one, the loading placeholder never shows.
                .id(SharedString::from(format!("{id}-img")))
                .size_full()
                .rounded(px(ATTACHMENT_RADIUS))
                .object_fit(ObjectFit::Contain)
                .with_loading(move || match thumbhash.clone() {
                    Some(hash) => crate::ui::thumbhash::preview(hash)
                        .size_full()
                        .rounded(px(ATTACHMENT_RADIUS))
                        .object_fit(ObjectFit::Fill)
                        .with_fallback(move || image_loading(colors.muted, ATTACHMENT_RADIUS))
                        .into_any_element(),
                    None => image_loading(colors.muted, ATTACHMENT_RADIUS),
                })
                .with_fallback(move || {
                    backdrop()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(colors.muted)
                        .child("Couldn't load image")
                        .into_any_element()
                }),
        )
}

/// A small pulsing square (or circle) standing in for an icon or emoji
/// while it downloads.
fn icon_loading(size: f32, round: bool) -> AnyElement {
    Skeleton::new()
        .size(px(size))
        .flex_none()
        .map(|el| {
            if round {
                el.rounded_full()
            } else {
                el.rounded_sm()
            }
        })
        .into_any_element()
}

/// What an image shows while it downloads (once that's taken a moment): a
/// pulsing box with a faint picture icon.
fn image_loading(icon_color: Hsla, radius: f32) -> AnyElement {
    div()
        .relative()
        .size_full()
        .child(
            Skeleton::new()
                .absolute()
                .inset_0()
                .size_full()
                .rounded(px(radius)),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::Image)
                        .large()
                        .text_color(icon_color.opacity(0.6)),
                ),
        )
        .into_any_element()
}

/// An embed picture that opens in the full-size viewer.
fn viewable_embed_image(
    id: SharedString,
    image: &EmbedImage,
    colors: RowColors,
    on_action: &OnRowAction,
) -> Stateful<Div> {
    let viewed = ViewedImage {
        url: image.url.clone(),
        name: "Image".into(),
        original: image.original,
    };
    let on_action = on_action.clone();
    // Remote images have no ThumbHash: they get the pulsing box.
    inline_image(id, &image.url, (image.width, image.height), None, colors)
        .cursor_pointer()
        .on_click(move |_, window, cx| on_action(RowAction::ViewImage(viewed.clone()), window, cx))
}

/// Widest an embed card gets: a large image plus the card's padding and bar.
const EMBED_CARD_MAX_WIDTH: f32 = 432.;
/// Width of an embed card's colour bar.
const EMBED_BAR_WIDTH: f32 = 4.;
/// Lines of a website's description shown before it's cut off.
const EMBED_DESCRIPTION_LINES: usize = 4;

fn render_embed(
    id: SharedString,
    embed: &EmbedView,
    colors: RowColors,
    on_action: &OnRowAction,
) -> AnyElement {
    match embed {
        EmbedView::Image(image) => {
            viewable_embed_image(id, image, colors, on_action).into_any_element()
        }
        EmbedView::Video { url } => {
            let open = url.clone();
            let shown = url
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            file_card(id, IconName::FileVideoCamera, shown, "Video", colors)
                .cursor_pointer()
                .on_click(move |_, _, cx| cx.open_url(&open))
                .child(Icon::new(IconName::ExternalLink).text_color(colors.muted))
                .into_any_element()
        }
        EmbedView::Card(card) => render_embed_card(id, card, colors, on_action),
    }
}

/// A link preview or bot embed: colour bar, site, title (linked), description,
/// and a picture beside or under the text.
fn render_embed_card(
    id: SharedString,
    card: &EmbedCard,
    colors: RowColors,
    on_action: &OnRowAction,
) -> AnyElement {
    let bar = card
        .colour
        .map(|colour| Hsla::from(rgba(colour)))
        .unwrap_or(colors.border);

    let site = card.site_name.clone().map(|name| {
        h_flex()
            .gap_1p5()
            .text_xs()
            .text_color(colors.muted)
            .when_some(card.icon.clone(), |el, icon| {
                el.child(
                    img(SharedUri::from(icon))
                        .id(SharedString::from(format!("{id}-site-icon")))
                        .size(px(16.))
                        .flex_none()
                        .rounded_sm()
                        .object_fit(ObjectFit::Contain)
                        .with_loading(|| icon_loading(16., false)),
                )
            })
            .child(div().truncate().child(name))
    });
    // Bot embeds have an icon but no site name: it goes by the title instead.
    let title_icon = card
        .site_name
        .is_none()
        .then(|| card.icon.clone())
        .flatten();
    let title = card.title.clone().map(|title| {
        let text = div()
            .id(SharedString::from(format!("{id}-title")))
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .child(title);
        let text = match card.link.clone() {
            Some(link) => text
                .text_color(colors.link)
                .cursor_pointer()
                .hover(|style| style.underline())
                .on_click(move |_, _, cx| cx.open_url(&link)),
            None => text,
        };
        h_flex()
            .gap_1p5()
            .items_start()
            .when_some(title_icon, |el, icon| {
                el.child(
                    img(SharedUri::from(icon))
                        .id(SharedString::from(format!("{id}-title-icon")))
                        .size(px(20.))
                        .flex_none()
                        .rounded_full()
                        .object_fit(ObjectFit::Cover)
                        .with_loading(|| icon_loading(20., true)),
                )
            })
            .child(text)
    });
    let description = card
        .description
        .clone()
        .map(|description| match description {
            Description::Plain(text) => div()
                .text_sm()
                .text_color(colors.muted)
                .line_clamp(EMBED_DESCRIPTION_LINES)
                .child(text)
                .into_any_element(),
            Description::Markdown { source, render_key } => {
                TextView::markdown(SharedString::from(render_key), source)
                    .plugin(MathPlugin)
                    .text_sm()
                    .selectable(true)
                    .on_link_click(|url, _, _, cx| {
                        if is_external_link(url) {
                            cx.open_url(url);
                        }
                    })
                    .into_any_element()
            }
        });
    let thumbnail = card
        .thumbnail
        .as_ref()
        .map(|image| viewable_embed_image(format!("{id}-thumb").into(), image, colors, on_action));
    let image = card.image.as_ref().map(|image| {
        let picture = inline_image(
            format!("{id}-image").into(),
            &image.url,
            (image.width, image.height),
            None,
            colors,
        )
        .cursor_pointer();
        match (card.playable, card.link.clone()) {
            // A video's picture: a play badge, and a click opens the video.
            (true, Some(link)) => picture
                .on_click(move |_, _, cx| cx.open_url(&link))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(48.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(black().opacity(0.6))
                                .text_color(white())
                                .child(Icon::new(IconName::Play).large()),
                        ),
                )
                .into_any_element(),
            _ => {
                let viewed = ViewedImage {
                    url: image.url.clone(),
                    name: card.title.clone().unwrap_or_else(|| "Image".into()),
                    original: image.original,
                };
                let on_action = on_action.clone();
                picture
                    .on_click(move |_, window, cx| {
                        on_action(RowAction::ViewImage(viewed.clone()), window, cx)
                    })
                    .into_any_element()
            }
        }
    });
    let media = card
        .media
        .as_ref()
        .map(|media| render_attachment(format!("{id}-media").into(), media, colors, on_action));

    div()
        .flex()
        .flex_row()
        .max_w(px(EMBED_CARD_MAX_WIDTH))
        .rounded(px(ATTACHMENT_RADIUS))
        .overflow_hidden()
        .border_1()
        .border_color(colors.border)
        .bg(colors.card_bg)
        .child(div().w(px(EMBED_BAR_WIDTH)).flex_none().bg(bar))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .p_3()
                .gap_1()
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .children(site)
                                .children(title)
                                .children(description),
                        )
                        .children(thumbnail),
                )
                .children(image)
                .children(media),
        )
        .into_any_element()
}

/// A bordered row with a file icon, name and size; callers add what's on the right.
fn file_card(
    id: SharedString,
    icon: IconName,
    name: &str,
    size_label: &str,
    colors: RowColors,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w(px(FILE_CARD_WIDTH))
        .max_w_full()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(ATTACHMENT_RADIUS))
        .border_1()
        .border_color(colors.border)
        .bg(colors.card_bg)
        .child(Icon::new(icon).large().text_color(colors.muted))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().truncate().child(name.to_string()))
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(size_label.to_string()),
                ),
        )
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
fn server_icon(server: &ServerEntry, media: &MediaUrls, selected: bool) -> AnyElement {
    let initials = server.initials.clone();
    match server.icon.as_ref().and_then(|icon| media.file(icon)) {
        Some(url) => img(SharedUri::from(url))
            .id(SharedString::from(format!("server-icon-{}", server.id)))
            .size(px(48.))
            .object_fit(ObjectFit::Cover)
            .map(|img| {
                if selected {
                    img.rounded_xl()
                } else {
                    img.rounded_full()
                }
            })
            .with_loading({
                let initials = initials.clone();
                move || div().child(initials.clone()).into_any_element()
            })
            .with_fallback(move || div().child(initials.clone()).into_any_element())
            .into_any_element(),
        None => div().child(initials).into_any_element(),
    }
}

/// A rail button, with a marker on the left edge when selected (an icon image
/// covers the button's own highlight colour).
/// What a rail button's "Mark as read" menu item does.
type MarkRead = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[allow(clippy::too_many_arguments)]
fn rail_button(
    id: impl Into<ElementId>,
    content: AnyElement,
    tooltip: impl Into<SharedString>,
    selected: bool,
    unread: bool,
    mentions: usize,
    mark_read: Option<MarkRead>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let tooltip: SharedString = tooltip.into();
    let hover_bg = theme.primary_hover;
    let hover_fg = theme.primary_foreground;
    let marker = theme.foreground;
    let badge_ring = theme.sidebar;
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
    // Right-click: "Mark as read" (servers only).
    let button = match mark_read {
        Some(mark_read) => button
            .context_menu(move |menu, _, _| {
                let mark_read = mark_read.clone();
                menu.item(
                    PopupMenuItem::new("Mark as read")
                        .on_click(move |event, window, cx| mark_read(event, window, cx)),
                )
            })
            .into_any_element(),
        None => button.into_any_element(),
    };

    div()
        .relative()
        .w_full()
        .flex()
        .justify_center()
        .child(button)
        // Left-edge marker: tall when selected, a short pill when unread.
        .when(selected || unread, |el| {
            let (top, height) = if selected { (8., 32.) } else { (20., 8.) };
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(top))
                    .w(px(4.))
                    .h(px(height))
                    .rounded_r_md()
                    .bg(marker),
            )
        })
        .when(mentions > 0, |el| {
            el.child(
                div()
                    .absolute()
                    .right(px(8.))
                    .bottom(px(-2.))
                    .border_2()
                    .border_color(badge_ring)
                    .rounded_full()
                    .child(mention_badge(mentions, cx)),
            )
        })
}

/// A red pill with an unread-mention count ("99+" past 99).
fn mention_badge(count: usize, cx: &App) -> Div {
    let theme = cx.theme();
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme.danger)
        .text_color(theme.danger_foreground)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .child(label)
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // After layout, check whether the open channel is scrolled to its top
        // (or too short to scroll) and has more history to load.
        if let Some(channel) = self.current_channel().map(|c| c.id) {
            cx.defer_in(window, move |this, window, cx| {
                this.maybe_load_older(&channel, cx);
                this.maybe_ack_current(window, cx);
            });
        }
        // Every unread change re-renders, so the Dock badge follows along.
        let attention = self.store.attention_count();
        if attention != self.dock_badge {
            log::debug!("dock badge: {attention}");
            for (channel, count) in self.store.attention_breakdown() {
                log::debug!("  +{count} {channel}");
            }
            crate::dock::set_badge(attention);
            self.dock_badge = attention;
        }
        self.dismiss_read_banners(cx);
        self.sync_composer(window, cx);
        let viewer = self
            .viewer
            .as_ref()
            .map(|image| self.render_viewer(image, window, cx).into_any_element());
        let theme = cx.theme();
        let (background, foreground) = (theme.background, theme.foreground);
        div()
            .relative()
            .size_full()
            .child(
                h_flex()
                    .size_full()
                    .bg(background)
                    .text_color(foreground)
                    .child(self.render_rail(cx))
                    .child(self.render_channel_list(cx))
                    .child(self.render_main(cx)),
            )
            .children(viewer)
    }
}
