//! Who's in each voice channel's call: from `Ready` and `ServerCreate`, kept
//! current by the voice events, and as the channel list shows it.

use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::store::ChatStore;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

/// A voice state as Stoat sends it, joined at `12:00:<second>`.
fn state(user: &str, second: u32, publishing: bool) -> Value {
    json!({
        "id": user, "joined_at": format!("2026-11-08T12:00:{second:02}.000Z"),
        "is_receiving": true, "is_publishing": publishing,
        "screensharing": false, "camera": false
    })
}

/// The `Ready` fixture with `voice_states` added.
fn store_with(voice_states: Value) -> ChatStore {
    let mut ready: Value = serde_json::from_str(&fixture("ready.json")).unwrap();
    ready["voice_states"] = voice_states;
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready fixture");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

fn store() -> ChatStore {
    store_with(json!([
        { "id": "ch-voice", "participants": [state("u-bob", 5, true), state("u-ada", 1, false)] }
    ]))
}

/// Apply one voice event frame to `store`.
fn apply(store: &mut ChatStore, frame: Value) {
    let event = parse_frame(&frame.to_string()).pop().expect("an event");
    match event {
        ServerEvent::VoiceChannelJoin { channel, state } => {
            store.apply_voice_join(&channel, *state)
        }
        ServerEvent::VoiceChannelLeave { channel, user } => {
            store.apply_voice_leave(&channel, &user);
        }
        ServerEvent::VoiceChannelMove {
            from, to, state, ..
        } => store.apply_voice_move(&from, &to, *state),
        ServerEvent::UserVoiceStateUpdate {
            user,
            channel,
            data,
        } => {
            store.apply_voice_update(&channel, &user, *data);
        }
        other => panic!("not a voice event: {other:?}"),
    }
}

fn ids(store: &ChatStore, channel: &str) -> Vec<String> {
    store
        .voice_participants(channel)
        .iter()
        .map(|p| p.id.clone())
        .collect()
}

#[test]
fn ready_fills_channels_in_the_order_people_joined() {
    let store = store();
    assert_eq!(ids(&store, "ch-voice"), ["u-ada", "u-bob"]);
    assert!(store.voice_participants("ch-general").is_empty());
}

#[test]
fn rows_show_names_avatars_and_mutes() {
    let store = store();
    let rows = store.voice_rows("ch-voice");
    assert_eq!(rows[0].name, "Ada L.");
    assert!(rows[0].muted, "not publishing shows as muted");
    assert_eq!(rows[1].name, "bob");
    assert!(!rows[1].muted);
    assert!(!rows[0].avatar_url.is_empty());
}

#[test]
fn the_users_own_row_uses_their_nickname() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "VoiceChannelJoin", "id": "ch-voice", "state": state("u-me", 9, true) }),
    );
    let rows = store.voice_rows("ch-voice");
    assert_eq!(rows.last().unwrap().name, "Captain Me");
}

#[test]
fn someone_not_loaded_shows_the_start_of_their_id_and_is_fetched() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "VoiceChannelJoin", "id": "ch-voice",
                "state": state("01JSTRANGERXXXXXXXXXXXXXXX", 9, true) }),
    );
    assert_eq!(store.voice_rows("ch-voice")[2].name, "01JSTRAN");
    assert_eq!(store.unknown_voice_users(), ["01JSTRANGERXXXXXXXXXXXXXXX"]);
}

#[test]
fn a_join_keeps_joined_at_order() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "VoiceChannelJoin", "id": "ch-voice", "state": state("u-me", 3, true) }),
    );
    assert_eq!(ids(&store, "ch-voice"), ["u-ada", "u-me", "u-bob"]);
}

#[test]
fn a_join_elsewhere_moves_the_user() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "VoiceChannelJoin", "id": "ch-other", "state": state("u-ada", 30, true) }),
    );
    assert_eq!(ids(&store, "ch-voice"), ["u-bob"]);
    assert_eq!(ids(&store, "ch-other"), ["u-ada"]);
}

