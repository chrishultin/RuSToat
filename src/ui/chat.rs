//! The signed-in UI: server rail, channel list, and the selected channel.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use gpui_kit::assets::IconName;
use gpui_kit::component::bubble::{Bubble, BubbleContent, BubbleReactions, BubbleVariant};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
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
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, Selectable, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::ApiError;
use crate::api::StoatClient;
use crate::api::events::{self, Connection, ConnectionEvent, ServerEvent};
use crate::api::models::{
    BulkMessageResponse, FieldsMessage, MessageSort, ReplyIntent, UserProfile,
};
use crate::attachment::{AttachmentView, fit_into};
use crate::complete::{Query, QueryKind, Suggestion, TokenRef, rank, rank_mentions};
use crate::draft::{AttachmentLimits, DraftAttachment, ReplyTarget};
use crate::embed::{Description, EmbedCard, EmbedImage, EmbedView};
use crate::friends::{AddFriendState, FRIENDS, FriendsTab};
use crate::markdown::{
    InternalLink, JUMBO_TEXT_PX, has_relative_time, is_external_link, parse_internal_link,
};
use crate::media::MediaUrls;
use crate::notify::{self, Delivery, Notice, Viewing};
use crate::places::{Place, Places, PlacesFile};
use crate::profile::{RelationshipAction, UserMenuItem};
use crate::runtime::spawn_io;
use crate::safety::{LinkCheck, SafetyFile, SafetySettings, classify, is_own_instance, origin};
use crate::search::{MAX_QUERY_CHARS, PINS_LIMIT, QueryCheck, RESULT_LIMIT, check_query};
use crate::store::{
    ChannelEntry, ChannelKind, ChannelSection, ChatStore, MemberList, MemberListRow, MessageRow,
    PendingAttachment, PendingMessage, PendingState, PickerEmoji, PresenceKind, ReactionEmoji,
    ReplyPreview, ReplyState, RowOp, ServerEntry, TimelineRow, diff_rows, display_name,
    fold_sections, with_unread_divider,
};
use crate::typing::{OutgoingTyping, TypingSignal, TypingState, typing_label};
use crate::ui::composer::{Composer, ComposerEvent, file_icon};
use crate::ui::emoji_picker::emoji_grid;
use crate::ui::friends::{RowButtons, render_add_friend, render_friends_page};
use crate::ui::markup::{MarkupPlugin, RevealSpoiler, code_block_actions};
use crate::ui::math::MathPlugin;
use crate::ui::profile_card::{CardBio, CardContent, OnLink, render_profile_card};

actions!(
    chat,
    [
        /// The channel above the open one in the list (⌥↑).
        PrevChannel,
        /// The channel below the open one (⌥↓).
        NextChannel,
        /// The nearest unread channel above (⌥⇧↑).
        PrevUnread,
        /// The nearest unread channel below (⌥⇧↓).
        NextUnread,
        /// The server above in the rail, Home first (⌘⌥↑).
        PrevServer,
        /// The server below in the rail (⌘⌥↓).
        NextServer,
        /// Go to the newest message and mark the channel read (Esc).
        JumpToPresent,
        /// Mark the open server read (⇧Esc).
        MarkServerRead,
    ]
);

/// The key context of a [`ChatView`]: its shortcuts work anywhere in it.
const KEY_CONTEXT: &str = "ChatView";

/// Bind the chat shortcuts. Call after `gpui_kit::init`, so the message
/// box's own bindings for the same keys (extra cursors) lose to these.
pub fn init(cx: &mut App) {
    cx.bind_keys(key_bindings());
}

/// The chat shortcuts, as [`init`] binds them.
pub fn key_bindings() -> Vec<KeyBinding> {
    // Text fields bind these to extra cursors; a chat box has no use for
    // those, so in the chat view the keys move around instead.
    let moves = |context| {
        [
            KeyBinding::new("alt-shift-up", PrevUnread, Some(context)),
            KeyBinding::new("alt-shift-down", NextUnread, Some(context)),
            KeyBinding::new("cmd-alt-up", PrevServer, Some(context)),
            KeyBinding::new("cmd-alt-down", NextServer, Some(context)),
        ]
    };
    let mut bindings = Vec::new();
    bindings.extend(moves(KEY_CONTEXT));
    bindings.extend(moves("ChatView > Input"));
    bindings.extend([
        // Text fields don't use these, so they work in the message box too.
        KeyBinding::new("alt-up", PrevChannel, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-down", NextChannel, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-escape", MarkServerRead, Some(KEY_CONTEXT)),
        // In a text field, Esc is the field's own `Escape` first; when
        // nothing there uses it, it reaches the view (`on_escape`).
        KeyBinding::new("escape", JumpToPresent, Some(KEY_CONTEXT)),
    ]);
    bindings
}

/// How many messages to load when a channel is opened.
pub const HISTORY_PAGE: usize = 50;

/// Most reply originals fetched at once.
const REPLY_FETCHES: usize = 4;

/// Most other servers' emoji fetched at once.
const EMOJI_FETCHES: usize = 4;

/// How long a message stays highlighted after a jump lands on it.
const HIGHLIGHT_FOR: Duration = Duration::from_secs(2);

/// Away from the present, newer messages are fetched once the view is this
/// many rows from the end of the list, so they're usually there before the
/// reader gets to the bottom.
const NEWER_PREFETCH_ROWS: usize = 12;

pub enum ChatEvent {
    LogOutRequested,
    /// The server no longer accepts our session.
    SessionRejected,
    /// The user clicked a notice for this account: show it.
    Activate,
    /// The user chose a new default skin tone (it's app-wide: the owner
    /// saves it and gives it to every account's view).
    SkinTone(crate::prefs::Tone),
    /// The user opened or closed the member list (also app-wide, saved the
    /// same way).
    MemberListOpen(bool),
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
    /// The rows end at the channel's newest message (see
    /// [`ChatStore::is_at_present`]) as of the last sync.
    at_present: bool,
    scroller: Entity<MessageScrollerState>,
    /// Watches scrolling, to load older messages at the top.
    _scroll: Subscription,
}

/// What the right-hand panel shows. The member list is also subject to
/// [`ChatView::member_list_open`]; search and pins are temporary and belong
/// to the channel they were opened in.
enum SidePanel {
    Members,
    Search(SearchPanel),
    Pins(PinsPanel),
}

/// The pinned messages of one channel, and how loading them went.
struct PinsPanel {
    /// The channel, the only one the panel is shown in.
    channel: String,
    /// Which request this panel is waiting for (see [`ChatView::pins_seq`]).
    seq: u64,
    state: PinsState,
}

enum PinsState {
    Loading,
    /// Newest first.
    Results(Rc<Vec<MessageRow>>),
    Empty,
    Error(SharedString),
}

/// The search panel: the query as sent, and how it went.
struct SearchPanel {
    /// The channel searched, the only one the panel is shown in.
    channel: String,
    /// What was typed when Enter was pressed.
    query: String,
    sort: MessageSort,
    /// Which request this panel is waiting for (see [`ChatView::search_seq`]).
    seq: u64,
    state: SearchState,
}

enum SearchState {
    Loading,
    Results(Rc<Vec<MessageRow>>),
    Empty,
    /// The query had this many characters: too many to send.
    TooLong(usize),
    Error(SharedString),
}

pub struct ChatView {
    client: StoatClient,
    store: ChatStore,
    has_ready: bool,
    status: Status,
    place: Place,
    /// Last channel picked in each place, so switching servers comes back to it.
    selected: HashMap<Place, String>,
    /// Category IDs folded away in the channel list (kept with `selected`).
    collapsed: HashSet<String>,
    /// Where `place` and `selected` are kept between launches (`None` if
    /// there's nowhere to keep them), and what was last written there.
    places_file: Option<PlacesFile>,
    saved_places: Places,
    /// Where `safety` is kept between launches (`None` if there's nowhere).
    safety_file: Option<SafetyFile>,
    /// Trusted sites and mature-channel confirmations, for this account.
    safety: SafetySettings,
    /// The external link the open "External links can be dangerous!" dialog
    /// asks about.
    link_prompt: Option<LinkPrompt>,
    /// File server base URL for icons, from `GET /`. `None` until loaded.
    /// Where images come from. Only the API base is known until `GET /` loads.
    media: MediaUrls,
    /// The instance's web client, for message links (`app` from `GET /`).
    /// `None` until it loads, or if the instance doesn't say.
    web_app: Option<String>,
    /// Where the user left off in the open channel: the channel, and its read
    /// position when it was opened if it had unread messages then (the "New
    /// messages" divider goes after it). Recorded before the open channel is
    /// marked read, so the divider stays while the user reads. `None` until
    /// a channel is opened, and again while this account is hidden.
    unread_marker: Option<(String, Option<String>)>,
    panes: HashMap<String, Pane>,
    /// History requests in flight, by channel. Results always land in their
    /// own channel, so a slow one can't overwrite the channel being viewed.
    loading: HashMap<String, Task<()>>,
    load_errors: HashMap<String, SharedString>,
    /// Older-page requests in flight, and the last failure, by channel.
    loading_older: HashMap<String, Task<()>>,
    older_errors: HashMap<String, SharedString>,
    /// The same for the page after the newest loaded message, which only
    /// happens in a window that isn't at the present.
    loading_newer: HashMap<String, Task<()>>,
    newer_errors: HashMap<String, SharedString>,
    /// Jumps to a message that isn't loaded, in flight by channel. A new one
    /// replaces (cancels) the old.
    jumps: HashMap<String, Task<()>>,
    /// Clears the highlight of the message last jumped to.
    highlight_task: Option<Task<()>>,
    /// The one read-acknowledgement waiting to go out: (channel, message, task).
    /// Replacing it (newer message, other channel) cancels the old one.
    pending_ack: Option<(String, String, Task<()>)>,
    /// The channel the user marked unread ("Mark as unread"). It isn't
    /// marked read again until they leave it or send a message there.
    held_unread: Option<String>,
    /// This account's ID, in its banners' and toasts' tags.
    account_id: String,
    /// Whether this account is the one on screen. A hidden account keeps
    /// receiving events and notifying, but never marks anything read.
    active: bool,
    /// Added to notice titles to say which account they're for, when
    /// several are signed in.
    account_label: Option<String>,
    /// macOS banners still showing: channel → the message it's for.
    /// Dismissed once the channel is read past it.
    banners: HashMap<String, String>,
    /// A friend request's banner is showing. Dismissed once no requests
    /// are left, or the Friends page is seen.
    friend_banner: bool,
    /// The image open in the full-size viewer, over everything else.
    viewer: Option<ViewedImage>,
    /// Focused while the viewer is open, so Esc reaches it.
    viewer_focus: FocusHandle,
    /// The profile card, when one is open.
    card: Option<OpenCard>,
    /// Focused while the card is open, so Esc reaches it.
    card_focus: FocusHandle,
    /// Profiles fetched this session, by user ID, shown while the card
    /// fetches them again. `None`: Stoat won't show this one.
    profiles: HashMap<String, Option<UserProfile>>,
    /// The person whose DM is being asked for ("Message" on their card).
    dm_opening: Option<String>,
    /// People whose friend request or block is on its way to Stoat.
    relationship_pending: HashSet<String>,
    /// The Friends page's open tab (while Home's Friends row is picked).
    friends_tab: FriendsTab,
    /// The Friends page's `username#1234` field. Enter sends the request.
    add_friend_input: Entity<InputState>,
    /// What happened to the last request sent from it, shown under it.
    add_friend_state: Option<AddFriendState>,
    _add_friend_input: Subscription,
    /// The message whose reaction picker is open.
    picker_for: Option<String>,
    /// Reaction changes waiting on the server, as (message, emoji). Another
    /// click on the same one waits, so requests can't cross.
    reacting: HashSet<(String, String)>,
    /// Reply originals being fetched, by message ID (each once).
    reply_fetches: HashSet<String>,
    /// Other servers' emoji asked for this session, by ID (each once, even
    /// if it failed), and how many are still loading.
    emoji_fetches: HashSet<String>,
    emoji_loading: usize,
    /// Servers whose full member list was asked for (once per session, for
    /// `@`), and those still loading.
    member_fetches: HashSet<String>,
    members_loading: HashSet<String>,
    /// The same for the online members of large servers, which is all the
    /// member list asks for there (`@` keeps its own full fetch).
    online_fetches: HashSet<String>,
    online_loading: HashSet<String>,
    /// Servers whose last member fetch failed.
    members_failed: HashSet<String>,
    /// The right-hand panel shows the member list (app-wide, remembered).
    member_list_open: bool,
    /// Member lists built for the panel, by channel. Dropped whenever
    /// members, users or permissions change (see [`Self::forget_member_lists`]).
    member_lists: HashMap<String, Rc<MemberList>>,
    /// What the right-hand panel shows.
    side_panel: SidePanel,
    /// The header's search box. Enter searches the open channel.
    search_input: Entity<InputState>,
    /// Counts searches, so that an answer for an old one (or for another
    /// channel) can be told from the one being waited for and dropped.
    search_seq: u64,
    /// The search in flight. Replacing or dropping it cancels the request.
    search_task: Option<Task<()>>,
    /// The same for the pinned messages list: counts requests (including
    /// refreshes), so that an old answer is dropped.
    pins_seq: u64,
    /// The pins request in flight.
    pins_task: Option<Task<()>>,
    /// A refresh of the pins list waiting out its delay (see
    /// [`PINS_REFRESH_DELAY`]); the next pin or unpin replaces it.
    pins_refresh: Option<Task<()>>,
    /// The channel the panel and search box were last drawn for, to notice
    /// when it changes and start them over.
    panel_channel: Option<String>,
    /// The message box, under the open channel.
    composer: Entity<Composer>,
    /// The default skin tone for emoji (`:` suggestions use it too).
    skin_tone: crate::prefs::Tone,
    _composer: Subscription,
    _search_input: Subscription,
    /// Others typing, and when to tell Stoat the user is.
    typing: TypingState,
    outgoing_typing: OutgoingTyping,
    /// Wakes up when a typist lapses or the user goes idle.
    typing_tick: Option<Task<()>>,
    /// Draws relative timestamps again as time passes.
    _relative_tick: Task<()>,
    connection: Connection,
    _events: Task<()>,
    _root: Task<()>,
    _activation: Subscription,
}

/// How often relative timestamps ("in 3 hours") are worked out again.
const RELATIVE_TIME_TICK: Duration = Duration::from_secs(60);

/// How long after a message is pinned or unpinned the open pins list is
/// fetched again, so that a burst of changes makes one request.
const PINS_REFRESH_DELAY: Duration = Duration::from_millis(500);

/// Height of the "is typing…" line above the message box.
const TYPING_LINE_HEIGHT: f32 = 16.;

/// How long the newest message must stay on screen before it's marked read,
/// so a burst of live messages sends one acknowledgement, not one each.
const ACK_DELAY: Duration = Duration::from_secs(1);

impl EventEmitter<ChatEvent> for ChatView {}

