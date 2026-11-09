//! A server's settings: who sees which page, and what the Overview sends.

use revolt_permissions::{ChannelPermission, DEFAULT_PERMISSION_SERVER};
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{FieldsServer, Invite, Server};
use stoat_gpui::profile_edit::ProfileImage;
use stoat_gpui::server_settings::{
    Access, OverviewForm, ServerImage, SystemChannels, SystemMessageKind, access, ban_rows,
    empty_server_edit, filter_bans, invite_rows, system_channel_choices, uses_label,
};
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
            "_id": "srv", "owner": "u-owner", "name": "Test",
            "channels": ["ch-b", "ch-voice", "ch-a"],
            "categories": [{ "id": "k", "title": "Chat", "channels": ["ch-a", "ch-voice"] }],
            "system_messages": { "user_joined": "ch-a", "user_left": "ch-gone" },
            "approximate_member_count": 4,
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "roles": {
                "admin": { "_id": "admin", "name": "Admin", "rank": 0, "permissions": { "a": MANAGE_SERVER, "d": 0 } },
                "banner": { "_id": "banner", "name": "Banner", "rank": 1, "permissions": { "a": BAN, "d": 0 } }
            }
        }],
        "channels": [
            { "channel_type": "TextChannel", "_id": "ch-a", "server": "srv", "name": "a" },
            { "channel_type": "TextChannel", "_id": "ch-voice", "server": "srv", "name": "talk", "voice": {} },
            { "channel_type": "TextChannel", "_id": "ch-b", "server": "srv", "name": "b" }
        ],
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
        ..OverviewForm::default()
    }
}