#[test]
fn a_leave_removes_and_the_last_leave_empties_the_channel() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "VoiceChannelLeave", "id": "ch-voice", "user": "u-ada" }),
    );
    assert_eq!(ids(&store, "ch-voice"), ["u-bob"]);
    // A repeat, or someone who isn't there, changes nothing.
    assert!(!store.apply_voice_leave("ch-voice", "u-ada"));
    assert!(store.apply_voice_leave("ch-voice", "u-bob"));
    assert!(store.voice_participants("ch-voice").is_empty());
}

#[test]
fn a_move_takes_the_user_from_one_channel_to_another() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "VoiceChannelMove", "user": "u-bob", "from": "ch-voice",
                "to": "ch-other", "state": state("u-bob", 40, true) }),
    );
    assert_eq!(ids(&store, "ch-voice"), ["u-ada"]);
    assert_eq!(ids(&store, "ch-other"), ["u-bob"]);
}

#[test]
fn an_update_merges_only_the_fields_sent() {
    let mut store = store();
    apply(
        &mut store,
        json!({ "type": "UserVoiceStateUpdate", "id": "u-ada", "channel_id": "ch-voice",
                "data": { "is_publishing": true } }),
    );
    let ada = &store.voice_participants("ch-voice")[0];
    assert!(ada.is_publishing);
    assert!(ada.is_receiving, "untouched fields stay");
    // Someone not listed there is ignored.
    let data = serde_json::from_value(json!({ "camera": true })).unwrap();
    assert!(!store.apply_voice_update("ch-voice", "u-nobody", data));
}

#[test]
fn a_new_ready_replaces_everything() {
    let mut store = store();
    let mut ready: Value = serde_json::from_str(&fixture("ready.json")).unwrap();
    ready["voice_states"] =
        json!([{ "id": "ch-other", "participants": [state("u-ada", 1, true)] }]);
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready fixture");
    };
    store.apply_ready(*ready);
    assert!(store.voice_participants("ch-voice").is_empty());
    assert_eq!(ids(&store, "ch-other"), ["u-ada"]);
}

#[test]
fn server_create_adds_its_channels_states() {
    let mut store = store();
    let Some(ServerEvent::ServerCreate {
        server,
        channels,
        emojis,
        voice_states,
    }) = parse_frame(
        &json!({
            "type": "ServerCreate", "id": "srv-new",
            "server": {
                "_id": "srv-new", "owner": "u-ada", "name": "New",
                "channels": ["ch-new-voice"], "default_permissions": 0,
                "approximate_member_count": 2
            },
            "channels": [{ "channel_type": "TextChannel", "_id": "ch-new-voice",
                           "server": "srv-new", "name": "lounge", "voice": {} }],
            "emojis": [],
            "voice_states": [{ "id": "ch-new-voice", "participants": [state("u-bob", 2, true)] }]
        })
        .to_string(),
    )
    .pop()
    else {
        panic!("ServerCreate");
    };
    let id = server.id.clone();
    store.add_server(*server, channels, emojis);
    store.set_server_voice(&id, voice_states);
    assert_eq!(ids(&store, "ch-new-voice"), ["u-bob"]);
    assert_eq!(
        ids(&store, "ch-voice"),
        ["u-ada", "u-bob"],
        "other servers untouched"
    );
    // Again with nobody in it: the old list goes.
    store.set_server_voice(&id, Vec::new());
    assert!(store.voice_participants("ch-new-voice").is_empty());
}

#[test]
fn deleting_a_channel_or_leaving_its_server_drops_its_list() {
    let mut store = store();
    assert!(store.remove_channel("ch-voice"));
    assert!(store.voice_participants("ch-voice").is_empty());

    let mut store = store_with(json!([
        { "id": "ch-voice", "participants": [state("u-ada", 1, true)] }
    ]));
    store.remove_server("srv-alpha");
    assert!(store.voice_participants("ch-voice").is_empty());
}

#[test]
fn a_users_call_on_a_server_is_found() {
    let store = store();
    let server = store.channel_server("ch-voice").unwrap().to_string();
    assert_eq!(store.voice_channel_of(&server, "u-ada"), Some("ch-voice"));
    assert_eq!(store.voice_channel_of(&server, "u-carol"), None);
    assert_eq!(store.voice_channel_of("another-server", "u-ada"), None);
}
