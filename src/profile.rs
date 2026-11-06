//! What the profile card shows about a person, worked out from the
//! [`ChatStore`]. Plain data with no GPUI, so the rules (which name, which
//! pronouns, which roles in what order) are unit-testable.

use chrono::{DateTime, TimeZone, Utc};

use crate::api::models::{Channel, RelationshipStatus};
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
    /// When the account was created (from its ID).
    pub created: Option<DateTime<Utc>>,
    /// Their relationship to the user.
    pub relationship: RelationshipStatus,
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
        created,
        relationship: person.relationship.clone(),
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
