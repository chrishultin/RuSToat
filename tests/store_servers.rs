//! Servers joined or created (`ServerCreate`) and deleted (`ServerDelete`)
//! while the client runs, and who owns them.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{
    BulkMessageResponse, CreateServerLegacyResponse, InviteJoinResponse, Member,
};
use stoat_gpui::rail::{Ordering, RailEntry, ServerFolders};
use stoat_gpui::store::ChatStore;

const VIEW: u64 = ChannelPermission::ViewChannel as u64;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn store() -> ChatStore {
    let Some(ServerEvent::Ready(ready)) = parse_frame(&fixture("ready.json")).pop() else {
        panic!("ready fixture");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

/// A `ServerCreate` for `srv-new`, owned by someone else: `ch-lobby`, and
/// `ch-staff`, which only the `staff` role may see.
fn server_create(name: &str) -> Value {
    json!({
        "type": "ServerCreate", "id": "srv-new",
        "server": {
            "_id": "srv-new", "owner": "u-ada", "name": name,
            "channels": ["ch-lobby", "ch-staff"],
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "approximate_member_count": 2,
            "roles": { "staff": { "_id": "staff", "name": "Staff", "permissions": { "a": 0, "d": 0 }, "rank": 0 } }
        },
        "channels": [
            { "channel_type": "TextChannel", "_id": "ch-lobby", "server": "srv-new", "name": "lobby" },
            {
                "channel_type": "TextChannel", "_id": "ch-staff", "server": "srv-new", "name": "staff",
                "default_permissions": { "a": 0, "d": VIEW },
                "role_permissions": { "staff": { "a": VIEW, "d": 0 } }
            }
        ],
        "emojis": [{
            "_id": "01ARYZ6S41TSV4RRFFQ69G5FAV", "parent": { "type": "Server", "id": "srv-new" },
            "creator_id": "u-ada", "name": "newmoji"
        }],
        "voice_states": []
    })
}

fn add(store: &mut ChatStore, event: Value) -> bool {
    let Some(ServerEvent::ServerCreate {
        server,
        channels,
        emojis,
        ..
    }) = parse_frame(&event.to_string()).pop()
    else {
        panic!("ServerCreate");
    };
    store.add_server(*server, channels, emojis)
}

fn channel_list(store: &ChatStore, server: &str) -> Vec<String> {
    store
        .server_channels(server)
        .into_iter()
        .flat_map(|section| section.channels)
        .map(|entry| entry.id)
        .collect()
}

#[test]
fn a_joined_server_goes_at_the_end_of_the_rail_with_its_channels_and_emoji() {
    let mut store = store();
    let before: Vec<String> = store.servers().into_iter().map(|s| s.id).collect();
    assert!(add(&mut store, server_create("New Place")));

    let after: Vec<String> = store.servers().into_iter().map(|s| s.id).collect();
    assert_eq!(after[..before.len()], before[..]);
    assert_eq!(after.last().map(String::as_str), Some("srv-new"));
    assert_eq!(store.servers().last().unwrap().name, "New Place");
    assert!(store.channel("ch-lobby").is_some());
    assert!(store.emoji("01ARYZ6S41TSV4RRFFQ69G5FAV").is_some());
}

#[test]
fn channels_are_not_filtered_until_the_membership_is_known() {
    let mut store = store();
    add(&mut store, server_create("New Place"));
    // Nothing to work permissions out from yet: everything sent shows.
    assert_eq!(channel_list(&store, "srv-new"), ["ch-lobby", "ch-staff"]);

    // The membership arrives (`GET /servers/{id}/members/{me}`), without
    // the staff role: the staff channel goes.
    let member: Member = serde_json::from_value(json!({
        "_id": { "server": "srv-new", "user": "u-me" },
        "joined_at": "2026-11-05T12:00:00.000Z"
    }))
    .unwrap();
    store.apply_member_join(member);
    assert_eq!(channel_list(&store, "srv-new"), ["ch-lobby"]);
}

#[test]
fn a_repeated_server_create_replaces_the_server_and_keeps_history() {
    let mut store = store();
    add(&mut store, server_create("New Place"));
    let page: BulkMessageResponse = serde_json::from_value(json!([
        { "_id": "01M6GDKW80AAAAAAAAAAAAAAAA", "channel": "ch-lobby", "author": "u-ada", "content": "welcome" }
    ]))
    .unwrap();
    store.set_history("ch-lobby", page, 50);

    assert!(!add(&mut store, server_create("Renamed Place")));
    let ids: Vec<String> = store.servers().into_iter().map(|s| s.id).collect();
    assert_eq!(ids.iter().filter(|id| *id == "srv-new").count(), 1);
    assert_eq!(store.server("srv-new").unwrap().name, "Renamed Place");
    assert_eq!(store.history("ch-lobby").unwrap().messages.len(), 1);
}

#[test]
fn a_deleted_server_goes_with_its_channels() {
    let mut store = store();
    add(&mut store, server_create("New Place"));
    let Some(ServerEvent::ServerDelete { id }) =
        parse_frame(r#"{"type":"ServerDelete","id":"srv-new"}"#).pop()
    else {
        panic!("ServerDelete");
    };
    // `ChatView` hands it to `leave_server`, which does this.
    let mut removed = store.remove_server(&id);
    removed.sort();
    assert_eq!(removed, ["ch-lobby", "ch-staff"]);
    assert!(!store.servers().iter().any(|s| s.id == "srv-new"));
}

/// What `POST /invites/{code}` answers for `srv-new`: the server and the
/// channels the user can see, with no emoji (its `ServerCreate` has them).
fn join_response() -> InviteJoinResponse {
    let event = server_create("New Place");
    serde_json::from_value(json!({
        "type": "Server",
        "server": event["server"],
        "channels": [event["channels"][0]]
    }))
    .unwrap()
}

fn add_joined(store: &mut ChatStore) -> bool {
    let InviteJoinResponse::Server { channels, server } = join_response() else {
        panic!("a server join");
    };
    store.add_server(server, channels, Vec::new())
}

fn rail_count(store: &ChatStore, id: &str) -> usize {
    store.servers().into_iter().filter(|s| s.id == id).count()
}

#[test]
fn a_join_response_then_its_server_create_gives_one_server_with_emoji() {
    let mut store = store();
    assert!(add_joined(&mut store));
    assert_eq!(
        store.servers().last().map(|s| s.id.as_str()),
        Some("srv-new")
    );
    assert!(store.channel("ch-lobby").is_some());

    assert!(!add(&mut store, server_create("New Place")));
    assert_eq!(rail_count(&store, "srv-new"), 1);
    assert!(store.emoji("01ARYZ6S41TSV4RRFFQ69G5FAV").is_some());
    assert!(store.channel("ch-staff").is_some());
}

#[test]
fn a_server_create_then_the_join_response_keeps_the_emoji() {
    let mut store = store();
    assert!(add(&mut store, server_create("New Place")));
    assert!(!add_joined(&mut store));
    assert_eq!(rail_count(&store, "srv-new"), 1);
    assert!(store.emoji("01ARYZ6S41TSV4RRFFQ69G5FAV").is_some());
    assert_eq!(
        store.servers().last().map(|s| s.id.as_str()),
        Some("srv-new")
    );
}

/// What `POST /servers/create` answers: the new server, owned by the
/// user, with no categories, and its one channel, "General".
fn created_json() -> Value {
    json!({
        "server": {
            "_id": "srv-mine", "owner": "u-me", "name": "Lounge",
            "channels": ["ch-general"],
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "approximate_member_count": 1
        },
        "channels": [
            { "channel_type": "TextChannel", "_id": "ch-general", "server": "srv-mine", "name": "General" }
        ]
    })
}

fn add_created(store: &mut ChatStore) -> bool {
    let created: CreateServerLegacyResponse = serde_json::from_value(created_json()).unwrap();
    store.add_server(created.server, created.channels, Vec::new())
}

/// The `ServerCreate` Stoat sends the creator after the response.
fn created_event() -> Value {
    let created = created_json();
    json!({
        "type": "ServerCreate", "id": "srv-mine",
        "server": created["server"], "channels": created["channels"],
        "emojis": [], "voice_states": []
    })
}

#[test]
fn a_create_response_then_its_server_create_gives_one_server_with_general() {
    let mut store = store();
    assert!(add_created(&mut store));
    assert!(!add(&mut store, created_event()));
    assert_eq!(rail_count(&store, "srv-mine"), 1);
    assert_eq!(
        store.servers().last().map(|s| s.id.as_str()),
        Some("srv-mine")
    );
    assert_eq!(channel_list(&store, "srv-mine"), ["ch-general"]);
}

#[test]
fn a_server_create_then_the_create_response_gives_one_server_with_general() {
    let mut store = store();
    assert!(add(&mut store, created_event()));
    assert!(!add_created(&mut store));
    assert_eq!(rail_count(&store, "srv-mine"), 1);
    assert_eq!(
        store.servers().last().map(|s| s.id.as_str()),
        Some("srv-mine")
    );
    assert_eq!(channel_list(&store, "srv-mine"), ["ch-general"]);
}

fn update(store: &mut ChatStore, event: Value) {
    let Some(ServerEvent::ServerUpdate { id, data, clear }) = parse_frame(&event.to_string()).pop()
    else {
        panic!("ServerUpdate");
    };
    store.apply_server_update(&id, *data, &clear);
}

#[test]
fn the_user_owns_only_their_own_servers() {
    let mut store = store();
    assert!(store.is_owner("srv-beta"));
    assert!(!store.is_owner("srv-alpha"));
    assert!(!store.is_owner("srv-gone"));
    add_created(&mut store);
    assert!(store.is_owner("srv-mine"));
    add_joined(&mut store);
    assert!(!store.is_owner("srv-new"));
}

#[test]
fn ownership_follows_a_server_update() {
    let mut store = store();
    // Alpha is handed to the user and Beta handed away. Whether Leave is
    // offered and sent hangs on this: for the owner, it deletes the server.
    update(
        &mut store,
        json!({ "type": "ServerUpdate", "id": "srv-alpha", "data": { "owner": "u-me" } }),
    );
    assert!(store.is_owner("srv-alpha"));
    update(
        &mut store,
        json!({ "type": "ServerUpdate", "id": "srv-beta", "data": { "owner": "u-ada" } }),
    );
    assert!(!store.is_owner("srv-beta"));
}

/// The fixture's `ordering` was written at this time.
const READY_ORDERING: i64 = 1730000000000;

fn rail(store: &ChatStore) -> Vec<String> {
    store.servers().into_iter().map(|s| s.id).collect()
}

fn ordering(value: Value) -> Ordering {
    Ordering::parse(&value.to_string())
}

fn folders(value: Value) -> ServerFolders {
    ServerFolders::parse(&value.to_string())
}

#[test]
fn the_rail_follows_the_sidebar_over_the_legacy_order() {
    let mut store = store();
    assert_eq!(rail(&store), ["srv-beta", "srv-alpha"], "Ready's `servers`");
    // The web client's newer order, fetched after Ready.
    let newer = ordering(json!({
        "servers": ["srv-beta", "srv-alpha"],
        "serverSidebar": ["srv-alpha", "srv-beta"]
    }));
    assert!(store.set_ordering(READY_ORDERING + 1, newer));
    assert_eq!(rail(&store), ["srv-alpha", "srv-beta"]);
    assert_eq!(store.ordering_updated(), READY_ORDERING + 1);
}

#[test]
fn servers_in_a_folder_are_drawn_in_its_place() {
    let mut store = store();
    add(&mut store, server_create("New Place"));
    store.set_ordering(
        READY_ORDERING + 1,
        ordering(json!({ "servers": [], "serverSidebar": ["srv-beta", "folder-1", "srv-new"] })),
    );
    assert!(store.set_server_folders(
        5,
        folders(json!({ "folders": [
            { "id": "folder-1", "name": "Pair", "servers": ["srv-new", "srv-alpha"] }
        ] }))
    ));
    let entries = store.rail_entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], RailEntry::Server("srv-beta".into()));
    let RailEntry::Folder { folder, servers } = &entries[1] else {
        panic!("a folder, got {entries:?}");
    };
    assert_eq!(folder.name, "Pair");
    assert_eq!(servers, &["srv-new", "srv-alpha"]);
    // For now the rail draws them as plain servers.
    assert_eq!(rail(&store), ["srv-beta", "srv-new", "srv-alpha"]);
}

#[test]
fn a_joined_server_goes_last_outside_any_folder() {
    let mut store = store();
    store.set_ordering(
        READY_ORDERING + 1,
        ordering(json!({ "servers": [], "serverSidebar": ["folder-1", "srv-beta", "srv-alpha"] })),
    );
    store.set_server_folders(
        5,
        folders(json!({ "folders": [{ "id": "folder-1", "servers": ["srv-beta", "srv-alpha"] }] })),
    );
    add(&mut store, server_create("New Place"));
    let entries = store.rail_entries();
    assert_eq!(entries.last(), Some(&RailEntry::Server("srv-new".into())));
    assert_eq!(entries.len(), 2);
    // Nothing is written for it: the settings held are as they were.
    assert_eq!(store.ordering_updated(), READY_ORDERING + 1);
    assert_eq!(store.server_folders_updated(), 5);
}

#[test]
fn older_copies_of_the_order_and_folders_are_ignored() {
    let mut store = store();
    let reversed = || ordering(json!({ "servers": ["srv-alpha", "srv-beta"] }));
    assert!(!store.set_ordering(READY_ORDERING - 1, reversed()));
    assert_eq!(rail(&store), ["srv-beta", "srv-alpha"]);
    assert_eq!(store.ordering_updated(), READY_ORDERING);

    let one = || folders(json!({ "folders": [{ "id": "folder-1", "servers": ["srv-alpha"] }] }));
    assert!(store.set_server_folders(10, one()));
    assert!(!store.set_server_folders(9, ServerFolders::default()));
    assert_eq!(store.server_folders(), &one());
    // The echo of what's held, at the same time, changes nothing.
    assert!(!store.set_server_folders(10, one()));
    // The same copy stamped earlier (Stoat clamping a clock that runs
    // ahead) moves the held time back, so other clients' changes apply.
    assert!(!store.set_server_folders(8, one()));
    assert_eq!(store.server_folders_updated(), 8);
    assert!(store.set_server_folders(9, ServerFolders::default()));
}

#[test]
fn a_new_ready_brings_the_order_and_keeps_the_folders() {
    let mut store = store();
    let one = folders(json!({ "folders": [{ "id": "folder-1", "servers": ["srv-alpha"] }] }));
    store.set_server_folders(10, one.clone());
    store.set_ordering(
        READY_ORDERING + 5,
        ordering(json!({ "servers": ["srv-alpha"] })),
    );
    let Some(ServerEvent::Ready(ready)) = parse_frame(&fixture("ready.json")).pop() else {
        panic!("ready fixture");
    };
    store.apply_ready(*ready);
    assert_eq!(store.ordering_updated(), READY_ORDERING);
    assert_eq!(store.ordering().servers(), ["srv-beta", "srv-alpha"]);
    // `Ready` can't carry `server-folders`; they're fetched after it.
    assert_eq!(store.server_folders(), &one);
    assert_eq!(store.server_folders_updated(), 10);

    // Stoat sends `Ready` without `ordering`, even when asked: the order
    // held stays until the fetch after it lands.
    let Some(ServerEvent::Ready(mut ready)) = parse_frame(&fixture("ready.json")).pop() else {
        panic!("ready fixture");
    };
    ready.ordering = None;
    store.apply_ready(*ready);
    assert_eq!(store.ordering_updated(), READY_ORDERING);
    assert_eq!(store.ordering().servers(), ["srv-beta", "srv-alpha"]);
}

#[test]
fn a_folder_whose_servers_are_all_gone_is_not_drawn() {
    let mut store = store();
    store.set_server_folders(
        5,
        folders(json!({ "folders": [{ "id": "folder-1", "servers": ["srv-alpha"] }] })),
    );
    store.remove_server("srv-alpha");
    assert_eq!(store.rail_entries(), [RailEntry::Server("srv-beta".into())]);
}
