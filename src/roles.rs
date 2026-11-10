//! A server's roles as the Server Settings window's Roles page lists and
//! edits them: the rows highest first, what the user may do with each, the
//! editor's form and what it sends, and why Stoat refused, in words. Plain
//! data with no views (only GPUI's colour types), so it's unit-testable.
//! The page is [`crate::ui::server_settings`].
//!
//! Stoat's rules (0.16.2): every role route needs Manage Role, and a role
//! may be edited or deleted only if it ranks below the user's best role
//! (the owner outranks everyone). A bot's role can be edited but not
//! deleted. A role's permissions need Manage Permissions instead, with the
//! same rank rule.

use std::collections::HashSet;
use std::path::Path;

use gpui_kit::{Hsla, Rgba};
use revolt_permissions::ChannelPermission;
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{DataEditRole, FieldsRole, PartialRole, Role};
use crate::permissions::{RoleLock, acting_rank, has, server_permissions};
use crate::profile_edit::{ProfileImage, error_text};
use crate::rail::parse_colour;
use crate::server_settings::image_change;
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
    /// Uploaded with the tag `icons`, within the server icon's limit.
    pub icon: ProfileImage,
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
            icon: role.icon.as_ref().map_or(ProfileImage::None, |icon| {
                ProfileImage::Current(icon.id.clone())
            }),
        }
    }

    /// The icon picked to upload before saving, if one was.
    pub fn new_file(&self) -> Option<&Path> {
        self.icon.new_file()
    }

    /// Whether Save has anything to do: a change to send, or an icon to
    /// upload first.
    pub fn has_changes(&self, role: &Role) -> bool {
        self.new_file().is_some() || self.changes(role).is_some()
    }

    /// Whether the name is 1 to [`ROLE_NAME_MAX`] characters (trimmed), so
    /// Save can send it.
    pub fn is_valid(&self) -> bool {
        check_role_name(&self.name).is_some()
    }

    /// The `PATCH /servers/{id}/roles/{role}` that turns `role` into this
    /// form: only what changed, the name trimmed, a picked colour as
    /// `#rrggbb`, and no colour or a removed icon in `remove`. Custom CSS is
    /// never sent: it's only ever the role's own, unchanged. A picked icon
    /// is left out until it's uploaded (and then it's
    /// [`ProfileImage::Current`] with its new ID, which replaces the old
    /// one without a remove). `None` when nothing changed, or the form
    /// isn't valid.
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
        match image_change(&Self::of(role).icon, &self.icon) {
            Some(Some(id)) => edit.icon = Some(id),
            Some(None) => edit.remove.push(FieldsRole::Icon),
            None => {}
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

/// Whether the user holds `permission` across `server`: its owner, or a
/// member (once their membership is known) whose roles give it.
fn holds(store: &ChatStore, server: &str, permission: ChannelPermission) -> bool {
    let me = store.me();
    let Some(known) = store.server(server) else {
        return false;
    };
    (known.owner == me || store.member(server, me).is_some())
        && has(server_permissions(store, me, server), permission)
}

/// Whether the user may create, edit, delete and reorder `server`'s roles
/// at all (Manage Role). Which roles is [`may_edit`]'s to say.
pub fn can_manage_roles(store: &ChatStore, server: &str) -> bool {
    holds(store, server, ChannelPermission::ManageRole)
}

/// Whether the user may change permissions on `server` at all (Manage
/// Permissions): its default permissions, and roles' permissions where
/// [`may_edit_permissions`] allows.
pub fn can_manage_permissions(store: &ChatStore, server: &str) -> bool {
    holds(store, server, ChannelPermission::ManagePermissions)
}

/// `role` on `server`, if both are known, and whether it ranks below the
/// user's best role (always, for the owner).
fn below_us<'a>(store: &'a ChatStore, server: &str, role: &str) -> Option<(&'a Role, bool)> {
    let role = store.server(server)?.roles.get(role)?;
    Some((role, role.rank > acting_rank(store, server)))
}

/// Whether the user may edit `role` on `server`, as Stoat checks: Manage
/// Role, and the role ranking below their best one (the owner may edit
/// any). A bot's role may be edited.
pub fn may_edit(store: &ChatStore, server: &str, role: &str) -> bool {
    below_us(store, server, role).is_some_and(|(_, below)| below) && can_manage_roles(store, server)
}

