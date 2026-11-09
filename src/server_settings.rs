//! A server's settings: who may see which page of the Server Settings
//! window, what its Overview form sends (name, description, icon, banner
//! and system-message channels), and why Stoat refused, in words.
//! Plain data with no GPUI, so it's unit-testable. The window is
//! [`crate::ui::server_settings`].

use std::path::Path;

use chrono::{DateTime, Utc};
use iso8601_timestamp::Timestamp;
use revolt_permissions::ChannelPermission;
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{
    AllMemberResponse, BanListResult, Channel, DataEditServer, FieldsServer, File, Invite, Server,
    SystemMessageChannels,
};
use crate::media::MediaUrls;
use crate::permissions::{has, server_permissions};
use crate::profile_edit::{ImageLimits, ProfileImage, error_text};
use crate::server_edit::valid_name;
use crate::store::ChatStore;
use crate::time::relative_time;

pub use crate::server_edit::NAME_MAX;

/// The longest server description Stoat takes, in characters.
pub const DESCRIPTION_MAX: usize = 1024;

/// Which pages of a server's settings the user may see.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Access {
    /// Name, description, icon, banner and system messages (Manage Server).
    pub overview: bool,
    /// The server's roles (Manage Role).
    pub roles: bool,
    /// The server's invites (Manage Server).
    pub invites: bool,
    /// The server's bans (Ban Members).
    pub bans: bool,
    /// Transfer ownership, and delete the server (the owner).
    pub ownership: bool,
}

impl Access {
    /// Whether there's any page to show, so "Server settings…" is offered.
    pub fn any(&self) -> bool {
        self.overview || self.roles || self.invites || self.bans || self.ownership
    }
}

/// Which pages of `server`'s settings the user may see, as Stoat checks
/// each one. The owner sees them all; anyone else needs a known membership.
pub fn access(store: &ChatStore, server: &str) -> Access {
    let me = store.me();
    let Some(owner) = store.server(server).map(|server| server.owner.as_str()) else {
        return Access::default();
    };
    let ownership = owner == me;
    if !ownership && store.member(server, me).is_none() {
        return Access::default();
    }
    let permissions = server_permissions(store, me, server);
    let manage = has(permissions, ChannelPermission::ManageServer);
    Access {
        overview: manage,
        roles: has(permissions, ChannelPermission::ManageRole),
        invites: manage,
        bans: has(permissions, ChannelPermission::BanMembers),
        ownership,
    }
}

/// Which of a server's images a row edits: where it's uploaded, and its
/// size limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerImage {
    Icon,
    Banner,
}

impl ServerImage {
    /// The file server's tag.
    pub fn tag(self) -> &'static str {
        match self {
            ServerImage::Icon => "icons",
            ServerImage::Banner => "banners",
        }
    }

    pub fn limit(self, limits: ImageLimits) -> u64 {
        match self {
            ServerImage::Icon => limits.server_icon,
            ServerImage::Banner => limits.server_banner,
        }
    }
}

/// The Overview form, as typed and picked. A missing description is the
/// empty string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverviewForm {
    pub name: String,
    pub description: String,
    pub icon: ProfileImage,
    pub banner: ProfileImage,
    /// Where each kind of system message goes.
    pub system: SystemChannels,
}

impl OverviewForm {
    /// The form filled with `server`'s values.
    pub fn of(server: &Server) -> Self {
        let image = |file: &Option<File>| {
            file.as_ref().map_or(ProfileImage::None, |file| {
                ProfileImage::Current(file.id.clone())
            })
        };
        Self {
            name: server.name.clone(),
            description: server.description.clone().unwrap_or_default(),
            icon: image(&server.icon),
            banner: image(&server.banner),
            system: SystemChannels::of(server),
        }
    }

    /// Whether the name is 1 to [`NAME_MAX`] characters and the description
    /// at most [`DESCRIPTION_MAX`] (both trimmed), so Save can send them.
    pub fn is_valid(&self) -> bool {
        valid_name(&self.name).is_some()
            && self.description.trim().chars().count() <= DESCRIPTION_MAX
    }

