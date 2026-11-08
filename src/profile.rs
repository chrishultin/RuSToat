//! What the profile card shows about a person, worked out from the
//! [`ChatStore`]. Plain data with no GPUI, so the rules (which name, which
//! pronouns, which roles in what order) are unit-testable.

use chrono::{DateTime, TimeZone, Utc};

use crate::api::models::{Channel, MutualResponse, RelationshipStatus};
use crate::store::{ChatStore, PresenceKind, display_name, presence_kind, solid_colour};

/// A person, as their profile card shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileCard {
    pub user_id: String,
    /// Nickname in the channel's server, else display name, else username.
    pub name: String,
    /// `username#discriminator`.
    pub username_tag: String,
    /// The server avatar if they have one, else their own (or the default).
    pub avatar_url: String,
    pub presence: PresenceKind,
    /// The custom status, if they've set one.
    pub status: Option<String>,
    /// The server's pronouns first, else their own.
    pub pronouns: Option<String>,
    pub bot: bool,
    /// Their roles in the channel's server, highest first. Empty outside
    /// server channels.
    pub roles: Vec<CardRole>,
    /// The server's name and when they joined it, in server channels.
    pub joined_server: Option<(String, DateTime<Utc>)>,
    /// When their timeout on the channel's server ends, while it runs.
    pub timed_out_until: Option<DateTime<Utc>>,
    /// When the account was created (from its ID).
    pub created: Option<DateTime<Utc>>,
    /// Their relationship to the user.
    pub relationship: RelationshipStatus,
    /// The friend buttons the card shows (see [`friend_buttons`]).
    pub friend_buttons: Vec<RelationshipAction>,
    /// The card is the user's own.
    pub is_me: bool,
}

/// One of a member's roles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardRole {
    pub name: String,
    /// `#rrggbb`, or `None` without a colour or with one that can't be drawn
    /// as a dot (a gradient).
    pub colour: Option<String>,
}

/// `user`'s card, as seen from `channel` (which decides the server, if
/// any). `None` if the user isn't known.
pub fn profile_card(store: &ChatStore, user: &str, channel: Option<&str>) -> Option<ProfileCard> {
    let person = store.user(user)?;
    let server = channel
        .and_then(|channel| store.channel_server(channel))
        .and_then(|server| store.server(server));
    let member = server.and_then(|server| store.member(&server.id, user));

    let nickname = member
        .and_then(|m| m.nickname.as_deref())
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let name = nickname.unwrap_or_else(|| display_name(person)).to_string();
    let avatar_url = member
        .and_then(|m| m.avatar.as_ref())
        .and_then(|file| store.media().file(file))
        .unwrap_or_else(|| store.user_avatar_url(user));
    let status = person
        .status
        .as_ref()
        .and_then(|status| status.text.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    let pronouns = member
        .and_then(|m| m.pronouns.as_deref())
        .or(person.pronouns.as_deref())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);

    let mut roles = Vec::new();
    if let (Some(server), Some(member)) = (server, member) {
        let mut held: Vec<_> = member
            .roles
            .iter()
            .filter_map(|id| Some((id, server.roles.get(id)?)))
            .collect();
        // A higher rank is a smaller number.
        held.sort_by(|(a_id, a), (b_id, b)| a.rank.cmp(&b.rank).then_with(|| a_id.cmp(b_id)));
        roles = held
            .into_iter()
            .map(|(_, role)| CardRole {
                name: role.name.clone(),
                colour: role.colour.as_deref().and_then(solid_colour),
            })
            .collect();
    }
    let joined_server = server
        .zip(member)
        .and_then(|(server, member)| Some((server.name.clone(), utc(&member.joined_at)?)));
    let timed_out_until = member
        .filter(|member| crate::permissions::is_timed_out(member))
        .and_then(|member| utc(member.timeout.as_ref()?));
    let created = crate::time::ulid_millis(user)
        .and_then(|millis| i64::try_from(millis).ok())
        .and_then(|millis| Utc.timestamp_millis_opt(millis).single());

    Some(ProfileCard {
        user_id: user.to_string(),
        name,
        username_tag: format!("{}#{}", person.username, person.discriminator),
        avatar_url,
        presence: presence_kind(person),
        status,
        pronouns,
        bot: person.bot.is_some(),
        roles,
        joined_server,
        timed_out_until,
        created,
        relationship: person.relationship.clone(),
        friend_buttons: friend_buttons(store, user),
        is_me: user == store.me(),
    })
}

