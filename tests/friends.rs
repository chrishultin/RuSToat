//! The Friends page's lists (`friends::friend_lists`).

use serde_json::{Value, json};
use stoat_gpui::api::ApiError;
use stoat_gpui::api::events::{ServerEvent, parse_frame};
use stoat_gpui::api::models::{RelationshipStatus, User};
use stoat_gpui::friends::{
    AddFriendState, FriendLists, FriendsTab, RowAction, USERNAME_HINT, check_username,
    friend_lists, incoming_count, request_refused, request_sent, row_actions,
};
use stoat_gpui::profile::RelationshipAction;
use stoat_gpui::store::{ChatStore, PresenceKind};

fn user(id: &str, relationship: &str, extra: Value) -> Value {
    let mut user = json!({
        "_id": id, "username": id.trim_start_matches("u-"), "discriminator": "0001",
        "relationship": relationship, "online": true
    });
    user.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    user
}

fn store_with(users: Vec<Value>) -> ChatStore {
    let mut all = vec![user("u-me", "User", json!({}))];
    all.extend(users);
    let ready =
        json!({ "type": "Ready", "users": all, "servers": [], "channels": [], "members": [] });
    let Some(ServerEvent::Ready(ready)) = parse_frame(&ready.to_string()).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    store
}

fn ids(rows: &[stoat_gpui::friends::FriendRow]) -> Vec<&str> {
    rows.iter().map(|row| row.user_id.as_str()).collect()
}

fn relationship_event(user: Value) -> User {
    let frame = json!({ "type": "UserRelationship", "id": "u-me", "user": user });
    let Some(ServerEvent::UserRelationship { user }) = parse_frame(&frame.to_string()).pop() else {
        panic!("UserRelationship");
    };
    *user
}

#[test]
fn each_relationship_goes_in_its_list() {
    let store = store_with(vec![
        user("u-friend", "Friend", json!({})),
        user("u-away", "Friend", json!({ "online": false })),
        user("u-in", "Incoming", json!({})),
        user("u-out", "Outgoing", json!({})),
        user("u-blocked", "Blocked", json!({})),
        user("u-none", "None", json!({})),
        user("u-blocker", "BlockedOther", json!({})),
    ]);
    let lists = friend_lists(&store);
    assert_eq!(ids(&lists.all), ["u-away", "u-friend"]);
    assert_eq!(ids(&lists.online), ["u-friend"]);
    assert_eq!(ids(&lists.incoming), ["u-in"]);
    assert_eq!(ids(&lists.outgoing), ["u-out"]);
    assert_eq!(ids(&lists.blocked), ["u-blocked"]);
    // Strangers, people who blocked the user, and the user aren't listed.
    let everyone: Vec<&str> = [&lists.all, &lists.incoming, &lists.outgoing, &lists.blocked]
        .into_iter()
        .flat_map(|list| ids(list))
        .collect();
    for id in ["u-none", "u-blocker", "u-me"] {
        assert!(!everyone.contains(&id), "{id}");
    }
}

#[test]
fn an_invisible_friend_is_not_online() {
    let store = store_with(vec![
        user(
            "u-hidden",
            "Friend",
            json!({ "status": { "presence": "Invisible" } }),
        ),
        user(
            "u-busy",
            "Friend",
            json!({ "status": { "presence": "Busy", "text": " deep work " } }),
        ),
        user("u-plain", "Friend", json!({})),
    ]);
    let lists = friend_lists(&store);
    assert_eq!(ids(&lists.online), ["u-busy", "u-plain"]);
    let hidden = lists.all.iter().find(|r| r.user_id == "u-hidden").unwrap();
    assert_eq!(hidden.presence, PresenceKind::Offline);
    assert_eq!(hidden.status, "");
    // A custom status wins over the presence's name.
    assert_eq!(lists.online[0].status, "deep work");
    assert_eq!(lists.online[0].presence, PresenceKind::Busy);
    assert_eq!(lists.online[1].status, "Online");
}

#[test]
fn lists_are_sorted_by_name_ignoring_case_then_by_id() {
    let store = store_with(vec![
        user("u-3", "Friend", json!({ "display_name": "bea" })),
        user("u-2", "Friend", json!({ "display_name": "Bea" })),
        user("u-1", "Friend", json!({ "display_name": "Cy" })),
        // A blank display name falls back to the username.
        user("u-alf", "Friend", json!({ "display_name": "  " })),
    ]);
    let lists = friend_lists(&store);
    assert_eq!(ids(&lists.all), ["u-alf", "u-2", "u-3", "u-1"]);
    assert_eq!(lists.all[0].name, "alf");
}

