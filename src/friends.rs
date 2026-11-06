//! The Friends page in Home: the user's friends, friend requests and blocks,
//! worked out from the [`ChatStore`]. Plain data with no GPUI, so which
//! list someone is in, and in what order, is unit-testable.
//!
//! Stoat's `Ready` brings everyone the user has a relationship with, and
//! [`ChatStore::apply_relationship`] keeps them current, so nothing here
//! needs a fetch of its own.

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{RelationshipStatus, User};
use crate::profile::{RelationshipAction, can_message, friend_buttons};
use crate::store::{ChatStore, PresenceKind, display_name, presence_kind};

/// The Friends page's ID in Home's selection, where channels' IDs go
/// otherwise. Stoat's IDs are 26-character ULIDs, so it can't be a channel's.
pub const FRIENDS: &str = "friends";

/// The page's tabs, as in the web client.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FriendsTab {
    /// Friends who are online (and not invisible).
    #[default]
    Online,
    /// Every friend.
    All,
    /// Requests to the user, then the user's own.
    Pending,
    Blocked,
}

impl FriendsTab {
    /// In the order the tab bar shows them.
    pub const ALL: [FriendsTab; 4] = [Self::Online, Self::All, Self::Pending, Self::Blocked];

    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::All => "All",
            Self::Pending => "Pending",
            Self::Blocked => "Blocked",
        }
    }
}

/// A person on the Friends page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendRow {
    pub user_id: String,
    /// Display name, else username.
    pub name: String,
    pub avatar_url: String,
    pub presence: PresenceKind,
    /// The custom status, else the presence's name (empty when offline).
    pub status: String,
    pub relationship: RelationshipStatus,
}

/// Everyone on the Friends page, by list, each sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FriendLists {
    /// Friends who show as online (see [`presence_kind`]).
    pub online: Vec<FriendRow>,
    pub all: Vec<FriendRow>,
    /// Friend requests to the user.
    pub incoming: Vec<FriendRow>,
    /// The user's own friend requests.
    pub outgoing: Vec<FriendRow>,
    pub blocked: Vec<FriendRow>,
}

impl FriendLists {
    /// What `tab` shows: (title, people) for each of its lists.
    pub fn sections(&self, tab: FriendsTab) -> Vec<(&'static str, &[FriendRow])> {
        match tab {
            FriendsTab::Online => vec![("Online", &self.online[..])],
            FriendsTab::All => vec![("All", &self.all[..])],
            FriendsTab::Pending => vec![
                ("Incoming", &self.incoming[..]),
                ("Outgoing", &self.outgoing[..]),
            ],
            FriendsTab::Blocked => vec![("Blocked", &self.blocked[..])],
        }
    }
}

/// The Friends page's lists. People the user has no relationship with,
/// those who blocked the user, and the user themselves are in none.
pub fn friend_lists(store: &ChatStore) -> FriendLists {
    let mut lists = FriendLists::default();
    for user in store.users() {
        if user.id == store.me() {
            continue;
        }
        let list = match user.relationship {
            RelationshipStatus::Friend => &mut lists.all,
            RelationshipStatus::Incoming => &mut lists.incoming,
            RelationshipStatus::Outgoing => &mut lists.outgoing,
            RelationshipStatus::Blocked => &mut lists.blocked,
            RelationshipStatus::None
            | RelationshipStatus::BlockedOther
            | RelationshipStatus::User => {
                continue;
            }
        };
        list.push(friend_row(store, user));
    }
    lists.online = lists
        .all
        .iter()
        .filter(|row| row.presence != PresenceKind::Offline)
        .cloned()
        .collect();
    for list in [
        &mut lists.online,
        &mut lists.all,
        &mut lists.incoming,
        &mut lists.outgoing,
        &mut lists.blocked,
    ] {
        list.sort_by_cached_key(|row| (row.name.to_lowercase(), row.user_id.clone()));
    }
    lists
}

/// A button on a person's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    /// Open the DM with them.
    Message,
    Relationship(RelationshipAction),
}

