//! What a person's profile card shows (`profile::profile_card`).

use chrono::{TimeZone, Utc};
use revolt_permissions::DEFAULT_PERMISSION_SERVER;
use serde_json::{Value, json};
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{MutualResponse, RelationshipStatus};
use stoat_gpui::profile::{
    CardRole, MutualKind, RelationshipAction, UserMenuItem, can_message, dm_with, friend_buttons,
    mutuals, overflow_text, profile_card, shows_mutuals, user_menu,
};
use stoat_gpui::store::{ChatStore, PresenceKind};

/// 2026-01-01T00:00:00Z as a ULID prefix, so "account created" is known.
const ADA: &str = "01KDVDNA00ADAADAADAADAADAA";

fn user(id: &str, extra: Value) -> Value {
    let mut user = json!({
        "_id": id, "username": id.trim_start_matches("u-").to_lowercase(), "discriminator": "1234",
        "relationship": "None", "online": true
    });
    user.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    user
}

fn member(user: &str, roles: &[&str], extra: Value) -> Value {
    let mut member = json!({
        "_id": { "server": "srv", "user": user },
        "joined_at": "2026-03-04T10:00:00.000Z",
        "roles": roles,
    });
    member
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    member
}

fn role(id: &str, name: &str, rank: i64, colour: Option<&str>) -> Value {
    let mut role =
        json!({ "_id": id, "name": name, "rank": rank, "permissions": { "a": 0, "d": 0 } });
    if let Some(colour) = colour {
        role["colour"] = json!(colour);
    }
    role
}

fn store() -> ChatStore {
    store_owned_by("u-me")
}