#[test]
fn incoming_requests_are_counted() {
    let store = store_with(vec![
        user("u-a", "Incoming", json!({})),
        user("u-b", "Incoming", json!({ "online": false })),
        user("u-c", "Outgoing", json!({})),
        user("u-d", "Friend", json!({})),
    ]);
    assert_eq!(incoming_count(&store), 2);
    assert_eq!(incoming_count(&store_with(vec![])), 0);
}

#[test]
fn tabs_show_their_lists() {
    let lists = FriendLists::default();
    let titles = |tab| {
        lists
            .sections(tab)
            .into_iter()
            .map(|(title, _)| title)
            .collect::<Vec<_>>()
    };
    assert_eq!(titles(FriendsTab::Online), ["Online"]);
    assert_eq!(titles(FriendsTab::All), ["All"]);
    assert_eq!(titles(FriendsTab::Pending), ["Incoming", "Outgoing"]);
    assert_eq!(titles(FriendsTab::Blocked), ["Blocked"]);
    assert_eq!(FriendsTab::default(), FriendsTab::Online);
}

#[test]
fn a_relationship_event_moves_people_between_lists() {
    let mut store = store_with(vec![]);
    let carol = |relationship: &str| {
        relationship_event(json!({
            "_id": "u-carol", "username": "carol", "discriminator": "0003",
            "relationship": relationship, "online": true
        }))
    };
    store.apply_relationship(carol("Incoming"));
    assert_eq!(ids(&friend_lists(&store).incoming), ["u-carol"]);
    assert_eq!(incoming_count(&store), 1);

    store.apply_relationship(carol("Friend"));
    let lists = friend_lists(&store);
    assert!(lists.incoming.is_empty());
    assert_eq!(ids(&lists.all), ["u-carol"]);
    assert_eq!(ids(&lists.online), ["u-carol"]);
    assert_eq!(lists.all[0].relationship, RelationshipStatus::Friend);
    assert_eq!(incoming_count(&store), 0);

    store.apply_relationship(carol("Blocked"));
    let lists = friend_lists(&store);
    assert!(lists.all.is_empty() && lists.online.is_empty());
    assert_eq!(ids(&lists.blocked), ["u-carol"]);

    store.apply_relationship(carol("None"));
    assert_eq!(friend_lists(&store), FriendLists::default());
}

#[test]
fn the_ready_fixture_lists_its_friends() {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let Some(ServerEvent::Ready(ready)) = parse_frame(&text).pop() else {
        panic!("ready");
    };
    let mut store = ChatStore::new("u-me");
    store.apply_ready(*ready);
    let lists = friend_lists(&store);
    // Ada has a display name; Bob is offline.
    assert_eq!(ids(&lists.all), ["u-ada", "u-bob"]);
    assert_eq!(lists.all[0].name, "Ada L.");
    assert_eq!(ids(&lists.online), ["u-ada"]);
    assert!(lists.incoming.is_empty() && lists.outgoing.is_empty() && lists.blocked.is_empty());
    assert_eq!(incoming_count(&store), 0);
}

#[test]
fn each_row_gets_the_buttons_for_its_relationship() {
    use RelationshipAction::*;
    let store = store_with(vec![
        user("u-friend", "Friend", json!({ "online": false })),
        user("u-in", "Incoming", json!({})),
        user("u-out", "Outgoing", json!({})),
        user("u-blocked", "Blocked", json!({})),
        user("u-none", "None", json!({})),
        user("u-blocker", "BlockedOther", json!({})),
    ]);
    assert_eq!(row_actions(&store, "u-friend"), [RowAction::Message]);
    assert_eq!(
        row_actions(&store, "u-in"),
        [
            RowAction::Relationship(AcceptFriend),
            RowAction::Relationship(RejectFriend)
        ]
    );
    assert_eq!(
        row_actions(&store, "u-out"),
        [RowAction::Relationship(CancelFriend)]
    );
    assert_eq!(
        row_actions(&store, "u-blocked"),
        [RowAction::Relationship(Unblock)]
    );
    // Not on the page, so no buttons (Add friend stays on the card).
    for id in ["u-none", "u-blocker", "u-me", "u-unknown"] {
        assert!(row_actions(&store, id).is_empty(), "{id}");
    }
}

