//! The right-hand panel's member list: sections, order, presence, colours
//! and who is left out.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::store::{ChatStore, MemberListRow, PresenceKind};

const VIEW: u64 = ChannelPermission::ViewChannel as u64;

fn user(id: &str, extra: Value) -> Value {
    let mut user = json!({
        "_id": id, "username": id.trim_start_matches("u-"), "discriminator": "0001",
        "relationship": "None", "online": true
    });
    user.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    user
}

fn member(user: &str, roles: &[&str], nickname: Option<&str>) -> Value {
    let mut member = json!({
        "_id": { "server": "srv", "user": user },
        "joined_at": "2026-01-01T00:00:00.000Z",
        "roles": roles,
    });
    if let Some(nickname) = nickname {
        member["nickname"] = json!(nickname);
    }
    member
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

fn ready(approximate_member_count: usize) -> Value {
    json!({
        "type": "Ready",
        "users": [
            user("u-me", json!({})),
            user("u-owner", json!({ "display_name": "Owner" })),
            // Nickname "zed" in the server, though their name is "ada".
            user("u-ada", json!({})),
            user("u-bob", json!({})),
            user("u-cy", json!({})),
            user("u-dee", json!({})),
            user("u-eve", json!({ "display_name": "Eve" })),
            user("u-fay", json!({ "online": false })),
            user("u-gil", json!({ "status": { "presence": "Invisible" } })),
            user("u-hal", json!({ "status": { "presence": "Busy", "text": "in a meeting" } })),
            user("u-ivy", json!({ "status": { "presence": "Idle" } })),
            user("u-jon", json!({ "status": { "presence": "Focus" } }))
        ],
        "servers": [{
            "_id": "srv", "owner": "u-owner", "name": "Test",
            "channels": ["ch-open", "ch-staff"],
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "approximate_member_count": approximate_member_count,
            "roles": {
                "admin": role("admin", "Admin", 1, true, Some("#ff0000")),
                "mods": role("mods", "Mods", 2, true, Some("linear-gradient(90deg, red, blue)")),
                "vip": role("vip", "VIP", 3, false, Some("gold")),
                "plain": role("plain", "Plain", 4, false, None)
            }
        }],
        "channels": [
            { "channel_type": "TextChannel", "_id": "ch-open", "server": "srv", "name": "open" },
            {
                "channel_type": "TextChannel", "_id": "ch-staff", "server": "srv", "name": "staff",
                "default_permissions": { "a": 0, "d": VIEW },
                "role_permissions": { "admin": { "a": VIEW, "d": 0 } }
            },
            { "channel_type": "DirectMessage", "_id": "dm", "active": true, "recipients": ["u-me", "u-ada"] },
            { "channel_type": "SavedMessages", "_id": "saved", "user": "u-me" },
            {
                "channel_type": "Group", "_id": "grp", "name": "Group", "owner": "u-me",
                "recipients": ["u-me", "u-eve", "u-bob", "u-fay"]
            }
        ],
        "members": [
            member("u-me", &[], None),
            member("u-owner", &[], None),
            member("u-ada", &["admin"], Some("zed")),
            member("u-bob", &["mods", "admin"], None),
            member("u-cy", &["mods"], None),
            member("u-dee", &["vip", "plain"], None),
            member("u-eve", &[], None),
            member("u-fay", &["admin"], None),
            member("u-gil", &["admin"], None),
            member("u-hal", &[], None),
            member("u-ivy", &[], None),
            member("u-jon", &[], None)
        ]
    })
}

fn store(approximate_member_count: usize) -> ChatStore {
    let Some(ServerEvent::Ready(ready)) =
        parse_frame(&ready(approximate_member_count).to_string()).pop()
    else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

/// The rows as text: "# Section (count)" and "name".
fn lines(store: &ChatStore, channel: &str) -> Vec<String> {
    store
        .member_list(channel)
        .rows
        .iter()
        .map(|row| match row {
            MemberListRow::Header { name, count } => format!("# {name} ({count})"),
            MemberListRow::Member { name, .. } => name.clone(),
        })
        .collect()
}

fn member_row(store: &ChatStore, channel: &str, id: &str) -> MemberListRow {
    store
        .member_list(channel)
        .rows
        .into_iter()
        .find(|row| matches!(row, MemberListRow::Member { user_id, .. } if user_id == id))
        .unwrap_or_else(|| panic!("{id} is not listed"))
}

#[test]
fn hoisted_roles_come_first_in_rank_order_then_online_then_offline() {
    let store = store(12);
    assert_eq!(
        lines(&store, "ch-open"),
        [
            "# Admin (2)",
            "bob",
            "zed",
            "# Mods (1)",
            "cy",
            "# Online (7)",
            "dee",
            "Eve",
            "hal",
            "ivy",
            "jon",
            "me",
            "Owner",
            "# Offline (2)",
            "fay",
            "gil",
        ]
    );
}

#[test]
fn an_online_member_goes_under_the_first_hoisted_role_they_hold() {
    // bob has `mods` (rank 2) and `admin` (rank 1), and vip/plain aren't hoisted.
    let store = store(12);
    let lines = lines(&store, "ch-open");
    let admin = lines.iter().position(|l| l == "# Admin (2)").unwrap();
    assert_eq!(lines[admin + 1], "bob");
    assert!(!lines.iter().skip(admin + 3).any(|l| l == "bob"));
}

#[test]
fn offline_members_go_under_offline_whatever_their_role() {
    let store = store(12);
    let lines = lines(&store, "ch-open");
    let offline = lines.iter().position(|l| l == "# Offline (2)").unwrap();
    // fay has the admin role, gil is online but invisible.
    assert_eq!(&lines[offline + 1..], ["fay", "gil"]);
}

#[test]
fn rows_are_sorted_by_nickname_else_display_name_else_username() {
    let store = store(12);
    // ada is "zed" here, so she sorts after bob; "Eve" sorts among lowercase names.
    let lines = lines(&store, "ch-open");
    let admin = lines.iter().position(|l| l == "# Admin (2)").unwrap();
    assert_eq!(&lines[admin + 1..admin + 3], ["bob", "zed"]);
    let online = lines
        .iter()
        .position(|l| l.starts_with("# Online"))
        .unwrap();
    assert_eq!(
        &lines[online + 1..online + 8],
        ["dee", "Eve", "hal", "ivy", "jon", "me", "Owner"]
    );
}

#[test]
fn presence_and_status_come_from_the_user() {
    let store = store(12);
    let MemberListRow::Member {
        presence,
        status,
        online,
        ..
    } = member_row(&store, "ch-open", "u-hal")
    else {
        unreachable!()
    };
    assert_eq!(presence, PresenceKind::Busy);
    assert_eq!(status, "in a meeting");
    assert!(online);

    // No custom status: the presence's name.
    for (id, expected, label) in [
        ("u-ivy", PresenceKind::Idle, "Idle"),
        ("u-jon", PresenceKind::Focus, "Focus"),
        ("u-eve", PresenceKind::Online, "Online"),
    ] {
        let MemberListRow::Member {
            presence, status, ..
        } = member_row(&store, "ch-open", id)
        else {
            unreachable!()
        };
        assert_eq!((presence, status.as_str()), (expected, label), "{id}");
    }

    // Offline, and online but invisible, both show as offline.
    for id in ["u-fay", "u-gil"] {
        let MemberListRow::Member {
            presence, online, ..
        } = member_row(&store, "ch-open", id)
        else {
            unreachable!()
        };
        assert_eq!((presence, online), (PresenceKind::Offline, false), "{id}");
    }
}

#[test]
fn a_restricted_channel_leaves_out_members_who_cannot_view_it() {
    // ch-staff is hidden from everyone but `admin` (and the owner): cy (mods),
    // dee, eve and the rest can't see it.
    let store = store(12);
    assert_eq!(
        lines(&store, "ch-staff"),
        [
            "# Admin (2)",
            "bob",
            "zed",
            "# Online (1)",
            "Owner",
            "# Offline (2)",
            "fay",
            "gil",
        ]
    );
}

#[test]
fn a_large_server_lists_only_online_members() {
    let large = store(1001);
    let list = large.member_list("ch-open");
    assert!(list.offline_hidden);
    let lines = lines(&large, "ch-open");
    assert!(!lines.iter().any(|l| l.starts_with("# Offline")));
    assert!(!lines.contains(&"fay".to_string()));
    assert!(!lines.contains(&"gil".to_string()));
    assert!(lines.contains(&"cy".to_string()));
    // 1000 is still small.
    assert!(!store(1000).member_list("ch-open").offline_hidden);
}

#[test]
fn a_group_is_one_flat_name_sorted_list_titled_with_its_size() {
    let store = store(12);
    let list = store.member_list("grp");
    assert_eq!(list.title, "4 members");
    assert!(!list.offline_hidden);
    assert_eq!(lines(&store, "grp"), ["bob", "Eve", "fay", "me"]);
    let MemberListRow::Member { online, colour, .. } = member_row(&store, "grp", "u-fay") else {
        unreachable!()
    };
    assert!(!online);
    assert_eq!(colour, None);
}

#[test]
fn dms_and_saved_notes_have_no_member_list() {
    let store = store(12);
    for channel in ["dm", "saved", "nope"] {
        assert!(!store.has_member_list(channel), "{channel}");
        assert!(store.member_list(channel).rows.is_empty(), "{channel}");
    }
    assert!(store.has_member_list("ch-open"));
    assert!(store.has_member_list("grp"));
}

#[test]
fn a_server_list_is_titled_members() {
    assert_eq!(store(12).member_list("ch-open").title, "Members");
}

#[test]
fn role_colour_is_the_highest_ranked_role_that_has_one() {
    let store = store(12);
    // bob has mods (a gradient) and admin (red): admin ranks higher.
    assert_eq!(
        store.role_colour("srv", "u-bob").as_deref(),
        Some("#ff0000")
    );
    // dee has vip (gold, rank 3) and plain (no colour, rank 4).
    assert_eq!(
        store.role_colour("srv", "u-dee").as_deref(),
        Some("#ffd700")
    );
    // No roles at all.
    assert_eq!(store.role_colour("srv", "u-eve"), None);
    // Not a member.
    assert_eq!(store.role_colour("srv", "u-nobody"), None);
    assert_eq!(store.role_colour("nope", "u-bob"), None);

    let MemberListRow::Member { colour, .. } = member_row(&store, "ch-open", "u-ada") else {
        unreachable!()
    };
    assert_eq!(colour.as_deref(), Some("#ff0000"));
}

#[test]
fn a_gradient_falls_back_to_the_default_colour() {
    // cy's only coloured role is the gradient: the next role down isn't tried.
    let store = store(12);
    assert_eq!(store.role_colour("srv", "u-cy"), None);
    let MemberListRow::Member { colour, .. } = member_row(&store, "ch-open", "u-cy") else {
        unreachable!()
    };
    assert_eq!(colour, None);
}

#[test]
fn solid_css_colours_are_normalised_to_hex() {
    for (css, expected) in [
        ("#abc", Some("#aabbcc")),
        ("#0A1B2C", Some("#0a1b2c")),
        ("rgb(1, 2, 3)", Some("#010203")),
        ("red", Some("#ff0000")),
        ("not a colour", None),
        ("radial-gradient(red, blue)", None),
    ] {
        let mut ready = ready(12);
        ready["servers"][0]["roles"]["admin"]["colour"] = json!(css);
        let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
            panic!("ready");
        };
        let mut store = ChatStore::new("u-me");
        store.apply_ready(*ready);
        assert_eq!(
            store.role_colour("srv", "u-ada").as_deref(),
            expected,
            "{css}"
        );
    }
}

#[test]
fn lists_member_follows_the_servers_members_and_a_groups_recipients() {
    let store = store(12);
    assert!(store.lists_member("ch-open", "u-ada"));
    assert!(!store.lists_member("ch-open", "u-nobody"));
    assert!(store.lists_member("grp", "u-eve"));
    assert!(!store.lists_member("grp", "u-ada"));
    assert!(!store.lists_member("dm", "u-ada"));
}

#[test]
fn live_presence_changes_move_a_member_between_sections() {
    use stoat_gpui::api::models::PartialUser;
    let mut store = store(12);
    assert!(lines(&store, "ch-open").contains(&"# Offline (2)".to_string()));
    let data: PartialUser = serde_json::from_value(json!({ "online": true })).unwrap();
    store.apply_user_update("u-fay", data, &[]);
    let lines = lines(&store, "ch-open");
    assert!(lines.contains(&"# Admin (3)".to_string()));
    assert!(lines.contains(&"# Offline (1)".to_string()));
}