/// Whether the user may change `role`'s permissions on `server`, as Stoat
/// checks: Manage Permissions, and the role ranking below their best one
/// (the owner may change any). A bot's role may be changed. Which settings
/// they may choose is the grant rule's to say.
pub fn may_edit_permissions(store: &ChatStore, server: &str, role: &str) -> bool {
    below_us(store, server, role).is_some_and(|(_, below)| below)
        && can_manage_permissions(store, server)
}

/// Why the role editor can't change some or all of `role`, a line each:
/// its rank (which locks everything), a missing Manage Role or Manage
/// Permissions (which lock its fields or its permissions), or a bot's
/// role (which can't be deleted). Empty when nothing's locked.
pub fn role_lock_notes(store: &ChatStore, server: &str, role: &str) -> Vec<&'static str> {
    let Some((role, below)) = below_us(store, server, role) else {
        return Vec::new();
    };
    if !below {
        return vec!["This role ranks at or above your highest role, so you can't change it."];
    }
    let mut notes = Vec::new();
    let roles = can_manage_roles(store, server);
    if !roles {
        notes.push(
            "You don't have Manage Roles, so you can't change this role's name, colour, icon \
             or hoisting.",
        );
    }
    if !can_manage_permissions(store, server) {
        notes.push(
            "You don't have Manage Permissions, so you can't change this role's permissions.",
        );
    }
    if roles && role.owner.is_some() {
        notes.push("A bot manages this role. You can edit it, but not delete it.");
    }
    notes
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

/// Where a dragged role would land in the list: above or below another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleDrop {
    Before(String),
    After(String),
}

/// `server`'s role IDs, highest first, as the list shows them.
pub fn role_order(store: &ChatStore, server: &str) -> Vec<String> {
    role_rows(store, server)
        .into_iter()
        .map(|row| row.id)
        .collect()
}

/// `order` with `dragged` moved to `target`. `None` if either isn't in it,
/// or nothing would change.
pub fn reorder(order: &[String], dragged: &str, target: &RoleDrop) -> Option<Vec<String>> {
    let (RoleDrop::Before(beside) | RoleDrop::After(beside)) = target;
    if beside == dragged || !order.iter().any(|id| id == dragged) {
        return None;
    }
    let mut new: Vec<String> = order.iter().filter(|id| *id != dragged).cloned().collect();
    let at = new.iter().position(|id| id == beside)?;
    let at = match target {
        RoleDrop::Before(_) => at,
        RoleDrop::After(_) => at + 1,
    };
    new.insert(at, dragged.to_string());
    (new != order).then_some(new)
}

/// Whether Stoat would take `new` as `server`'s role order, from `old` (the
/// order now): every role exactly once, and, unless the user owns the
/// server, no role at or above their best one in a new position. So a
/// moderator reorders only the roles below them, among the positions below
/// them, and someone with no roles nothing.
pub fn may_reorder(store: &ChatStore, server: &str, old: &[String], new: &[String]) -> bool {
    let Some(known) = store.server(server) else {
        return false;
    };
    let unique: HashSet<&String> = new.iter().collect();
    if !can_manage_roles(store, server)
        || new.len() != known.roles.len()
        || unique.len() != new.len()
        || !known.roles.keys().all(|id| unique.contains(id))
    {
        return false;
    }
    if known.owner == store.me() {
        return true;
    }
    let ours = acting_rank(store, server);
    let position = |order: &[String], id: &str| order.iter().position(|listed| listed == id);
    known
        .roles
        .iter()
        .filter(|(_, role)| role.rank <= ours)
        .all(|(id, _)| position(old, id) == position(new, id))
}

/// Whether the user may drag `role` to another place: as [`may_edit`] (a
/// bot's role moves like any other). Where it may land is
/// [`may_reorder`]'s to say.
pub fn draggable(store: &ChatStore, server: &str, role: &str) -> bool {
    may_edit(store, server, role)
}

/// Why Stoat refused a new role order.
pub fn reorder_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::NotElevated) => {
            "You can only move roles below your highest role, among the places below it.".into()
        }
        Some(ErrorType::InvalidOperation) => {
            "The server's roles changed meanwhile. Try again.".into()
        }
        Some(ErrorType::MissingPermission { .. }) => {
            "You don't have permission to manage roles.".into()
        }
        _ => error_text(err, "Couldn't reorder the roles."),
    }
}