/// The user's DM with `user`, open or closed, if there is one.
pub fn dm_with(store: &ChatStore, user: &str) -> Option<String> {
    let me = store.me();
    if user == me {
        return None;
    }
    store.channels().find_map(|channel| match channel {
        Channel::DirectMessage { id, recipients, .. }
            if recipients.iter().any(|r| r == me) && recipients.iter().any(|r| r == user) =>
        {
            Some(id.clone())
        }
        _ => None,
    })
}

/// Whether the card offers "Message": for friends and bots (whom Stoat
/// lets anyone open a DM with), and for anyone the user already has a DM
/// with. Never for the user themselves.
pub fn can_message(store: &ChatStore, user: &str) -> bool {
    if user == store.me() {
        return false;
    }
    let friend_or_bot = store.user(user).is_some_and(|person| {
        person.relationship == RelationshipStatus::Friend || person.bot.is_some()
    });
    friend_or_bot || dm_with(store, user).is_some()
}

/// Whether `user`'s card shows what the user shares with them: not on the
/// user's own card (Stoat refuses it) or a bot's (as the web client), nor
/// for someone unknown.
pub fn shows_mutuals(store: &ChatStore, user: &str) -> bool {
    user != store.me() && store.user(user).is_some_and(|person| person.bot.is_none())
}

/// What the user shares with someone, as their card shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mutuals {
    /// Friends of both, in Stoat's order.
    pub friends: Vec<MutualFriend>,
    /// The servers both are in, then the groups (not DMs).
    pub places: Vec<MutualPlace>,
}

/// A friend both have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutualFriend {
    pub user_id: String,
    /// Their display name, else username.
    pub name: String,
    pub avatar_url: String,
}

/// A server or group both are in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutualPlace {
    pub id: String,
    pub name: String,
    /// The uploaded icon, if there's one.
    pub icon_url: Option<String>,
    pub kind: MutualKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutualKind {
    Server,
    Group,
}

/// `mutual` (Stoat's IDs) as the card shows it: friends, then servers and
/// groups, each looked up in the store. Ones it doesn't know (a server
/// left since) are left out, and so are DMs. Nothing is capped; the card
/// shows the first few.
pub fn mutuals(store: &ChatStore, mutual: &MutualResponse) -> Mutuals {
    let friends = mutual
        .users
        .iter()
        .filter_map(|id| {
            let person = store.user(id)?;
            Some(MutualFriend {
                user_id: id.clone(),
                name: display_name(person).to_string(),
                avatar_url: store.user_avatar_url(id),
            })
        })
        .collect();
    let servers = mutual.servers.iter().filter_map(|id| {
        let server = store.server(id)?;
        Some(MutualPlace {
            id: id.clone(),
            name: server.name.clone(),
            icon_url: server
                .icon
                .as_ref()
                .and_then(|icon| store.media().file(icon)),
            kind: MutualKind::Server,
        })
    });
    let groups = mutual
        .channels
        .iter()
        .filter_map(|id| match store.channel(id)? {
            Channel::Group { name, icon, .. } => Some(MutualPlace {
                id: id.clone(),
                name: name.clone(),
                icon_url: icon.as_ref().and_then(|icon| store.media().file(icon)),
                kind: MutualKind::Group,
            }),
            _ => None,
        });
    Mutuals {
        friends,
        places: servers.chain(groups).collect(),
    }
}

/// What follows the icons a row has room for: "+3" for three more.
pub fn overflow_text(more: usize) -> String {
    format!("+{more}")
}