/// `srv` with an icon (`f-icon`) and a banner (`f-banner`).
fn server_with_images() -> Server {
    let file = |id: &str, tag: &str| {
        json!({
            "_id": id, "tag": tag, "filename": "x.png",
            "metadata": { "type": "Image", "width": 1, "height": 1 },
            "content_type": "image/png", "size": 1
        })
    };
    serde_json::from_value(json!({
        "_id": "srv", "owner": "u-owner", "name": "Test", "channels": [],
        "default_permissions": 0, "approximate_member_count": 1,
        "icon": file("f-icon", "icons"), "banner": file("f-banner", "banners"),
    }))
    .unwrap()
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

#[test]
fn the_form_starts_with_the_servers_images() {
    let form = OverviewForm::of(&server_with_images());
    assert_eq!(form.icon, ProfileImage::Current("f-icon".into()));
    assert_eq!(form.banner, ProfileImage::Current("f-banner".into()));
    assert_eq!(OverviewForm::of(&server(None)).icon, ProfileImage::None);
}

#[test]
fn a_new_icon_is_sent_by_its_id_without_a_remove() {
    let server = server_with_images();
    let mut form = OverviewForm::of(&server);
    form.icon = ProfileImage::Current("f-new".into());
    let edit = form.changes(&server).unwrap();
    assert_eq!(edit.icon.as_deref(), Some("f-new"));
    assert!(edit.banner.is_none() && edit.remove.is_empty());
}

#[test]
fn a_removed_banner_is_in_remove() {
    let server = server_with_images();
    let mut form = OverviewForm::of(&server);
    form.banner = ProfileImage::None;
    let edit = form.changes(&server).unwrap();
    assert_eq!(edit.remove, [FieldsServer::Banner]);
    assert!(edit.icon.is_none() && edit.banner.is_none());
    // Removing both, and the description too.
    form.icon = ProfileImage::None;
    let edit = form.changes(&server).unwrap();
    assert_eq!(edit.remove, [FieldsServer::Icon, FieldsServer::Banner]);
}

#[test]
fn unchanged_images_are_left_out() {
    let with_images = server_with_images();
    let mut form = OverviewForm::of(&with_images);
    form.name = "Renamed".into();
    let edit = form.changes(&with_images).unwrap();
    assert!(edit.icon.is_none() && edit.banner.is_none() && edit.remove.is_empty());
    // Removing an image there isn't: nothing to do.
    let bare = server(None);
    let mut form = OverviewForm::of(&bare);
    form.banner = ProfileImage::None;
    assert!(!form.has_changes(&bare));
}

#[test]
fn a_picked_image_waits_for_its_upload() {
    let server = server_with_images();
    let mut form = OverviewForm::of(&server);
    form.banner = ProfileImage::New("/tmp/banner.png".into());
    assert!(form.has_changes(&server));
    assert_eq!(form.changes(&server), None);
    assert_eq!(
        form.new_files(),
        [(ServerImage::Banner, std::path::Path::new("/tmp/banner.png"))]
    );
}

#[test]
fn server_images_have_their_own_tags_and_limits() {
    use stoat_gpui::profile_edit::ImageLimits;
    let limits = ImageLimits::default();
    assert_eq!(ServerImage::Icon.tag(), "icons");
    assert_eq!(ServerImage::Banner.tag(), "banners");
    assert_eq!(ServerImage::Icon.limit(limits), 2_500_000);
    assert_eq!(ServerImage::Banner.limit(limits), 6_000_000);
}

/// `srv` as the store fixture has it: channels `ch-b`, `ch-voice` and
/// `ch-a`, with joins going to `ch-a` and leaves to a deleted channel.
fn system_server() -> Server {
    store("u-owner").server("srv").unwrap().clone()
}

#[test]
fn a_deleted_channel_shows_as_off() {
    let form = OverviewForm::of(&system_server());
    assert_eq!(
        form.system,
        SystemChannels {
            user_joined: Some("ch-a".into()),
            ..SystemChannels::default()
        }
    );
    assert_eq!(form.system.get(SystemMessageKind::Left), None);
    // Untouched, so nothing is sent.
    assert_eq!(form.changes(&system_server()), None);
}

#[test]
fn setting_one_sends_the_whole_object_without_deleted_channels() {
    let server = system_server();
    let mut form = OverviewForm::of(&server);
    form.system
        .set(SystemMessageKind::Banned, Some("ch-voice".into()));
    let edit = form.changes(&server).unwrap();
    let sent = edit.system_messages.unwrap();
    // Joins stay where they were; the deleted channel isn't sent.
    assert_eq!(sent.user_joined.as_deref(), Some("ch-a"));
    assert_eq!(sent.user_left, None);
    assert_eq!(sent.user_kicked, None);
    assert_eq!(sent.user_banned.as_deref(), Some("ch-voice"));
    assert!(edit.remove.is_empty() && edit.name.is_none());
    // As Stoat reads it: a missing key is off.
    assert_eq!(
        serde_json::to_value(&sent).unwrap(),
        json!({ "user_joined": "ch-a", "user_banned": "ch-voice" })
    );
}

#[test]
fn all_off_removes_system_messages() {
    let server = system_server();
    let mut form = OverviewForm::of(&server);
    form.system.set(SystemMessageKind::Joined, None);
    let edit = form.changes(&server).unwrap();
    assert_eq!(edit.system_messages, None);
    assert_eq!(edit.remove, [FieldsServer::SystemMessages]);
}

#[test]
fn a_channel_picked_then_deleted_is_not_sent() {
    let server = system_server();
    let mut form = OverviewForm::of(&server);
    form.system
        .set(SystemMessageKind::Kicked, Some("ch-gone".into()));
    assert_eq!(form.changes(&server), None);
}

#[test]
fn choices_follow_the_channel_list_and_include_voice_channels() {
    let store = store("u-owner");
    let choices = system_channel_choices(&store, "srv");
    let ids: Vec<&str> = choices.iter().map(|(id, _)| id.as_str()).collect();
    // Uncategorised first, then the category's order.
    assert_eq!(ids, ["ch-b", "ch-a", "ch-voice"]);
    assert_eq!(choices[2].1, "talk");
    assert!(system_channel_choices(&store, "srv-unknown").is_empty());
}

#[test]
fn system_message_labels() {
    let labels = SystemMessageKind::ALL.map(SystemMessageKind::label);
    assert_eq!(
        labels,
        ["User joined", "User left", "User kicked", "User banned"]
    );
}

fn invites(value: Value) -> Vec<Invite> {
    serde_json::from_value(value).unwrap()
}

/// 2026-11-09T12:00:00Z.
fn noon() -> chrono::DateTime<chrono::Utc> {
    "2026-11-09T12:00:00Z".parse().unwrap()
}

#[test]
fn invite_rows_name_the_creator_and_channel() {
    let store = store("u-owner");
    let rows = invite_rows(
        &store,
        &invites(json!([
            { "type": "Server", "_id": "abc", "server": "srv", "creator": "u-admin", "channel": "ch-a", "uses": 3 },
            { "type": "Server", "_id": "def", "server": "srv", "creator": "u-stranger", "channel": "ch-gone",
              "uses": 3, "max_uses": 10, "expires": "2026-11-12T12:00:00.000Z" }
        ])),
        noon(),
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].code, "abc");
    assert_eq!(rows[0].creator_name, "u-admin");
    assert!(rows[0].creator_avatar.is_some());
    assert_eq!(rows[0].channel, "#a");
    assert_eq!(rows[0].uses, "3 uses");
    assert_eq!(rows[0].expiry, "Never expires");
    // Someone the store doesn't know, and a channel it doesn't have.
    assert_eq!(rows[1].creator_name, "Unknown user");
    assert_eq!(rows[1].creator_avatar, None);
    assert_eq!(rows[1].channel, "Unknown channel");
    assert_eq!(rows[1].uses, "3 / 10 uses");
    assert_eq!(rows[1].expiry, "Expires in 3 days");
}