/// [`store`], with its server owned by `owner`.
fn store_owned_by(owner: &str) -> ChatStore {
    let ready = json!({
        "type": "Ready",
        "users": [
            user("u-me", json!({})),
            user(ADA, json!({
                "username": "ada", "display_name": "Ada Lovelace", "pronouns": "she/her",
                "status": { "text": "  counting  ", "presence": "Busy" }
            })),
            user("u-bob", json!({ "online": false, "relationship": "Friend" })),
            user("u-bot", json!({ "bot": { "owner": "u-me" }, "status": { "text": "   " } })),
            user("u-plain", json!({})),
            user("u-blocked", json!({ "relationship": "Blocked" })),
            user("u-closed", json!({}))
        ],
        "servers": [{
            "_id": "srv", "owner": owner, "name": "Test Server", "channels": ["ch"],
            "default_permissions": *DEFAULT_PERMISSION_SERVER as i64,
            "approximate_member_count": 4,
            "roles": {
                "admin": role("admin", "Admin", 0, Some("#ff0000")),
                "fancy": role("fancy", "Fancy", 1, Some("linear-gradient(90deg, red, blue)")),
                "plain": role("plain", "Plain", 2, None)
            }
        }],
        "channels": [
            { "channel_type": "TextChannel", "_id": "ch", "server": "srv", "name": "general" },
            { "channel_type": "DirectMessage", "_id": "dm", "active": true, "recipients": ["u-me", ADA] },
            // Never messaged, so closed.
            { "channel_type": "DirectMessage", "_id": "dm-closed", "active": false, "recipients": ["u-closed", "u-me"] },
            {
                "channel_type": "Group", "_id": "grp", "name": "Group", "owner": "u-me",
                "recipients": ["u-me", ADA, "u-bob"]
            }
        ],
        "members": [
            member("u-me", &[], json!({})),
            member(ADA, &["plain", "admin", "fancy", "deleted-role"], json!({
                "nickname": "Countess", "pronouns": "they/them"
            })),
            member("u-bob", &[], json!({ "nickname": "   " })),
            member("u-plain", &[], json!({})),
            member("u-blocked", &[], json!({}))
        ]
    });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

fn role_names(roles: &[CardRole]) -> Vec<&str> {
    roles.iter().map(|r| r.name.as_str()).collect()
}

#[test]
fn in_a_server_the_nickname_and_server_pronouns_win() {
    let card = profile_card(&store(), ADA, Some("ch")).unwrap();
    assert_eq!(card.name, "Countess");
    assert_eq!(card.pronouns.as_deref(), Some("they/them"));
    assert_eq!(card.username_tag, "ada#1234");
    assert_eq!(card.status.as_deref(), Some("counting"));
    assert_eq!(card.presence, PresenceKind::Busy);
    assert!(!card.bot && !card.is_me);
}

#[test]
fn elsewhere_the_display_name_and_own_pronouns_show() {
    for channel in [Some("dm"), Some("grp"), None] {
        let card = profile_card(&store(), ADA, channel).unwrap();
        assert_eq!(card.name, "Ada Lovelace", "{channel:?}");
        assert_eq!(card.pronouns.as_deref(), Some("she/her"));
    }
    // A blank nickname falls back too, then to the username.
    let bob = profile_card(&store(), "u-bob", Some("ch")).unwrap();
    assert_eq!(bob.name, "bob");
    assert_eq!(bob.pronouns, None);
    assert_eq!(bob.presence, PresenceKind::Offline);
    assert_eq!(bob.relationship, RelationshipStatus::Friend);
}

#[test]
fn roles_are_highest_first_with_solid_colours_only() {
    let card = profile_card(&store(), ADA, Some("ch")).unwrap();
    // The unknown role is left out.
    assert_eq!(role_names(&card.roles), ["Admin", "Fancy", "Plain"]);
    assert_eq!(card.roles[0].colour.as_deref(), Some("#ff0000"));
    // A gradient can't be a dot.
    assert_eq!(card.roles[1].colour, None);
    assert_eq!(card.roles[2].colour, None);
}

#[test]
fn roles_follow_a_ranks_change() {
    let mut store = store();
    store.apply_role_ranks("srv", &["plain".into(), "fancy".into(), "admin".into()]);
    let card = profile_card(&store, ADA, Some("ch")).unwrap();
    assert_eq!(role_names(&card.roles), ["Plain", "Fancy", "Admin"]);
}

#[test]
fn there_are_no_roles_or_server_join_date_outside_server_channels() {
    for channel in [Some("dm"), Some("grp"), None] {
        let card = profile_card(&store(), ADA, channel).unwrap();
        assert!(card.roles.is_empty(), "{channel:?}");
        assert_eq!(card.joined_server, None);
    }
}

#[test]
fn join_dates_come_from_the_membership_and_the_id() {
    let card = profile_card(&store(), ADA, Some("ch")).unwrap();
    assert_eq!(
        card.joined_server,
        Some((
            "Test Server".to_string(),
            Utc.with_ymd_and_hms(2026, 3, 4, 10, 0, 0).unwrap()
        ))
    );
    assert_eq!(
        card.created,
        Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap())
    );
    // "u-bob" isn't a ULID: no creation date.
    assert_eq!(profile_card(&store(), "u-bob", None).unwrap().created, None);
}

#[test]
fn bots_are_flagged_and_blank_statuses_hidden() {
    let card = profile_card(&store(), "u-bot", None).unwrap();
    assert!(card.bot);
    assert_eq!(card.status, None);
}

#[test]
fn the_users_own_card_says_so() {
    assert!(profile_card(&store(), "u-me", Some("ch")).unwrap().is_me);
}

#[test]
fn an_unknown_user_has_no_card() {
    assert_eq!(profile_card(&store(), "u-nobody", Some("ch")), None);
}

#[test]
fn message_is_offered_for_friends_bots_and_existing_dms() {
    let store = store();
    // Bob is a friend, with no DM yet.
    assert!(can_message(&store, "u-bob"));
    assert!(can_message(&store, "u-bot"));
    // Not friends, but there's a DM (open, or closed).
    assert!(can_message(&store, ADA));
    assert!(can_message(&store, "u-closed"));
}

#[test]
fn message_isnt_offered_for_yourself_strangers_or_the_blocked() {
    let store = store();
    assert!(!can_message(&store, "u-me"));
    // Sharing a server isn't enough: Stoat refuses a DM with a non-friend.
    assert!(!can_message(&store, "u-plain"));
    assert!(!can_message(&store, "u-blocked"));
    assert!(!can_message(&store, "u-nobody"));
}

