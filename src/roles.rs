//! A server's roles as the Server Settings window's Roles page lists and
//! edits them: the rows highest first, what the user may do with each, and
//! why Stoat refused, in words. Plain data with no GPUI, so it's
//! unit-testable. The page is [`crate::ui::server_settings`].
//!
//! Stoat's rules (0.16.2): every role route needs Manage Role, and a role
//! may be edited or deleted only if it ranks below the user's best role
//! (the owner outranks everyone). A bot's role can be edited but not
//! deleted.

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{FieldsRole, PartialRole, Role};
use crate::permissions::{RoleLock, acting_rank};
use crate::profile_edit::error_text;
use crate::store::{ChatStore, solid_colour};

/// The longest role name Stoat takes, in characters.
pub const ROLE_NAME_MAX: usize = 32;

/// How many roles a server may have (`server_roles` in Stoat's
/// `Revolt.toml`; `GET /` doesn't say).
pub const ROLES_MAX: usize = 200;

/// One row of the Roles page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRow {
    pub id: String,
    pub name: String,
    /// The role's colour as `#rrggbb`, if it's a solid colour.
    pub colour: Option<String>,
    /// The role's icon, by file ID (tag `icons`).
    pub icon: Option<String>,
    pub hoist: bool,
    pub rank: i64,
    /// Why the user may not change the role, or delete it.
    pub lock: Option<RoleLock>,
}

impl RoleRow {
    /// Why the row is locked, for its tooltip.
    pub fn lock_text(&self) -> Option<&'static str> {
        self.lock.map(lock_text)
    }
}

/// Why a role is locked, in words.
pub fn lock_text(lock: RoleLock) -> &'static str {
    match lock {
        RoleLock::Rank => "Ranks at or above your highest role",
        RoleLock::Managed => "Managed by a bot",
    }
}

/// `server`'s roles, highest-ranked first (ties by ID, as
/// [`crate::permissions::assignable_roles`] sorts them), each with why the
/// user may not change it: [`RoleLock::Rank`] if it ranks at or above their
/// best role (never for the owner), else [`RoleLock::Managed`] if it's a
/// bot's, which can be edited but not deleted.
pub fn role_rows(store: &ChatStore, server: &str) -> Vec<RoleRow> {
    let Some(roles) = store.server(server).map(|server| &server.roles) else {
        return Vec::new();
    };
    let ours = acting_rank(store, server);
    let mut roles: Vec<_> = roles.iter().collect();
    roles.sort_by(|(a_id, a), (b_id, b)| a.rank.cmp(&b.rank).then_with(|| a_id.cmp(b_id)));
    roles
        .into_iter()
        .map(|(id, role)| RoleRow {
            id: id.clone(),
            name: role.name.clone(),
            colour: role.colour.as_deref().and_then(solid_colour),
            icon: role.icon.as_ref().map(|icon| icon.id.clone()),
            hoist: role.hoist,
            rank: role.rank,
            lock: role_lock(role, ours),
        })
        .collect()
}

/// Why the user, acting with rank `ours`, may not change `role`.
fn role_lock(role: &Role, ours: i64) -> Option<RoleLock> {
    if role.rank <= ours {
        Some(RoleLock::Rank)
    } else if role.owner.is_some() {
        Some(RoleLock::Managed)
    } else {
        None
    }
}

/// `text` as a role name Stoat accepts: trimmed, 1 to [`ROLE_NAME_MAX`]
/// characters. `None` when it's blank or too long.
pub fn check_role_name(text: &str) -> Option<String> {
    let name = text.trim();
    (1..=ROLE_NAME_MAX)
        .contains(&name.chars().count())
        .then(|| name.to_string())
}

/// Why Stoat refused to create a role.
pub fn create_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::TooManyRoles { max }) => {
            format!("This server has {max} roles, the most Stoat allows.")
        }
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to manage roles.".into()
        }
        Some(ErrorType::FailedValidation { .. }) => {
            format!("Role names are 1–{ROLE_NAME_MAX} characters.")
        }
        _ => error_text(err, "Couldn't create the role."),
    }
}

/// `role` as a `ServerRoleUpdate` would carry it whole: every field, and
/// the colour and icon it doesn't have to clear. Applied to the store, it
/// makes the cached role exactly `role`.
pub fn role_update(role: &Role) -> (PartialRole, Vec<FieldsRole>) {
    let partial = PartialRole {
        id: Some(role.id.clone()),
        name: Some(role.name.clone()),
        permissions: Some(role.permissions),
        colour: role.colour.clone(),
        hoist: Some(role.hoist),
        rank: Some(role.rank),
        icon: role.icon.clone(),
        owner: role.owner.clone(),
    };
    let mut clear = Vec::new();
    if role.colour.is_none() {
        clear.push(FieldsRole::Colour);
    }
    if role.icon.is_none() {
        clear.push(FieldsRole::Icon);
    }
    (partial, clear)
}