    /// The files picked to upload before saving.
    pub fn new_files(&self) -> Vec<(ServerImage, &Path)> {
        [
            (ServerImage::Icon, self.icon.new_file()),
            (ServerImage::Banner, self.banner.new_file()),
        ]
        .into_iter()
        .filter_map(|(kind, path)| Some((kind, path?)))
        .collect()
    }

    /// Whether Save has anything to do: a change to send, or a file to
    /// upload first.
    pub fn has_changes(&self, server: &Server) -> bool {
        !self.new_files().is_empty() || self.changes(server).is_some()
    }

    /// The `PATCH /servers/{id}` that turns `server` into this form: only
    /// what changed, trimmed, with a blank description or a removed image
    /// in `remove`. A picked image is left out until it's uploaded (and
    /// then it's [`ProfileImage::Current`] with its new ID, which replaces
    /// the old one without a remove). `None` when nothing changed, or the
    /// form isn't valid.
    pub fn changes(&self, server: &Server) -> Option<DataEditServer> {
        if !self.is_valid() {
            return None;
        }
        let mut edit = empty_server_edit();
        let name = self.name.trim();
        if name != server.name.trim() {
            edit.name = Some(name.to_string());
        }
        let description = self.description.trim();
        let before = server.description.as_deref().unwrap_or_default().trim();
        if description != before {
            if description.is_empty() {
                edit.remove.push(FieldsServer::Description);
            } else {
                edit.description = Some(description.to_string());
            }
        }
        let stored = Self::of(server);
        match image_change(&stored.icon, &self.icon) {
            Some(Some(id)) => edit.icon = Some(id),
            Some(None) => edit.remove.push(FieldsServer::Icon),
            None => {}
        }
        match image_change(&stored.banner, &self.banner) {
            Some(Some(id)) => edit.banner = Some(id),
            Some(None) => edit.remove.push(FieldsServer::Banner),
            None => {}
        }
        let system = self.system.existing(server);
        if system != stored.system {
            if system == SystemChannels::default() {
                edit.remove.push(FieldsServer::SystemMessages);
            } else {
                edit.system_messages = Some(system.into());
            }
        }
        (edit != empty_server_edit()).then_some(edit)
    }
}

/// A kind of system message Stoat posts in a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemMessageKind {
    Joined,
    Left,
    Kicked,
    Banned,
}

impl SystemMessageKind {
    pub const ALL: [SystemMessageKind; 4] = [
        SystemMessageKind::Joined,
        SystemMessageKind::Left,
        SystemMessageKind::Kicked,
        SystemMessageKind::Banned,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SystemMessageKind::Joined => "User joined",
            SystemMessageKind::Left => "User left",
            SystemMessageKind::Kicked => "User kicked",
            SystemMessageKind::Banned => "User banned",
        }
    }
}

/// Where each kind of system message goes: a channel's ID, or `None` for
/// off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemChannels {
    pub user_joined: Option<String>,
    pub user_left: Option<String>,
    pub user_kicked: Option<String>,
    pub user_banned: Option<String>,
}

impl SystemChannels {
    /// `server`'s, with any channel it no longer has shown as off.
    pub fn of(server: &Server) -> Self {
        let stored = server
            .system_messages
            .as_ref()
            .map(|system| Self {
                user_joined: system.user_joined.clone(),
                user_left: system.user_left.clone(),
                user_kicked: system.user_kicked.clone(),
                user_banned: system.user_banned.clone(),
            })
            .unwrap_or_default();
        stored.existing(server)
    }

    /// These, without any channel `server` doesn't have: Stoat refuses the
    /// whole edit for one it can't find.
    pub fn existing(&self, server: &Server) -> Self {
        let keep = |id: &Option<String>| id.clone().filter(|id| server.channels.contains(id));
        Self {
            user_joined: keep(&self.user_joined),
            user_left: keep(&self.user_left),
            user_kicked: keep(&self.user_kicked),
            user_banned: keep(&self.user_banned),
        }
    }