impl ChatView {
    /// `client` must carry the session token. `me` is the logged-in user's
    /// ID, and `account_id` the account's (see [`crate::accounts`]).
    ///
    /// Banner clicks and the Dock badge are app-wide, so the owner handles
    /// them: [`Self::open_channel`], [`Self::sync_outside`].
    pub fn new(
        client: StoatClient,
        me: String,
        account_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Coming back to the window counts as reading what's on screen.
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.maybe_ack_current(window, cx);
            if window.is_window_active() && this.active && this.friends_open() {
                this.dismiss_friend_banner(cx);
            }
        });
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
                    log::debug!(
                        "media: {media:?}, attachments: {limits:?}, web app: {:?}",
                        root.app
                    );
                    this.update(cx, |this, cx| {
                        this.web_app = root.app;
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
        // Relative timestamps ("in 3 hours") are worked out when drawn, so
        // the open channel is drawn again each minute while it has any.
        let relative_tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(RELATIVE_TIME_TICK).await;
                let alive = this.update(cx, |this, cx| {
                    if this.open_channel_has_relative_times() {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
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
        let safety_file = SafetyFile::for_account(&api, &me);
        let safety = safety_file
            .as_ref()
            .map(SafetyFile::load)
            .unwrap_or_default();
        let typing = TypingState::new(me.clone());
        let mut store = ChatStore::new(me);
        store.set_media(media.clone());
        let composer = cx.new(|cx| Composer::new(window, cx));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search messages…"));
        let search_events = cx.subscribe_in(
            &search_input,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.run_search(cx);
                }
            },
        );
        let add_friend_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("username#1234"));
        let add_friend_events = cx.subscribe_in(
            &add_friend_input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.send_friend_request(window, cx),
                // Editing what was sent forgets how it went.
                InputEvent::Change
                    if this.add_friend_state.is_some()
                        && this.add_friend_state != Some(AddFriendState::Sending) =>
                {
                    this.add_friend_state = None;
                    // The dialog showing it is drawn by the window's root.
                    window.refresh();
                    cx.notify();
                }
                _ => {}
            },
        );
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
                ComposerEvent::SkinTone(tone) => {
                    this.skin_tone = *tone;
                    cx.emit(ChatEvent::SkinTone(*tone));
                }
                ComposerEvent::OpenEmojiPicker => {
                    let Some(channel) = composer.read(cx).channel().map(str::to_string) else {
                        return;
                    };
                    let custom = this.store.custom_emoji_for(&channel);
                    composer.update(cx, |composer, cx| {
                        composer.open_emoji_picker(custom, window, cx)
                    });
                }
                ComposerEvent::ClosedShortcode { range, code } => {
                    let Some(channel) = composer.read(cx).channel().map(str::to_string) else {
                        return;
                    };
                    let custom = this.store.custom_emoji_for(&channel);
                    if let Some(emoji) = crate::emoji::custom_by_name(&custom, code) {
                        composer.update(cx, |composer, cx| {
                            composer.replace_with_custom_emoji(range.clone(), emoji, window, cx)
                        });
                    }
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
            collapsed: saved_places.collapsed.clone(),
            places_file,
            saved_places,
            safety_file,
            safety,
            link_prompt: None,
            media,
            web_app: None,
            unread_marker: None,
            panes: HashMap::new(),
            loading: HashMap::new(),
            load_errors: HashMap::new(),
            loading_older: HashMap::new(),
            older_errors: HashMap::new(),
            loading_newer: HashMap::new(),
            newer_errors: HashMap::new(),
            jumps: HashMap::new(),
            highlight_task: None,
            pending_ack: None,
            held_unread: None,
            account_id,
            active: true,
            account_label: None,
            banners: HashMap::new(),
            friend_banner: false,
            viewer: None,
            viewer_focus: cx.focus_handle(),
            card: None,
            card_focus: cx.focus_handle(),
            profiles: HashMap::new(),
            dm_opening: None,
            relationship_pending: HashSet::new(),
            friends_tab: FriendsTab::default(),
            add_friend_input,
            add_friend_state: None,
            _add_friend_input: add_friend_events,
            picker_for: None,
            reacting: HashSet::new(),
            reply_fetches: HashSet::new(),
            emoji_fetches: HashSet::new(),
            emoji_loading: 0,
            member_fetches: HashSet::new(),
            members_loading: HashSet::new(),
            online_fetches: HashSet::new(),
            online_loading: HashSet::new(),
            members_failed: HashSet::new(),
            member_list_open: true,
            member_lists: HashMap::new(),
            side_panel: SidePanel::Members,
            search_input,
            search_seq: 0,
            search_task: None,
            pins_seq: 0,
            pins_task: None,
            pins_refresh: None,
            panel_channel: None,
            composer,
            skin_tone: crate::prefs::Tone::default(),
            _composer: composer_events,
            _search_input: search_events,
            typing,
            outgoing_typing: OutgoingTyping::default(),
            typing_tick: None,
            _relative_tick: relative_tick,
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
                self.card = None;
                self.has_ready = true;
                self.status = Status::Connected;
                self.drop_stale_selection();
                self.member_lists.clear();
                if reconnect {
                    // `Ready` brings only the user's own memberships back.
                    self.member_fetches.clear();
                    self.online_fetches.clear();
                    // Events missed while offline would leave gaps, so drop
                    // loaded history and reload what's on screen.
                    log::info!("reconnected: reloading message history");
                    self.store.clear_histories();
                    self.panes.clear();
                    self.loading.clear();
                    self.loading_older.clear();
                    self.older_errors.clear();
                    self.loading_newer.clear();
                    self.newer_errors.clear();
                    self.jumps.clear();
                    // Pins changed while offline are missed the same way.
                    if let SidePanel::Pins(panel) = &self.side_panel {
                        let channel = panel.channel.clone();
                        self.start_pins(channel, true, cx);
                    }
                }
                self.ensure_current_loaded(cx);
            }
            ConnectionEvent::Event(ServerEvent::Message(message)) => {
                let channel = message.channel.clone();
                // Sending ends their typing.
                self.typing.stop(&channel, &message.author);
                log::debug!("live: new message {} in {channel}", message.id);
                // Decide before applying: what the user was looking at when
                // it arrived. A hidden account isn't being looked at.
                let current = self.current_channel().map(|c| c.id).filter(|_| self.active);
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
                // Pinned or unpinned (by anyone): the pins list is out of date.
                let pin_change = data.pinned.is_some()
                    || clear.iter().any(|f| matches!(f, FieldsMessage::Pinned));
                if self.store.apply_update(&channel, &id, *data, &clear) {
                    self.sync_pane(&channel, cx);
                }
                if pin_change {
                    self.schedule_pins_refresh(&channel, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::MessageDelete { id, channel }) => {
                log::debug!("live: delete {id} in {channel}");
                self.drop_pins(&channel, std::slice::from_ref(&id));
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
                self.drop_pins(&channel, &ids);
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
                // Permission overrides decide who sees the channel, and who
                // may pin in it.
                self.forget_member_lists();
                self.sync_pane(&id, cx);
                // A hidden channel, or a DM closed elsewhere, can't stay open.
                let selected = self.selected.len();
                self.drop_stale_selection();
                if self.selected.len() != selected {
                    self.remember_places();
                    self.ensure_current_loaded(cx);
                }
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
            ConnectionEvent::Event(ServerEvent::ServerMemberJoin { server, member }) => {
                let user = member.id.user.clone();
                log::debug!("live: {user} joined {server}");
                self.store.apply_member_join(*member);
                // Only worth a request where `@` already loaded everyone;
                // elsewhere the first `@` loads them along with the rest.
                if !self.store.knows_user(&user) && self.member_fetches.contains(&server) {
                    self.fetch_user(user, cx);
                }
                self.refresh_suggestions(cx);
                self.forget_member_lists();
            }
            ConnectionEvent::Event(ServerEvent::ServerMemberLeave { server, user }) => {
                log::debug!("live: {user} left {server}");
                if self.store.apply_member_leave(&server, &user) {
                    self.leave_server(&server, cx);
                }
                self.refresh_suggestions(cx);
                self.forget_member_lists();
            }
            ConnectionEvent::Event(ServerEvent::ServerMemberUpdate { id, data, clear }) => {
                let shown = self.store.apply_member_update(&id, *data, &clear);
                // Roles and timeouts change the list without changing a row's name.
                let listed = self.member_panel_lists(&id.user);
                self.forget_member_lists();
                if !shown {
                    if listed {
                        cx.notify();
                    }
                    return;
                }
                log::debug!("live: member {} of {} updated", id.user, id.server);
                for channel in self.store.channels_showing(&id.user) {
                    if self.store.channel_server(&channel) == Some(id.server.as_str()) {
                        self.sync_pane(&channel, cx);
                    }
                }
                self.refresh_suggestions(cx);
            }
            ConnectionEvent::Event(ServerEvent::UserUpdate { id, data, clear }) => {
                let shown = self.store.apply_user_update(&id, *data, &clear);
                // The member list shows presence and status too.
                let listed = self.member_panel_lists(&id);
                if shown || listed {
                    self.forget_member_lists();
                }
                if shown {
                    log::debug!("live: user {id} updated");
                    for channel in self.store.channels_showing(&id) {
                        self.sync_pane(&channel, cx);
                    }
                    self.refresh_suggestions(cx);
                } else if id != self.store.me() && !listed && !self.friends_lists(&id) {
                    // Someone else's presence or status: nothing on screen.
                    return;
                }
            }
            ConnectionEvent::Event(ServerEvent::ServerUpdate { id, data, clear }) => {
                log::debug!("live: server {id} updated");
                if !self.store.apply_server_update(&id, *data, &clear) {
                    return;
                }
                self.server_permissions_changed(&id, cx);
            }
            ConnectionEvent::Event(ServerEvent::ServerRoleUpdate {
                server,
                role_id,
                data,
                clear,
            }) => {
                log::debug!("live: role {role_id} of {server} updated");
                if !self
                    .store
                    .apply_role_update(&server, &role_id, *data, &clear)
                {
                    return;
                }
                self.server_permissions_changed(&server, cx);
            }
            ConnectionEvent::Event(ServerEvent::ServerRoleDelete { server, role_id }) => {
                log::debug!("live: role {role_id} of {server} deleted");
                if !self.store.apply_role_delete(&server, &role_id) {
                    return;
                }
                self.server_permissions_changed(&server, cx);
            }
            ConnectionEvent::Event(ServerEvent::ServerRoleRanksUpdate { server, ranks }) => {
                log::debug!("live: roles of {server} reordered");
                if !self.store.apply_role_ranks(&server, &ranks) {
                    return;
                }
                self.server_permissions_changed(&server, cx);
            }
            ConnectionEvent::Event(ServerEvent::ServerCreate {
                server,
                channels,
                emojis,
            }) => {
                let id = server.id.clone();
                log::info!("live: joined server {id} ({} channels)", channels.len());
                self.store.add_server(*server, channels, emojis);
                // Its channels aren't filtered until the membership arrives.
                if self.store.member(&id, self.store.me()).is_none() {
                    self.fetch_own_member(id, cx);
                }
            }
            ConnectionEvent::Event(ServerEvent::ServerDelete { id }) => {
                log::info!("live: server {id} deleted");
                self.leave_server(&id, cx);
            }
            ConnectionEvent::Event(ServerEvent::UserRelationship { user }) => {
                log::debug!(
                    "live: relationship with {} is {:?}",
                    user.id,
                    user.relationship
                );
                let id = user.id.clone();
                let was = self.relationship_changed(*user, cx).unwrap_or_default();
                // Only this event can bring a request: the user's own
                // actions never make anyone Incoming.
                let window_active = window.is_window_active();
                let viewing_pending =
                    self.active && self.friends_open() && self.friends_tab == FriendsTab::Pending;
                let notice = notify::friend_request_notice(
                    &self.store,
                    &id,
                    &was,
                    viewing_pending,
                    window_active,
                );
                if let Some(notice) = notice {
                    match notify::delivery(window_active, crate::banners::available()) {
                        Delivery::Banner => self.show_friend_banner(notice, cx),
                        Delivery::Toast => self.show_toast(notice, window, cx),
                    }
                }
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

    /// `server`'s roles or default permissions changed: member list sections
    /// and colours, who may pin (on every loaded row), and which channels the
    /// user can see may all have too. If the open channel is now hidden, the
    /// server's first channel opens instead.
    fn server_permissions_changed(&mut self, server: &str, cx: &mut Context<Self>) {
        self.forget_member_lists();
        let loaded: Vec<String> = self
            .panes
            .keys()
            .filter(|channel| self.store.channel_server(channel) == Some(server))
            .cloned()
            .collect();
        for channel in loaded {
            self.sync_pane(&channel, cx);
        }
        let selected = self.selected.len();
        self.drop_stale_selection();
        if self.selected.len() != selected {
            self.remember_places();
        }
        self.ensure_current_loaded(cx);
    }

    /// The user left `server` (or was kicked or banned): drop it, its
    /// channels and everything loaded for them. If it was open, Home opens.
    fn leave_server(&mut self, server: &str, cx: &mut Context<Self>) {
        let removed = self.store.remove_server(server);
        log::info!("left server {server} ({} channels)", removed.len());
        for channel in &removed {
            self.panes.remove(channel);
            self.loading.remove(channel);
            self.load_errors.remove(channel);
            self.loading_older.remove(channel);
            self.older_errors.remove(channel);
            self.loading_newer.remove(channel);
            self.newer_errors.remove(channel);
            self.jumps.remove(channel);
            if self.banners.remove(channel).is_some() {
                cx.dismiss_system_notification(&notify::banner_tag(&self.account_id, channel));
            }
        }
        if self
            .pending_ack
            .as_ref()
            .is_some_and(|(channel, ..)| removed.contains(channel))
        {
            self.pending_ack = None;
        }
        self.member_fetches.remove(server);
        self.members_loading.remove(server);
        self.online_fetches.remove(server);
        self.online_loading.remove(server);
        self.members_failed.remove(server);
        self.forget_member_lists();
        self.selected.remove(&Place::Server(server.to_string()));
        self.drop_stale_selection();
        self.remember_places();
        self.ensure_current_loaded(cx);
    }

    /// Fetch the user's own membership of a server they just joined: Stoat
    /// doesn't send this session its `ServerMemberJoin`. Its roles decide
    /// which channels show. If it fails, the next reconnect brings it.
    fn fetch_own_member(&mut self, server: String, cx: &mut Context<Self>) {
        let request = spawn_io({
            let (client, server, me) = (
                self.client.clone(),
                server.clone(),
                self.store.me().to_string(),
            );
            async move { client.fetch_member(&server, &me).await }
        });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| match result {
                Ok(Ok(member)) => {
                    // Left again before it arrived.
                    if this.store.server(&server).is_none() {
                        return;
                    }
                    this.store.apply_member_join(member);
                    this.server_permissions_changed(&server, cx);
                    cx.notify();
                }
                Ok(Err(err)) => log::warn!("couldn't load own membership of {server}: {err}"),
                Err(err) => log::warn!("membership task failed: {err:?}"),
            })
            .ok();
        })
        .detach();
    }

    /// Fetch one user the client doesn't know yet (someone who just joined),
    /// so `@` can suggest them.
    fn fetch_user(&mut self, user: String, cx: &mut Context<Self>) {
        let request = spawn_io({
            let (client, user) = (self.client.clone(), user.clone());
            async move { client.fetch_user(&user).await }
        });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| match result {
                Ok(Ok(fetched)) => {
                    this.store.add_user(fetched);
                    this.forget_member_lists();
                    this.refresh_suggestions(cx);
                    cx.notify();
                }
                Ok(Err(err)) => log::warn!("couldn't load user {user}: {err}"),
                Err(err) => log::warn!("user task failed: {err:?}"),
            })
            .ok();
        })
        .detach();
    }

    /// After a (re)connect or a permission change, forget selections that
    /// point at things that are gone.
    fn drop_stale_selection(&mut self) {
        if let Place::Server(id) = &self.place
            && self.store.server(id).is_none()
        {
            self.place = Place::Home;
        }
        // Also channels a role edit has hidden. The Friends page stays.
        self.selected.retain(|place, channel| {
            (*place == Place::Home && channel == FRIENDS) || self.store.shows_channel(channel)
        });
        // Categories deleted while folded.
        self.collapsed.retain(|id| self.store.has_category(id));
    }

    /// Image URLs are known: give them to the store and redraw anything loaded.
    fn set_media(&mut self, media: MediaUrls, cx: &mut Context<Self>) {
        self.media = media.clone();
        self.composer
            .update(cx, |composer, _| composer.set_media(media.clone()));
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
                if self.store.discard_pending(channel, &key).is_some() {
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
            RowAction::TogglePin {
                id,
                pin,
                confirmed: true,
            } => self.toggle_pin(channel.to_string(), id, pin, window, cx),
            RowAction::TogglePin {
                id,
                pin,
                confirmed: false,
            } => self.confirm_pin(channel.to_string(), id, pin, window, cx),
            RowAction::Quote(id) => {
                if let Some(content) = self.store.raw_content(channel, &id) {
                    let store = &self.store;
                    let labels = |token: &TokenRef| store.token_label(channel, token);
                    self.composer.update(cx, |composer, cx| {
                        composer.append_quote(content, &labels, window, cx)
                    });
                }
            }
            RowAction::CopyText(id) => {
                if let Some(text) = self.store.raw_content(channel, &id) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                }
            }
            RowAction::CopyLink(id) => {
                if let Some(app) = &self.web_app {
                    let server = self.store.channel_server(channel);
                    let link = crate::links::message_link(app, server, channel, &id);
                    cx.write_to_clipboard(ClipboardItem::new_string(link));
                }
            }
            RowAction::CopyId(id) => cx.write_to_clipboard(ClipboardItem::new_string(id)),
            RowAction::MarkUnread(id) => self.mark_unread(channel, &id, cx),
            RowAction::JumpTo(id) => self.jump_to_message(channel, id, window, cx),
            RowAction::RevealBlocked(ids) => {
                self.store.reveal_blocked(ids);
                self.sync_pane(channel, cx);
            }
            RowAction::OpenLink(url) => self.open_external(url, None, window, cx),
        }
        cx.notify();
    }

    /// Scroll to message `id` in `channel` and highlight it. If it isn't
    /// loaded, the channel's history is replaced by a window around it
    /// first (see [`ChatStore::set_window`]).
    fn jump_to_message(
        &mut self,
        channel: &str,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.store.has_message(channel, &id) {
            self.jumps.remove(channel);
            self.store.reveal_blocked_run_of(channel, &id);
            self.land_jump(channel, &id, cx);
            return;
        }
        let client = self.client.clone();
        let request = spawn_io({
            let (channel, id) = (channel.to_string(), id.clone());
            async move {
                client
                    .fetch_messages_around(&channel, &id, HISTORY_PAGE as i64)
                    .await
            }
        });
        let channel = channel.to_string();
        let task = cx.spawn_in(window, {
            let channel = channel.clone();
            async move |this, cx| {
                let result = request.await;
                this.update_in(cx, |this, window, cx| {
                    this.jumps.remove(&channel);
                    let error = match result {
                        Ok(Ok(page)) if page_has_message(&page, &id) => {
                            this.store.set_window(&channel, page, &id, HISTORY_PAGE);
                            this.store.reveal_blocked_run_of(&channel, &id);
                            // Pages fetched for the old rows would leave a gap here.
                            this.loading_older.remove(&channel);
                            this.loading_newer.remove(&channel);
                            this.older_errors.remove(&channel);
                            this.newer_errors.remove(&channel);
                            this.land_jump_in_window(&channel, &id, cx);
                            cx.notify();
                            return;
                        }
                        Ok(Ok(_)) => "That message has been deleted.".to_string(),
                        Ok(Err(err)) => {
                            log::warn!("couldn't load messages around {id} in {channel}: {err:?}");
                            "Couldn't load that message.".to_string()
                        }
                        Err(err) => {
                            log::warn!("jump task failed: {err:?}");
                            "Couldn't load that message.".to_string()
                        }
                    };
                    window.push_notification(Notification::error(error), cx);
                })
                .ok();
            }
        });
        self.jumps.insert(channel, task);
    }

    /// `id` is loaded in `channel`: scroll to it and highlight it.
    fn land_jump(&mut self, channel: &str, id: &str, cx: &mut Context<Self>) {
        self.highlight(id, channel, cx);
        self.sync_pane(channel, cx);
        self.scroll_to_row(channel, id, cx);
    }

    /// `channel`'s history was just replaced by a window around `id`: show it,
    /// from `id`, with nothing left over from the old rows.
    fn land_jump_in_window(&mut self, channel: &str, id: &str, cx: &mut Context<Self>) {
        self.highlight(id, channel, cx);
        self.reset_pane(channel, Some(id), cx);
    }

    /// Highlight `id` (in `channel`) for [`HIGHLIGHT_FOR`], and only it. The
    /// rows are updated by the caller's next sync.
    fn highlight(&mut self, id: &str, channel: &str, cx: &mut Context<Self>) {
        self.store.set_highlight(id);
        let (id, channel) = (id.to_string(), channel.to_string());
        self.highlight_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT_FOR).await;
            this.update(cx, |this, cx| {
                if this.store.clear_highlight(&id) {
                    this.sync_pane(&channel, cx);
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Scroll `channel`'s list so that message `id` is at the top.
    fn scroll_to_row(&mut self, channel: &str, id: &str, cx: &mut Context<Self>) {
        let Some(pane) = self.panes.get(channel) else {
            return;
        };
        if let Some(ix) = row_index(&pane.rows, id) {
            pane.scroller
                .update(cx, |scroller, cx| scroller.scroll_to_item(ix, cx));
        }
    }

    /// Start `channel`'s list over from its store rows, instead of diffing
    /// from the old ones (which can't keep a reader's place when the whole
    /// history was swapped): at message `target` if given, else the end.
    ///
    /// `MessageScrollerState::reset` turns tail following back on, so the
    /// scroll that follows it must be the last thing to touch the state:
    /// `scroll_to_item` ends following (and leaves it off until the reader
    /// reaches the bottom by hand), `scroll_to_end` resumes it.
    fn reset_pane(&mut self, channel: &str, target: Option<&str>, cx: &mut Context<Self>) {
        // Makes the pane if there isn't one yet.
        self.sync_pane(channel, cx);
        let Some(history) = self.store.history(channel) else {
            return;
        };
        let at_present = history.at_present;
        let rows = Rc::new(self.timeline(channel));
        let Some(pane) = self.panes.get_mut(channel) else {
            return;
        };
        let ix = target.and_then(|id| row_index(&rows, id));
        pane.rows = rows.clone();
        pane.at_present = at_present;
        pane.scroller.update(cx, |scroller, cx| {
            scroller.reset(rows.len(), cx);
            match ix {
                Some(ix) => {
                    scroller.scroll_to_item(ix, cx);
                }
                None => scroller.scroll_to_end(cx),
            }
        });
        self.fetch_reply_sources(channel, cx);
        self.fetch_missing_emojis(channel, cx);
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
        if self.store.is_at_present(&channel) {
            if let Some(pane) = self.panes.get(&channel) {
                pane.scroller
                    .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
            }
        } else {
            // The message belongs after the newest one, not after a gap: go
            // back to the present, where it'll show up.
            self.jump_to_present(channel.clone(), cx);
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
                // Roles, `@everyone` and `@online` only where the user may
                // mention them (never outside a server).
                let roles = if self.store.can_mention_roles(channel) {
                    self.store.role_candidates(channel)
                } else {
                    Vec::new()
                };
                rank_mentions(
                    &query.query,
                    &self.store.mention_candidates(channel),
                    &roles,
                    self.store.can_mention_everyone(channel),
                )
            }
            QueryKind::Channel => {
                let channels = self.store.channel_candidates(channel);
                rank(&query.query, &channels, |c| vec![c.name.as_str()], |_| false)
                    .iter()
                    .map(|c| Suggestion::channel(&c.id, &c.name))
                    .collect()
            }
            QueryKind::Emoji => {
                crate::emoji::search(&query.query, &self.store.custom_emoji_for(channel))
                    .iter()
                    .map(|choice| choice.suggestion(self.skin_tone))
                    .collect()
            }
        };
        self.composer.update(cx, |composer, cx| {
            composer.set_suggestions(query, suggestions, loading, cx)
        });
    }

    /// Fetch `server`'s whole member list, unless it was already asked for
    /// this session. When it arrives, open `@` suggestions there refresh.
    /// Returns whether it's still loading.
    fn fetch_members(&mut self, server: String, cx: &mut Context<Self>) -> bool {
        self.request_members(server, false, cx)
    }

    /// [`Self::fetch_members`], or with `online_only` just the members who
    /// are online (the member list's request for large servers), tracked
    /// separately.
    fn request_members(
        &mut self,
        server: String,
        online_only: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let (fetches, loading) = if online_only {
            (&mut self.online_fetches, &mut self.online_loading)
        } else {
            (&mut self.member_fetches, &mut self.members_loading)
        };
        if !fetches.insert(server.clone()) {
            return loading.contains(&server);
        }
        loading.insert(server.clone());
        self.members_failed.remove(&server);
        let request = spawn_io({
            let (client, server) = (self.client.clone(), server.clone());
            async move { client.fetch_members(&server, online_only).await }
        });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                let (fetches, loading) = if online_only {
                    (&mut this.online_fetches, &mut this.online_loading)
                } else {
                    (&mut this.member_fetches, &mut this.members_loading)
                };
                loading.remove(&server);
                match result {
                    Ok(Ok(members)) => {
                        log::info!(
                            "{} {}members in {server}",
                            members.members.len(),
                            if online_only { "online " } else { "" }
                        );
                        this.store.add_members(members);
                    }
                    // Asked again on the next `@` (or channel open) there.
                    Ok(Err(err)) => {
                        log::warn!("couldn't load members of {server}: {err}");
                        fetches.remove(&server);
                        this.members_failed.insert(server.clone());
                    }
                    Err(err) => {
                        log::warn!("member task failed: {err:?}");
                        fetches.remove(&server);
                        this.members_failed.insert(server.clone());
                    }
                }
                this.forget_member_lists();
                // Not fetching again: after a failure, that would loop.
                this.refresh_suggestions(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        true
    }

    /// Load the people the member list shows for the open channel, if the
    /// list is open and they aren't loaded yet. A small server's members all
    /// come with the one fetch `@` uses; a large server's online members are
    /// fetched on their own, unless `@` has already fetched everyone.
    fn ensure_members(&mut self, cx: &mut Context<Self>) {
        if !self.has_ready || !self.member_list_open {
            return;
        }
        let Some(channel) = self.current_channel() else {
            return;
        };
        let Some(server) = self.store.channel_server(&channel.id).map(str::to_string) else {
            return;
        };
        let large = self
            .store
            .server(&server)
            .is_some_and(|s| s.approximate_member_count > crate::store::LARGE_SERVER_MEMBERS);
        if !large {
            self.fetch_members(server, cx);
        } else if !self.member_fetches.contains(&server) {
            self.request_members(server, true, cx);
        }
    }

    /// Forget the member lists built so far: something they show changed.
    fn forget_member_lists(&mut self) {
        self.member_lists.clear();
    }

    /// Whether the member list is on screen and lists `user`.
    fn member_panel_lists(&self, user: &str) -> bool {
        self.member_list_open
            && self
                .current_channel()
                .is_some_and(|c| self.store.lists_member(&c.id, user))
    }

    /// Whether the Friends page is on screen and may list `user` (anyone
    /// the user has a relationship with).
    fn friends_lists(&self, user: &str) -> bool {
        self.friends_open()
            && self.store.user(user).is_some_and(|person| {
                person.relationship != crate::api::models::RelationshipStatus::None
            })
    }

    /// Open or close the member list, and tell the owner to remember it.
    fn toggle_member_list(&mut self, cx: &mut Context<Self>) {
        self.set_member_list_open(!self.member_list_open, cx);
        cx.emit(ChatEvent::MemberListOpen(self.member_list_open));
    }

    pub fn member_list_open(&self) -> bool {
        self.member_list_open
    }

    /// Show or hide the member list (the app-wide preference).
    pub fn set_member_list_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.member_list_open = open;
        self.ensure_members(cx);
        cx.notify();
    }

    /// Start the right-hand panel and the search box over when the open
    /// channel has changed: search belongs to the channel it was run in.
    fn sync_side_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let channel = self.current_channel().map(|c| c.id);
        if channel == self.panel_channel {
            return;
        }
        self.panel_channel = channel;
        self.search_task = None;
        self.search_seq += 1;
        self.pins_task = None;
        self.pins_refresh = None;
        self.pins_seq += 1;
        self.side_panel = SidePanel::Members;
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    /// Enter in the search box: search the open channel for what was typed.
    fn run_search(&mut self, cx: &mut Context<Self>) {
        let text = self.search_input.read(cx).value().to_string();
        self.search_for(text, cx);
    }

    /// Search the open channel for `text` (as typed), if it's a query Stoat
    /// would take; otherwise say why not, or do nothing for a blank one.
    fn search_for(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(channel) = self.current_channel().map(|c| c.id) else {
            return;
        };
        let sort = self.search_sort(&channel);
        match check_query(&text) {
            QueryCheck::Empty => {}
            QueryCheck::TooLong { chars } => {
                self.search_task = None;
                self.search_seq += 1;
                self.side_panel = SidePanel::Search(SearchPanel {
                    channel,
                    query: text,
                    sort,
                    seq: self.search_seq,
                    state: SearchState::TooLong(chars),
                });
                cx.notify();
            }
            QueryCheck::Ok(query) => self.start_search(channel, query, sort, cx),
        }
    }

    /// The sort the search panel for `channel` is using (Latest to start with).
    fn search_sort(&self, channel: &str) -> MessageSort {
        match &self.side_panel {
            SidePanel::Search(panel) if panel.channel == channel => panel.sort.clone(),
            _ => MessageSort::Latest,
        }
    }

    /// Change the order of the results, and search again for the same query.
    fn set_search_sort(&mut self, sort: MessageSort, cx: &mut Context<Self>) {
        let Some(channel) = self.current_channel().map(|c| c.id) else {
            return;
        };
        let SidePanel::Search(panel) = &mut self.side_panel else {
            return;
        };
        if panel.channel != channel {
            return;
        }
        panel.sort = sort;
        let query = panel.query.clone();
        self.search_for(query, cx);
    }

    /// Ask Stoat for `query` in `channel` and show the panel, waiting. An
    /// answer lands only if no later search (or channel change) has replaced
    /// this one meanwhile.
    fn start_search(
        &mut self,
        channel: String,
        query: String,
        sort: MessageSort,
        cx: &mut Context<Self>,
    ) {
        self.search_seq += 1;
        let seq = self.search_seq;
        self.side_panel = SidePanel::Search(SearchPanel {
            channel: channel.clone(),
            query: query.clone(),
            sort: sort.clone(),
            seq,
            state: SearchState::Loading,
        });
        let client = self.client.clone();
        let request = spawn_io({
            let channel = channel.clone();
            async move {
                client
                    .search_messages(&channel, &query, sort, RESULT_LIMIT)
                    .await
            }
        });
        self.search_task = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if !matches!(&this.side_panel, SidePanel::Search(panel) if panel.seq == seq) {
                    return;
                }
                let state = match result {
                    Ok(Ok(page)) => this.search_results(&channel, page),
                    Ok(Err(err)) => {
                        log::warn!("searching {channel} failed: {err:?}");
                        SearchState::Error(search_error_text(&err))
                    }
                    Err(err) => {
                        log::warn!("search task failed: {err:?}");
                        SearchState::Error("Something went wrong searching.".into())
                    }
                };
                if let SidePanel::Search(panel) = &mut this.side_panel {
                    panel.state = state;
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Rows for a search's or pins request's `page` in `channel`, taking in
    /// the authors that came with it.
    fn result_rows(&mut self, channel: &str, page: BulkMessageResponse) -> Vec<MessageRow> {
        let messages = self.store.absorb_results(page);
        // Authors (and members) may be new.
        self.forget_member_lists();
        self.store.result_rows(channel, &messages)
    }

    /// What the panel shows for a search's `page` in `channel`.
    fn search_results(&mut self, channel: &str, page: BulkMessageResponse) -> SearchState {
        let rows = self.result_rows(channel, page);
        if rows.is_empty() {
            SearchState::Empty
        } else {
            SearchState::Results(Rc::new(rows))
        }
    }

    /// Esc in the search box: forget the search and give the panel back to
    /// the member list, then go back to typing a message.
    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_task = None;
        self.search_seq += 1;
        // Pins showing instead are not the search's to close.
        if matches!(self.side_panel, SidePanel::Search(_)) {
            self.side_panel = SidePanel::Members;
        }
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
        cx.notify();
    }

    fn on_search_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.close_search(window, cx);
    }

    /// The header's pin button: show the open channel's pinned messages in
    /// the panel, or put them away if they're showing.
    fn toggle_pins(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.current_channel().map(|c| c.id) else {
            return;
        };
        if matches!(&self.side_panel, SidePanel::Pins(panel) if panel.channel == channel) {
            self.close_pins(cx);
        } else {
            self.start_pins(channel, false, cx);
        }
    }

    /// Give the panel back to the member list.
    fn close_pins(&mut self, cx: &mut Context<Self>) {
        self.pins_task = None;
        self.pins_refresh = None;
        self.pins_seq += 1;
        self.side_panel = SidePanel::Members;
        cx.notify();
    }

    /// Ask Stoat for `channel`'s pinned messages. With `keep_rows`, a list
    /// already showing stays up until the new one arrives (a refresh);
    /// otherwise the panel opens waiting, replacing any search. An answer
    /// lands only if no later request (or channel change) has replaced this
    /// one meanwhile.
    fn start_pins(&mut self, channel: String, keep_rows: bool, cx: &mut Context<Self>) {
        self.pins_seq += 1;
        let seq = self.pins_seq;
        self.pins_refresh = None;
        match &mut self.side_panel {
            SidePanel::Pins(panel) if keep_rows && panel.channel == channel => panel.seq = seq,
            _ => {
                self.search_task = None;
                self.search_seq += 1;
                self.side_panel = SidePanel::Pins(PinsPanel {
                    channel: channel.clone(),
                    seq,
                    state: PinsState::Loading,
                });
            }
        }
        let client = self.client.clone();
        let request = spawn_io({
            let channel = channel.clone();
            async move { client.fetch_pins(&channel, PINS_LIMIT).await }
        });
        self.pins_task = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if !matches!(&this.side_panel, SidePanel::Pins(panel) if panel.seq == seq) {
                    return;
                }
                let state = match result {
                    Ok(Ok(page)) => {
                        let rows = this.result_rows(&channel, page);
                        if rows.is_empty() {
                            PinsState::Empty
                        } else {
                            PinsState::Results(Rc::new(rows))
                        }
                    }
                    Ok(Err(err)) => {
                        log::warn!("loading pins of {channel} failed: {err:?}");
                        PinsState::Error(pins_error_text(&err))
                    }
                    Err(err) => {
                        log::warn!("pins task failed: {err:?}");
                        PinsState::Error("Something went wrong loading pins.".into())
                    }
                };
                if let SidePanel::Pins(panel) = &mut this.side_panel {
                    panel.state = state;
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Fetch the pins list again now, if `channel`'s is showing.
    fn refresh_pins(&mut self, channel: &str, cx: &mut Context<Self>) {
        if matches!(&self.side_panel, SidePanel::Pins(panel) if panel.channel == channel) {
            self.start_pins(channel.to_string(), true, cx);
        }
    }

    /// A message in `channel` was pinned or unpinned: fetch the pins list
    /// again, if it's showing, once things have settled.
    fn schedule_pins_refresh(&mut self, channel: &str, cx: &mut Context<Self>) {
        if !matches!(&self.side_panel, SidePanel::Pins(panel) if panel.channel == channel) {
            return;
        }
        let channel = channel.to_string();
        self.pins_refresh = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PINS_REFRESH_DELAY).await;
            this.update(cx, |this, cx| this.refresh_pins(&channel, cx))
                .ok();
        }));
    }

    /// Messages were deleted: take them off the pins list, if it's showing.
    fn drop_pins(&mut self, channel: &str, ids: &[String]) {
        let SidePanel::Pins(panel) = &mut self.side_panel else {
            return;
        };
        let PinsState::Results(rows) = &panel.state else {
            return;
        };
        if panel.channel != channel || !rows.iter().any(|row| ids.contains(&row.id)) {
            return;
        }
        let kept: Vec<MessageRow> = rows
            .iter()
            .filter(|row| !ids.contains(&row.id))
            .cloned()
            .collect();
        panel.state = if kept.is_empty() {
            PinsState::Empty
        } else {
            PinsState::Results(Rc::new(kept))
        };
    }

    /// Ask before pinning or unpinning: a dialog showing the message, with
    /// Cancel and Pin / Unpin (Enter confirms, Esc cancels). Pinning posts a
    /// message everyone in the channel sees.
    fn confirm_pin(
        &mut self,
        channel: String,
        id: String,
        pin: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snippet: SharedString = self
            .store
            .message_snippet(&channel, &id)
            .unwrap_or_default()
            .into();
        let (title, ok_text) = if pin {
            ("Pin message", "Pin")
        } else {
            ("Unpin message", "Unpin")
        };
        let description = if pin {
            "Everyone in this channel will see that you pinned it. Hold Shift when choosing Pin message to skip this."
        } else {
            "Everyone in this channel will see that you unpinned it. Hold Shift when choosing Unpin message to skip this."
        };
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let theme = cx.theme();
            let (view, channel, id) = (view.clone(), channel.clone(), id.clone());
            alert
                .confirm()
                .title(title)
                .description(description)
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
                .ok_text(ok_text)
                .cancel_text("Cancel")
                .on_ok(move |_, window, cx| {
                    let (channel, id) = (channel.clone(), id.clone());
                    view.update(cx, |this, cx| this.toggle_pin(channel, id, pin, window, cx))
                        .ok();
                    true
                })
        });
    }

    /// Pin (`pin`) or unpin message `id`. Stoat answers with the update,
    /// which changes the row and the pins list; nothing is shown before it.
    /// A message already in the state asked for counts as done. Anything
    /// else gets a toast.
    fn toggle_pin(
        &mut self,
        channel: String,
        id: String,
        pin: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let client = self.client.clone();
        let request = spawn_io({
            let (channel, id) = (channel.clone(), id.clone());
            async move {
                if pin {
                    client.pin_message(&channel, &id).await
                } else {
                    client.unpin_message(&channel, &id).await
                }
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                let error = match result {
                    Ok(Ok(())) => {
                        log::debug!("pin={pin} of {id} in {channel} accepted");
                        return;
                    }
                    // Someone got there first: what we wanted. The update that
                    // said so was missed, so catch up.
                    Ok(Err(err)) if err.pin_already_done() => {
                        if this.store.set_pinned(&channel, &id, pin) {
                            this.sync_pane(&channel, cx);
                        }
                        this.refresh_pins(&channel, cx);
                        return;
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't pin={pin} {id} in {channel}: {err:?}");
                        pin_error_text(pin, &err)
                    }
                    Err(err) => {
                        log::warn!("pin task failed: {err:?}");
                        if pin {
                            "Couldn't pin the message.".into()
                        } else {
                            "Couldn't unpin the message.".into()
                        }
                    }
                };
                window.push_notification(Notification::error(error), cx);
            })
            .ok();
        })
        .detach();
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
    /// Open an external link (`http`, `https` or `mailto`) whose text, if
    /// it said something other than the address, was `text`. Links to the
    /// user's own instance and to trusted sites open at once; anything else
    /// asks first.
    fn open_external(
        &mut self,
        url: String,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let own = self.media.bases().chain(self.web_app.as_deref());
        if is_own_instance(&url, own) || self.safety.trusts(&url) {
            cx.open_url(&url);
            return;
        }
        let check = classify(&url, text.as_deref());
        self.link_prompt = Some(LinkPrompt {
            // Only a link that shows its own address can be trusted.
            origin: origin(&url).filter(|_| check == LinkCheck::Plain),
            url,
            check,
            trust: false,
            understood: false,
        });
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let Some(prompt) = view
                .upgrade()
                .and_then(|view| view.read(cx).link_prompt.clone())
            else {
                return alert;
            };
            let go = {
                let view = view.clone();
                move |window: &mut Window, cx: &mut App| {
                    view.update(cx, |this, cx| this.follow_link_prompt(cx)).ok();
                    window.close_dialog(cx);
                }
            };
            // A checkbox's change, to the prompt.
            let tick = |set: fn(&mut LinkPrompt, bool)| {
                let view = view.clone();
                move |ticked: &bool, _: &mut Window, cx: &mut App| {
                    let ticked = *ticked;
                    view.update(cx, |this, cx| {
                        if let Some(prompt) = &mut this.link_prompt {
                            set(prompt, ticked);
                        }
                        cx.notify();
                    })
                    .ok();
                }
            };
            let theme = cx.theme();
            let ready = prompt.ready();
            alert
                .title("External links can be dangerous!")
                .description(format!("Are you sure you want to go to {}?", prompt.url))
                .map(|alert| match &prompt.check {
                    LinkCheck::Plain => alert,
                    LinkCheck::Text(text) => {
                        alert.child(div().text_sm().child(format!("You clicked on \"{text}\"")))
                    }
                    LinkCheck::Mismatch(text) => alert
                        .child(
                            v_flex()
                                .gap_2()
                                .text_sm()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.danger)
                                        .child(
                                            "Be careful! This is not the same as the link \
                                             that was displayed:",
                                        ),
                                )
                                .child(
                                    div()
                                        .px_3()
                                        .py_2()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(theme.border)
                                        .bg(theme.secondary)
                                        .line_clamp(3)
                                        .child(text.clone()),
                                ),
                        )
                        .child(
                            Checkbox::new("link-understood")
                                .label("I understand the risk")
                                .checked(prompt.understood)
                                .on_click(tick(|prompt, ticked| prompt.understood = ticked)),
                        ),
                })
                .when_some(prompt.origin.clone(), |alert, origin| {
                    alert.child(
                        Checkbox::new("link-trust")
                            .label(format!("Don't ask me again for {origin}"))
                            .checked(prompt.trust)
                            .on_click(tick(|prompt, ticked| prompt.trust = ticked)),
                    )
                })
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("link-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("link-continue")
                                .primary()
                                .label("Continue")
                                .disabled(!ready)
                                .on_click({
                                    let go = go.clone();
                                    move |_, window, cx| go(window, cx)
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    if ready {
                        go(window, cx);
                    }
                    false
                })
        });
    }

    /// The link dialog's Continue: open its link, and trust its site from
    /// now on if the user ticked "Don't ask me again".
    fn follow_link_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.link_prompt.take_if(|prompt| prompt.ready()) else {
            return;
        };
        if prompt.trust && self.safety.trust(&prompt.url) {
            self.save_safety();
        }
        cx.open_url(&prompt.url);
    }

    /// Save trusted sites and mature-channel confirmations for next launch.
    fn save_safety(&self) {
        if let Some(file) = &self.safety_file
            && let Err(err) = file.save(&self.safety)
        {
            log::warn!("couldn't save safety settings: {err:#}");
        }
    }

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
                        this.ack_sent(&channel, &message.id, cx);
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
                        if this.store.discard_pending(&channel, &key).is_some() {
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

    /// Open `user`'s profile card at `position`, as seen from the open
    /// channel, and fetch their profile (bio and banner) again. The last one
    /// fetched shows meanwhile.
    fn open_card(
        &mut self,
        user: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.store.knows_user(&user) {
            return;
        }
        self.card = Some(OpenCard {
            user: user.clone(),
            channel: self.current_channel().map(|c| c.id),
            position,
            closing: false,
        });
        window.focus(&self.card_focus, cx);
        let request = spawn_io({
            let (client, user) = (self.client.clone(), user.clone());
            async move { client.fetch_profile(&user).await }
        });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(profile)) => {
                        this.profiles.insert(user, Some(profile));
                    }
                    // Not connected to them closely enough: no bio or banner.
                    Ok(Err(err))
                        if matches!(
                            err.error_type(),
                            Some(revolt_result::ErrorType::MissingPermission { .. })
                        ) =>
                    {
                        this.profiles.insert(user, None);
                    }
                    Ok(Err(err)) => log::warn!("couldn't load the profile of {user}: {err}"),
                    Err(err) => log::warn!("profile task failed: {err:?}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Change how the user relates to `user` (a friend request, a block).
    /// Remove friend and Block are asked about first, unless `confirmed`
    /// (Shift was held). Stoat answers with them as they now relate, which
    /// updates the card and menus; a `UserRelationship` follows too. One
    /// already as asked (another client got there first) is fine; other
    /// refusals get a toast.
    fn set_relationship(
        &mut self,
        user: String,
        action: RelationshipAction,
        confirmed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action.needs_confirming() && !confirmed {
            self.confirm_relationship(user, action, window, cx);
            return;
        }
        if !self.relationship_pending.insert(user.clone()) {
            return;
        }
        cx.notify();
        let request = spawn_io({
            let (client, user) = (self.client.clone(), user.clone());
            async move {
                match action {
                    RelationshipAction::AddFriend | RelationshipAction::AcceptFriend => {
                        client.add_friend(&user).await
                    }
                    RelationshipAction::RejectFriend
                    | RelationshipAction::CancelFriend
                    | RelationshipAction::RemoveFriend => client.remove_friend(&user).await,
                    RelationshipAction::Block => client.block_user(&user).await,
                    RelationshipAction::Unblock => client.unblock_user(&user).await,
                }
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.relationship_pending.remove(&user);
                cx.notify();
                let error: SharedString = match result {
                    Ok(Ok(person)) => {
                        log::info!("{action:?} {user}: now {:?}", person.relationship);
                        this.relationship_changed(person, cx);
                        return;
                    }
                    Ok(Err(err)) if err.relationship_already_done() => {
                        log::debug!("{action:?} {user} was already done");
                        return;
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't {action:?} {user}: {err:?}");
                        err.to_string().into()
                    }
                    Err(err) => {
                        log::warn!("relationship task failed: {err:?}");
                        "Couldn't reach Stoat. Try again.".into()
                    }
                };
                window.push_notification(Notification::error(error), cx);
            })
            .ok();
        })
        .detach();
    }

    /// Ask before removing a friend or blocking someone.
    fn confirm_relationship(
        &mut self,
        user: String,
        action: RelationshipAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .store
            .user(&user)
            .map(|person| display_name(person).to_string())
            .unwrap_or_else(|| "them".into());
        let (title, description, ok): (SharedString, SharedString, &'static str) = match action {
            RelationshipAction::Block => (
                format!("Block {name}").into(),
                "They won't be able to message you or send you friend requests. \
                 Hold Shift when choosing Block to skip this."
                    .into(),
                "Block",
            ),
            _ => (
                format!("Remove {name} as a friend").into(),
                "Hold Shift when choosing Remove friend to skip this.".into(),
                "Remove friend",
            ),
        };
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (view, user) = (view.clone(), user.clone());
            alert
                .confirm()
                .title(title.clone())
                .description(description.clone())
                .ok_text(ok)
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Cancel")
                .on_ok(move |_, window, cx| {
                    let user = user.clone();
                    view.update(cx, |this, cx| {
                        this.set_relationship(user, action, true, window, cx)
                    })
                    .ok();
                    true
                })
        });
    }

    /// Someone's relationship to the user changed (a route's answer, or
    /// `UserRelationship`): the card, menus, member list and Friends page
    /// follow. Returns how they related before (`None` if unknown).
    fn relationship_changed(
        &mut self,
        person: crate::api::models::User,
        cx: &mut Context<Self>,
    ) -> Option<crate::api::models::RelationshipStatus> {
        use crate::api::models::RelationshipStatus;
        let blocked = person.relationship == RelationshipStatus::Blocked;
        let was = self.store.apply_relationship(person);
        self.forget_member_lists();
        // Their messages fold away, or come back.
        if blocked != (was == Some(RelationshipStatus::Blocked)) {
            let loaded: Vec<String> = self.panes.keys().cloned().collect();
            for channel in loaded {
                self.sync_pane(&channel, cx);
            }
        }
        was
    }

    /// The Friends page's "Add friend": a dialog with a `username#1234`
    /// field, empty each time it opens. It stays open after a request goes
    /// out, saying so, so another can follow.
    fn open_add_friend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_friend_state = None;
        self.add_friend_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        let view = cx.entity().downgrade();
        let input = self.add_friend_input.clone();
        let first_build = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first_build.replace(false) {
                let focus = input.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            let state = view
                .upgrade()
                .and_then(|view| view.read(cx).add_friend_state.clone());
            let send = {
                let view = view.clone();
                move |window: &mut Window, cx: &mut App| {
                    view.update(cx, |this, cx| this.send_friend_request(window, cx))
                        .ok();
                }
            };
            let close = |window: &mut Window, cx: &mut App| window.close_dialog(cx);
            dialog
                .title("Add friend")
                .w(px(ADD_FRIEND_DIALOG_WIDTH))
                .child(render_add_friend(&input, state.as_ref(), send, close, cx))
        });
    }

    /// Send a friend request to whoever is typed in the add-friend dialog
    /// (`username#1234`). How it went shows under the field; once it's sent,
    /// the field clears and they're under Pending → Outgoing.
    fn send_friend_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_friend_state == Some(AddFriendState::Sending) {
            return;
        }
        let typed = self.add_friend_input.read(cx).value().to_string();
        let username = match crate::friends::check_username(&typed) {
            Ok(username) => username.to_string(),
            Err(hint) => {
                self.add_friend_state = Some(AddFriendState::Error(hint.into()));
                window.refresh();
                cx.notify();
                return;
            }
        };
        self.add_friend_state = Some(AddFriendState::Sending);
        window.refresh();
        cx.notify();
        let request = spawn_io({
            let (client, username) = (self.client.clone(), username.clone());
            async move { client.send_friend_request(&username).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.add_friend_state = Some(match result {
                    Ok(Ok(person)) => {
                        log::info!(
                            "friend request to {username}: now {:?}",
                            person.relationship
                        );
                        let state = crate::friends::request_sent(&person);
                        this.relationship_changed(person, cx);
                        this.add_friend_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        state
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't send a friend request to {username}: {err:?}");
                        crate::friends::request_refused(&username, &err)
                    }
                    Err(err) => {
                        log::warn!("friend request task failed: {err:?}");
                        AddFriendState::Error("Couldn't reach Stoat. Try again.".into())
                    }
                });
                window.refresh();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// "Message" on `user`'s card: open the DM with them. One that's known
    /// opens here (reopened if it was closed). Otherwise Stoat is asked for
    /// it, which creates it if it's allowed (see [`crate::profile::can_message`]).
    fn message_user(&mut self, user: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dm) = crate::profile::dm_with(&self.store, &user) {
            self.store.activate_dm(&dm);
            self.close_card(true, window, cx);
            self.open_channel(&dm, cx);
            return;
        }
        if self.dm_opening.is_some() {
            return;
        }
        self.dm_opening = Some(user.clone());
        cx.notify();
        let request = spawn_io({
            let (client, user) = (self.client.clone(), user.clone());
            async move { client.open_dm(&user).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.dm_opening = None;
                cx.notify();
                let error: SharedString = match result {
                    Ok(Ok(channel)) => {
                        let id = crate::store::channel_id(&channel).to_string();
                        log::info!("opened DM {id} with {user}");
                        // Its `ChannelCreate` may come too; adding is idempotent.
                        this.store.add_channel(channel);
                        this.store.activate_dm(&id);
                        this.close_card(true, window, cx);
                        this.open_channel(&id, cx);
                        return;
                    }
                    Ok(Err(err))
                        if matches!(
                            err.error_type(),
                            Some(revolt_result::ErrorType::MissingPermission { .. })
                        ) =>
                    {
                        CANNOT_MESSAGE_TEXT.into()
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't open a DM with {user}: {err:?}");
                        err.to_string().into()
                    }
                    Err(err) => {
                        log::warn!("DM task failed: {err:?}");
                        "Couldn't open the conversation.".into()
                    }
                };
                window.push_notification(Notification::error(error), cx);
            })
            .ok();
        })
        .detach();
    }

    /// The buttons on `user`'s card: Message, if they can be messaged, and
    /// "…" for the rest of their menu.
    fn card_actions(&self, user: &str, people: &People, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut actions = Vec::new();
        let pending = self.relationship_pending.contains(user);
        for action in crate::profile::friend_buttons(&self.store, user) {
            let target = user.to_string();
            let (label, icon) = relationship_label(action);
            let button = Button::new(SharedString::from(format!("card-{action:?}")))
                .small()
                .icon(icon)
                .disabled(pending)
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    let shift = event.modifiers().shift;
                    this.set_relationship(target.clone(), action, shift, window, cx)
                }));
            // Accept and Add are labelled; Reject is just its icon, beside Accept.
            let button = match action {
                RelationshipAction::AddFriend => button.primary().label("Add friend"),
                RelationshipAction::AcceptFriend => button.primary().label("Accept"),
                RelationshipAction::CancelFriend => button.outline().label("Cancel request"),
                _ => button.outline().tooltip(label),
            };
            actions.push(button.into_any_element());
        }
        if crate::profile::can_message(&self.store, user) {
            let opening = self.dm_opening.as_deref() == Some(user);
            let target = user.to_string();
            actions.push(
                Button::new("card-message")
                    .primary()
                    .small()
                    .icon(IconName::MessageCircle)
                    .label("Message")
                    .loading(opening)
                    .disabled(opening)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.message_user(target.clone(), window, cx)
                    }))
                    .into_any_element(),
            );
        }
        actions.push(
            Button::new("card-more")
                .ghost()
                .small()
                .icon(IconName::Ellipsis)
                .tooltip("More")
                .dropdown_menu(people.menu(user, false))
                .into_any_element(),
        );
        actions
    }

    /// What the people shown in `channel` do when clicked (see [`People`]).
    fn people(&self, channel: Option<String>, cx: &Context<Self>) -> People {
        People {
            view: cx.entity().downgrade(),
            channel,
        }
    }

    /// An item of a person's menu was chosen.
    fn on_user_menu(
        &mut self,
        user: String,
        channel: Option<String>,
        item: UserMenuItem,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            UserMenuItem::Relationship(action) => {
                self.set_relationship(user, action, shift, window, cx)
            }
            UserMenuItem::Profile => self.open_card(user, window.mouse_position(), window, cx),
            UserMenuItem::Message => self.message_user(user, window, cx),
            UserMenuItem::Mention => {
                let Some(channel) = channel else {
                    return;
                };
                let label = self
                    .store
                    .token_label(&channel, &TokenRef::User(user.clone()));
                self.close_card(false, window, cx);
                self.composer.update(cx, |composer, cx| {
                    composer.insert_mention(&user, label, window, cx)
                });
            }
            UserMenuItem::CopyUsername => {
                if let Some(person) = self.store.user(&user) {
                    let tag = format!("{}#{}", person.username, person.discriminator);
                    cx.write_to_clipboard(ClipboardItem::new_string(tag));
                }
            }
            UserMenuItem::CopyUserId => cx.write_to_clipboard(ClipboardItem::new_string(user)),
            UserMenuItem::Separator => {}
        }
    }

    /// Close the profile card. `refocus`: give focus back to the message box
    /// (after Esc; a click elsewhere focuses what it clicked).
    fn close_card(&mut self, refocus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.card.take().is_none() {
            return;
        }
        if refocus {
            self.composer
                .update(cx, |composer, cx| composer.focus(window, cx));
        }
        cx.notify();
    }

    /// A button came up outside the card: close it if it went down outside.
    fn finish_closing_card(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.card.as_ref().is_some_and(|card| card.closing) {
            self.close_card(false, window, cx);
        }
    }

    /// A card opened from another channel than the open one closes.
    fn sync_card(&mut self) {
        let current = self.current_channel().map(|c| c.id);
        if self
            .card
            .as_ref()
            .is_some_and(|card| card.channel != current)
        {
            self.card = None;
        }
    }

    /// The open card, floating at its position and kept inside the window.
    fn render_card(&self, open: &OpenCard, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = crate::profile::profile_card(&self.store, &open.user, open.channel.as_deref())?;
        let profile = self.profiles.get(&open.user).and_then(Option::as_ref);
        let banner_url = profile
            .and_then(|p| p.background.as_ref())
            .and_then(|file| self.store.media().file(file));
        let bio = profile
            .and_then(|p| p.content.as_deref())
            .map(str::trim)
            .filter(|content| !content.is_empty())
            .map(|content| {
                let markdown = crate::markdown::prepare(content, &self.store);
                CardBio {
                    render_key: crate::store::render_key(
                        &format!("bio-{}", open.user),
                        Some(&markdown),
                    ),
                    markdown,
                }
            });
        let people = self.people(open.channel.clone(), cx);
        let on_link = people.link_handler();
        let content = CardContent {
            card,
            banner_url,
            bio,
        };
        let actions = self.card_actions(&open.user, &people, cx);
        let element = render_profile_card(&content, actions, on_link, cx)
            .id("profile-card")
            .track_focus(&self.card_focus)
            .occlude()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_card(true, window, cx);
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                if let Some(card) = &mut this.card {
                    card.closing = true;
                }
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::finish_closing_card))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::finish_closing_card))
            .on_scroll(|_, _, cx| cx.stop_propagation());
        Some(
            deferred(
                anchored()
                    .position(open.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(element),
            )
            .with_priority(1)
            .into_any_element(),
        )
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
        self.friends_seen(cx);
        self.ensure_current_loaded(cx);
        cx.notify();
    }

    /// An in-app toast for a notifying message; one per channel (a newer
    /// message replaces the older toast). Clicking it opens the channel.
    fn show_toast(&mut self, notice: Notice, window: &mut Window, cx: &mut Context<Self>) {
        let notice = self.labelled(notice);
        log::debug!("notify: {} | {}", notice.title, notice.body);
        let view = cx.entity().downgrade();
        let channel = notice.channel.clone();
        let toast = Notification::new()
            .id1::<ChatView>(SharedString::from(notify::banner_tag(
                &self.account_id,
                &notice.tag,
            )))
            .title(notice.title)
            .message(notice.body)
            .on_click(move |_, _, cx| {
                view.update(cx, |this, cx| {
                    this.open_channel(&channel, cx);
                    cx.emit(ChatEvent::Activate);
                })
                .ok();
            });
        window.push_notification(toast, cx);
    }

    /// A macOS banner for a notifying message while RuSToat is in the
    /// background. Tagged with the channel and account, so a newer message
    /// replaces the channel's older banner, and the owner can route a click
    /// ([`notify::parse_banner_tag`]). Reading the channel takes the banner
    /// away ([`Self::sync_outside`]).
    fn show_banner(&mut self, notice: Notice, message_id: String, cx: &mut Context<Self>) {
        let notice = self.labelled(notice);
        log::debug!("banner: {} | {}", notice.title, notice.body);
        cx.show_system_notification(SystemNotification {
            tag: notify::banner_tag(&self.account_id, &notice.channel).into(),
            title: notice.title.into(),
            body: notice.body.into(),
            actions: Vec::new(),
        });
        self.banners.insert(notice.channel, message_id);
    }

    /// A macOS banner for a new friend request while RuSToat is in the
    /// background. All of an account's share one tag, so a newer request
    /// replaces the older banner, and a click opens Friends → Pending (see
    /// [`Self::open_channel`]).
    fn show_friend_banner(&mut self, notice: Notice, cx: &mut Context<Self>) {
        let notice = self.labelled(notice);
        log::debug!("banner: {} | {}", notice.title, notice.body);
        cx.show_system_notification(SystemNotification {
            tag: notify::banner_tag(&self.account_id, FRIENDS).into(),
            title: notice.title.into(),
            body: notice.body.into(),
            actions: Vec::new(),
        });
        self.friend_banner = true;
    }

    /// Take the friend request's banner away, if one is showing.
    fn dismiss_friend_banner(&mut self, cx: &mut App) {
        if std::mem::take(&mut self.friend_banner) {
            log::debug!("banner: dismissing friend requests");
            cx.dismiss_system_notification(&notify::banner_tag(&self.account_id, FRIENDS));
        }
    }

    /// Keep what's outside the window in step with this account, after any
    /// change: take away banners for channels read since (here, in another
    /// session, by posting, or with Mark as read), delete pasted images no
    /// pending message needs any more, and return how many things need
    /// attention, for the Dock badge. The owner calls this whenever the view
    /// notifies, which hidden views do too.
    pub fn sync_outside(&mut self, cx: &mut Context<Self>) -> usize {
        for file in self.store.take_released_files() {
            crate::draft::remove_temporary(&file);
        }
        let store = &self.store;
        let account = &self.account_id;
        self.banners.retain(|channel, message| {
            let read = store.has_read(channel, message);
            if read {
                log::debug!("banner: dismissing {channel} (read)");
                cx.dismiss_system_notification(&notify::banner_tag(account, channel));
            }
            !read
        });
        // Every request handled (here or elsewhere).
        if self.friend_banner && crate::friends::incoming_count(&self.store) == 0 {
            self.dismiss_friend_banner(cx);
        }
        self.store.attention_count()
    }

    /// `notice`, with the account's label (if any) added to its title.
    fn labelled(&self, mut notice: Notice) -> Notice {
        if let Some(label) = &self.account_label {
            notice.title = format!("{} · {label}", notice.title);
        }
        notice
    }

    /// Say which account notices are for (see [`Self::labelled`]), or not.
    pub fn set_account_label(&mut self, label: Option<String>) {
        self.account_label = label;
    }

    /// For the account's switcher button: anything unread, and how many
    /// unread mentions (see [`ChatStore::account_summary`]).
    pub fn account_summary(&self) -> (bool, usize) {
        self.store.account_summary()
    }

    /// The signed-in user's display name and avatar, once `Ready` has them.
    pub fn me_summary(&self) -> Option<(String, String)> {
        let me = self.store.me();
        let user = self.store.user(me)?;
        Some((
            display_name(user).to_string(),
            self.store.user_avatar_url(me),
        ))
    }

    /// Log what makes up [`Self::sync_outside`]'s count.
    pub fn log_attention(&self) {
        for (channel, count) in self.store.attention_breakdown() {
            log::debug!("  +{count} {channel} ({})", self.account_id);
        }
    }

    /// Use `tone` for toneable emoji from now on (the app-wide preference).
    pub fn set_skin_tone(&mut self, tone: crate::prefs::Tone, cx: &mut Context<Self>) {
        self.skin_tone = tone;
        self.composer
            .update(cx, |composer, cx| composer.set_skin_tone(tone, cx));
        self.refresh_suggestions(cx);
    }

    /// Show or hide this account. Only the account on screen marks things
    /// read, and its open channel doesn't count as being looked at while
    /// hidden.
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if !active {
            self.pending_ack = None;
            // Coming back counts as opening the channel again.
            self.held_unread = None;
            if let Some((channel, _)) = self.unread_marker.take() {
                self.sync_pane(&channel, cx);
            }
        }
        // Showing it again re-renders, which acks what's now on screen.
        cx.notify();
    }

    /// Show `channel`: switch to its server (or Home) and select it.
    /// [`FRIENDS`] (a friend request's notice) opens Home's Friends page on
    /// its Pending tab.
    pub fn open_channel(&mut self, channel: &str, cx: &mut Context<Self>) {
        let place = match self.store.channel(channel) {
            Some(crate::api::models::Channel::TextChannel { server, .. }) => {
                Place::Server(server.clone())
            }
            Some(_) => Place::Home,
            None if channel == FRIENDS => Place::Home,
            None => return,
        };
        self.place = place.clone();
        self.selected.insert(place, channel.to_string());
        if channel == FRIENDS {
            self.friends_tab = FriendsTab::Pending;
        }
        self.remember_places();
        self.friends_seen(cx);
        self.ensure_current_loaded(cx);
        cx.notify();
    }

    fn select_channel(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected.insert(self.place.clone(), id);
        self.remember_places();
        self.friends_seen(cx);
        self.ensure_current_loaded(cx);
        cx.notify();
    }

    /// After choosing what's open: on the Friends page, its banner has been
    /// seen.
    fn friends_seen(&mut self, cx: &mut Context<Self>) {
        if self.friends_open() {
            self.dismiss_friend_banner(cx);
        }
    }

    /// Save the open place and picked channels for next launch, if they changed.
    fn remember_places(&mut self) {
        let Some(file) = &self.places_file else {
            return;
        };
        let places = Places {
            place: self.place.clone(),
            selected: self.selected.clone(),
            collapsed: self.collapsed.clone(),
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
        self.ensure_members(cx);
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
                            let was_away = !this.store.is_at_present(&channel);
                            this.store.set_history(&channel, page, HISTORY_PAGE);
                            log::debug!(
                                "loaded {} messages for {channel}",
                                this.store.history(&channel).map_or(0, |h| h.messages.len())
                            );
                            if was_away {
                                // Back from a window: the old rows don't lead
                                // to the new ones, and neither do pages still
                                // on their way for them.
                                this.loading_older.remove(&channel);
                                this.loading_newer.remove(&channel);
                                this.older_errors.remove(&channel);
                                this.newer_errors.remove(&channel);
                                this.reset_pane(&channel, None, cx);
                            } else {
                                this.sync_pane(&channel, cx);
                            }
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
    /// `channel`'s rows, with the "New messages" divider if it's the open
    /// channel and had unread messages when opened.
    fn timeline(&self, channel: &str) -> Vec<TimelineRow> {
        let rows = self.store.timeline(channel);
        match &self.unread_marker {
            Some((c, Some(after))) if c == channel => with_unread_divider(rows, after),
            _ => rows,
        }
    }

    /// Record where the user left off when a channel is opened, and drop the
    /// divider from the one they left. Runs on every render, before the
    /// deferred [`Self::maybe_ack_current`] can mark the new channel read.
    fn sync_unread_marker(&mut self, cx: &mut Context<Self>) {
        // Read positions come with Ready; until then there's nothing to record.
        if !self.has_ready || !self.active {
            return;
        }
        let current = self.current_channel().map(|c| c.id);
        let marked = self.unread_marker.as_ref().map(|(c, _)| c);
        if marked == current.as_ref() {
            return;
        }
        let left = self.unread_marker.take().map(|(c, _)| c);
        // Left the channel marked unread: it can be marked read again.
        self.held_unread = None;
        if let Some(channel) = current {
            let after = self
                .store
                .is_unread(&channel)
                .then(|| self.store.read_position(&channel).map(str::to_string))
                .flatten();
            self.unread_marker = Some((channel.clone(), after));
            self.sync_pane(&channel, cx);
        }
        if let Some(left) = left {
            self.sync_pane(&left, cx);
        }
    }

    /// Take the divider out of `channel` (the user sent a message there).
    fn clear_unread_divider(&mut self, channel: &str, cx: &mut Context<Self>) {
        if let Some((c, after)) = &mut self.unread_marker
            && c == channel
            && after.take().is_some()
        {
            self.sync_pane(channel, cx);
        }
    }

    fn sync_pane(&mut self, channel: &str, cx: &mut Context<Self>) {
        // Not loaded yet (a message sent while it loads): the load draws it.
        if self.store.history(channel).is_none() {
            return;
        }
        let rows = Rc::new(self.timeline(channel));
        let at_present = self.store.is_at_present(channel);
        if let Some(pane) = self.panes.get_mut(channel) {
            let ops = diff_rows(&pane.rows, &rows);
            // Rows added at the end of a window away from the present are
            // newer pages. The reader being at its bottom must not carry them
            // to the new bottom, so the place is kept.
            let was_away = !pane.at_present;
            pane.rows = rows;
            pane.at_present = at_present;
            pane.scroller.update(cx, |scroller, cx| {
                for op in ops {
                    let applied = match op {
                        RowOp::Reset(count) => {
                            scroller.reset(count, cx);
                            scroller.scroll_to_end(cx);
                            true
                        }
                        RowOp::Prepend(count) => scroller.prepend(count, cx),
                        RowOp::Append(count) if was_away && scroller.is_following_tail() => {
                            let top = scroller.logical_scroll_top().item_ix;
                            let appended = scroller.append(count, cx);
                            scroller.scroll_to_item(top, cx);
                            appended
                        }
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
        self.fetch_missing_emojis(channel, cx);
            return;
        }

        let scroller = cx.new(|cx| MessageScrollerState::new(rows.len(), cx));
        let watched = channel.to_string();
        let scroll = cx.observe(&scroller, move |this, _, cx| {
            this.maybe_load_older(&watched, cx);
            this.maybe_load_newer(&watched, cx);
        });
        self.panes.insert(
            channel.to_string(),
            Pane {
                rows,
                at_present,
                scroller,
                _scroll: scroll,
            },
        );
        self.fetch_reply_sources(channel, cx);
        self.fetch_missing_emojis(channel, cx);
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

    /// Fetch custom emoji that `channel`'s messages use but that aren't the
    /// user's servers', so reactions and text can name them, a few at a time.
    /// Each result re-syncs the channel, which starts the next ones.
    fn fetch_missing_emojis(&mut self, channel: &str, cx: &mut Context<Self>) {
        for id in self.store.missing_emojis(channel) {
            if self.emoji_loading >= EMOJI_FETCHES {
                break;
            }
            if !self.emoji_fetches.insert(id.clone()) {
                continue;
            }
            self.emoji_loading += 1;
            let request = spawn_io({
                let (client, id) = (self.client.clone(), id.clone());
                async move { client.fetch_emoji(&id).await }
            });
            let channel = channel.to_string();
            cx.spawn(async move |this, cx| {
                let result = request.await;
                if let Ok(Err(ApiError::RateLimited { retry_after_ms })) = &result {
                    let wait = Duration::from_millis(retry_after_ms.unwrap_or(2_000));
                    log::debug!("emoji {id}: rate limited, waiting {wait:?}");
                    cx.background_executor().timer(wait).await;
                }
                this.update(cx, |this, cx| {
                    this.emoji_loading -= 1;
                    match result {
                        Ok(Ok(emoji)) => this.store.add_other_emoji(emoji),
                        // Tried again below, now the wait is over.
                        Ok(Err(ApiError::RateLimited { .. })) => {
                            this.emoji_fetches.remove(&id);
                        }
                        // Left as `:emoji:`.
                        Ok(Err(err)) => log::warn!("couldn't load emoji {id}: {err:?}"),
                        Err(err) => log::warn!("emoji task failed: {err:?}"),
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

    /// Load the page after the newest loaded message, if `channel` is a window
    /// away from the present and the reader is near its bottom.
    ///
    /// "Not scrolled up" is also true of a list too short to scroll, so this
    /// can be asked again right after a page lands. It never repeats itself:
    /// a request is in flight at most once, it stops at the present, and a
    /// page that adds nothing without reaching the present is an error that
    /// waits for Retry.
    fn maybe_load_newer(&mut self, channel: &str, cx: &mut Context<Self>) {
        if self.loading_newer.contains_key(channel)
            || self.newer_errors.contains_key(channel)
            || self.loading.contains_key(channel)
            || self.jumps.contains_key(channel)
        {
            return;
        }
        let Some(history) = self.store.history(channel) else {
            return;
        };
        if history.at_present || history.messages.is_empty() {
            return;
        }
        let Some(pane) = self.panes.get(channel) else {
            return;
        };
        let near_end = {
            let scroller = pane.scroller.read(cx);
            let count = scroller.item_count();
            let top = scroller.logical_scroll_top().item_ix;
            count > 0
                && (!scroller.is_scrolled_up() || count.saturating_sub(top) <= NEWER_PREFETCH_ROWS)
        };
        if near_end {
            self.load_newer(channel.to_string(), cx);
        }
    }

    fn load_newer(&mut self, channel: String, cx: &mut Context<Self>) {
        let Some(after) = self.store.newest_message_id(&channel).map(str::to_string) else {
            return;
        };
        self.newer_errors.remove(&channel);
        let client = self.client.clone();
        let request = spawn_io({
            let (channel, after) = (channel.clone(), after.clone());
            async move {
                client
                    .fetch_messages_after(&channel, &after, HISTORY_PAGE as i64)
                    .await
            }
        });
        let task = cx.spawn({
            let channel = channel.clone();
            async move |this, cx| {
                let result = request.await;
                this.update(cx, |this, cx| {
                    this.loading_newer.remove(&channel);
                    match result {
                        // The history changed under the request (a jump, a
                        // reload): this page doesn't follow what's loaded.
                        Ok(Ok(_))
                            if this.store.newest_message_id(&channel) != Some(after.as_str()) => {}
                        Ok(Ok(page)) => {
                            let added = this.store.append_page(&channel, page, HISTORY_PAGE);
                            log::debug!(
                                "loaded newer messages for {channel} ({} total, at present: {})",
                                this.store.history(&channel).map_or(0, |h| h.messages.len()),
                                this.store.is_at_present(&channel)
                            );
                            if !added && !this.store.is_at_present(&channel) {
                                // Nothing new, yet not the present: asking
                                // again would get the same answer.
                                this.newer_errors.insert(
                                    channel.clone(),
                                    "Couldn't load newer messages.".into(),
                                );
                            }
                            this.sync_pane(&channel, cx);
                        }
                        Ok(Err(err)) => {
                            log::warn!("loading newer messages for {channel} failed: {err:?}");
                            this.newer_errors.insert(channel, err.to_string().into());
                        }
                        Err(err) => {
                            log::warn!("newer-messages task failed: {err:?}");
                            this.newer_errors
                                .insert(channel, "Couldn't load newer messages.".into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        });
        self.loading_newer.insert(channel, task);
        cx.notify();
    }

    fn retry_newer(&mut self, channel: String, cx: &mut Context<Self>) {
        self.newer_errors.remove(&channel);
        self.load_newer(channel, cx);
    }

    /// Leave a window for the present: reload the latest page, then show the
    /// end of the list (see [`Self::load_history`]).
    fn jump_to_present(&mut self, channel: String, cx: &mut Context<Self>) {
        if self.loading.contains_key(&channel) {
            return;
        }
        self.loading_newer.remove(&channel);
        self.newer_errors.remove(&channel);
        self.load_history(channel, cx);
        cx.notify();
    }

    /// Mark the open channel read on Stoat, if the user can actually see its
    /// newest message: the window is active and the list is at the bottom.
    /// Debounced by [`ACK_DELAY`]; at most one acknowledgement is pending.
    fn maybe_ack_current(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.has_ready || !self.active || !window.is_window_active() {
            return;
        }
        let Some(channel) = self.current_channel().map(|c| c.id) else {
            return;
        };
        if self.held_unread.as_ref() == Some(&channel) {
            return;
        }
        // What's on screen isn't the newest message, so reading it says
        // nothing about the messages after it.
        if !self.store.is_at_present(&channel) {
            return;
        }
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

    /// Tell Stoat the user has read `channel` up to `message`, which they just
    /// sent. Stoat doesn't do this itself, and the store already counts the
    /// user's own messages as read, so [`Self::maybe_ack_current`] never would.
    fn ack_sent(&mut self, channel: &str, message: &str, cx: &mut Context<Self>) {
        // The user has replied, so a divider above their reply says nothing,
        // and the channel they marked unread has been read after all.
        self.clear_unread_divider(channel, cx);
        if self.held_unread.as_deref() == Some(channel) {
            self.held_unread = None;
        }
        // A pending ack for something older would move the read position back.
        if matches!(&self.pending_ack, Some((c, m, _)) if c == channel && m.as_str() < message) {
            self.pending_ack = None;
        }
        self.post_ack(channel, message, "sent", cx);
    }

    /// "Mark as unread" on message `id`: move the read position back to the
    /// message before it, here and on Stoat (other clients follow), and
    /// keep the channel unread until the user leaves it. The divider moves
    /// there too.
    fn mark_unread(&mut self, channel: &str, id: &str, cx: &mut Context<Self>) {
        // The oldest loaded message has nothing before it in memory, so it's
        // acknowledged itself, as revite does: the channel is unread from the
        // message after it, one late.
        let read = self
            .store
            .message_before(channel, id)
            .unwrap_or(id)
            .to_string();
        if matches!(&self.pending_ack, Some((c, ..)) if c == channel) {
            self.pending_ack = None;
        }
        self.held_unread = Some(channel.to_string());
        self.store.set_read_position(channel, Some(&read));
        if self.current_channel().is_some_and(|c| c.id == channel) {
            self.unread_marker = Some((channel.to_string(), Some(read.clone())));
        }
        self.sync_pane(channel, cx);
        self.post_ack(channel, &read, "marked unread", cx);
        cx.notify();
    }

    /// Send `PUT /channels/{channel}/ack/{message}`, logging the outcome
    /// (`why` says what it was for).
    fn post_ack(&self, channel: &str, message: &str, why: &'static str, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let request = spawn_io({
            let (channel, message) = (channel.to_string(), message.to_string());
            async move { client.ack_message(&channel, &message).await }
        });
        let (channel, message) = (channel.to_string(), message.to_string());
        cx.spawn(async move |_, _| match request.await {
            Ok(Ok(())) => log::debug!("acked {channel} up to {message} ({why})"),
            Ok(Err(err)) => log::warn!("couldn't mark {channel} read: {err:?}"),
            Err(err) => log::warn!("ack task failed: {err:?}"),
        })
        .detach();
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

    /// Whether a shortcut should leave the keys alone: the image viewer or
    /// the emoji picker is open, and they're its keys.
    fn shortcuts_blocked(&self, cx: &App) -> bool {
        self.viewer.is_some() || self.composer.read(cx).emoji_picker_open()
    }

    /// ⌥↑/⌥↓ (and with ⇧, `unread_only`): open the channel `step` places
    /// away in the list as shown (folded channels are skipped), wrapping
    /// round at the ends. Home's Friends row counts as a channel.
    fn move_channel(&mut self, step: isize, unread_only: bool, cx: &mut Context<Self>) {
        if self.shortcuts_blocked(cx) {
            cx.propagate();
            return;
        }
        let mut ids = Vec::new();
        let mut unread = HashSet::new();
        if self.place == Place::Home && self.has_ready {
            ids.push(FRIENDS.to_string());
        }
        for channel in self.visible_sections().into_iter().flat_map(|s| s.channels) {
            if channel.unread || channel.mentions > 0 {
                unread.insert(channel.id.clone());
            }
            ids.push(channel.id);
        }
        let current = if self.friends_open() {
            Some(FRIENDS.to_string())
        } else {
            self.current_channel().map(|c| c.id)
        };
        let next = crate::nav::neighbour(&ids, current.as_ref(), step, |id| {
            !unread_only || unread.contains(id)
        });
        if let Some(id) = next.cloned() {
            self.select_channel(id, cx);
        }
    }

    /// ⌘⌥↑/⌘⌥↓: open the place `step` away in the rail (Home, then the
    /// servers), wrapping round at the ends.
    fn move_server(&mut self, step: isize, cx: &mut Context<Self>) {
        if self.shortcuts_blocked(cx) {
            cx.propagate();
            return;
        }
        let places: Vec<Place> = std::iter::once(Place::Home)
            .chain(
                self.store
                    .servers()
                    .into_iter()
                    .map(|s| Place::Server(s.id)),
            )
            .collect();
        if let Some(place) = crate::nav::neighbour(&places, Some(&self.place), step, |_| true) {
            self.select_place(place.clone(), cx);
        }
    }

    /// Esc: show the open channel's newest message and mark it read now.
    /// In the message box, only once Esc has nothing to cancel there.
    fn go_to_present(&mut self, cx: &mut Context<Self>) {
        let channel = self.current_channel().map(|c| c.id);
        let Some(channel) = channel.filter(|_| !self.shortcuts_blocked(cx)) else {
            cx.propagate();
            return;
        };
        if self.store.is_at_present(&channel) {
            if let Some(pane) = self.panes.get(&channel) {
                pane.scroller
                    .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
            }
        } else {
            self.jump_to_present(channel.clone(), cx);
        }
        self.mark_read_now(&channel, cx);
    }

    /// Mark `channel` read straight away (no [`ACK_DELAY`]), even if the
    /// user marked it unread.
    fn mark_read_now(&mut self, channel: &str, cx: &mut Context<Self>) {
        if self.held_unread.as_deref() == Some(channel) {
            self.held_unread = None;
        }
        let Some(latest) = self.store.unacked_latest(channel) else {
            return;
        };
        self.pending_ack = None;
        self.store.apply_ack(channel, &latest);
        self.post_ack(channel, &latest, "Esc", cx);
        cx.notify();
    }

    /// ⇧Esc: mark the open server read. Nothing on Home.
    fn on_mark_server_read(&mut self, _: &MarkServerRead, _: &mut Window, cx: &mut Context<Self>) {
        match &self.place {
            Place::Server(id) => self.mark_server_read(id.clone(), cx),
            Place::Home => cx.propagate(),
        }
    }

    fn retry_older(&mut self, channel: String, cx: &mut Context<Self>) {
        self.older_errors.remove(&channel);
        self.load_older(channel, cx);
    }

    fn sections(&self) -> Vec<ChannelSection> {
        match &self.place {
            Place::Home => vec![ChannelSection {
                title: None,
                id: None,
                channels: self.store.home_channels(),
            }],
            Place::Server(id) => self.store.server_channels(id),
        }
    }

    /// [`Self::sections`] as the channel list shows them, with folded
    /// categories cut down (see [`fold_sections`]).
    fn visible_sections(&self) -> Vec<ChannelSection> {
        let current = self.current_channel().map(|c| c.id);
        fold_sections(self.sections(), &self.collapsed, current.as_deref())
    }

    /// Fold category `id` away, or unfold it.
    fn toggle_category(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
        self.remember_places();
        cx.notify();
    }

    /// Show spoiler `index` of `message` (in `channel`) for the rest of the
    /// session.
    fn reveal_spoiler(
        &mut self,
        channel: Option<&str>,
        message: &str,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        self.store.reveal_spoiler(message, index);
        if let Some(channel) = channel {
            self.sync_pane(channel, cx);
        }
        cx.notify();
    }

    /// Whether the open channel shows a relative timestamp, which needs
    /// drawing again as time passes.
    fn open_channel_has_relative_times(&self) -> bool {
        let Some(channel) = self.current_channel() else {
            return false;
        };
        self.panes.get(&channel.id).is_some_and(|pane| {
            pane.rows.iter().any(|row| match row {
                TimelineRow::Message(message) => {
                    message.body.as_deref().is_some_and(has_relative_time)
                }
                _ => false,
            })
        })
    }

    /// The picked channel for this place, or else its first channel.
    /// `None` on the Friends page, which isn't a channel.
    fn current_channel(&self) -> Option<ChannelEntry> {
        if self.friends_open() {
            return None;
        }
        if let Some(id) = self.selected.get(&self.place)
            && let Some(entry) = self.store.channel_entry(id)
        {
            return Some(entry);
        }
        self.sections()
            .into_iter()
            .flat_map(|section| section.channels)
            .next()
    }

    /// Whether Home is open on its Friends page.
    fn friends_open(&self) -> bool {
        self.place == Place::Home
            && self
                .selected
                .get(&Place::Home)
                .is_some_and(|id| id == FRIENDS)
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
            Vec::new(),
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
                let mark_read: MenuAction = Rc::new(move |_, _, cx| {
                    let server_id = server_id.clone();
                    view.update(cx, |this, cx| this.mark_server_read(server_id, cx))
                        .ok();
                });
                let copy_id: MenuAction = {
                    let server_id = server.id.clone();
                    Rc::new(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(server_id.clone()))
                    })
                };
                rail_button(
                    SharedString::from(format!("rail-{}", server.id)),
                    icon,
                    server.name,
                    selected,
                    server.unread,
                    server.mentions,
                    vec![("Mark as read", mark_read), ("Copy server ID", copy_id)],
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
        // Folded channels still count: a fully folded server isn't empty.
        let empty = self
            .sections()
            .iter()
            .all(|section| section.channels.is_empty());
        let theme = cx.theme();
        let (muted, hover_fg) = (theme.muted_foreground, theme.sidebar_foreground);

        let rows: Vec<_> = self
            .visible_sections()
            .into_iter()
            .map(|section| {
                let header = section.title.map(|title| {
                    let id = section.id.clone().unwrap_or_default();
                    let folded = self.collapsed.contains(&id);
                    let chevron = if folded {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    };
                    let toggle = {
                        let id = id.clone();
                        cx.listener(move |this, _, _, cx| this.toggle_category(id.clone(), cx))
                    };
                    h_flex()
                        .id(SharedString::from(format!("category-{id}")))
                        .gap_1()
                        .px_1()
                        .pt_3()
                        .pb_1()
                        .cursor_pointer()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted)
                        .hover(move |style| style.text_color(hover_fg))
                        .on_click(toggle)
                        .child(Icon::new(chevron).xsmall())
                        .child(div().truncate().child(title.to_uppercase()))
                        .context_menu(move |menu, _, _| {
                            menu.item(copy_id_item("Copy category ID", id.clone()))
                        })
                });
                v_flex()
                    .gap_0p5()
                    .children(header)
                    .children(section.channels.into_iter().map(|channel| {
                        let selected = current.as_deref() == Some(channel.id.as_str());
                        self.render_channel_row(channel, selected, cx)
                    }))
            })
            .collect();

        let friends =
            (self.place == Place::Home && self.has_ready).then(|| self.render_friends_row(cx));

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
                    .children(friends)
                    .when(self.has_ready && empty && self.place != Place::Home, |el| {
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

    /// Home's Friends row, with a badge counting friend requests.
    fn render_friends_row(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let selected = self.friends_open();
        let (hover_bg, hover_fg) = (theme.sidebar_accent, theme.sidebar_accent_foreground);
        let incoming = crate::friends::incoming_count(&self.store);
        h_flex()
            .id("channel-friends")
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
            .on_click(cx.listener(|this, _, _, cx| this.select_channel(FRIENDS.into(), cx)))
            .child(Icon::new(IconName::Users).small())
            .child(div().flex_1().min_w_0().truncate().child("Friends"))
            .when(incoming > 0, |el| el.child(mention_badge(incoming, cx)))
    }

    fn render_channel_row(
        &self,
        channel: ChannelEntry,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .context_menu(move |menu, _, _| {
                menu.item(copy_id_item("Copy channel ID", channel.id.clone()))
            })
            .into_any_element()
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

    fn render_main(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.friends_open() && self.has_ready {
            return self.render_friends(cx);
        }
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

        // Enter searches this channel; Esc forgets the search.
        let search_box = div()
            .w(px(SEARCH_BOX_WIDTH))
            .flex_none()
            .on_action(cx.listener(Self::on_search_escape))
            .child(
                Input::new(&self.search_input)
                    .small()
                    .cleanable(true)
                    .prefix(
                        Icon::new(IconName::Search)
                            .small()
                            .text_color(theme.muted_foreground),
                    ),
            );
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
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(channel.name.clone()),
                    ),
                None => el.child(div().text_color(theme.muted_foreground).child(
                    if self.has_ready {
                        "No channel selected"
                    } else {
                        "Connecting to Stoat…"
                    },
                )),
            })
            // Right side: search in every channel, but DMs and Saved Notes
            // have no member list to toggle.
            .when(channel.is_some(), |el| el.child(search_box))
            // Pins are in every channel.
            .when(channel.is_some(), |el| {
                let open = matches!(
                    (&self.side_panel, &channel),
                    (SidePanel::Pins(panel), Some(c)) if panel.channel == c.id
                );
                el.child(
                    Button::new("toggle-pins")
                        .ghost()
                        .icon(IconName::Pin)
                        .selected(open)
                        .tooltip("Pinned messages")
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_pins(cx))),
                )
            })
            .when(
                channel
                    .as_ref()
                    .is_some_and(|c| self.store.has_member_list(&c.id)),
                |el| {
                    let open = self.member_list_open;
                    el.child(
                        Button::new("toggle-members")
                            .ghost()
                            .icon(IconName::Users)
                            .selected(open)
                            .tooltip(if open {
                                "Hide member list"
                            } else {
                                "Show member list"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_member_list(cx))),
                    )
                },
            );

        let side_panel = channel.as_ref().and_then(|c| self.render_side_panel(c, cx));
        let colors = row_colors(cx);
        let theme = cx.theme();
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
                            can_link: self.web_app.is_some(),
                        });
                        let view = cx.entity().downgrade();
                        let channel_id = channel.id.clone();
                        let on_action: OnRowAction = Rc::new(move |action, window, cx| {
                            view.update(cx, |this, cx| {
                                this.on_row_action(&channel_id, action, window, cx)
                            })
                            .ok();
                        });
                        let people = self.people(Some(channel.id.clone()), cx);
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
                        // Away from the present, "scroll to the end" would only
                        // reach the end of the window: the pill below takes
                        // over from the scroller's own button.
                        let away = !self.store.is_at_present(&channel.id);
                        let newer = if away {
                            let (id, retry_id) = (channel.id.clone(), channel.id.clone());
                            let error = self.newer_errors.get(&channel.id).map(|error| {
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
                                        Button::new("retry-newer")
                                            .xsmall()
                                            .label("Retry")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.retry_newer(retry_id.clone(), cx)
                                            })),
                                    )
                            });
                            let reloading = self.loading.contains_key(&channel.id);
                            Some(
                                v_flex()
                                    .items_center()
                                    .gap_1()
                                    .children(error)
                                    .child(
                                        Button::new("jump-to-present")
                                            .secondary()
                                            .small()
                                            .rounded(theme.radius_full())
                                            .icon(IconName::ArrowDown)
                                            .label(if reloading {
                                                "Loading…"
                                            } else {
                                                "Jump to present"
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.jump_to_present(id.clone(), cx)
                                            })),
                                    )
                                    .into_any_element(),
                            )
                        } else {
                            None
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
                                        render_row(&rows[ix], colors, &on_action, &people, &pickers)
                                    },
                                )
                                // The scroller's default rows are spaced for whole
                                // chat turns (`pb_8` under every row, plus `px_3`).
                                // Ours are single messages, which space themselves.
                                .with_row_style(StyleRefinement::default().pb(px(0.)).px(px(0.)))
                                .jump_button(!away)
                                .flex_1(),
                            )
                            .when_some(newer, |el, pill| {
                                el.child(
                                    div()
                                        .absolute()
                                        .bottom_2()
                                        .left_0()
                                        .right_0()
                                        .flex()
                                        .justify_center()
                                        .child(pill),
                                )
                            })
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

        let (warning, warning_foreground) = (theme.warning, theme.warning_foreground);
        let mut conversation = v_flex().flex_1().min_w_0().h_full().child(body);
        if let Some(channel) = &channel {
            conversation = conversation
                .child(self.render_typing_line(&channel.id, cx))
                .child(self.composer.clone());
        }

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
                        .bg(warning)
                        .text_color(warning_foreground)
                        .child(banner),
                )
            })
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(conversation)
                    .when_some(side_panel, |el, panel| el.child(panel)),
            )
            .when(channel.is_some(), |el| el.child(render_drop_overlay(cx)))
            .into_any_element()
    }

    /// The Friends page, in place of a channel.
    fn render_friends(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = MemberColors {
            foreground: theme.foreground,
            muted: theme.muted_foreground,
            avatar_bg: theme.secondary,
            avatar_fg: theme.secondary_foreground,
            ring: theme.background,
            hover: theme.secondary,
            online: theme.success,
            idle: theme.warning,
            focus: theme.info,
            busy: theme.danger,
        };
        let lists = crate::friends::friend_lists(&self.store);
        let view = cx.entity().downgrade();
        let on_add_friend = move |window: &mut Window, cx: &mut App| {
            view.update(cx, |this, cx| this.open_add_friend(window, cx))
                .ok();
        };
        let buttons = |user: &str| RowButtons {
            actions: crate::friends::row_actions(&self.store, user),
            pending: self.relationship_pending.contains(user),
            opening: self.dm_opening.as_deref() == Some(user),
        };
        let view = cx.entity().downgrade();
        let on_tab = move |tab, _: &mut Window, cx: &mut App| {
            view.update(cx, |this, cx| {
                this.friends_tab = tab;
                cx.notify();
            })
            .ok();
        };
        render_friends_page(
            &lists,
            self.friends_tab,
            on_add_friend,
            colors,
            buttons,
            on_tab,
            self.people(None, cx),
            cx,
        )
    }

    /// Build the member list the panel shows for the open channel, unless
    /// it's already built.
    fn refresh_member_list(&mut self) {
        if !self.has_ready || !self.member_list_open {
            return;
        }
        let Some(channel) = self.current_channel() else {
            return;
        };
        if self.store.has_member_list(&channel.id) && !self.member_lists.contains_key(&channel.id) {
            let list = Rc::new(self.store.member_list(&channel.id));
            self.member_lists.insert(channel.id, list);
        }
    }

    /// The right-hand panel for `channel`: its search or pins, else its
    /// people if the member list is open. `None` when there's nothing to show.
    fn render_side_panel(
        &self,
        channel: &ChannelEntry,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match &self.side_panel {
            SidePanel::Search(panel) if panel.channel == channel.id => {
                Some(self.render_search_panel(panel, channel, cx))
            }
            SidePanel::Pins(panel) if panel.channel == channel.id => {
                Some(self.render_pins_panel(panel, channel, cx))
            }
            _ => (self.member_list_open && self.store.has_member_list(&channel.id))
                .then(|| self.render_member_panel(channel, cx)),
        }
    }

    /// The search panel: the order to show matches in, and the matches as
    /// compact rows that jump to the message when clicked.
    fn render_search_panel(
        &self,
        panel: &SearchPanel,
        channel: &ChannelEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = row_colors(cx);
        let theme = cx.theme();
        let (foreground, border, background) = (
            theme.sidebar_foreground,
            theme.sidebar_border,
            theme.sidebar,
        );
        let note = |text: SharedString, colour: Hsla| {
            div()
                .px_3()
                .py_1()
                .text_sm()
                .text_color(colour)
                .child(text)
                .into_any_element()
        };
        let sorts = [
            ("search-sort-latest", "Latest", MessageSort::Latest),
            ("search-sort-oldest", "Oldest", MessageSort::Oldest),
            ("search-sort-relevance", "Relevance", MessageSort::Relevance),
        ];
        let sort_buttons = h_flex()
            .gap_1()
            .px_2()
            .pb_1()
            .children(sorts.into_iter().map(|(id, label, sort)| {
                let selected = panel.sort == sort;
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .label(label)
                    .selected(selected)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.set_search_sort(sort.clone(), cx)),
                    )
            }));
        let body = match &panel.state {
            SearchState::Loading => note("Searching…".into(), colors.muted),
            SearchState::Empty => note(
                "No messages match. Search matches whole words.".into(),
                colors.muted,
            ),
            SearchState::TooLong(chars) => note(
                format!("Searches can be up to {MAX_QUERY_CHARS} characters. This one is {chars}.")
                    .into(),
                colors.danger,
            ),
            SearchState::Error(error) => note(error.clone(), colors.danger),
            SearchState::Results(rows) => {
                let count = if rows.len() as i64 >= RESULT_LIMIT {
                    format!("The first {} matches", rows.len())
                } else if rows.len() == 1 {
                    "1 match".to_string()
                } else {
                    format!("{} matches", rows.len())
                };
                self.render_result_list("search-results", rows, count, channel, cx)
            }
        };
        v_flex()
            .id("search-panel")
            .w(px(MEMBER_PANEL_WIDTH))
            .h_full()
            .flex_none()
            .bg(background)
            .text_color(foreground)
            .border_l_1()
            .border_color(border)
            .child(
                h_flex()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .flex_none()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.muted)
                            .truncate()
                            .child(format!("SEARCH RESULTS FOR “{}”", panel.query.trim())),
                    )
                    .child(
                        Button::new("search-close")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Close search (Esc)")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_search(window, cx)),
                            ),
                    ),
            )
            .child(sort_buttons)
            .child(body)
            .into_any_element()
    }

    /// A count and `rows` as compact messages, for the search and pins
    /// panels. Clicking one asks to jump to it; nothing else on them is live
    /// (they're not kept up to date message by message).
    fn render_result_list(
        &self,
        id: &'static str,
        rows: &Rc<Vec<MessageRow>>,
        count: String,
        channel: &ChannelEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = row_colors(cx);
        let view = cx.entity().downgrade();
        let channel_id = channel.id.clone();
        let on_action: OnRowAction = Rc::new(move |action, window, cx| {
            view.update(cx, |this, cx| {
                this.on_row_action(&channel_id, action, window, cx)
            })
            .ok();
        });
        let pickers = Pickers {
            choices: Rc::default(),
            open_for: None,
            can_link: false,
        };
        let people = self.people(Some(channel.id.clone()), cx);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .px_3()
                    .pb_1()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted)
                    .child(count),
            )
            .child(
                v_flex()
                    .id(id)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb_2()
                    .children(
                        rows.iter().map(|row| {
                            render_preview_row(row, colors, &on_action, &people, &pickers)
                        }),
                    ),
            )
            .into_any_element()
    }

    /// The pins panel: the channel's pinned messages, newest first, as
    /// compact rows that jump to the message when clicked.
    fn render_pins_panel(
        &self,
        panel: &PinsPanel,
        channel: &ChannelEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = row_colors(cx);
        let theme = cx.theme();
        let (foreground, border, background) = (
            theme.sidebar_foreground,
            theme.sidebar_border,
            theme.sidebar,
        );
        let note = |text: SharedString, colour: Hsla| {
            div()
                .px_3()
                .py_1()
                .text_sm()
                .text_color(colour)
                .child(text)
                .into_any_element()
        };
        let body = match &panel.state {
            PinsState::Loading => note("Loading pinned messages…".into(), colors.muted),
            PinsState::Empty => note(
                "No messages are pinned in this channel.".into(),
                colors.muted,
            ),
            PinsState::Error(error) => v_flex()
                .child(note(error.clone(), colors.danger))
                .child(
                    div()
                        .px_3()
                        .child(Button::new("retry-pins").xsmall().label("Retry").on_click(
                            cx.listener(|this, _, _, cx| {
                                if let Some(channel) = this.current_channel() {
                                    this.start_pins(channel.id, false, cx);
                                }
                            }),
                        )),
                )
                .into_any_element(),
            PinsState::Results(rows) => {
                let count = if rows.len() as i64 >= PINS_LIMIT {
                    format!("The newest {} pinned messages", rows.len())
                } else if rows.len() == 1 {
                    "1 pinned message".to_string()
                } else {
                    format!("{} pinned messages", rows.len())
                };
                self.render_result_list("pinned-messages", rows, count, channel, cx)
            }
        };
        v_flex()
            .id("pins-panel")
            .w(px(MEMBER_PANEL_WIDTH))
            .h_full()
            .flex_none()
            .bg(background)
            .text_color(foreground)
            .border_l_1()
            .border_color(border)
            .child(
                h_flex()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .flex_none()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.muted)
                            .child("PINNED MESSAGES"),
                    )
                    .child(
                        Button::new("pins-close")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Close pinned messages")
                            .on_click(cx.listener(|this, _, _, cx| this.close_pins(cx))),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    /// The right-hand panel: `channel`'s people (see [`Self::refresh_member_list`]).
    fn render_member_panel(&self, channel: &ChannelEntry, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let list: Rc<MemberList> = self
            .member_lists
            .get(&channel.id)
            .cloned()
            .unwrap_or_default();
        let server = self.store.channel_server(&channel.id);
        let loading = server
            .is_some_and(|s| self.members_loading.contains(s) || self.online_loading.contains(s));
        let failed = server.is_some_and(|s| self.members_failed.contains(s));
        let colors = MemberColors {
            foreground: theme.sidebar_foreground,
            muted: theme.muted_foreground,
            avatar_bg: theme.secondary,
            avatar_fg: theme.secondary_foreground,
            ring: theme.sidebar,
            hover: theme.sidebar_accent,
            online: theme.success,
            idle: theme.warning,
            focus: theme.info,
            busy: theme.danger,
        };
        let note = |text: &'static str, colour: Hsla| {
            div()
                .px_3()
                .pb_1()
                .text_xs()
                .text_color(colour)
                .child(text)
                .into_any_element()
        };
        let title: SharedString = if list.title.is_empty() {
            "Members".into()
        } else {
            list.title.clone().into()
        };
        let rows = list.clone();
        let people = self.people(Some(channel.id.clone()), cx);
        v_flex()
            .id("member-panel")
            .w(px(MEMBER_PANEL_WIDTH))
            .h_full()
            .flex_none()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .border_l_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .flex_none()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.muted)
                    .child(title),
            )
            .when(loading, |el| {
                el.child(note("Loading members…", colors.muted))
            })
            .when(failed, |el| {
                el.child(note(
                    "Couldn't load all members. Reopen the channel to try again.",
                    theme.danger,
                ))
            })
            .when(list.offline_hidden, |el| {
                el.child(note(
                    "Offline members aren't listed in large servers.",
                    colors.muted,
                ))
            })
            .when(list.rows.is_empty() && !loading, |el| {
                el.child(note("No members to show.", colors.muted))
            })
            .child(
                div().flex_1().min_h_0().child(
                    uniform_list(
                        SharedString::from(format!("members-{}", channel.id)),
                        list.rows.len(),
                        move |range, _, _| {
                            range
                                .map(|ix| render_member_row(&rows.rows[ix], colors, &people))
                                .collect::<Vec<_>>()
                        },
                    )
                    .size_full(),
                ),
            )
            .into_any_element()
    }
}

