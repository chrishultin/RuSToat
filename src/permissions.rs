//! What a user may see and do in a channel or server, worked out by Stoat's own
//! permission code (`revolt-permissions`, the same crate the backend runs)
//! over what the [`ChatStore`] knows: the user, the server's default
//! permissions and roles, the member's roles and timeout, and the channel's
//! overrides.
//!
//! That crate asks its questions through an `async` trait, but every answer
//! here comes from memory, so each call is run to completion on the spot.
//!
//! What the client can know limits the answer. Server roles, default
//! permissions and channel overrides are as of `Ready`, plus the live
//! `ServerUpdate`, `ServerRoleUpdate`, `ServerRoleDelete`,
//! `ServerRoleRanksUpdate` and `ChannelUpdate` events since.

use revolt_permissions::{
    ChannelPermission, ChannelType, DEFAULT_PERMISSION_DIRECT_MESSAGE, Override, PermissionQuery,
    RelationshipStatus, calculate_channel_permissions, calculate_server_permissions,
};

use crate::api::models::{self, Channel, Member, Server, User};
use crate::store::ChatStore;

/// The permission bits `user` has in `channel`, or none if either is unknown.
pub fn channel_permissions(store: &ChatStore, user: &str, channel: &str) -> u64 {
    let Some(channel) = store.channel(channel) else {
        return 0;
    };
    let server = match channel {
        Channel::TextChannel { server, .. } => store.server(server),
        _ => None,
    };
    let mut query = StoreQuery {
        store,
        user,
        channel: Some(channel),
        server,
        member: server.and_then(|server| store.member(&server.id, user)),
        other: None,
    };
    futures::executor::block_on(calculate_channel_permissions(&mut query)).into_raw()
}

/// The permission bits `user` has across `server` (no channel's overrides),
/// or none if it's unknown. Its owner has every one.
pub fn server_permissions(store: &ChatStore, user: &str, server: &str) -> u64 {
    let Some(server) = store.server(server) else {
        return 0;
    };
    let mut query = StoreQuery {
        store,
        user,
        channel: None,
        server: Some(server),
        member: store.member(&server.id, user),
        other: None,
    };
    futures::executor::block_on(calculate_server_permissions(&mut query)).into_raw()
}

/// Whether `user` may change their own nickname on `server` (Change
/// Nickname). Stoat checks the same when the change arrives.
pub fn can_change_nickname(store: &ChatStore, user: &str, server: &str) -> bool {
    has(
        server_permissions(store, user, server),
        ChannelPermission::ChangeNickname,
    )
}

/// Whether `user` may change their own avatar on `server` (Change Avatar).
/// Their pronouns there need no permission.
pub fn can_change_avatar(store: &ChatStore, user: &str, server: &str) -> bool {
    has(
        server_permissions(store, user, server),
        ChannelPermission::ChangeAvatar,
    )
}

/// Whether `permissions` (from [`channel_permissions`]) include `permission`.
pub fn has(permissions: u64, permission: ChannelPermission) -> bool {
    permissions & permission as u64 == permission as u64
}

/// Whether `user` can see `channel` (a server channel may be hidden from
/// some roles).
pub fn can_view(store: &ChatStore, user: &str, channel: &str) -> bool {
    has(
        channel_permissions(store, user, channel),
        ChannelPermission::ViewChannel,
    )
}

/// Whether `user` can pin and unpin messages in `channel`: anyone in a DM
/// (either person may), otherwise those with Manage Messages. Stoat decides
/// in the same way when the request arrives.
pub fn can_pin(store: &ChatStore, user: &str, channel: &str) -> bool {
    match store.channel(channel) {
        Some(Channel::DirectMessage { .. }) => true,
        Some(_) => has(
            channel_permissions(store, user, channel),
            ChannelPermission::ManageMessages,
        ),
        None => false,
    }
}

/// Whether `user` is exempt from `channel`'s slowmode: those with Bypass
/// Slowmode, which a server's owner has. Manage Messages isn't enough.
/// Slowmode only exists in server channels.
pub fn can_bypass_slowmode(store: &ChatStore, user: &str, channel: &str) -> bool {
    in_server(store, channel)
        && has(
            channel_permissions(store, user, channel),
            ChannelPermission::BypassSlowmode,
        )
}