#[test]
fn the_dm_with_someone_is_found_open_or_closed() {
    let store = store();
    assert_eq!(dm_with(&store, ADA).as_deref(), Some("dm"));
    assert_eq!(dm_with(&store, "u-closed").as_deref(), Some("dm-closed"));
    assert_eq!(dm_with(&store, "u-bob"), None);
    // Not Saved Notes, nor a group.
    assert_eq!(dm_with(&store, "u-me"), None);
}

mod menu {
    use super::*;
    use RelationshipAction::*;
    use UserMenuItem::*;

    #[test]
    fn a_friend_in_a_server_channel_has_every_item_in_order() {
        assert_eq!(
            user_menu(&store(), "u-bob", Some("ch")),
            [
                Profile,
                Message,
                CreateGroupWith,
                Mention,
                Separator,
                Kick,
                Ban,
                Separator,
                Relationship(RemoveFriend),
                Relationship(Block),
                Separator,
                CopyUsername,
                CopyUserId
            ]
        );
    }

    #[test]
    fn mention_is_for_server_channels_and_groups_only() {
        let store = store();
        assert!(user_menu(&store, ADA, Some("grp")).contains(&Mention));
        assert!(!user_menu(&store, ADA, Some("dm")).contains(&Mention));
        assert!(!user_menu(&store, ADA, None).contains(&Mention));
    }

    #[test]
    fn create_group_with_is_for_friends_only() {
        let store = store();
        assert!(user_menu(&store, "u-bob", None).contains(&CreateGroupWith));
        for user in [ADA, "u-plain", "u-bot", "u-blocked", "u-me"] {
            assert!(
                !user_menu(&store, user, Some("ch")).contains(&CreateGroupWith),
                "{user}"
            );
        }
        // Becoming friends offers it.
        let mut store = store;
        store.apply_relationship(
            serde_json::from_value(user("u-plain", json!({ "relationship": "Friend" }))).unwrap(),
        );
        assert!(user_menu(&store, "u-plain", Some("ch")).contains(&CreateGroupWith));
    }

    #[test]
    fn your_own_menu_has_no_message_or_mention() {
        assert_eq!(
            user_menu(&store(), "u-me", Some("ch")),
            [Profile, Separator, CopyUsername, CopyUserId]
        );
    }

    #[test]
    fn message_follows_can_message() {
        let store = store();
        assert!(!user_menu(&store, "u-plain", Some("ch")).contains(&Message));
        assert!(user_menu(&store, "u-bot", Some("ch")).contains(&Message));
        assert_eq!(
            user_menu(&store, "u-plain", Some("ch")),
            [
                Profile,
                Mention,
                Separator,
                Kick,
                Ban,
                Separator,
                Relationship(AddFriend),
                Separator,
                Relationship(Block),
                Separator,
                CopyUsername,
                CopyUserId
            ]
        );
    }

    #[test]
    fn the_group_owner_can_remove_other_members() {
        let store = store();
        assert_eq!(
            user_menu(&store, "u-bob", Some("grp")),
            [
                Profile,
                Message,
                CreateGroupWith,
                Mention,
                Separator,
                RemoveFromGroup,
                MakeGroupOwner,
                Separator,
                Relationship(RemoveFriend),
                Relationship(Block),
                Separator,
                CopyUsername,
                CopyUserId
            ]
        );
        assert!(user_menu(&store, ADA, Some("grp")).contains(&RemoveFromGroup));
        // Not themselves, nor anyone outside the group.
        assert!(!user_menu(&store, "u-me", Some("grp")).contains(&RemoveFromGroup));
        assert!(!user_menu(&store, "u-plain", Some("grp")).contains(&RemoveFromGroup));
        // Only in the group's own menus.
        for channel in [Some("ch"), Some("dm"), None] {
            assert!(
                !user_menu(&store, "u-bob", channel).contains(&RemoveFromGroup),
                "{channel:?}"
            );
        }
    }