/// Width of the add-friend dialog.
const ADD_FRIEND_DIALOG_WIDTH: f32 = 420.;

/// Width of the right-hand panel.
const MEMBER_PANEL_WIDTH: f32 = 240.;

/// Width of the header's search box.
const SEARCH_BOX_WIDTH: f32 = 220.;

/// Height of every row of the member list (section titles too, which is what
/// lets it be a `uniform_list`).
const MEMBER_ROW_HEIGHT: f32 = 42.;

/// The colours a member row is drawn with.
#[derive(Clone, Copy)]
pub(crate) struct MemberColors {
    pub(crate) foreground: Hsla,
    pub(crate) muted: Hsla,
    pub(crate) avatar_bg: Hsla,
    pub(crate) avatar_fg: Hsla,
    /// Behind the presence dot, so it reads against the avatar.
    pub(crate) ring: Hsla,
    /// Behind a row the mouse is over.
    pub(crate) hover: Hsla,
    pub(crate) online: Hsla,
    pub(crate) idle: Hsla,
    pub(crate) focus: Hsla,
    pub(crate) busy: Hsla,
}

impl MemberColors {
    pub(crate) fn presence(&self, presence: PresenceKind) -> Hsla {
        match presence {
            PresenceKind::Online => self.online,
            PresenceKind::Idle => self.idle,
            PresenceKind::Focus => self.focus,
            PresenceKind::Busy => self.busy,
            PresenceKind::Offline => self.muted,
        }
    }
}