/// An entry in the menu of things to do with a person (right-click, or
/// the card's "…").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserMenuItem {
    /// Open their profile card.
    Profile,
    /// Open the DM with them (see [`can_message`]).
    Message,
    /// Open "Create a new group" with them picked (friends only).
    CreateGroupWith,
    /// Put a mention of them in the message box.
    Mention,
    /// Take them out of the group the menu was opened in (its owner only,
    /// after asking).
    RemoveFromGroup,
    /// Make them the owner of the group the menu was opened in (its owner
    /// only, after asking).
    MakeGroupOwner,
    /// Change their nickname or remove their server avatar on the server
    /// the menu was opened in.
    EditIdentity,
    /// Change their roles on the server the menu was opened in.
    EditRoles,
    /// Server mute them (`true`) or unmute them, while they're in one of
    /// the server's calls.
    ServerMute(bool),
    /// Server deafen them (`true`) or undeafen them.
    ServerDeafen(bool),
    /// Move them to another of the server's voice channels (a submenu of
    /// [`move_targets`]).
    MoveTo,
    /// Take them out of their call.
    DisconnectVoice,
    /// Time them out on the server the menu was opened in (a dialog asks
    /// for how long).
    TimeOut,
    /// End their running timeout early (after asking).
    EndTimeout,
    /// Kick them from the server the menu was opened in (after asking).
    Kick,
    /// Ban them from the server the menu was opened in (a dialog asks for
    /// a reason and how much of their history goes).
    Ban,
    /// Change how the user relates to them (friend requests, blocks).
    Relationship(RelationshipAction),
    Separator,
    /// Copy `username#discriminator`.
    CopyUsername,
    CopyUserId,
}

impl UserMenuItem {
    /// Whether it's drawn as destructive (in the danger colour).
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Kick | Self::Ban)
    }
}

/// The voice channels of `server` someone in `call` can be moved to, in
/// the server's order: the others the user may join (Connect), as Stoat
/// checks.
pub fn move_targets(store: &ChatStore, server: &str, call: &str) -> Vec<String> {
    let Some(server) = store.server(server) else {
        return Vec::new();
    };
    server
        .channels
        .iter()
        .filter(|id| id.as_str() != call)
        .filter(|id| {
            matches!(
                store.channel(id),
                Some(Channel::TextChannel { voice: Some(_), .. })
            )
        })
        .filter(|id| crate::permissions::can_connect(store, store.me(), id))
        .cloned()
        .collect()
}

/// A change to how the user relates to someone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipAction {
    /// Send a friend request.
    AddFriend,
    /// Accept theirs.
    AcceptFriend,
    /// Turn theirs down.
    RejectFriend,
    /// Take back the user's own.
    CancelFriend,
    RemoveFriend,
    Block,
    Unblock,
}

impl RelationshipAction {
    /// Whether it's asked about first (unless Shift is held).
    pub fn needs_confirming(self) -> bool {
        matches!(self, Self::RemoveFriend | Self::Block)
    }
}

/// The friend-request actions for `user` (the web client's rules): Add to
/// a stranger who isn't a bot, Accept or Reject an incoming request, and
/// Cancel an outgoing one. The card shows these as buttons.
pub fn friend_buttons(store: &ChatStore, user: &str) -> Vec<RelationshipAction> {
    use RelationshipAction::*;
    let Some(person) = store.user(user).filter(|_| user != store.me()) else {
        return Vec::new();
    };
    match person.relationship {
        RelationshipStatus::None if person.bot.is_none() => vec![AddFriend],
        RelationshipStatus::Incoming => vec![AcceptFriend, RejectFriend],
        RelationshipStatus::Outgoing => vec![CancelFriend],
        _ => Vec::new(),
    }
}

/// Remove friend (for friends), then Block or Unblock (anyone but the user).
fn standing_actions(store: &ChatStore, user: &str) -> Vec<RelationshipAction> {
    use RelationshipAction::*;
    let Some(person) = store.user(user).filter(|_| user != store.me()) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    if person.relationship == RelationshipStatus::Friend {
        actions.push(RemoveFriend);
    }
    actions.push(if person.relationship == RelationshipStatus::Blocked {
        Unblock
    } else {
        Block
    });
    actions
}