    #[test]
    fn a_plain_member_of_a_group_removes_nobody() {
        let mut store = store();
        store.add_channel(
            serde_json::from_value(json!({
                "channel_type": "Group", "_id": "grp-ada", "name": "Ada's", "owner": ADA,
                "recipients": [ADA, "u-me", "u-bob"]
            }))
            .unwrap(),
        );
        for user in [ADA, "u-bob", "u-me"] {
            let menu = user_menu(&store, user, Some("grp-ada"));
            assert!(!menu.contains(&RemoveFromGroup), "{user}");
            assert!(!menu.contains(&MakeGroupOwner), "{user}");
        }
    }

    #[test]
    fn the_group_owner_can_hand_the_group_to_another_member() {
        let mut store = store();
        assert!(user_menu(&store, "u-bob", Some("grp")).contains(&MakeGroupOwner));
        assert!(user_menu(&store, ADA, Some("grp")).contains(&MakeGroupOwner));
        // Not themselves, nor anyone outside the group or in another channel.
        assert!(!user_menu(&store, "u-me", Some("grp")).contains(&MakeGroupOwner));
        assert!(!user_menu(&store, "u-plain", Some("grp")).contains(&MakeGroupOwner));
        for channel in [Some("ch"), Some("dm"), None] {
            assert!(
                !user_menu(&store, "u-bob", channel).contains(&MakeGroupOwner),
                "{channel:?}"
            );
        }
        // Once handed on, neither membership item is the user's any more.
        assert!(store.set_group_owner("grp", "u-bob"));
        for user in [ADA, "u-bob"] {
            let menu = user_menu(&store, user, Some("grp"));
            assert!(!menu.contains(&RemoveFromGroup), "{user}");
            assert!(!menu.contains(&MakeGroupOwner), "{user}");
        }
    }

    #[test]
    fn kick_and_ban_are_for_server_channels_with_the_permission() {
        let store = store();
        // The user owns the server, so may kick and ban anyone else in it.
        for user in [ADA, "u-plain", "u-bot"] {
            let menu = user_menu(&store, user, Some("ch"));
            assert!(menu.contains(&Kick) && menu.contains(&Ban), "{user}");
        }
        // Not themselves, nor outside a server.
        assert!(!user_menu(&store, "u-me", Some("ch")).contains(&Kick));
        for channel in [Some("dm"), Some("grp"), None] {
            let menu = user_menu(&store, ADA, channel);
            assert!(!menu.contains(&Kick) && !menu.contains(&Ban), "{channel:?}");
        }
        // A plain member has neither permission, and nobody acts on the owner.
        let store = store_owned_by("u-plain");
        for user in [ADA, "u-plain", "u-bob"] {
            let menu = user_menu(&store, user, Some("ch"));
            assert!(!menu.contains(&Kick) && !menu.contains(&Ban), "{user}");
        }
    }

    #[test]
    fn only_kick_and_ban_are_destructive() {
        assert!(Kick.is_destructive() && Ban.is_destructive());
        assert!(!Profile.is_destructive() && !Relationship(Block).is_destructive());
    }

    /// `u-plain`, related to the user by `relationship`.
    fn related(relationship: &str) -> ChatStore {
        let mut store = store();
        store.apply_relationship(
            serde_json::from_value(user("u-plain", json!({ "relationship": relationship })))
                .unwrap(),
        );
        store
    }