/// `#rrggbb` as a colour.
pub(crate) fn hex_colour(css: &str) -> Option<Hsla> {
    let digits = css.strip_prefix('#')?;
    (digits.len() == 6)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
        .map(|value| Hsla::from(rgb(value)))
}

fn render_member_row(row: &MemberListRow, colors: MemberColors, people: &People) -> AnyElement {
    match row {
        MemberListRow::Header { name, count } => h_flex()
            .h(px(MEMBER_ROW_HEIGHT))
            .px_3()
            .pb_1()
            .items_end()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(colors.muted)
            .child(format!("{} — {count}", name.to_uppercase()))
            .into_any_element(),
        MemberListRow::Member {
            user_id,
            name,
            avatar_url,
            colour,
            presence,
            status,
            online,
        } => {
            let name_colour = colour
                .as_deref()
                .and_then(hex_colour)
                .unwrap_or(colors.foreground);
            let (open, user) = (people.clone(), user_id.clone());
            h_flex()
                .id(SharedString::from(format!("member-{user_id}")))
                .h(px(MEMBER_ROW_HEIGHT))
                .px_3()
                .gap_2()
                .items_center()
                .rounded_md()
                .cursor_pointer()
                .hover(|el| el.bg(colors.hover))
                .on_click(move |_, window, cx| open.open_card(user.clone(), window, cx))
                .when(!online, |el| el.opacity(0.5))
                .child(
                    div()
                        .relative()
                        .size(px(AVATAR_SIZE))
                        .flex_none()
                        .child(avatar(
                            format!("member-avatar-{user_id}"),
                            Some(avatar_url.clone()),
                            name,
                            AVATAR_SIZE,
                            colors.avatar_bg,
                            colors.avatar_fg,
                        ))
                        .child(
                            div()
                                .absolute()
                                .right(px(-2.))
                                .bottom(px(-2.))
                                .size(px(12.))
                                .rounded_full()
                                .border_2()
                                .border_color(colors.ring)
                                .bg(colors.presence(*presence)),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .justify_center()
                        .child(
                            div()
                                .text_sm()
                                .truncate()
                                .text_color(name_colour)
                                .child(name.clone()),
                        )
                        .when(!status.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(colors.muted)
                                    .child(status.clone()),
                            )
                        }),
                )
                .context_menu(people.menu(user_id, true))
                .into_any_element()
        }
    }
}

