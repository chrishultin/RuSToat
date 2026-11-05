//! Live server and role edits (`ServerUpdate`, `ServerRoleUpdate`,
//! `ServerRoleDelete`, `ServerRoleRanksUpdate`): the member list, role
//! colours, permissions and the channel list follow them.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::Channel;
use stoat_gpui::permissions::{can_pin, can_view};
use stoat_gpui::store::{ChatStore, MemberListRow, channel_id};

const VIEW: u64 = ChannelPermission::ViewChannel as u64;
const MANAGE: u64 = ChannelPermission::ManageMessages as u64;

fn user(id: &str) -> Value {
    json!({
        "_id": id, "username": id.trim_start_matches("u-"), "discriminator": "0001",
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

fn role(id: &str, name: &str, rank: i64, hoist: bool, colour: Option<&str>) -> Value {
    let mut role = json!({
        "_id": id, "name": name, "rank": rank, "hoist": hoist, "permissions": { "a": 0, "d": 0 }
    });
    if let Some(colour) = colour {
        role["colour"] = json!(colour);
    }
    role
}

/// `admin` (highest, hoisted, red), `mods` (hoisted, gold), and `plain`
/// (the user's own role). Bob holds both hoisted roles.
fn store() -> ChatStore {
    let ready = json!({
        "type": "Ready",
        "users": [user("u-me"), user("u-owner"), user("u-ada"), user("u-bob"), user("u-cy")],
        "servers": [{
            "_id": "srv", "owner": "u-owner", "name": "Test",
            "channels": ["ch-open", "ch-test"],
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "approximate_member_count": 5,
            "roles": {
                "admin": role("admin", "Admin", 0, true, Some("#ff0000")),
                "mods": role("mods", "Mods", 1, true, Some("gold")),
                "plain": role("plain", "Plain", 2, false, None)
            }
        }],
        "channels": [
            { "channel_type": "TextChannel", "_id": "ch-open", "server": "srv", "name": "open" },
            {
                "channel_type": "TextChannel", "_id": "ch-test", "server": "srv", "name": "test",
                "role_permissions": { "admin": { "a": MANAGE, "d": 0 } }
            }
        ],
        "members": [
            member("u-me", &["plain"]),
            member("u-owner", &[]),
            member("u-ada", &["admin"]),
            member("u-bob", &["mods", "admin"]),
            member("u-cy", &[])
        ]
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

/// Decode `event` and apply it as `ChatView` does. Returns what the store
/// method returned.
fn apply(store: &mut ChatStore, event: Value) -> bool {
    let events = parse_frame(&event.to_string());
    let [event] = <[ServerEvent; 1]>::try_from(events).expect("one event");
    match event {
        ServerEvent::ServerUpdate { id, data, clear } => {
            store.apply_server_update(&id, *data, &clear)
        }
        ServerEvent::ServerRoleUpdate {
            server,
            role_id,
            data,
            clear,
        } => store.apply_role_update(&server, &role_id, *data, &clear),
        ServerEvent::ServerRoleDelete { server, role_id } => {
            store.apply_role_delete(&server, &role_id)
        }
        ServerEvent::ServerRoleRanksUpdate { server, ranks } => {
            store.apply_role_ranks(&server, &ranks)
        }
        ServerEvent::ServerMemberUpdate { id, data, clear } => {
            store.apply_member_update(&id, *data, &clear)
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn role_update(role_id: &str, data: Value, clear: &[&str]) -> Value {
    json!({ "type": "ServerRoleUpdate", "id": "srv", "role_id": role_id, "data": data, "clear": clear })
}

/// The member list as text: "# Section (count)" and "name".
fn lines(store: &ChatStore) -> Vec<String> {
    store
        .member_list("ch-open")
        .rows
        .iter()
        .map(|row| match row {
            MemberListRow::Header { name, count } => format!("# {name} ({count})"),
            MemberListRow::Member { name, .. } => name.clone(),
        })
        .collect()
}

fn channel_list(store: &ChatStore) -> Vec<String> {
    store
        .server_channels("srv")
        .into_iter()
        .flat_map(|section| section.channels)
        .map(|entry| entry.id)
        .collect()
}

#[test]
fn renames_recolours_and_hoisting_follow_live() {
    let mut store = store();
    assert_eq!(
        lines(&store),
        [
            "# Admin (2)",
            "ada",
            "bob",
            "# Online (3)",
            "cy",
            "me",
            "owner"
        ]
    );
    assert_eq!(
        store.role_colour("srv", "u-ada").as_deref(),
        Some("#ff0000")
    );

    // A rename sends only the name: the colour stays.
    assert!(apply(
        &mut store,
        role_update("admin", json!({ "name": "Admins" }), &[])
    ));
    assert_eq!(lines(&store)[0], "# Admins (2)");
    assert_eq!(
        store.role_colour("srv", "u-ada").as_deref(),
        Some("#ff0000")
    );

    apply(
        &mut store,
        role_update("admin", json!({ "colour": "#00ff00" }), &[]),
    );
    assert_eq!(
        store.role_colour("srv", "u-ada").as_deref(),
        Some("#00ff00")
    );
    // Cleared: Ada has no other role; Bob's next one (Mods, gold) shows.
    apply(&mut store, role_update("admin", json!({}), &["Colour"]));
    assert_eq!(store.role_colour("srv", "u-ada"), None);
    assert_eq!(
        store.role_colour("srv", "u-bob").as_deref(),
        Some("#ffd700")
    );

    // Unhoisted: Ada joins Online, and Bob falls to his next hoisted role.
    apply(
        &mut store,
        role_update("admin", json!({ "hoist": false }), &[]),
    );
    assert_eq!(
        lines(&store),
        [
            "# Mods (1)",
            "bob",
            "# Online (4)",
            "ada",
            "cy",
            "me",
            "owner"
        ]
    );
    apply(
        &mut store,
        role_update("admin", json!({ "hoist": true }), &[]),
    );
    assert_eq!(lines(&store)[..3], ["# Admins (2)", "ada", "bob"]);
}

#[test]
fn a_new_role_arrives_as_a_role_update_and_follows_the_whole_pipeline() {
    let mut store = store();
    // What creating a role sends: the whole role, ranked last.
    let created = role_update(
        "r-new",
        json!({ "_id": "r-new", "name": "Newbies", "permissions": { "a": 0, "d": 0 }, "hoist": false, "rank": 3 }),
        &[],
    );
    assert!(apply(&mut store, created));
    let new = &store.server("srv").unwrap().roles["r-new"];
    assert_eq!(
        (new.name.as_str(), new.rank, new.hoist),
        ("Newbies", 3, false)
    );
    assert_eq!(new.id, "r-new");

    // Hoisted and coloured in the web app, then given to Cy.
    apply(
        &mut store,
        role_update("r-new", json!({ "hoist": true, "colour": "blue" }), &[]),
    );
    apply(
        &mut store,
        json!({
            "type": "ServerMemberUpdate", "id": { "server": "srv", "user": "u-cy" },
            "data": { "roles": ["r-new"] }, "clear": []
        }),
    );
    assert_eq!(
        lines(&store),
        [
            "# Admin (2)",
            "ada",
            "bob",
            "# Newbies (1)",
            "cy",
            "# Online (2)",
            "me",
            "owner"
        ]
    );
    assert_eq!(store.role_colour("srv", "u-cy").as_deref(), Some("#0000ff"));

    // Dragged to the top.
    apply(
        &mut store,
        json!({ "type": "ServerRoleRanksUpdate", "id": "srv", "ranks": ["r-new", "admin", "mods", "plain"] }),
    );
    assert_eq!(lines(&store)[..2], ["# Newbies (1)", "cy"]);

    // Deleted: the section goes, and Cy is back under Online.
    assert!(apply(
        &mut store,
        json!({ "type": "ServerRoleDelete", "id": "srv", "role_id": "r-new" })
    ));
    assert_eq!(
        lines(&store),
        [
            "# Admin (2)",
            "ada",
            "bob",
            "# Online (3)",
            "cy",
            "me",
            "owner"
        ]
    );
    assert_eq!(store.role_colour("srv", "u-cy"), None);
}

#[test]
fn a_new_role_without_a_name_is_ignored() {
    let mut store = store();
    assert!(!apply(
        &mut store,
        role_update("r-mystery", json!({ "colour": "red" }), &[])
    ));
    assert!(!store.server("srv").unwrap().roles.contains_key("r-mystery"));
    // Nor does an unknown server do anything.
    assert!(!apply(
        &mut store,
        json!({ "type": "ServerRoleUpdate", "id": "srv-other", "role_id": "admin", "data": { "name": "X" } })
    ));
}

#[test]
fn a_new_role_without_a_rank_goes_last() {
    let mut store = store();
    apply(
        &mut store,
        role_update("r-new", json!({ "name": "Late" }), &[]),
    );
    let new = &store.server("srv").unwrap().roles["r-new"];
    assert_eq!(new.rank, 3);
    assert_eq!((new.permissions.a, new.permissions.d), (0, 0));
}

#[test]
fn a_deleted_role_is_gone_from_members_overrides_sections_and_colours() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "ServerRoleDelete", "id": "srv", "role_id": "admin" }),
    );
    assert!(!store.server("srv").unwrap().roles.contains_key("admin"));
    assert!(store.member("srv", "u-ada").unwrap().roles.is_empty());
    assert_eq!(store.member("srv", "u-bob").unwrap().roles, ["mods"]);
    let Some(Channel::TextChannel {
        role_permissions, ..
    }) = store.channel("ch-test")
    else {
        panic!("ch-test");
    };
    assert!(role_permissions.is_empty());
    assert_eq!(
        lines(&store),
        [
            "# Mods (1)",
            "bob",
            "# Online (4)",
            "ada",
            "cy",
            "me",
            "owner"
        ]
    );
    assert_eq!(store.role_colour("srv", "u-ada"), None);
    assert_eq!(
        store.role_colour("srv", "u-bob").as_deref(),
        Some("#ffd700")
    );
}

#[test]
fn a_ranks_update_reorders_sections_and_changes_which_colour_wins() {
    let mut store = store();
    assert_eq!(
        store.role_colour("srv", "u-bob").as_deref(),
        Some("#ff0000")
    );
    apply(
        &mut store,
        json!({ "type": "ServerRoleRanksUpdate", "id": "srv", "ranks": ["mods", "admin", "plain"] }),
    );
    // Bob is listed under his highest hoisted role, now Mods.
    assert_eq!(
        lines(&store),
        [
            "# Mods (1)",
            "bob",
            "# Admin (1)",
            "ada",
            "# Online (3)",
            "cy",
            "me",
            "owner"
        ]
    );
    assert_eq!(
        store.role_colour("srv", "u-bob").as_deref(),
        Some("#ffd700")
    );
}

#[test]
fn a_role_permission_change_flips_pinning_and_viewing() {
    let mut store = store();
    assert!(!can_pin(&store, "u-me", "ch-open"));
    assert!(!store.can_pin("ch-open"));

    // `PUT /servers/{id}/permissions/{role}`.
    apply(
        &mut store,
        role_update(
            "plain",
            json!({ "permissions": { "a": MANAGE, "d": 0 } }),
            &[],
        ),
    );
    assert!(can_pin(&store, "u-me", "ch-open"));
    assert!(store.can_pin("ch-open"));

    apply(
        &mut store,
        role_update(
            "plain",
            json!({ "permissions": { "a": 0, "d": VIEW } }),
            &[],
        ),
    );
    assert!(!can_view(&store, "u-me", "ch-open"));
    assert!(!store.can_pin("ch-open"));
    // Other people's views don't change.
    assert!(can_view(&store, "u-cy", "ch-open"));
}

#[test]
fn default_permissions_from_a_server_update_flip_viewing() {
    let mut store = store();
    assert!(can_view(&store, "u-me", "ch-open"));
    // `PUT /servers/{id}/permissions/default`.
    assert!(apply(
        &mut store,
        json!({ "type": "ServerUpdate", "id": "srv", "data": { "default_permissions": 0 } })
    ));
    assert!(!can_view(&store, "u-me", "ch-open"));
    assert!(!can_view(&store, "u-cy", "ch-open"));
    // The owner sees everything regardless.
    assert!(can_view(&store, "u-owner", "ch-open"));
    // Nobody but the owner is left in the list.
    assert_eq!(lines(&store), ["# Online (1)", "owner"]);
}

#[test]
fn a_server_update_merges_and_clears_fields() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "ServerUpdate", "id": "srv", "data": { "name": "Renamed", "description": "hi" } }),
    );
    let server = store.server("srv").unwrap();
    assert_eq!(server.name, "Renamed");
    assert_eq!(server.description.as_deref(), Some("hi"));
    // Untouched fields are kept.
    assert_eq!(server.roles.len(), 3);
    assert_eq!(store.servers()[0].name, "Renamed");

    apply(
        &mut store,
        json!({ "type": "ServerUpdate", "id": "srv", "data": {}, "clear": ["Description"] }),
    );
    assert_eq!(store.server("srv").unwrap().description, None);
    assert!(!apply(
        &mut store,
        json!({ "type": "ServerUpdate", "id": "srv-gone", "data": { "name": "X" } })
    ));
}

#[test]
fn server_channels_hide_a_channel_a_role_edit_hides() {
    let mut store = store();
    assert_eq!(channel_list(&store), ["ch-open", "ch-test"]);
    assert!(store.shows_channel("ch-test"));

    // The user's only role is denied viewing.
    apply(
        &mut store,
        role_update(
            "plain",
            json!({ "permissions": { "a": 0, "d": VIEW } }),
            &[],
        ),
    );
    assert!(channel_list(&store).is_empty());
    assert!(!store.shows_channel("ch-open"));

    // Undone: both come back (they were never forgotten).
    apply(
        &mut store,
        role_update("plain", json!({ "permissions": { "a": 0, "d": 0 } }), &[]),
    );
    assert_eq!(channel_list(&store), ["ch-open", "ch-test"]);
}

/// A guard on the permission maths: Stoat sends only channels the user can
/// see in `Ready`, so filtering must keep every one of them.
#[test]
fn every_server_channel_in_ready_stays_visible() {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let Some(ServerEvent::Ready(ready)) =
        parse_frame(&std::fs::read_to_string(path).unwrap()).pop()
    else {
        panic!("ready");
    };
    let in_ready: Vec<String> = ready
        .channels
        .iter()
        .filter(|channel| matches!(channel, Channel::TextChannel { .. }))
        .map(|channel| channel_id(channel).to_string())
        .collect();
    let mut fixture = ChatStore::new("u-me");
    fixture.apply_ready(*ready);
    for channel in &in_ready {
        assert!(fixture.shows_channel(channel), "{channel} was hidden");
    }
    let listed: Vec<String> = fixture
        .servers()
        .iter()
        .flat_map(|server| fixture.server_channels(&server.id))
        .flat_map(|section| section.channels)
        .map(|entry| entry.id)
        .collect();
    for channel in &in_ready {
        assert!(listed.contains(channel), "{channel} isn't listed");
    }
    // And the store used above.
    assert_eq!(channel_list(&store()), ["ch-open", "ch-test"]);
}