    pub fn get(&self, kind: SystemMessageKind) -> Option<&str> {
        match kind {
            SystemMessageKind::Joined => self.user_joined.as_deref(),
            SystemMessageKind::Left => self.user_left.as_deref(),
            SystemMessageKind::Kicked => self.user_kicked.as_deref(),
            SystemMessageKind::Banned => self.user_banned.as_deref(),
        }
    }

    pub fn set(&mut self, kind: SystemMessageKind, channel: Option<String>) {
        let slot = match kind {
            SystemMessageKind::Joined => &mut self.user_joined,
            SystemMessageKind::Left => &mut self.user_left,
            SystemMessageKind::Kicked => &mut self.user_kicked,
            SystemMessageKind::Banned => &mut self.user_banned,
        };
        *slot = channel;
    }
}

impl From<SystemChannels> for SystemMessageChannels {
    fn from(system: SystemChannels) -> Self {
        SystemMessageChannels {
            user_joined: system.user_joined,
            user_left: system.user_left,
            user_kicked: system.user_kicked,
            user_banned: system.user_banned,
        }
    }
}

/// The channels system messages can go to, as `(id, name)`: the server's
/// channels in channel-list order (voice channels too, which have text
/// chat, and Stoat takes any of the server's channels), then any channel
/// already chosen that the list doesn't show the user.
pub fn system_channel_choices(store: &ChatStore, server: &str) -> Vec<(String, String)> {
    let mut choices: Vec<(String, String)> = store
        .server_channels(server)
        .into_iter()
        .flat_map(|section| section.channels)
        .map(|entry| (entry.id, entry.name))
        .collect();
    let Some(server) = store.server(server) else {
        return choices;
    };
    let chosen = SystemChannels::of(server);
    for kind in SystemMessageKind::ALL {
        let Some(id) = chosen.get(kind) else {
            continue;
        };
        if choices.iter().any(|(listed, _)| listed == id) {
            continue;
        }
        let name = match store.channel(id) {
            Some(Channel::TextChannel { name, .. }) => name.clone(),
            _ => "Hidden channel".into(),
        };
        choices.push((id.to_string(), name));
    }
    choices
}

/// How an image changed from `stored`: `None` if it didn't (or it's
/// picked and not uploaded yet), `Some(None)` if it was removed,
/// `Some(Some(id))` for a new one.
pub(crate) fn image_change(stored: &ProfileImage, edited: &ProfileImage) -> Option<Option<String>> {
    match edited {
        ProfileImage::New(_) => None,
        ProfileImage::Current(id) => (stored != edited).then(|| Some(id.clone())),
        ProfileImage::None => (*stored != ProfileImage::None).then_some(None),
    }
}

/// One row of the Invites page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteRow {
    pub code: String,
    /// Who made it, by ID.
    pub creator: String,
    /// Their display name, or "Unknown user" if they aren't cached.
    pub creator_name: String,
    /// Their avatar, if they're cached.
    pub creator_avatar: Option<String>,
    /// "#general", or "Unknown channel".
    pub channel: String,
    /// "3 uses", "1 use", "3 / 10 uses".
    pub uses: String,
    /// "Never expires", "Expires in 3 days".
    pub expiry: String,
}