/// The list's row for message `id`, if it's among `rows`.
fn row_index(rows: &[TimelineRow], id: &str) -> Option<usize> {
    rows.iter()
        .position(|row| matches!(row, TimelineRow::Message(m) if m.id == id))
}

/// Whether a fetched page holds message `id`.
fn page_has_message(page: &crate::api::models::BulkMessageResponse, id: &str) -> bool {
    use crate::api::models::BulkMessageResponse;
    let (BulkMessageResponse::JustMessages(messages)
    | BulkMessageResponse::MessagesAndUsers { messages, .. }) = page;
    messages.iter().any(|m| m.id == id)
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

/// The colours message rows are drawn with, from the theme.
fn row_colors(cx: &App) -> RowColors {
    let theme = cx.theme();
    RowColors {
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
        highlight: theme.accent.opacity(0.5),
    }
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
    /// Behind the message a jump just landed on.
    highlight: Hsla,
}

/// What the "External links can be dangerous!" dialog asks about.
#[derive(Clone)]
struct LinkPrompt {
    url: String,
    /// What the link's text said, so how hard to warn.
    check: LinkCheck,
    /// What "Don't ask me again" would trust. `None`: it isn't offered.
    origin: Option<String>,
    /// "Don't ask me again" is ticked.
    trust: bool,
    /// "I understand the risk" is ticked (only asked for a
    /// [`LinkCheck::Mismatch`]).
    understood: bool,
}

impl LinkPrompt {
    /// Whether Continue may open the link.
    fn ready(&self) -> bool {
        self.understood || !matches!(self.check, LinkCheck::Mismatch(_))
    }
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
    /// Scroll to a message (a reply's original), loading the messages around
    /// it first if it isn't loaded.
    JumpTo(String),
    /// Show a run of blocked users' messages (by ID).
    RevealBlocked(Vec<String>),
    /// Open an external link from an embed, asking first unless its site
    /// is trusted.
    OpenLink(String),
    /// Reply to a message (from the hover toolbar or the right-click menu).
    Reply(ReplyTarget),
    /// Quote a message (by ID) in the message box.
    Quote(String),
    /// Copy a message's text as typed (its Markdown source).
    CopyText(String),
    /// Copy a link to a message in the instance's web client.
    CopyLink(String),
    /// Copy a message's ID.
    CopyId(String),
    /// Make the channel unread from this message on.
    MarkUnread(String),
    /// Edit one of the user's messages in the message box.
    Edit(String),
    /// Delete one of the user's messages, after asking (unless Shift was
    /// held: `confirmed`).
    Delete { id: String, confirmed: bool },
    /// Pin (`pin`) or unpin a message, after asking (unless Shift was held:
    /// `confirmed`).
    TogglePin {
        id: String,
        pin: bool,
        confirmed: bool,
    },
}

type OnRowAction = Rc<dyn Fn(RowAction, &mut Window, &mut App)>;

/// The open profile card.
struct OpenCard {
    user: String,
    /// The channel it was opened from (which decides the server, for
    /// roles and nickname). It closes when another channel opens.
    channel: Option<String>,
    /// Where it was opened, in window coordinates: its top-left corner.
    position: Point<Pixels>,
    /// A mouse button went down outside it: it closes when the button comes
    /// back up. (Closing on the press would also close the "…" menu, which
    /// is outside the card, before a click on one of its items lands.)
    closing: bool,
}

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
pub(crate) fn avatar(
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
    /// Whether the message menu offers "Copy message link": only when the
    /// instance says where its web client is.
    can_link: bool,
}

fn render_row(
    row: &TimelineRow,
    colors: RowColors,
    on_action: &OnRowAction,
    people: &People,
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
        TimelineRow::NewMessages => h_flex()
            .px_4()
            .pt_3()
            .gap_2()
            .items_center()
            .child(div().flex_1().h(px(1.)).bg(colors.danger))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.danger)
                    .child("New"),
            )
            .into_any_element(),
        TimelineRow::Message(message) if message.system => {
            render_system_row(message, colors, people)
        }
        TimelineRow::Message(message) => {
            render_message_row(message, colors, on_action, people, pickers, false)
        }
        TimelineRow::Blocked { ids } => render_blocked_row(ids, colors, on_action),
    }
}

