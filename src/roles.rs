//! A server's roles as the Server Settings window's Roles page lists and
//! edits them: the rows highest first, what the user may do with each, the
//! editor's form and what it sends, and why Stoat refused, in words. Plain
//! data with no views (only GPUI's colour types), so it's unit-testable.
//! The page is [`crate::ui::server_settings`].
//!
//! Stoat's rules (0.16.2): every role route needs Manage Role, and a role
//! may be edited or deleted only if it ranks below the user's best role
//! (the owner outranks everyone). A bot's role can be edited but not
//! deleted.

use gpui_kit::{Hsla, Rgba};
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{DataEditRole, FieldsRole, PartialRole, Role};
use crate::permissions::{RoleLock, acting_rank};
use crate::profile_edit::error_text;
use crate::rail::parse_colour;
use crate::server_settings::access;
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

/// The role editor's form, as typed, ticked and picked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleForm {
    pub name: String,
    /// Shown separately in the member list.
    pub hoist: bool,
    pub colour: ColourChoice,
}

/// A role's colour as the editor holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ColourChoice {
    /// No colour.
    #[default]
    None,
    /// A solid colour, as lower-case `#rrggbb`.
    Hex(String),
    /// CSS RuSToat can't pick (a gradient, a colour name, `#rrggbbaa`), set
    /// by another client. It's kept as it is unless another is picked.
    Custom(String),
}

impl ColourChoice {
    /// The choice for a role's CSS `colour`: `#rgb` and `#rrggbb` are
    /// [`ColourChoice::Hex`] (normalised), anything else is custom.
    pub fn of(colour: Option<&str>) -> Self {
        match colour {
            None => ColourChoice::None,
            Some(css) => match parse_colour(css) {
                Some(colour) => ColourChoice::Hex(rgba_hex(colour)),
                None => ColourChoice::Custom(css.to_string()),
            },
        }
    }

    /// The colour to draw, as `#rrggbb`, if it's a solid colour: a picked
    /// one, or custom CSS that happens to be solid (a colour name).
    pub fn solid(&self) -> Option<String> {
        match self {
            ColourChoice::None => None,
            ColourChoice::Hex(hex) => Some(hex.clone()),
            ColourChoice::Custom(css) => solid_colour(css),
        }
    }
}

/// `colour` as lower-case `#rrggbb`, rounded to the nearest step and
/// ignoring transparency: what RuSToat writes as a role's colour.
pub fn hex_of(colour: Hsla) -> String {
    rgba_hex(colour.to_rgb())
}

fn rgba_hex(colour: Rgba) -> String {
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(colour.r),
        channel(colour.g),
        channel(colour.b)
    )
}

impl RoleForm {
    /// The form filled with `role`'s values.
    pub fn of(role: &Role) -> Self {
        Self {
            name: role.name.clone(),
            hoist: role.hoist,
            colour: ColourChoice::of(role.colour.as_deref()),
        }
    }

    /// Whether the name is 1 to [`ROLE_NAME_MAX`] characters (trimmed), so
    /// Save can send it.
    pub fn is_valid(&self) -> bool {
        check_role_name(&self.name).is_some()
    }

    /// The `PATCH /servers/{id}/roles/{role}` that turns `role` into this
    /// form: only what changed, the name trimmed, a picked colour as
    /// `#rrggbb` and no colour in `remove`. Custom CSS is never sent: it's
    /// only ever the role's own, unchanged. `None` when nothing changed, or
    /// the form isn't valid.
    pub fn changes(&self, role: &Role) -> Option<DataEditRole> {
        let name = check_role_name(&self.name)?;
        let mut edit = empty_role_edit();
        if name != role.name.trim() {
            edit.name = Some(name);
        }
        if self.hoist != role.hoist {
            edit.hoist = Some(self.hoist);
        }
        if self.colour != ColourChoice::of(role.colour.as_deref()) {
            match &self.colour {
                ColourChoice::None => edit.remove.push(FieldsRole::Colour),
                ColourChoice::Hex(hex) => edit.colour = Some(hex.clone()),
                ColourChoice::Custom(_) => {}
            }
        }
        (edit != empty_role_edit()).then_some(edit)
    }
}

/// A `DataEditRole` that changes nothing (it has no `Default`).
pub fn empty_role_edit() -> DataEditRole {
    DataEditRole {
        name: None,
        colour: None,
        hoist: None,
        rank: None,
        icon: None,
        remove: Vec::new(),
    }
}

/// Whether the user may edit `role` on `server`, as Stoat checks: Manage
/// Role, and the role ranking below their best one (the owner may edit
/// any). A bot's role may be edited.
pub fn may_edit(store: &ChatStore, server: &str, role: &str) -> bool {
    let Some(role) = store
        .server(server)
        .and_then(|server| server.roles.get(role))
    else {
        return false;
    };
    access(store, server).roles && role.rank > acting_rank(store, server)
}

/// Whether the user may delete `role` on `server`: as [`may_edit`], and not
/// a bot's.
pub fn may_delete(store: &ChatStore, server: &str, role: &str) -> bool {
    may_edit(store, server, role)
        && store
            .server(server)
            .and_then(|server| server.roles.get(role))
            .is_some_and(|role| role.owner.is_none())
}

/// Why Stoat refused to save a role.
pub fn edit_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::NotElevated) => "That role ranks at or above your highest role.".into(),
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to manage roles.".into()
        }
        Some(ErrorType::NotFound) => "Stoat couldn't find something in that change.".into(),
        Some(ErrorType::FailedValidation { .. }) => {
            format!("Role names are 1–{ROLE_NAME_MAX} characters.")
        }
        _ => error_text(err, "Couldn't save the role."),
    }
}

/// Why Stoat refused to delete a role. (`NotFound` isn't a refusal: it's
/// already gone.)
pub fn delete_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::NotElevated) => "That role ranks at or above your highest role.".into(),
        Some(ErrorType::InvalidOperation) => "Roles that belong to a bot can't be deleted.".into(),
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to manage roles.".into()
        }
        _ => error_text(err, "Couldn't delete the role."),
    }
}
