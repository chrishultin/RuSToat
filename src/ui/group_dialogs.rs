//! "Create a new group": a name, and friends picked from a list, and
//! Stoat makes the group with them in it. "Add friends to group": friends
//! picked from the same list ([`FriendPicker`]), added one by one. The
//! rules and the error texts are in [`crate::groups`].

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::StoatClient;
use crate::api::models::Channel;
use crate::friends::FriendRow;
use crate::groups::{
    add_limit, add_member_error_text, create_group_error_text, filter_friends, limit_text,
    new_group_limit, pickable_friends, toggle_pick,
};
use crate::runtime::spawn_io;
use crate::server_edit::valid_name;
use crate::store::ChatStore;
use crate::ui::chat::{ChatView, avatar};
use crate::ui::server_dialogs::{field_label, name_field};

/// Create a new group's width.
pub const CREATE_GROUP_DIALOG_WIDTH: f32 = 420.;
/// Add friends to group's width.
pub const ADD_MEMBERS_DIALOG_WIDTH: f32 = 420.;

/// The height of the picker's list, which scrolls.
const PICKER_HEIGHT: f32 = 260.;
/// Every row of the list is this tall (it's a `uniform_list`).
const ROW_HEIGHT: f32 = 40.;
const AVATAR: f32 = 28.;

/// A search field over a list of the user's friends, each with a checkbox,
/// sorted by name. Friends are read once, when it opens. Up to `limit` can
/// be picked; the picks keep their order, and stay picked while the search
/// hides them. Clicking a row picks without taking the focus from the
/// field. It emits [`PicksChanged`] when someone is picked or unpicked.
pub struct FriendPicker {
    friends: Vec<FriendRow>,
    search: Entity<InputState>,
    picked: Vec<String>,
    limit: usize,
    /// While the dialog's request is out: nothing can be picked.
    disabled: bool,
    scroll: UniformListScrollHandle,
    _search: Subscription,
}

/// Someone was picked or unpicked in a [`FriendPicker`].
pub struct PicksChanged;

impl EventEmitter<PicksChanged> for FriendPicker {}

/// A row of the picker's list, as drawn.
#[derive(Clone)]
struct PickerRow {
    friend: FriendRow,
    picked: bool,
    /// It can be clicked: it's picked (to unpick), or there's room for it.
    enabled: bool,
}

impl FriendPicker {
    /// A picker of the user's friends in `store`, without those in
    /// `exclude` (a group's members), with those in `picked` picked (any of
    /// them who can be, up to `limit`).
    pub fn new(
        store: &ChatStore,
        exclude: &[String],
        picked: &[String],
        limit: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search for users…"));
        let subscription = cx.subscribe_in(&search, window, |this, _, event, _, cx| {
            if let InputEvent::Change = event {
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        });
        let friends = pickable_friends(store, exclude);
        let mut chosen = Vec::new();
        for user in picked {
            if friends.iter().any(|friend| &friend.user_id == user) {
                toggle_pick(&mut chosen, user, limit);
            }
        }
        Self {
            friends,
            search,
            picked: chosen,
            limit,
            disabled: false,
            scroll: UniformListScrollHandle::new(),
            _search: subscription,
        }
    }

    /// Who's picked, in the order they were.
    pub fn picked(&self) -> &[String] {
        &self.picked
    }

    /// The search field, to focus when the dialog opens.
    pub fn search(&self) -> &Entity<InputState> {
        &self.search
    }

    /// `user`'s name as the list shows it, if they're in it.
    pub fn name_of(&self, user: &str) -> Option<&str> {
        self.friends
            .iter()
            .find(|friend| friend.user_id == user)
            .map(|friend| friend.name.as_str())
    }

    /// Take `users` out of the list and the picks (they've joined the
    /// group), leaving room for `limit` people.
    pub fn exclude(&mut self, users: &[String], limit: usize, cx: &mut Context<Self>) {
        self.friends
            .retain(|friend| !users.contains(&friend.user_id));
        self.picked.retain(|user| !users.contains(user));
        self.picked.truncate(limit);
        self.limit = limit;
        cx.notify();
    }

    /// Stop picking (while a request is out), or start again.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    fn toggle(&mut self, user: &str, cx: &mut Context<Self>) {
        if !self.disabled && toggle_pick(&mut self.picked, user, self.limit) {
            cx.emit(PicksChanged);
            cx.notify();
        }
    }
}