/// The menu for `user`, as seen from `channel`, in the web client's order:
/// Profile, Message, Create group with (RuSToat's own, for friends),
/// Mention; Edit identity and Edit roles; Server mute, Server deafen, Move
/// to and Disconnect (while they're in one of the server's calls); Remove
/// from group and Make group owner; Time out (or End timeout), Kick and Ban; the friend requests;
/// Remove friend and Block or Unblock; then the copy items. Message
/// follows [`can_message`]; Mention is only for others, in server channels
/// and groups (where the message box can mention them). Remove from group
/// and Make group owner are for the owner of the group `channel`, on its
/// other members. The server items are for server channels, as
/// [`crate::permissions::moderation`] allows; the two edits also need the
/// membership known, and Edit identity something to edit (a nickname, or
/// an avatar to remove). End timeout replaces Time out while one runs.
/// Groups are split by separators.
pub fn user_menu(store: &ChatStore, user: &str, channel: Option<&str>) -> Vec<UserMenuItem> {
    use UserMenuItem::*;
    let me = user == store.me();
    let mentionable = channel
        .and_then(|channel| store.channel(channel))
        .is_some_and(|channel| {
            matches!(channel, Channel::TextChannel { .. } | Channel::Group { .. })
        });
    let mut people = vec![Profile];
    if can_message(store, user) {
        people.push(Message);
    }
    let friend = store
        .user(user)
        .is_some_and(|person| person.relationship == RelationshipStatus::Friend);
    if friend && !me {
        people.push(CreateGroupWith);
    }
    if mentionable && !me {
        people.push(Mention);
    }
    let mut membership = Vec::new();
    if let Some(channel) = channel.filter(|channel| store.is_group_owner(channel))
        && !me
        && store.lists_member(channel, user)
    {
        membership.extend([RemoveFromGroup, MakeGroupOwner]);
    }
    let mut server_items = Vec::new();
    let mut voice = Vec::new();
    let mut moderation = Vec::new();
    if let Some(server) = channel.and_then(|channel| store.channel_server(channel)) {
        let allowed = crate::permissions::moderation(store, server, user);
        // These edit a membership as it is, so it has to be known.
        if let Some(member) = store.member(server, user) {
            if allowed.nickname || (allowed.remove_avatar && member.avatar.is_some()) {
                server_items.push(EditIdentity);
            }
            if allowed.roles {
                server_items.push(EditRoles);
            }
        }
        if let Some(call) = store.voice_channel_of(server, user) {
            let member = store.member(server, user);
            if allowed.mute {
                voice.push(ServerMute(member.is_none_or(|m| m.can_publish)));
            }
            if allowed.deafen {
                voice.push(ServerDeafen(member.is_none_or(|m| m.can_receive)));
            }
            if allowed.move_members {
                if !move_targets(store, server, call).is_empty() {
                    voice.push(MoveTo);
                }
                voice.push(DisconnectVoice);
            }
        }
        if allowed.timeout {
            let timed_out = store
                .member(server, user)
                .is_some_and(crate::permissions::is_timed_out);
            moderation.push(if timed_out { EndTimeout } else { TimeOut });
        }
        if allowed.kick {
            moderation.push(Kick);
        }
        if allowed.ban {
            moderation.push(Ban);
        }
    }
    let requests = friend_buttons(store, user)
        .into_iter()
        .map(Relationship)
        .collect();
    let standing = standing_actions(store, user)
        .into_iter()
        .map(Relationship)
        .collect();
    let groups = [
        people,
        server_items,
        voice,
        membership,
        moderation,
        requests,
        standing,
        vec![CopyUsername, CopyUserId],
    ];
    let mut items = Vec::new();
    for group in groups.into_iter().filter(|group| !group.is_empty()) {
        if !items.is_empty() {
            items.push(Separator);
        }
        items.extend(group);
    }
    items
}

/// A Stoat timestamp as a date. The placeholder a membership gets when an
/// old server didn't send one (the Unix epoch) counts as unknown.
fn utc(timestamp: &impl serde::Serialize) -> Option<DateTime<Utc>> {
    let serde_json::Value::String(text) = serde_json::to_value(timestamp).ok()? else {
        return None;
    };
    let when = DateTime::parse_from_rfc3339(&text)
        .ok()?
        .with_timezone(&Utc);
    (when.timestamp() > 0).then_some(when)
}