/// Whether `user` may mention roles in `channel`: only in server channels,
/// with Mention Roles.
pub fn can_mention_roles(store: &ChatStore, user: &str, channel: &str) -> bool {
    in_server(store, channel)
        && has(
            channel_permissions(store, user, channel),
            ChannelPermission::MentionRoles,
        )
}

/// Whether `user` may mention `@everyone` and `@online` in `channel`: only
/// in server channels, with Mention Everyone.
pub fn can_mention_everyone(store: &ChatStore, user: &str, channel: &str) -> bool {
    in_server(store, channel)
        && has(
            channel_permissions(store, user, channel),
            ChannelPermission::MentionEveryone,
        )
}

/// Whether `user` may create invites to `channel`: only server channels
/// here (group invites come with group management), with Invite Others.
pub fn can_invite(store: &ChatStore, user: &str, channel: &str) -> bool {
    in_server(store, channel)
        && has(
            channel_permissions(store, user, channel),
            ChannelPermission::InviteOthers,
        )
}

/// Whether `user` may send embeds in `channel` (which a GIF is).
pub fn can_send_embeds(store: &ChatStore, user: &str, channel: &str) -> bool {
    has(
        channel_permissions(store, user, channel),
        ChannelPermission::SendEmbeds,
    )
}

fn in_server(store: &ChatStore, channel: &str) -> bool {
    matches!(store.channel(channel), Some(Channel::TextChannel { .. }))
}

/// The store, asked the questions `revolt-permissions` has about `user`
/// in `channel`, or across `server`.
struct StoreQuery<'a> {
    store: &'a ChatStore,
    /// Whose permissions these are.
    user: &'a str,
    /// `None` for server permissions, which ask nothing about a channel.
    channel: Option<&'a Channel>,
    /// The channel's server, for server channels.
    server: Option<&'a Server>,
    /// `user`'s membership of `server`.
    member: Option<&'a Member>,
    /// The other person in a DM, once the crate has asked for them.
    other: Option<&'a str>,
}

impl<'a> StoreQuery<'a> {
    fn user(&self) -> Option<&'a User> {
        self.store.user(self.user)
    }

    fn other_user(&self) -> Option<&'a User> {
        self.other.and_then(|id| self.store.user(id))
    }

    /// The member's roles that exist, lowest-ranked first (a higher rank is
    /// a smaller number), so the highest-ranked role is applied last and wins.
    fn roles_lowest_first(&self) -> Vec<&'a str> {
        let (Some(server), Some(member)) = (self.server, self.member) else {
            return Vec::new();
        };
        let mut roles: Vec<(&str, i64)> = member
            .roles
            .iter()
            .filter_map(|id| server.roles.get(id).map(|role| (id.as_str(), role.rank)))
            .collect();
        roles.sort_by_key(|(_, rank)| std::cmp::Reverse(*rank));
        roles.into_iter().map(|(id, _)| id).collect()
    }
}