/// The Invites page's rows for `invites` (as `GET /servers/{id}/invites`
/// returns them), in Stoat's order: codes say nothing of when they were
/// made. Invites that have expired by `now` are left out (Stoat lists
/// them until they're cleaned up), and so is anything but a server invite.
pub fn invite_rows(store: &ChatStore, invites: &[Invite], now: DateTime<Utc>) -> Vec<InviteRow> {
    invites
        .iter()
        .filter_map(|invite| {
            let Invite::Server {
                code,
                creator,
                channel,
                max_uses,
                uses,
                expires,
                ..
            } = invite
            else {
                return None;
            };
            let expires = expires.as_ref().and_then(timestamp_utc);
            if expires.is_some_and(|at| at <= now) {
                return None;
            }
            let user = store.user(creator);
            Some(InviteRow {
                code: code.clone(),
                creator: creator.clone(),
                creator_name: user.map_or_else(
                    || "Unknown user".into(),
                    |user| crate::store::display_name(user).to_string(),
                ),
                creator_avatar: user.map(|_| store.user_avatar_url(creator)),
                channel: match store.channel(channel) {
                    Some(Channel::TextChannel { name, .. }) => format!("#{name}"),
                    _ => "Unknown channel".into(),
                },
                uses: uses_label(*uses, *max_uses),
                expiry: match expires {
                    Some(at) => format!("Expires {}", relative_time(&at, &now)),
                    None => "Never expires".into(),
                },
            })
        })
        .collect()
}

/// How often an invite has been used, out of how many: "3 uses", "1 use",
/// "3 / 10 uses".
pub fn uses_label(uses: u64, max_uses: Option<u64>) -> String {
    match max_uses {
        Some(max) => format!("{uses} / {max} uses"),
        None if uses == 1 => "1 use".into(),
        None => format!("{uses} uses"),
    }
}

/// One of Stoat's timestamps, as a time.
fn timestamp_utc(timestamp: &Timestamp) -> Option<DateTime<Utc>> {
    let serde_json::Value::String(text) = serde_json::to_value(timestamp).ok()? else {
        return None;
    };
    DateTime::parse_from_rfc3339(&text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// Why the invites couldn't be listed.
pub fn invites_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to see this server's invites.".into()
        }
        _ => error_text(err, "Couldn't load the invites."),
    }
}

/// Why Stoat refused to revoke an invite. (`NotFound` isn't a refusal:
/// it's already gone.)
pub fn revoke_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => "You can't revoke this invite.".into(),
        _ => error_text(err, "Couldn't revoke the invite."),
    }
}

/// One row of the Bans page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanRow {
    /// The banned user's ID.
    pub user: String,
    /// `username#1234`, or their ID if Stoat didn't send the user.
    pub name: String,
    pub avatar: Option<String>,
    pub reason: Option<String>,
}

/// The Bans page's rows: each ban joined with its user, sorted by name. A
/// ban whose user is missing keeps its row under the ID, so it can still
/// be lifted. Avatars come from the file server directly: banned users
/// aren't in the store.
pub fn ban_rows(store: &ChatStore, list: &BanListResult) -> Vec<BanRow> {
    let mut rows: Vec<BanRow> = list
        .bans
        .iter()
        .map(|ban| {
            let user = list.users.iter().find(|user| user.id == ban.id.user);
            BanRow {
                user: ban.id.user.clone(),
                name: user.map_or_else(
                    || ban.id.user.clone(),
                    |user| format!("{}#{}", user.username, user.discriminator),
                ),
                avatar: user
                    .and_then(|user| user.avatar.as_ref())
                    .and_then(|avatar| store.media().file(avatar)),
                reason: ban
                    .reason
                    .as_ref()
                    .map(|reason| reason.trim().to_string())
                    .filter(|reason| !reason.is_empty()),
            }
        })
        .collect();
    rows.sort_by_cached_key(|row| row.name.to_lowercase());
    rows
}

/// The rows whose name or reason contains `query`, ignoring case. A blank
/// query keeps them all.
pub fn filter_bans<'a>(rows: &'a [BanRow], query: &str) -> Vec<&'a BanRow> {
    let query = query.trim().to_lowercase();
    rows.iter()
        .filter(|row| {
            query.is_empty()
                || row.name.to_lowercase().contains(&query)
                || row
                    .reason
                    .as_ref()
                    .is_some_and(|reason| reason.to_lowercase().contains(&query))
        })
        .collect()
}