#[test]
fn a_bot_friend_can_be_messaged() {
    let store = store_with(vec![user(
        "u-bot",
        "Friend",
        json!({ "bot": { "owner": "u-someone" } }),
    )]);
    assert_eq!(row_actions(&store, "u-bot"), [RowAction::Message]);
}

#[test]
fn usernames_need_a_name_and_a_four_digit_tag() {
    assert_eq!(check_username("ada#0001"), Ok("ada#0001"));
    // Trimmed; the name itself is left for Stoat to judge.
    assert_eq!(
        check_username("  Ada Lovelace#1234 "),
        Ok("Ada Lovelace#1234")
    );
    assert_eq!(check_username("ada.l-_#9999"), Ok("ada.l-_#9999"));
    for bad in [
        "",
        "   ",
        "ada",
        "ada#",
        "ada#12",
        "ada#12345",
        "#1234",
        "  #1234",
        "ada#12a4",
        "ada#１２３４",
        "ada#12#4",
    ] {
        assert_eq!(check_username(bad), Err(USERNAME_HINT), "{bad:?}");
    }
}

fn refusal(error: &str) -> ApiError {
    let body = json!({ "type": error, "location": "x", "max": 100 }).to_string();
    ApiError::from_response(reqwest::StatusCode::BAD_REQUEST, body.as_bytes())
}

#[test]
fn refusals_read_well_and_some_are_only_information() {
    let error = |text: &str| AddFriendState::Error(text.into());
    let info = |text: &str| AddFriendState::Info(text.into());
    assert_eq!(
        request_refused("ghost#0000", &refusal("NotFound")),
        error("There's no one called ghost#0000.")
    );
    assert_eq!(
        request_refused("x", &refusal("InvalidProperty")),
        error(USERNAME_HINT)
    );
    assert_eq!(
        request_refused("x", &refusal("IsBot")),
        error("Bots can't be friends.")
    );
    assert_eq!(
        request_refused("x", &refusal("AlreadySentRequest")),
        info("You've already sent them a request.")
    );
    assert_eq!(
        request_refused("x", &refusal("AlreadyFriends")),
        info("You're already friends.")
    );
    assert_eq!(
        request_refused("x", &refusal("Blocked")),
        error("You've blocked them. Unblock them first.")
    );
    assert_eq!(
        request_refused("x", &refusal("BlockedByOther")),
        error("They've blocked you.")
    );
    assert_eq!(
        request_refused("x", &refusal("TooManyPendingFriendRequests")),
        error("You have too many pending friend requests (at most 100).")
    );
}

#[test]
fn a_sent_request_says_who_it_went_to() {
    let bob = |relationship: &str| {
        relationship_event(json!({
            "_id": "u-bob", "username": "bob", "discriminator": "0042",
            "relationship": relationship, "online": true
        }))
    };
    assert_eq!(
        request_sent(&bob("Outgoing")),
        AddFriendState::Sent("Friend request sent to bob#0042.".into())
    );
    // They'd already asked, so sending accepts theirs.
    assert_eq!(
        request_sent(&bob("Friend")),
        AddFriendState::Sent("You're now friends with bob#0042.".into())
    );
}

#[test]
fn a_group_can_be_made_with_friends_only_sorted_by_name() {
    use stoat_gpui::groups::pickable_friends;
    let store = store_with(vec![
        user("u-zed", "Friend", json!({ "display_name": "Ada" })),
        user("u-away", "Friend", json!({ "online": false })),
        user("u-in", "Incoming", json!({})),
        user("u-out", "Outgoing", json!({})),
        user("u-blocked", "Blocked", json!({})),
        user("u-none", "None", json!({})),
    ]);
    // Offline friends too, by display name; not the user.
    let all = pickable_friends(&store, &[]);
    assert_eq!(ids(&all), ["u-zed", "u-away"]);
    assert_eq!(all[0].username, "zed");
    // Without those already in the group.
    let rest = pickable_friends(&store, &["u-zed".into(), "u-none".into()]);
    assert_eq!(ids(&rest), ["u-away"]);
}