/// "N blocked messages": a run of messages from blocked users, which shows
/// them when clicked.
fn render_blocked_row(ids: &[String], colors: RowColors, on_action: &OnRowAction) -> AnyElement {
    let label = match ids.len() {
        1 => "1 blocked message".to_string(),
        n => format!("{n} blocked messages"),
    };
    let (on_action, ids) = (on_action.clone(), ids.to_vec());
    h_flex()
        .w_full()
        .justify_center()
        .px_4()
        .py_1p5()
        .child(
            h_flex()
                .id(SharedString::from(format!("blocked-{}", ids[0])))
                .gap_1p5()
                .items_center()
                .cursor_pointer()
                .text_xs()
                .text_color(colors.system)
                .hover(|this| this.underline())
                .child(Icon::new(IconName::UserX).xsmall())
                .child(label)
                .on_click(move |_, window, cx| {
                    on_action(RowAction::RevealBlocked(ids.clone()), window, cx)
                }),
        )
        .into_any_element()
}

/// A search result: a compact message row that jumps to the message when
/// clicked (see [`render_message_row`]).
fn render_preview_row(
    row: &MessageRow,
    colors: RowColors,
    on_action: &OnRowAction,
    people: &People,
    pickers: &Pickers,
) -> AnyElement {
    if row.system {
        render_system_row(row, colors, people)
    } else {
        render_message_row(row, colors, on_action, people, pickers, true)
    }
}