impl Render for FriendPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let full = self.picked.len() >= self.limit;
        let query = self.search.read(cx).value();
        let rows: Rc<Vec<PickerRow>> = Rc::new(
            filter_friends(&self.friends, &query)
                .into_iter()
                .map(|friend| {
                    let picked = self.picked.contains(&friend.user_id);
                    PickerRow {
                        friend: friend.clone(),
                        picked,
                        enabled: !self.disabled && (picked || !full),
                    }
                })
                .collect(),
        );
        let note = |text: &'static str| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child(text)
                .into_any_element()
        };
        let list = if self.friends.is_empty() {
            note("No friends to add.")
        } else if rows.is_empty() {
            note("No friends match your search.")
        } else {
            let picker = cx.entity().downgrade();
            uniform_list("friend-picker", rows.len(), move |range, _, cx| {
                range
                    .map(|ix| picker_row(&rows[ix], &picker, cx))
                    .collect::<Vec<_>>()
            })
            .size_full()
            .track_scroll(&self.scroll)
            .into_any_element()
        };
        // Once full, it says so; until then, how many are picked (some
        // may be hidden by the search).
        let count = if full {
            limit_text(self.limit)
        } else {
            format!("{} selected", self.picked.len())
        };

        v_flex()
            .gap_2()
            .child(
                Input::new(&self.search)
                    .cleanable(true)
                    .disabled(self.disabled)
                    .prefix(Icon::new(IconName::Search).small().text_color(muted)),
            )
            .child(
                div()
                    .h(px(PICKER_HEIGHT))
                    .p_1()
                    .border_1()
                    .border_color(border)
                    .rounded(theme.radius)
                    .child(list),
            )
            .child(div().text_xs().text_color(muted).child(count))
    }
}