/// The buttons on `user`'s row: Message for friends (bots too, who can only
/// be friends through a bot invite), Accept and Reject for a request to the
/// user, Cancel for the user's own, and Unblock for someone blocked. The
/// same rules as the profile card's ([`friend_buttons`], [`can_message`]).
/// Remove friend and Block stay in the person's menu.
pub fn row_actions(store: &ChatStore, user: &str) -> Vec<RowAction> {
    let Some(person) = store.user(user) else {
        return Vec::new();
    };
    match person.relationship {
        RelationshipStatus::Friend if can_message(store, user) => vec![RowAction::Message],
        RelationshipStatus::Incoming | RelationshipStatus::Outgoing => friend_buttons(store, user)
            .into_iter()
            .map(RowAction::Relationship)
            .collect(),
        RelationshipStatus::Blocked => vec![RowAction::Relationship(RelationshipAction::Unblock)],
        _ => Vec::new(),
    }
}

/// What the add-friend field says when what's typed isn't `name#1234`.
pub const USERNAME_HINT: &str = "Enter a username and its four-digit tag, like ada#1234.";

/// What's typed in the add-friend field, trimmed, if it has the shape Stoat
/// needs: a name, `#`, and four digits. Else the hint to show. The name
/// itself isn't checked (Stoat decides who exists), and it's split at the
/// first `#`, as Stoat does.
pub fn check_username(text: &str) -> Result<&str, &'static str> {
    let text = text.trim();
    let (name, tag) = text.split_once('#').ok_or(USERNAME_HINT)?;
    let tag_ok = tag.len() == 4 && tag.bytes().all(|b| b.is_ascii_digit());
    if name.trim().is_empty() || !tag_ok {
        return Err(USERNAME_HINT);
    }
    Ok(text)
}

/// What the add-friend field says under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddFriendState {
    /// The request is on its way.
    Sending,
    /// It went through.
    Sent(String),
    /// Nothing to do: they're already friends, or already asked.
    Info(String),
    Error(String),
}

/// The request to `user` went through. Stoat answers with them as they now
/// relate: a friend, if they'd already asked the user (it accepts theirs).
pub fn request_sent(user: &User) -> AddFriendState {
    let name = format!("{}#{}", user.username, user.discriminator);
    AddFriendState::Sent(match user.relationship {
        RelationshipStatus::Friend => format!("You're now friends with {name}."),
        _ => format!("Friend request sent to {name}."),
    })
}

/// Why Stoat turned down a request to `username` (as typed), in words.
pub fn request_refused(username: &str, err: &ApiError) -> AddFriendState {
    match err.error_type() {
        Some(ErrorType::NotFound) => {
            AddFriendState::Error(format!("There's no one called {username}."))
        }
        Some(ErrorType::InvalidProperty) => AddFriendState::Error(USERNAME_HINT.into()),
        Some(ErrorType::AlreadySentRequest) => {
            AddFriendState::Info("You've already sent them a request.".into())
        }
        Some(ErrorType::AlreadyFriends) => AddFriendState::Info("You're already friends.".into()),
        // Blocked, BlockedByOther, IsBot, TooManyPendingFriendRequests and
        // the rest read well as they are.
        _ => AddFriendState::Error(err.to_string()),
    }
}

/// How many friend requests are waiting for the user.
pub fn incoming_count(store: &ChatStore) -> usize {
    store
        .users()
        .filter(|user| user.relationship == RelationshipStatus::Incoming && user.id != store.me())
        .count()
}

fn friend_row(store: &ChatStore, user: &User) -> FriendRow {
    let presence = presence_kind(user);
    let custom = user
        .status
        .as_ref()
        .and_then(|status| status.text.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty());
    FriendRow {
        user_id: user.id.clone(),
        name: display_name(user).to_string(),
        avatar_url: store.user_avatar_url(&user.id),
        presence,
        status: custom.unwrap_or(presence.label()).to_string(),
        relationship: user.relationship.clone(),
    }
}