fn render_system_row(message: &MessageRow, colors: RowColors, people: &People) -> AnyElement {
    let on_link = people.link_handler();
    // Centred between the two sides of the conversation. The people in it
    // are links that open their cards.
    h_flex()
        .w_full()
        .justify_center()
        .px_4()
        .py_1p5()
        .gap_2()
        .text_xs()
        .text_color(colors.system)
        .child(
            div().italic().child(
                TextView::markdown(
                    SharedString::from(message.render_key.clone()),
                    message.body.clone().unwrap_or_default(),
                )
                .plugin(MarkupPlugin::default())
                .code_block_actions(code_block_actions)
                .text_xs()
                .on_link_click(move |url, _, window, cx| on_link(url, window, cx)),
            ),
        )
        .child(div().child(message.time.clone()))
        .into_any_element()
}

/// `child` (an author's name or avatar), opening `user`'s profile card when
/// clicked, and only that: the row's own click (a jump) doesn't happen too.
/// Right-clicking it opens the person's menu instead of the row's. Without a
/// person (a webhook or system message), just `child`.
fn person_target(
    id: SharedString,
    user: Option<&str>,
    people: &People,
    child: impl IntoElement,
) -> AnyElement {
    let Some(user) = user else {
        return div().flex_none().child(child).into_any_element();
    };
    let (open, target) = (people.clone(), user.to_string());
    div()
        .id(id)
        .flex_none()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            open.open_card(target.clone(), window, cx)
        })
        .child(child)
        .context_menu(people.menu(user, true))
        .into_any_element()
}

/// What the people shown in a channel's views (names, avatars, mentions)
/// do: open their profile card, or their menu of things to do with them.
#[derive(Clone)]
pub(crate) struct People {
    view: WeakEntity<ChatView>,
    /// The channel they're shown in (where Mention mentions them).
    channel: Option<String>,
}

impl People {
    /// What clicking a hidden spoiler in `message` does: reveal it.
    fn spoiler_revealer(&self, message: &str) -> RevealSpoiler {
        let (view, channel, message) =
            (self.view.clone(), self.channel.clone(), message.to_string());
        Arc::new(move |index, _, cx| {
            view.update(cx, |this, cx| {
                this.reveal_spoiler(channel.as_deref(), &message, index, cx)
            })
            .ok();
        })
    }

    /// Open `user`'s card where the mouse is.
    pub(crate) fn open_card(&self, user: String, window: &mut Window, cx: &mut App) {
        let position = window.mouse_position();
        self.view
            .update(cx, |this, cx| this.open_card(user, position, window, cx))
            .ok();
    }

    /// A Friends page row's button: message `user`, or change how the user
    /// relates to them (`shift` skips the confirmation).
    pub(crate) fn act(
        &self,
        user: String,
        action: crate::friends::RowAction,
        shift: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.view
            .update(cx, |this, cx| match action {
                crate::friends::RowAction::Message => this.message_user(user, window, cx),
                crate::friends::RowAction::Relationship(action) => {
                    this.set_relationship(user, action, shift, window, cx)
                }
            })
            .ok();
    }

