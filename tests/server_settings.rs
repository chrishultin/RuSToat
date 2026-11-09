//! A server's settings: who sees which page, and what the Overview sends.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{FieldsServer, Server};
use stoat_gpui::server_settings::{Access, OverviewForm, access, empty_server_edit};
use stoat_gpui::store::ChatStore;

const MANAGE_SERVER: u64 = ChannelPermission::ManageServer as u64;
const BAN: u64 = ChannelPermission::BanMembers as u64;

fn user(id: &str) -> Value {
    json!({
        "_id": id, "username": id, "discriminator": "0001",
        "relationship": "None", "online": true
    })
}

fn member(user: &str, roles: &[&str]) -> Value {
    json!({
        "_id": { "server": "srv", "user": user },
        "joined_at": "2026-01-01T00:00:00.000Z",
        "roles": roles,
    })
}

/// `srv`, owned by `u-owner`: `u-admin` may manage it, `u-banner` may only
/// ban, and `u-plain` has no roles. `u-gone`'s membership isn't known.
fn store(me: &str) -> ChatStore {
    let users: Vec<Value> = ["u-owner", "u-admin", "u-banner", "u-plain", "u-gone"]
        .map(user)
        .into();
    let ready = json!({
        "type": "Ready",
        "users": users,
        "servers": [{
            "_id": "srv", "owner": "u-owner", "name": "Test", "channels": [],
            "approximate_member_count": 4,
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "roles": {
                "admin": { "_id": "admin", "name": "Admin", "rank": 0, "permissions": { "a": MANAGE_SERVER, "d": 0 } },
                "banner": { "_id": "banner", "name": "Banner", "rank": 1, "permissions": { "a": BAN, "d": 0 } }
            }
        }],
        "channels": [],
        "members": [
            member("u-owner", &[]),
            member("u-admin", &["admin"]),
            member("u-banner", &["banner"]),
            member("u-plain", &[]),
        ],
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new(me);
    store.apply_ready(*ready);
    store
}

#[test]
fn the_owner_sees_every_page() {
    let all = Access {
        overview: true,
        invites: true,
        bans: true,
        ownership: true,
    };
    assert_eq!(access(&store("u-owner"), "srv"), all);
}

#[test]
fn manage_server_shows_the_overview_and_invites() {
    let access = access(&store("u-admin"), "srv");
    assert!(access.overview && access.invites);
    assert!(!access.bans && !access.ownership);
    assert!(access.any());
}

#[test]
fn ban_members_shows_only_bans() {
    let access = access(&store("u-banner"), "srv");
    assert_eq!(
        access,
        Access {
            bans: true,
            ..Access::default()
        }
    );
}

#[test]
fn a_plain_member_sees_nothing() {
    assert!(!access(&store("u-plain"), "srv").any());
    // Nor does anyone whose membership isn't known, or for an unknown server.
    assert!(!access(&store("u-gone"), "srv").any());
    assert!(!access(&store("u-owner"), "srv-unknown").any());
}

fn server(description: Option<&str>) -> Server {
    serde_json::from_value(json!({
        "_id": "srv", "owner": "u-owner", "name": "Test", "channels": [],
        "default_permissions": 0, "approximate_member_count": 1,
        "description": description,
    }))
    .unwrap()
}

fn form(name: &str, description: &str) -> OverviewForm {
    OverviewForm {
        name: name.into(),
        description: description.into(),
    }
}

#[test]
fn an_unchanged_form_sends_nothing() {
    let server = server(Some("About us"));
    assert_eq!(OverviewForm::of(&server), form("Test", "About us"));
    assert_eq!(OverviewForm::of(&server).changes(&server), None);
    // Spaces around a value aren't a change.
    assert_eq!(form(" Test ", "About us\n").changes(&server), None);
}

#[test]
fn a_rename_sends_only_the_name() {
    let edit = form(" Lounge ", "About us")
        .changes(&server(Some("About us")))
        .unwrap();
    assert_eq!(
        edit,
        stoat_gpui::api::models::DataEditServer {
            name: Some("Lounge".into()),
            ..empty_server_edit()
        }
    );
}

#[test]
fn a_new_description_is_sent_trimmed() {
    let edit = form("Test", "  Hello\n").changes(&server(None)).unwrap();
    assert_eq!(edit.description.as_deref(), Some("Hello"));
    assert!(edit.name.is_none() && edit.remove.is_empty());
}

#[test]
fn a_blank_description_is_removed() {
    let edit = form("Test", "   ")
        .changes(&server(Some("About us")))
        .unwrap();
    assert_eq!(edit.description, None);
    assert_eq!(edit.remove, [FieldsServer::Description]);
    // Already none: nothing to remove.
    assert_eq!(form("Test", "").changes(&server(None)), None);
}

#[test]
fn names_are_1_to_32_characters_and_descriptions_up_to_1024() {
    let server = server(None);
    assert!(form(&"n".repeat(32), "").is_valid());
    assert!(!form(&"n".repeat(33), "").is_valid());
    assert!(!form("  ", "").is_valid());
    assert!(form("Test", &"d".repeat(1024)).is_valid());
    assert!(!form("Test", &"d".repeat(1025)).is_valid());
    // An invalid form sends nothing.
    assert_eq!(form(&"n".repeat(33), "").changes(&server), None);
    assert_eq!(form("Test", &"d".repeat(1025)).changes(&server), None);
}