/// A friend in the picker: the checkbox, their avatar and name. The whole
/// row is the checkbox, so a click anywhere on it picks them.
fn picker_row(row: &PickerRow, picker: &WeakEntity<FriendPicker>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (picker, user) = (picker.clone(), row.friend.user_id.clone());
    div()
        .h(px(ROW_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .rounded_md()
        .hover(|style| style.bg(theme.secondary))
        .child(
            Checkbox::new(SharedString::from(format!("pick-friend-{user}")))
                .w_full()
                .h_full()
                .items_center()
                .tab_stop(false)
                .checked(row.picked)
                .disabled(!row.enabled)
                .on_click(move |_, _, cx| {
                    picker.update(cx, |this, cx| this.toggle(&user, cx)).ok();
                })
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(avatar(
                            format!("pick-avatar-{}", row.friend.user_id),
                            Some(row.friend.avatar_url.clone()),
                            &row.friend.name,
                            AVATAR,
                            theme.secondary,
                            theme.secondary_foreground,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .truncate()
                                .child(row.friend.name.clone()),
                        ),
                ),
        )
        .into_any_element()
}

/// The Create a new group dialog's content, for one account.
pub struct CreateGroupDialog {
    chat: WeakEntity<ChatView>,
    client: StoatClient,
    input: Entity<InputState>,
    picker: Entity<FriendPicker>,
    creating: bool,
    error: Option<String>,
    _input: Subscription,
    _picker: Subscription,
}

impl CreateGroupDialog {
    /// The dialog for `chat`'s account, with `preselected` (a friend whose
    /// menu it was opened from) already picked.
    pub fn new(
        chat: &ChatView,
        weak: WeakEntity<ChatView>,
        preselected: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        let input_events =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.create(window, cx),
                _ => {}
            });
        let limit = new_group_limit(chat.group_size());
        let picker = cx.new(|cx| {
            FriendPicker::new(chat.store(), &[], preselected.as_slice(), limit, window, cx)
        });
        // A refusal is about who was picked, as often as not.
        let picks = cx.subscribe(&picker, |this, _, _: &PicksChanged, cx| {
            this.error = None;
            cx.notify();
        });
        Self {
            chat: weak,
            client: chat.client().clone(),
            input,
            picker,
            creating: false,
            error: None,
            _input: input_events,
            _picker: picks,
        }
    }

    /// The name field, to focus when the dialog opens.
    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Create the group with the typed name and the picked friends (maybe
    /// none: a group of one). Success closes the dialog and opens the
    /// group; a refusal says why and keeps it.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating {
            return;
        }
        let Some(name) = valid_name(&self.input.read(cx).value()) else {
            return;
        };
        let users = self.picker.read(cx).picked().to_vec();
        self.creating = true;
        self.error = None;
        self.picker
            .update(cx, |picker, cx| picker.set_disabled(true, cx));
        cx.notify();
        let request = spawn_io({
            let client = self.client.clone();
            async move { client.create_group(&name, &users).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                this.picker
                    .update(cx, |picker, cx| picker.set_disabled(false, cx));
                match result {
                    Ok(Ok(channel)) => {
                        if let Some(chat) = this.chat.upgrade() {
                            chat.update(cx, |chat, cx| chat.group_created(channel, cx));
                        }
                        window.close_dialog(cx);
                    }
                    Ok(Err(err)) => {
                        log::warn!("couldn't create a group: {err:?}");
                        this.error = Some(create_group_error_text(&err));
                    }
                    Err(err) => {
                        log::warn!("group create task failed: {err:?}");
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

impl Render for CreateGroupDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let valid = valid_name(&self.input.read(cx).value()).is_some();
        let entity = cx.entity().downgrade();

        v_flex()
            .gap_3()
            .child(name_field(
                "Group name",
                &self.input,
                self.creating,
                self.error.clone(),
                cx,
            ))
            .child(
                v_flex()
                    .gap_1()
                    .child(field_label("Select members to add", cx))
                    .child(self.picker.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-create-group")
                            .outline()
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("create-group-submit")
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

/// The Add friends to group dialog's content, for one account and group.
pub struct AddMembersDialog {
    chat: WeakEntity<ChatView>,
    client: StoatClient,
    group: String,
    picker: Entity<FriendPicker>,
    /// How many more the group can take.
    limit: usize,
    adding: bool,
    error: Option<String>,
    _picker: Subscription,
}

impl AddMembersDialog {
    /// The dialog for adding `chat`'s user's friends to `group`, listing
    /// those who aren't in it yet.
    pub fn new(
        chat: &ChatView,
        weak: WeakEntity<ChatView>,
        group: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let members = match chat.store().channel(&group) {
            Some(Channel::Group { recipients, .. }) => recipients.clone(),
            _ => Vec::new(),
        };
        let limit = add_limit(chat.group_size(), members.len());
        let picker = cx.new(|cx| FriendPicker::new(chat.store(), &members, &[], limit, window, cx));
        let picks = cx.subscribe(&picker, |this, _, _: &PicksChanged, cx| {
            this.error = None;
            cx.notify();
        });
        Self {
            chat: weak,
            client: chat.client().clone(),
            group,
            picker,
            limit,
            adding: false,
            error: None,
            _picker: picks,
        }
    }

    /// The picker's search field, to focus when the dialog opens.
    pub fn search(&self, cx: &App) -> Entity<InputState> {
        self.picker.read(cx).search().clone()
    }

    /// Add the picked friends, one request each in the order they were
    /// picked, each shown in the group as Stoat takes them. Someone
    /// already in it is passed over. The first refusal stops it and says
    /// whom it was for; those added leave the list, and the rest stay
    /// picked. Otherwise the dialog closes.
    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = self.picker.read(cx).picked().to_vec();
        if self.adding || picked.is_empty() {
            return;
        }
        let users: Vec<(String, String)> = {
            let picker = self.picker.read(cx);
            picked
                .into_iter()
                .map(|user| {
                    let name = picker.name_of(&user).unwrap_or(&user).to_string();
                    (user, name)
                })
                .collect()
        };
        self.adding = true;
        self.error = None;
        self.picker
            .update(cx, |picker, cx| picker.set_disabled(true, cx));
        cx.notify();
        let (client, group, chat) = (self.client.clone(), self.group.clone(), self.chat.clone());
        cx.spawn_in(window, async move |this, cx| {
            let mut added = Vec::new();
            let mut error = None;
            for (user, name) in users {
                let result = spawn_io({
                    let (client, group, user) = (client.clone(), group.clone(), user.clone());
                    async move { client.add_group_member(&group, &user).await }
                })
                .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(err))
                        if matches!(
                            err.error_type(),
                            Some(revolt_result::ErrorType::AlreadyInGroup)
                        ) => {}
                    Ok(Err(err)) => {
                        log::warn!("couldn't add {user} to group {group}: {err:?}");
                        error = Some(add_member_error_text(&err, &name));
                        break;
                    }
                    Err(err) => {
                        log::warn!("add group member task failed: {err:?}");
                        error = Some(format!("Couldn't add {name}. Try again."));
                        break;
                    }
                }
                // In the member list at once; `ChannelGroupJoin` repeats it.
                // This carries on if the dialog is closed meanwhile.
                cx.update(|_, cx| {
                    if let Some(chat) = chat.upgrade() {
                        chat.update(cx, |chat, cx| {
                            chat.group_member_added(&group, user.clone(), cx)
                        });
                    }
                })
                .ok();
                added.push(user);
            }
            this.update_in(cx, |this, window, cx| {
                this.adding = false;
                this.limit = this.limit.saturating_sub(added.len());
                let limit = this.limit;
                this.picker.update(cx, |picker, cx| {
                    picker.exclude(&added, limit, cx);
                    picker.set_disabled(false, cx);
                });
                match error {
                    None => window.close_dialog(cx),
                    Some(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for AddMembersDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let none = self.picker.read(cx).picked().is_empty();
        let danger = cx.theme().danger;
        let entity = cx.entity().downgrade();

        v_flex()
            .gap_3()
            .child(self.picker.clone())
            .children(
                self.error
                    .clone()
                    .map(|error| div().text_sm().text_color(danger).child(error)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-add-members")
                            .outline()
                            .label("Close")
                            .disabled(self.adding)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("add-members-submit")
                            .primary()
                            .label("Add")
                            .loading(self.adding)
                            .disabled(self.adding || none)
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| this.add(window, cx)).ok();
                            }),
                    ),
            )
    }
}