    /// What clicking a link in Markdown does: a person (a mention, or a
    /// name in a system message) opens their card, and web links open in
    /// the browser.
    fn link_handler(&self) -> OnLink {
        let people = self.clone();
        Arc::new(move |url, window, cx| match parse_internal_link(url) {
            Some(InternalLink::User(user)) => people.open_card(user, window, cx),
            // Its text wasn't its address: the warning says what it was.
            Some(InternalLink::External { url, text }) => {
                people.open_link(url, Some(text), window, cx)
            }
            // Channel links don't go anywhere yet; `ui::markup` draws the
            // rest, so they aren't links.
            Some(
                InternalLink::Channel(_)
                | InternalLink::Role { .. }
                | InternalLink::Everyone
                | InternalLink::Online
                | InternalLink::Time { .. }
                | InternalLink::Spoiler { .. },
            ) => {}
            None => {
                if is_external_link(url) {
                    people.open_link(url.to_string(), None, window, cx);
                }
            }
        })
    }

    /// Open an external link, asking first (see [`ChatView::open_external`]).
    pub(crate) fn open_link(
        &self,
        url: String,
        text: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.view
            .update(cx, |this, cx| this.open_external(url, text, window, cx))
            .ok();
    }

    /// `user`'s menu (see [`crate::profile::user_menu`]), worked out when it
    /// opens. Without `profile`, there's no Profile item (it's for the card).
    pub(crate) fn menu(
        &self,
        user: &str,
        profile: bool,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let (people, user) = (self.clone(), user.to_string());
        move |menu, _, cx| {
            let Some(view) = people.view.upgrade() else {
                return menu;
            };
            let items =
                crate::profile::user_menu(&view.read(cx).store, &user, people.channel.as_deref());
            items
                .into_iter()
                .filter(|item| profile || *item != UserMenuItem::Profile)
                .fold(menu, |menu, item| {
                    let Some((label, icon)) = user_menu_label(item) else {
                        return menu.separator();
                    };
                    let (view, user, channel) =
                        (people.view.clone(), user.clone(), people.channel.clone());
                    menu.item(PopupMenuItem::new(label).icon(icon).on_click(
                        move |event: &ClickEvent, window, cx| {
                            let shift = event.modifiers().shift;
                            view.update(cx, |this, cx| {
                                this.on_user_menu(
                                    user.clone(),
                                    channel.clone(),
                                    item,
                                    shift,
                                    window,
                                    cx,
                                )
                            })
                            .ok();
                        },
                    ))
                })
        }
    }
}

/// A person's menu item as shown; `None` for a separator.
fn user_menu_label(item: UserMenuItem) -> Option<(&'static str, IconName)> {
    Some(match item {
        UserMenuItem::Profile => ("Profile", IconName::CircleUser),
        UserMenuItem::Message => ("Message", IconName::MessageCircle),
        UserMenuItem::Mention => ("Mention", IconName::AtSign),
        UserMenuItem::CopyUsername => ("Copy username", IconName::Copy),
        UserMenuItem::CopyUserId => ("Copy user ID", IconName::IdCard),
        UserMenuItem::Relationship(action) => relationship_label(action),
        UserMenuItem::Separator => return None,
    })
}

/// A relationship action as a menu item or button.
fn relationship_label(action: RelationshipAction) -> (&'static str, IconName) {
    match action {
        RelationshipAction::AddFriend => ("Add friend", IconName::UserPlus),
        RelationshipAction::AcceptFriend => ("Accept friend request", IconName::UserCheck),
        RelationshipAction::RejectFriend => ("Reject friend request", IconName::UserX),
        RelationshipAction::CancelFriend => ("Cancel friend request", IconName::UserX),
        RelationshipAction::RemoveFriend => ("Remove friend", IconName::UserMinus),
        RelationshipAction::Block => ("Block", IconName::Ban),
        RelationshipAction::Unblock => ("Unblock", IconName::ShieldCheck),
    }
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

/// A message in the timeline. With `preview` (a search result), it's drawn
/// compactly and always on the left, with just its text and attachments: no
/// hover toolbar, reactions, link previews or right-click menu, and clicking
/// it jumps to the message instead.
fn render_message_row(
    message: &MessageRow,
    colors: RowColors,
    on_action: &OnRowAction,
    people: &People,
    pickers: &Pickers,
    preview: bool,
) -> AnyElement {
    let author_id = message.author_id.as_deref();
    let first = !message.continuation;
    let last = message.group_end;
    // In a preview the whole row is the click (to jump), so the pictures in
    // it do nothing of their own.
    let attachment_action: OnRowAction = if preview {
        Rc::new(|_, _, _| {})
    } else {
        on_action.clone()
    };
    // Previews are all on one side, whoever wrote them.
    let right = message.mine && !preview;
    let alignment = if right {
        MessageAlignment::End
    } else {
        MessageAlignment::Start
    };

    // The corners on the sender's side flatten where bubbles from the same
    // person stack, so a group reads as one unit.
    let round = px(BUBBLE_RADIUS);
    let top = px(if first { BUBBLE_RADIUS } else { JOINED_RADIUS });
    let bottom = px(if last { BUBBLE_RADIUS } else { JOINED_RADIUS });
    let surface = if right {
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
                        // Timestamps, and spoilers revealed by a click
                        // (not in search results, where a click jumps).
                        .plugin(if preview {
                            MarkupPlugin::default()
                        } else {
                            MarkupPlugin::revealing(people.spoiler_revealer(&message.id))
                        })
                        // A copy button (and the language) on each code block.
                        .code_block_actions(code_block_actions)
                        // Emoji-only messages are drawn big (custom ones are sized
                        // in the Markdown itself).
                        .map(|text| {
                            if message.jumbo {
                                text.text_size(px(JUMBO_TEXT_PX))
                            } else {
                                text.text_sm()
                            }
                        })
                        // A click on a result jumps; it doesn't select.
                        .selectable(!preview)
                        .on_link_click({
                            let on_link = people.link_handler();
                            move |url, _, window, cx| on_link(url, window, cx)
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
                                render_attachment(id, attachment, colors, &attachment_action)
                            },
                        )),
                )
            })
            .when(!preview && !message.embeds.is_empty(), |el| {
                el.child(
                    v_flex()
                        .gap_1()
                        .when(
                            message.body.is_some() || !message.attachments.is_empty(),
                            |el| el.mt_1(),
                        )
                        .children(message.embeds.iter().enumerate().map(|(ix, embed)| {
                            let id = SharedString::from(format!("embed-{}-{ix}", message.id));
                            render_embed(id, embed, colors, on_action, people)
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
        // Nothing to react or reply to until it's on the server (or in a
        // search result).
        .when(message.pending.is_none() && !preview, |el| {
            el.reactions(message_toolbar(message, on_action, pickers))
        });

    // The avatar sits beside the last bubble of a group; earlier ones keep
    // the empty slot so the bubbles line up.
    let avatar_slot = if last {
        MessageAvatar::new().child(person_target(
            format!("avatar-target-{}", message.id).into(),
            author_id,
            people,
            avatar(
                format!("avatar-{}", message.id),
                message.avatar.clone(),
                &message.author,
                AVATAR_SIZE,
                colors.avatar_bg,
                colors.avatar_fg,
            ),
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
            .child(person_target(
                format!("author-{}", message.id).into(),
                author_id,
                people,
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(author_id.is_some(), |el| el.hover(|el| el.underline()))
                    .child(name),
            ))
            .child(div().text_color(colors.muted).child(message.time.clone()))
    });

    let layout = MessageLayout::new()
        .alignment(alignment)
        .avatar_slot(avatar_slot)
        .when_some(header, |el, header| el.header(header))
        .content(
            MessageContent::new()
                .when(!message.replies.is_empty(), |el| {
                    el.gap(px(REPLY_GAP))
                        .children(message.replies.iter().enumerate().map(|(ix, reply)| {
                            render_reply_preview(message, ix, reply, colors, on_action, people)
                        }))
                })
                .bubble(bubble),
        )
        .when_some(
            render_reactions(message, colors, on_action).filter(|_| !preview),
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
        .when(preview, |el| el.px_2())
        .when(!preview, |el| el.px_4())
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
        .when(message.highlighted && !preview, |el| {
            el.bg(colors.highlight)
        })
        .child(layout);
    if preview {
        let (on_action, id) = (on_action.clone(), message.id.clone());
        return row
            .cursor_pointer()
            .hover(|style| style.bg(colors.highlight))
            .on_click(move |_, window, cx| on_action(RowAction::JumpTo(id.clone()), window, cx))
            .into_any_element();
    }
    if message.pending.is_some() {
        return row.into_any_element();
    }
    // Right-click anywhere on the row (text, padding, avatar).
    let (on_action, id, mine) = (on_action.clone(), message.id.clone(), message.mine);
    let (can_pin, pinned) = (message.can_pin, message.pinned);
    let can_link = pickers.can_link;
    // Only messages with text can be quoted.
    let has_text = message.body.is_some();
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
        .when(has_text, |menu| {
            menu.item(
                PopupMenuItem::new("Quote")
                    .icon(IconName::TextQuote)
                    .on_click(act(RowAction::Quote(id.clone()))),
            )
        })
        .item(
            PopupMenuItem::new("Mark as unread")
                .icon(IconName::Mail)
                .on_click(act(RowAction::MarkUnread(id.clone()))),
        )
        .item(
            PopupMenuItem::new("Copy text")
                .icon(IconName::Copy)
                .on_click(act(RowAction::CopyText(id.clone()))),
        )
        .when(can_link, |menu| {
            menu.item(
                PopupMenuItem::new("Copy message link")
                    .icon(IconName::Link)
                    .on_click(act(RowAction::CopyLink(id.clone()))),
            )
        })
        .item(
            PopupMenuItem::new("Copy message ID")
                .icon(IconName::IdCard)
                .on_click(act(RowAction::CopyId(id.clone()))),
        )
        .when(can_pin, |menu| {
            let toggle = {
                let (on_action, id) = (on_action.clone(), id.clone());
                move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let confirmed = event.modifiers().shift;
                    let action = RowAction::TogglePin {
                        id: id.clone(),
                        pin: !pinned,
                        confirmed,
                    };
                    on_action(action, window, cx)
                }
            };
            let (label, icon) = if pinned {
                ("Unpin message", IconName::PinOff)
            } else {
                ("Pin message", IconName::Pin)
            };
            menu.item(PopupMenuItem::new(label).icon(icon).on_click(toggle))
        })
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

/// Shown when Stoat won't open a DM with someone (they aren't a friend).
const CANNOT_MESSAGE_TEXT: &str = "You can only message friends and bots.";

/// Shown when replying to a sixth message at once.
const TOO_MANY_REPLIES_TEXT: &str = "You can reply to up to 5 messages at once.";

/// Height of a reply preview line, the same whatever its state, so a row
/// doesn't change height when its original loads.
const REPLY_LINE_HEIGHT: f32 = 18.;
/// Space between reply previews and the bubble.
const REPLY_GAP: f32 = 2.;
/// The original author's avatar in a reply preview.
const REPLY_AVATAR_SIZE: f32 = 14.;

/// "↱ Name: snippet" above a reply. Clicking an original that's been fetched
/// scrolls to it, loading the messages around it if it's not in the list.
fn render_reply_preview(
    message: &MessageRow,
    ix: usize,
    reply: &ReplyPreview,
    colors: RowColors,
    on_action: &OnRowAction,
    people: &People,
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
            // The author (avatar and name) opens their card; the rest jumps.
            let author_id = reply.author_id.as_deref();
            line.cursor_pointer()
                .on_click(jump)
                .child(person_target(
                    format!("{id}-author").into(),
                    author_id,
                    people,
                    h_flex()
                        .gap_1()
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
                                .font_weight(FontWeight::SEMIBOLD)
                                .when(author_id.is_some(), |el| el.hover(|el| el.underline()))
                                .child(name),
                        ),
                ))
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
            // Read from disk as it's sent (see `upload_attachment`).
            client
                .upload_attachment(&file_server, &file.name, &file.content_type, &file.path, move |sent, total| {
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
        ApiError::File(err) => format!("couldn't read it ({err})."),
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

/// What the pins panel says when loading them fails.
fn pins_error_text(err: &ApiError) -> SharedString {
    use revolt_result::ErrorType;
    match err {
        ApiError::RateLimited { .. } => "Too many requests. Wait a moment, then try again.".into(),
        _ => match err.error_type() {
            Some(ErrorType::MissingPermission { .. }) => {
                "You can't see this channel's pins.".into()
            }
            Some(ErrorType::IsBot) => "Bots can't list pinned messages.".into(),
            _ => format!("Couldn't load pins: {err}").into(),
        },
    }
}

/// What a refused pin's (`pin`) or unpin's toast says.
fn pin_error_text(pin: bool, err: &ApiError) -> SharedString {
    use revolt_result::ErrorType;
    let (done, doing) = if pin {
        ("pin", "pinning")
    } else {
        ("unpin", "unpinning")
    };
    match err {
        ApiError::RateLimited { .. } => {
            format!("Too many requests while {doing}. Wait a moment, then try again.").into()
        }
        _ => match err.error_type() {
            Some(ErrorType::MissingPermission { .. }) => {
                format!("You can't {done} messages here.").into()
            }
            _ => format!("Couldn't {done} the message: {err}").into(),
        },
    }
}

/// What the search panel says when a search fails.
fn search_error_text(err: &ApiError) -> SharedString {
    use revolt_result::ErrorType;
    match err {
        ApiError::RateLimited { .. } => "Searching too fast. Wait a moment, then try again.".into(),
        _ => match err.error_type() {
            Some(ErrorType::MissingPermission { .. }) => "You can't search this channel.".into(),
            Some(ErrorType::IsBot) => "Bots can't search messages.".into(),
            Some(ErrorType::InvalidOperation) => "Stoat didn't accept that search.".into(),
            _ => format!("Couldn't search: {err}").into(),
        },
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
    people: &People,
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
            let on_action = on_action.clone();
            file_card(id, IconName::FileVideoCamera, shown, "Video", colors)
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    on_action(RowAction::OpenLink(open.clone()), window, cx)
                })
                .child(Icon::new(IconName::ExternalLink).text_color(colors.muted))
                .into_any_element()
        }
        EmbedView::Card(card) => render_embed_card(id, card, colors, on_action, people),
    }
}

/// A link preview or bot embed: colour bar, site, title (linked), description,
/// and a picture beside or under the text.
fn render_embed_card(
    id: SharedString,
    card: &EmbedCard,
    colors: RowColors,
    on_action: &OnRowAction,
    people: &People,
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
            Some(link) => {
                let on_action = on_action.clone();
                text.text_color(colors.link)
                    .cursor_pointer()
                    .hover(|style| style.underline())
                    .on_click(move |_, window, cx| {
                        on_action(RowAction::OpenLink(link.clone()), window, cx)
                    })
            }
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
                    .plugin(MarkupPlugin::default())
                    .code_block_actions(code_block_actions)
                    .text_sm()
                    .selectable(true)
                    .on_link_click({
                        let on_link = people.link_handler();
                        move |url, _, window, cx| on_link(url, window, cx)
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
                .on_click({
                    let on_action = on_action.clone();
                    move |_, window, cx| on_action(RowAction::OpenLink(link.clone()), window, cx)
                })
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

/// A right-click menu item that copies an ID (a channel's, a category's).
fn copy_id_item(label: &'static str, id: String) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .icon(IconName::IdCard)
        .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(id.clone())))
}

/// What a rail button's right-click menu item does.
pub(crate) type MenuAction = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A rail button, with a marker on the left edge when selected (an icon image
/// covers the button's own highlight colour). Also used by the account
/// switcher. `menu` is its right-click menu: each item's label and action.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rail_button(
    id: impl Into<ElementId>,
    content: AnyElement,
    tooltip: impl Into<SharedString>,
    selected: bool,
    unread: bool,
    mentions: usize,
    menu: Vec<(&'static str, MenuAction)>,
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
    // Right-click: e.g. "Mark as read" on servers.
    let button = if menu.is_empty() {
        button.into_any_element()
    } else {
        button
            .context_menu(move |popup, _, _| {
                menu.iter().fold(popup, |popup, (label, action)| {
                    let action = action.clone();
                    popup.item(
                        PopupMenuItem::new(*label)
                            .on_click(move |event, window, cx| action(event, window, cx)),
                    )
                })
            })
            .into_any_element()
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
pub(crate) fn mention_badge(count: usize, cx: &App) -> Div {
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
        self.sync_unread_marker(cx);
        // After layout, check whether the open channel is scrolled to its top
        // (or too short to scroll) and has more history to load.
        if let Some(channel) = self.current_channel().map(|c| c.id) {
            cx.defer_in(window, move |this, window, cx| {
                this.maybe_load_older(&channel, cx);
                this.maybe_load_newer(&channel, cx);
                this.maybe_ack_current(window, cx);
            });
        }
        self.sync_composer(window, cx);
        self.sync_side_panel(window, cx);
        self.refresh_member_list();
        self.sync_card();
        let viewer = self
            .viewer
            .as_ref()
            .map(|image| self.render_viewer(image, window, cx).into_any_element());
        let card = self
            .card
            .as_ref()
            .and_then(|open| self.render_card(open, cx));
        let theme = cx.theme();
        let (background, foreground) = (theme.background, theme.foreground);
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(|this, _: &PrevChannel, _, cx| this.move_channel(-1, false, cx)))
            .on_action(cx.listener(|this, _: &NextChannel, _, cx| this.move_channel(1, false, cx)))
            .on_action(cx.listener(|this, _: &PrevUnread, _, cx| this.move_channel(-1, true, cx)))
            .on_action(cx.listener(|this, _: &NextUnread, _, cx| this.move_channel(1, true, cx)))
            .on_action(cx.listener(|this, _: &PrevServer, _, cx| this.move_server(-1, cx)))
            .on_action(cx.listener(|this, _: &NextServer, _, cx| this.move_server(1, cx)))
            .on_action(cx.listener(|this, _: &JumpToPresent, _, cx| this.go_to_present(cx)))
            .on_action(cx.listener(|this, _: &Escape, _, cx| this.go_to_present(cx)))
            .on_action(cx.listener(Self::on_mark_server_read))
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
            .children(card)
            .children(viewer)
    }
}