/// Why the bans couldn't be listed.
pub fn bans_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to see this server's bans.".into()
        }
        _ => error_text(err, "Couldn't load the bans."),
    }
}

/// Why Stoat refused to lift a ban.
pub fn unban_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::NotFound) => "They aren't banned any more.".into(),
        Some(ErrorType::MissingPermission { .. }) => "You can't lift bans in this server.".into(),
        _ => error_text(err, "Couldn't lift the ban."),
    }
}

/// Someone the server could be handed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    /// Their nickname here, else their display name.
    pub name: String,
    /// `@username#1234`.
    pub handle: String,
    pub avatar: String,
}

/// Who the user may hand the server to: its members (from
/// `GET /servers/{id}/members`), but not the user and not bots (Stoat
/// refuses those), sorted by name.
pub fn transfer_candidates(
    members: &AllMemberResponse,
    me: &str,
    media: &MediaUrls,
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = members
        .members
        .iter()
        .filter(|member| member.id.user != me)
        .filter_map(|member| {
            let user = members.users.iter().find(|u| u.id == member.id.user)?;
            if user.bot.is_some() {
                return None;
            }
            let name = member
                .nickname
                .clone()
                .filter(|nick| !nick.trim().is_empty())
                .unwrap_or_else(|| crate::store::display_name(user).to_string());
            Some(Candidate {
                id: user.id.clone(),
                name,
                handle: format!("@{}#{}", user.username, user.discriminator),
                avatar: user
                    .avatar
                    .as_ref()
                    .and_then(|avatar| media.file(avatar))
                    .unwrap_or_else(|| media.default_avatar(&user.id)),
            })
        })
        .collect();
    candidates.sort_by_cached_key(|c| (c.name.to_lowercase(), c.handle.to_lowercase()));
    candidates
}

/// The candidates whose name or handle contains `query`, ignoring case. A
/// blank query keeps them all.
pub fn filter_candidates<'a>(candidates: &'a [Candidate], query: &str) -> Vec<&'a Candidate> {
    let query = query.trim().to_lowercase();
    candidates
        .iter()
        .filter(|c| {
            query.is_empty()
                || c.name.to_lowercase().contains(&query)
                || c.handle.to_lowercase().contains(&query)
        })
        .collect()
}

/// Whether what's typed to confirm deleting a server is its name: exactly,
/// case and all, apart from spaces around it.
pub fn delete_confirmed(typed: &str, name: &str) -> bool {
    !name.is_empty() && typed.trim() == name
}

/// Stoat's reasons for refusing a transfer, in words; `None` for anything
/// else (such as an expired confirmation, which the confirmation dialog
/// explains itself).
pub fn transfer_refusal(err: &ApiError) -> Option<String> {
    Some(match err.error_type()? {
        ErrorType::NotOwner => "Only the server's owner can transfer it.".into(),
        ErrorType::InvalidOperation => "Bots can't own servers.".into(),
        ErrorType::NotFound => "They're not a member any more.".into(),
        _ => return None,
    })
}

/// Why Stoat refused to delete a server.
pub fn delete_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::NotOwner) => "Only the server's owner can delete it.".into(),
        _ => error_text(err, "Couldn't delete the server."),
    }
}

/// A `DataEditServer` that changes nothing (it has no `Default`).
pub fn empty_server_edit() -> DataEditServer {
    DataEditServer {
        name: None,
        description: None,
        icon: None,
        banner: None,
        categories: None,
        system_messages: None,
        flags: None,
        discoverable: None,
        analytics: None,
        owner: None,
        remove: Vec::new(),
    }
}

/// Why Stoat refused to save the Overview.
pub fn save_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to change this server.".into()
        }
        Some(ErrorType::FailedValidation { .. }) => format!(
            "Server names are 1–{NAME_MAX} characters, and descriptions at most \
             {DESCRIPTION_MAX}."
        ),
        Some(ErrorType::NotFound) => "Stoat couldn't find something in that change.".into(),
        _ => error_text(err, "Couldn't save the server's settings."),
    }
}
