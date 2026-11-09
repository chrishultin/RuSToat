//! A server's settings: who may see which page of the Server Settings
//! window, what its Overview form sends, and why Stoat refused, in words.
//! Plain data with no GPUI, so it's unit-testable. The window is
//! [`crate::ui::server_settings`].

use revolt_permissions::ChannelPermission;
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{DataEditServer, FieldsServer, Server};
use crate::permissions::{has, server_permissions};
use crate::profile_edit::error_text;
use crate::server_edit::valid_name;
use crate::store::ChatStore;

pub use crate::server_edit::NAME_MAX;

/// The longest server description Stoat takes, in characters.
pub const DESCRIPTION_MAX: usize = 1024;

/// Which pages of a server's settings the user may see.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Access {
    /// Name, description, icon, banner and system messages (Manage Server).
    pub overview: bool,
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
        self.overview || self.invites || self.bans || self.ownership
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
        invites: manage,
        bans: has(permissions, ChannelPermission::BanMembers),
        ownership,
    }
}

/// The Overview form's text fields, as typed. A missing description is the
/// empty string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverviewForm {
    pub name: String,
    pub description: String,
}

impl OverviewForm {
    /// The form filled with `server`'s values.
    pub fn of(server: &Server) -> Self {
        Self {
            name: server.name.clone(),
            description: server.description.clone().unwrap_or_default(),
        }
    }

    /// Whether the name is 1 to [`NAME_MAX`] characters and the description
    /// at most [`DESCRIPTION_MAX`] (both trimmed), so Save can send them.
    pub fn is_valid(&self) -> bool {
        valid_name(&self.name).is_some()
            && self.description.trim().chars().count() <= DESCRIPTION_MAX
    }

    /// The `PATCH /servers/{id}` that turns `server` into this form: only
    /// what changed, trimmed, with a blank description removed. `None` when
    /// nothing changed, or the form isn't valid.
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
        (edit != empty_server_edit()).then_some(edit)
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