#[async_trait::async_trait]
impl PermissionQuery for StoreQuery<'_> {
    async fn are_we_privileged(&mut self) -> bool {
        self.user().is_some_and(|user| user.privileged)
    }

    async fn are_we_a_bot(&mut self) -> bool {
        self.user().is_some_and(|user| user.bot.is_some())
    }

    async fn are_the_users_same(&mut self) -> bool {
        self.other == Some(self.user)
    }

    async fn user_relationship(&mut self) -> RelationshipStatus {
        // A user's `relationship` is to the logged-in user, so it only says
        // something when that's whose permissions these are.
        if self.user != self.store.me() {
            return RelationshipStatus::None;
        }
        match self.other_user().map(|other| &other.relationship) {
            Some(models::RelationshipStatus::User) => RelationshipStatus::User,
            Some(models::RelationshipStatus::Friend) => RelationshipStatus::Friend,
            Some(models::RelationshipStatus::Outgoing) => RelationshipStatus::Outgoing,
            Some(models::RelationshipStatus::Incoming) => RelationshipStatus::Incoming,
            Some(models::RelationshipStatus::Blocked) => RelationshipStatus::Blocked,
            Some(models::RelationshipStatus::BlockedOther) => RelationshipStatus::BlockedOther,
            Some(models::RelationshipStatus::None) | None => RelationshipStatus::None,
        }
    }

    async fn user_is_bot(&mut self) -> bool {
        self.other_user().is_some_and(|other| other.bot.is_some())
    }

    async fn have_mutual_connection(&mut self) -> bool {
        self.other
            .is_some_and(|other| self.store.have_mutual_connection(self.user, other))
    }

    async fn are_we_server_owner(&mut self) -> bool {
        self.server.is_some_and(|server| server.owner == self.user)
    }

    async fn are_we_a_member(&mut self) -> bool {
        self.member.is_some()
    }

    async fn get_default_server_permissions(&mut self) -> u64 {
        self.server
            .map_or(0, |server| server.default_permissions as u64)
    }

    async fn get_our_server_role_overrides(&mut self) -> Vec<Override> {
        let Some(server) = self.server else {
            return Vec::new();
        };
        self.roles_lowest_first()
            .into_iter()
            .filter_map(|id| server.roles.get(id))
            .map(|role| Override::from(role.permissions))
            .collect()
    }

    async fn are_we_timed_out(&mut self) -> bool {
        self.member.is_some_and(is_timed_out)
    }

    async fn do_we_have_publish_overwrites(&mut self) -> bool {
        self.member.is_none_or(|member| member.can_publish)
    }

    async fn do_we_have_receive_overwrites(&mut self) -> bool {
        self.member.is_none_or(|member| member.can_receive)
    }

    async fn get_channel_type(&mut self) -> ChannelType {
        match self.channel {
            Some(Channel::SavedMessages { .. }) => ChannelType::SavedMessages,
            Some(Channel::DirectMessage { .. }) => ChannelType::DirectMessage,
            Some(Channel::Group { .. }) => ChannelType::Group,
            Some(Channel::TextChannel { .. }) => ChannelType::ServerChannel,
            None => ChannelType::Unknown,
        }
    }

    async fn get_default_channel_permissions(&mut self) -> Override {
        match self.channel {
            Some(Channel::TextChannel {
                default_permissions,
                ..
            }) => default_permissions.map(Override::from).unwrap_or_default(),
            // A group's permissions are what its members may do, so they
            // count as allowed bits.
            Some(Channel::Group { permissions, .. }) => Override {
                allow: permissions.map_or(*DEFAULT_PERMISSION_DIRECT_MESSAGE, |bits| bits as u64),
                deny: 0,
            },
            _ => Override::default(),
        }
    }

    async fn get_our_channel_role_overrides(&mut self) -> Vec<Override> {
        let Some(Channel::TextChannel {
            role_permissions, ..
        }) = self.channel
        else {
            return Vec::new();
        };
        self.roles_lowest_first()
            .into_iter()
            .filter_map(|id| role_permissions.get(id))
            .map(|field| Override::from(*field))
            .collect()
    }

    async fn do_we_own_the_channel(&mut self) -> bool {
        match self.channel {
            Some(Channel::Group { owner, .. }) => owner == self.user,
            Some(Channel::SavedMessages { user, .. }) => user == self.user,
            _ => false,
        }
    }

    async fn are_we_part_of_the_channel(&mut self) -> bool {
        match self.channel {
            Some(Channel::DirectMessage { recipients, .. } | Channel::Group { recipients, .. }) => {
                recipients.iter().any(|id| id == self.user)
            }
            _ => false,
        }
    }

    async fn set_recipient_as_user(&mut self) {
        if let Some(Channel::DirectMessage { recipients, .. }) = self.channel {
            // A DM with yourself has only you in it.
            self.other = recipients
                .iter()
                .map(String::as_str)
                .find(|id| *id != self.user)
                .or(Some(self.user));
        }
    }

    async fn set_server_from_channel(&mut self) {
        // The server is looked up when the query is made.
    }
}

/// Whether `member`'s timeout is still running.
fn is_timed_out(member: &Member) -> bool {
    let Some(until) = &member.timeout else {
        return false;
    };
    let Ok(serde_json::Value::String(until)) = serde_json::to_value(until) else {
        return false;
    };
    chrono::DateTime::parse_from_rfc3339(&until)
        .is_ok_and(|until| until.with_timezone(&chrono::Utc) > chrono::Utc::now())
}