    /// The relationship items in `u-plain`'s menu.
    fn relationship_items(store: &ChatStore) -> Vec<RelationshipAction> {
        user_menu(store, "u-plain", Some("ch"))
            .into_iter()
            .filter_map(|item| match item {
                Relationship(action) => Some(action),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn relationship_items_and_card_buttons_follow_the_relationship() {
        let cases: [(&str, &[RelationshipAction], &[RelationshipAction]); 6] = [
            ("None", &[AddFriend, Block], &[AddFriend]),
            (
                "Incoming",
                &[AcceptFriend, RejectFriend, Block],
                &[AcceptFriend, RejectFriend],
            ),
            ("Outgoing", &[CancelFriend, Block], &[CancelFriend]),
            ("Friend", &[RemoveFriend, Block], &[]),
            ("Blocked", &[Unblock], &[]),
            // Blocked by them: they can still be blocked back.
            ("BlockedOther", &[Block], &[]),
        ];
        for (relationship, menu, buttons) in cases {
            let store = related(relationship);
            assert_eq!(relationship_items(&store), menu, "{relationship}");
            assert_eq!(friend_buttons(&store, "u-plain"), buttons, "{relationship}");
            let card = profile_card(&store, "u-plain", Some("ch")).unwrap();
            assert_eq!(card.friend_buttons, buttons, "{relationship}");
        }
    }

    #[test]
    fn yourself_and_bots_have_no_friend_items() {
        let store = store();
        // "User" is how Stoat marks the user themselves.
        assert!(
            !user_menu(&store, "u-me", Some("ch"))
                .iter()
                .any(|i| matches!(i, Relationship(_)))
        );
        assert!(friend_buttons(&store, "u-me").is_empty());
        // A bot can be blocked, not friended.
        let bot: Vec<_> = user_menu(&store, "u-bot", Some("ch"))
            .into_iter()
            .filter(|i| matches!(i, Relationship(_)))
            .collect();
        assert_eq!(bot, [Relationship(Block)]);
        assert!(friend_buttons(&store, "u-bot").is_empty());
    }

    #[test]
    fn only_remove_friend_and_block_are_confirmed() {
        let confirmed: Vec<_> = [
            AddFriend,
            AcceptFriend,
            RejectFriend,
            CancelFriend,
            RemoveFriend,
            Block,
            Unblock,
        ]
        .into_iter()
        .filter(|a| a.needs_confirming())
        .collect();
        assert_eq!(confirmed, [RemoveFriend, Block]);
    }
}

fn mutual(users: &[&str], servers: &[&str], channels: &[&str]) -> MutualResponse {
    let owned = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect();
    MutualResponse {
        users: owned(users),
        servers: owned(servers),
        channels: owned(channels),
    }
}

#[test]
fn mutuals_are_friends_then_servers_then_groups() {
    let store = store();
    let shared = mutuals(&store, &mutual(&["u-bob", ADA], &["srv"], &["dm", "grp"]));
    let friends: Vec<_> = shared
        .friends
        .iter()
        .map(|f| (f.user_id.as_str(), f.name.as_str()))
        .collect();
    assert_eq!(friends, [("u-bob", "bob"), (ADA, "Ada Lovelace")]);
    assert_eq!(shared.friends[1].avatar_url, store.user_avatar_url(ADA));
    let places: Vec<_> = shared
        .places
        .iter()
        .map(|p| (p.id.as_str(), p.name.as_str(), p.kind, p.icon_url.clone()))
        .collect();
    assert_eq!(
        places,
        [
            ("srv", "Test Server", MutualKind::Server, None),
            ("grp", "Group", MutualKind::Group, None)
        ]
    );
}

#[test]
fn mutuals_leave_out_dms_server_channels_and_what_isnt_known() {
    let store = store();
    let shared = mutuals(
        &store,
        &mutual(
            &["u-gone", "u-bob"],
            &["srv-gone", "srv"],
            &["dm", "ch", "grp-gone", "grp"],
        ),
    );
    let friends: Vec<_> = shared.friends.iter().map(|f| f.user_id.as_str()).collect();
    assert_eq!(friends, ["u-bob"]);
    let places: Vec<_> = shared.places.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(places, ["srv", "grp"]);
    assert_eq!(
        mutuals(&store, &mutual(&[], &[], &["dm"])),
        Default::default()
    );
}

#[test]
fn mutuals_are_not_capped() {
    let store = store();
    let shared = mutuals(&store, &mutual(&["u-bob"; 12], &["srv"; 9], &[]));
    assert_eq!((shared.friends.len(), shared.places.len()), (12, 9));
}

#[test]
fn the_overflow_counts_the_rest() {
    assert_eq!(overflow_text(3), "+3");
    assert_eq!(overflow_text(42), "+42");
}

#[test]
fn mutuals_show_for_others_but_not_yourself_bots_or_strangers() {
    let store = store();
    assert!(shows_mutuals(&store, ADA));
    assert!(shows_mutuals(&store, "u-blocked"));
    assert!(!shows_mutuals(&store, "u-me"));
    assert!(!shows_mutuals(&store, "u-bot"));
    assert!(!shows_mutuals(&store, "u-unknown"));
}