#[test]
fn expired_and_group_invites_are_left_out() {
    let store = store("u-owner");
    let rows = invite_rows(
        &store,
        &invites(json!([
            { "type": "Server", "_id": "old", "server": "srv", "creator": "u-admin", "channel": "ch-a",
              "uses": 0, "expires": "2026-11-09T11:59:00.000Z" },
            { "type": "Group", "_id": "grp", "creator": "u-admin", "channel": "grp-1", "uses": 0 },
            { "type": "Server", "_id": "new", "server": "srv", "creator": "u-admin", "channel": "ch-a", "uses": 0 }
        ])),
        noon(),
    );
    let codes: Vec<&str> = rows.iter().map(|row| row.code.as_str()).collect();
    assert_eq!(codes, ["new"]);
}

#[test]
fn uses_read_naturally() {
    assert_eq!(uses_label(1, None), "1 use");
    assert_eq!(uses_label(0, None), "0 uses");
    assert_eq!(uses_label(3, Some(10)), "3 / 10 uses");
}

fn ban_list() -> stoat_gpui::api::models::BanListResult {
    serde_json::from_value(json!({
        "users": [
            { "_id": "u-zed", "username": "zed", "discriminator": "0002" },
            {
                "_id": "u-ada", "username": "Ada", "discriminator": "0001",
                "avatar": {
                    "_id": "f-ada", "tag": "avatars", "filename": "a.png",
                    "metadata": { "type": "Image", "width": 1, "height": 1 },
                    "content_type": "image/png", "size": 1
                }
            }
        ],
        "bans": [
            { "_id": { "server": "srv", "user": "u-zed" }, "reason": "Posting SPAM links" },
            { "_id": { "server": "srv", "user": "u-ada" }, "reason": "  " },
            { "_id": { "server": "srv", "user": "u-gone" } }
        ]
    }))
    .unwrap()
}

#[test]
fn bans_are_joined_with_their_users_and_sorted() {
    let rows = ban_rows(&store("u-owner"), &ban_list());
    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    // A ban without its user keeps its row, under the ID.
    assert_eq!(names, ["Ada#0001", "u-gone", "zed#0002"]);
    assert_eq!(rows[2].reason.as_deref(), Some("Posting SPAM links"));
    // A blank reason is no reason.
    assert_eq!(rows[0].reason, None);
    assert_eq!(rows[1].user, "u-gone");
    assert_eq!(rows[1].avatar, None);
}

#[test]
fn bans_filter_by_name_or_reason_ignoring_case() {
    let rows = ban_rows(&store("u-owner"), &ban_list());
    let names = |query: &str| -> Vec<String> {
        filter_bans(&rows, query)
            .into_iter()
            .map(|row| row.name.clone())
            .collect()
    };
    assert_eq!(names("ADA"), ["Ada#0001"]);
    assert_eq!(names("spam"), ["zed#0002"]);
    assert_eq!(names("  "), ["Ada#0001", "u-gone", "zed#0002"]);
    assert!(names("nobody").is_empty());
}
