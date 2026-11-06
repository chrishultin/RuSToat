//! Notification rules, mirroring Stoat's web client (NotificationsWorker.tsx):
//! one test per skip rule, plus titles, bodies, and muted-channel indicators.

use stoat_gpui::api::events::{Ready, ServerEvent, parse_frame};
use stoat_gpui::api::models::{Message, RelationshipStatus};
use stoat_gpui::friends::FRIENDS;
use stoat_gpui::notify;
use stoat_gpui::notify::{
    Delivery, Notice, NotificationSettings, Viewing, delivery, friend_request_notice, should_notify,
};
use stoat_gpui::store::ChatStore;

const NOW: i64 = 1_800_000_000_000;
const AWAY: Viewing<'static> = Viewing {
    channel: None,
    window_active: false,
};

fn ready() -> Ready {
    let path = format!("{}/tests/fixtures/ready.json", env!("CARGO_MANIFEST_DIR"));
    let Some(ServerEvent::Ready(ready)) =
        parse_frame(&std::fs::read_to_string(path).unwrap()).pop()
    else {
        panic!("ready fixture");
    };
    *ready
}

/// The fixture store, with notification settings (JSON) and our presence.
fn store(settings: &str, presence: Option<&str>) -> ChatStore {
    let mut ready = ready();
    ready.notifications = Some(NotificationSettings::parse(settings));
    ready.users.push(
        serde_json::from_value(serde_json::json!({
            "_id": "u-blocked", "username": "troll", "discriminator": "6666",
            "relationship": "Blocked", "online": true
        }))
        .unwrap(),
    );
    if let Some(presence) = presence {
        let me = ready.users.iter_mut().find(|u| u.id == "u-me").unwrap();
        me.status =
            Some(serde_json::from_value(serde_json::json!({ "presence": presence })).unwrap());
    }
    let mut store = ChatStore::new("u-me");
    store.apply_ready(ready);
    store
}

fn default_store() -> ChatStore {
    store("{}", None)
}

fn message(channel: &str, author: &str, extra: serde_json::Value) -> Message {
    let mut value = serde_json::json!({
        "_id": "01JC00000000000000000000ZZ", "channel": channel, "author": author, "content": "hello there"
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

fn notify(store: &ChatStore, message: &Message, viewing: Viewing<'_>) -> Option<Notice> {
    should_notify(store, message, viewing, NOW)
}

fn mention() -> serde_json::Value {
    serde_json::json!({ "mentions": ["u-me"] })
}

#[test]
fn a_dm_notifies_by_default_with_the_web_title() {
    let notice = notify(
        &default_store(),
        &message("dm-ada", "u-ada", serde_json::json!({})),
        AWAY,
    )
    .expect("DMs default to all");
    assert_eq!(notice.title, "@Ada L.");
    assert_eq!(notice.body, "hello there");
    assert_eq!(notice.tag, "dm-ada");
    assert_eq!(notice.channel, "dm-ada");
}

#[test]
fn looking_at_the_channel_only_skips_while_the_window_is_active() {
    let store = default_store();
    let msg = message("dm-ada", "u-ada", serde_json::json!({}));
    let looking = |active| Viewing {
        channel: Some("dm-ada"),
        window_active: active,
    };
    assert!(notify(&store, &msg, looking(true)).is_none());
    assert!(notify(&store, &msg, looking(false)).is_some());
    // Looking at a different channel doesn't count.
    let elsewhere = Viewing {
        channel: Some("ch-general"),
        window_active: true,
    };
    assert!(notify(&store, &msg, elsewhere).is_some());
}

#[test]
fn own_and_blocked_authors_never_notify() {
    let store = default_store();
    assert!(
        notify(
            &store,
            &message("dm-ada", "u-me", serde_json::json!({})),
            AWAY
        )
        .is_none()
    );
    assert!(notify(&store, &message("dm-ada", "u-blocked", mention()), AWAY).is_none());
}

#[test]
fn suppressed_messages_never_notify() {
    // MessageFlags::SuppressNotifications = 1 → mask 1 << 1.
    let msg = message("dm-ada", "u-ada", serde_json::json!({ "flags": 1 << 1 }));
    assert!(notify(&default_store(), &msg, AWAY).is_none());
}

#[test]
fn servers_default_to_mentions_only() {
    let store = default_store();
    assert!(
        notify(
            &store,
            &message("ch-general", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_none()
    );
    let notice = notify(&store, &message("ch-general", "u-ada", mention()), AWAY).unwrap();
    assert_eq!(notice.title, "@Ada L. (#general, Alpha Team)");
}

#[test]
fn levels_resolve_channel_then_server_then_default() {
    let all = store(r#"{"server":{"srv-alpha":"all"}}"#, None);
    assert!(
        notify(
            &all,
            &message("ch-general", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_some()
    );

    // The channel's own level beats its server's, even for mentions.
    let silenced = store(
        r#"{"server":{"srv-alpha":"all"},"channel":{"ch-general":"none"}}"#,
        None,
    );
    assert!(notify(&silenced, &message("ch-general", "u-ada", mention()), AWAY).is_none());
    // Other channels in the server still follow the server.
    assert!(
        notify(
            &silenced,
            &message("ch-random", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_some()
    );

    // DMs can be turned down too.
    let quiet_dm = store(r#"{"channel":{"dm-ada":"mention"}}"#, None);
    assert!(
        notify(
            &quiet_dm,
            &message("dm-ada", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_none()
    );
    assert!(notify(&quiet_dm, &message("dm-ada", "u-ada", mention()), AWAY).is_some());
}

#[test]
fn mutes_silence_even_mentions_until_they_expire() {
    let channel_muted = store(r#"{"channel_mutes":{"dm-ada":{}}}"#, None);
    assert!(notify(&channel_muted, &message("dm-ada", "u-ada", mention()), AWAY).is_none());

    let server_muted = store(r#"{"server_mutes":{"srv-alpha":{}}}"#, None);
    assert!(
        notify(
            &server_muted,
            &message("ch-general", "u-ada", mention()),
            AWAY
        )
        .is_none()
    );

    let expired = store(
        &format!(
            r#"{{"channel_mutes":{{"dm-ada":{{"until":{}}}}}}}"#,
            NOW - 1
        ),
        None,
    );
    assert!(
        notify(
            &expired,
            &message("dm-ada", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_some()
    );

    // Legacy "muted" level means muted.
    let legacy = store(r#"{"channel":{"dm-ada":"muted"}}"#, None);
    assert!(notify(&legacy, &message("dm-ada", "u-ada", mention()), AWAY).is_none());
}

#[test]
fn busy_silences_everything_and_focus_allows_mentions() {
    let busy = store("{}", Some("Busy"));
    assert!(notify(&busy, &message("dm-ada", "u-ada", mention()), AWAY).is_none());

    let focus = store("{}", Some("Focus"));
    assert!(
        notify(
            &focus,
            &message("dm-ada", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_none()
    );
    assert!(notify(&focus, &message("dm-ada", "u-ada", mention()), AWAY).is_some());

    let online = store("{}", Some("Online"));
    assert!(
        notify(
            &online,
            &message("dm-ada", "u-ada", serde_json::json!({})),
            AWAY
        )
        .is_some()
    );
}

#[test]
fn saved_notes_never_notify() {
    assert!(
        notify(
            &default_store(),
            &message("ch-saved", "u-ada", mention()),
            AWAY
        )
        .is_none()
    );
}

#[test]
fn group_titles_and_system_messages() {
    let store = default_store();
    let notice = notify(
        &store,
        &message("grp-weekend", "u-ada", serde_json::json!({})),
        AWAY,
    )
    .unwrap();
    assert_eq!(notice.title, "@Ada L. - Weekend plans");

    let system = message(
        "grp-weekend",
        "00000000000000000000000000",
        serde_json::json!({ "content": null, "system": { "type": "user_added", "id": "u-bob", "by": "u-ada" } }),
    );
    let notice = notify(&store, &system, AWAY).unwrap();
    assert_eq!(notice.title, "Weekend plans");
    assert_eq!(notice.body, "Ada L. added bob");
}

#[test]
fn bodies_are_plain_and_truncated() {
    let store = default_store();
    let msg = message(
        "dm-ada",
        "u-ada",
        serde_json::json!({ "content": "hey <@u-bob>, see <#ch-general>" }),
    );
    assert_eq!(
        notify(&store, &msg, AWAY).unwrap().body,
        "hey @bob, see #general"
    );

    let long = message(
        "dm-ada",
        "u-ada",
        serde_json::json!({ "content": "x".repeat(500) }),
    );
    let body = notify(&store, &long, AWAY).unwrap().body;
    assert_eq!(body.chars().count(), 201);
    assert!(body.ends_with('…'));
}

#[test]
fn muted_channels_hide_their_dot_but_keep_mentions() {
    let mut store = store(r#"{"channel_mutes":{"ch-random":{}}}"#, None);
    // The fixture has no read positions; read #general so only #random matters.
    store.apply_ack("ch-general", "01JC0000000000000000000009");
    store.apply_message(message("ch-random", "u-ada", mention()));
    assert!(store.is_unread("ch-random"), "still unread underneath");
    let random = store
        .server_channels("srv-alpha")
        .into_iter()
        .flat_map(|s| s.channels)
        .find(|c| c.id == "ch-random")
        .unwrap();
    assert!(!random.unread, "no dot while muted");
    assert_eq!(random.mentions, 1, "mentions still count");
    let alpha = store
        .servers()
        .into_iter()
        .find(|s| s.id == "srv-alpha")
        .unwrap();
    assert!(!alpha.unread);
    assert_eq!(alpha.mentions, 1);
    // The account's switcher button counts the muted mention too.
    let (_, home_mentions) = store.home_summary();
    assert_eq!(store.account_summary().1, home_mentions + 1);
}

#[test]
fn attention_count_is_mentions_plus_unread_conversations() {
    // The fixture has no read positions, so dm-ada, dm-bob and grp-weekend
    // (all with messages, level "all" by default) are unread: 3.
    let mut store = default_store();
    assert_eq!(store.attention_count(), 3);

    // A server mention adds one; a plain server message doesn't.
    store.apply_message(message("ch-random", "u-ada", mention()));
    let mut plain = message("ch-general", "u-ada", serde_json::json!({}));
    plain.id = "01JC00000000000000000000ZY".into();
    store.apply_message(plain);
    assert_eq!(store.attention_count(), 4);

    // A DM with two mentions counts as two, not three.
    let mut second = message("dm-ada", "u-ada", mention());
    second.id = "01JC00000000000000000000ZX".into();
    store.apply_message(message("dm-ada", "u-ada", mention()));
    store.apply_message(second);
    assert_eq!(store.attention_count(), 5);

    // Reading it drops it.
    store.apply_ack("dm-ada", "01JC00000000000000000000ZZ");
    assert_eq!(store.attention_count(), 3);
}

#[test]
fn attention_count_skips_muted_and_mention_only_conversations() {
    let store = store(
        r#"{"channel_mutes":{"dm-bob":{}},"channel":{"grp-weekend":"mention"}}"#,
        None,
    );
    // Only dm-ada is left: dm-bob is muted, the group only counts mentions.
    assert_eq!(store.attention_count(), 1);
}

#[test]
fn a_dm_opened_just_now_notifies_its_first_message() {
    // Seen live: another account DMed us for the first time. The channel
    // didn't exist at Ready, so the message was skipped as "unknown channel".
    let mut store = default_store();
    let first = message(
        "dm-new",
        "u-new",
        serde_json::json!({
            "user": {
                "_id": "u-new", "username": "newcomer", "discriminator": "0001",
                "relationship": "None", "online": true
            }
        }),
    );
    assert_eq!(notify(&store, &first, AWAY), None, "unknown channel");

    store.add_channel(
        serde_json::from_value(serde_json::json!({
            "channel_type": "DirectMessage", "_id": "dm-new",
            "active": false, "recipients": ["u-me", "u-new"]
        }))
        .unwrap(),
    );
    store.apply_channel_update(
        "dm-new",
        serde_json::from_value(serde_json::json!({ "active": true })).unwrap(),
        &[],
    );
    let notice = notify(&store, &first, AWAY).expect("a new DM notifies");
    assert_eq!(notice.title, "@newcomer");

    store.apply_message(first);
    assert!(store.is_unread("dm-new"));
    assert!(store.home_channels().iter().any(|e| e.id == "dm-new"));
    assert_eq!(
        store.attention_count(),
        default_store().attention_count() + 1
    );
}

#[test]
fn banners_only_in_the_background_and_when_possible() {
    // In the background, with banners possible (bundled, not denied).
    assert_eq!(delivery(false, true), Delivery::Banner);
    // In front: a toast, even though banners would work.
    assert_eq!(delivery(true, true), Delivery::Toast);
    // Not bundled, or banners denied: toasts either way, so nothing is lost.
    assert_eq!(delivery(false, false), Delivery::Toast);
    assert_eq!(delivery(true, false), Delivery::Toast);
}

#[test]
fn banner_tags_round_trip_with_the_account() {
    let account = "https://api.stoat.chat#01USER";
    let tag = notify::banner_tag(account, "01CHANNEL");
    assert_eq!(notify::parse_banner_tag(&tag), (Some(account), "01CHANNEL"));
    // An account from before multiple accounts: its ID is the bare URL.
    let tag = notify::banner_tag("https://stoat.example.com/api", "01CHANNEL");
    assert_eq!(
        notify::parse_banner_tag(&tag),
        (Some("https://stoat.example.com/api"), "01CHANNEL")
    );
}

#[test]
fn banner_tags_from_before_accounts_are_just_the_channel() {
    assert_eq!(notify::parse_banner_tag("01CHANNEL"), (None, "01CHANNEL"));
}

// --- friend requests -----------------------------------------------------

/// `user` now relates to the user as `relationship` (a `UserRelationship`).
/// Returns how they related before.
fn relate(store: &mut ChatStore, user: &str, relationship: &str) -> RelationshipStatus {
    let frame = serde_json::json!({
        "type": "UserRelationship", "id": "u-me",
        "user": {
            "_id": user, "username": user.trim_start_matches("u-"), "discriminator": "0001",
            "display_name": "Carol C.", "relationship": relationship, "online": true
        }
    });
    let Some(ServerEvent::UserRelationship { user }) = parse_frame(&frame.to_string()).pop() else {
        panic!("UserRelationship");
    };
    store.apply_relationship(*user).unwrap_or_default()
}

#[test]
fn a_new_friend_request_notifies() {
    let mut store = default_store();
    let was = relate(&mut store, "u-carol", "Incoming");
    let notice =
        friend_request_notice(&store, "u-carol", &was, false, false).expect("a new request");
    assert_eq!(notice.title, "Friend request");
    assert_eq!(notice.body, "Carol C. sent you a friend request.");
    // Every request shares the Friends page's tag, so a newer one replaces it.
    assert_eq!(notice.tag, FRIENDS);
    assert_eq!(notice.channel, FRIENDS);

    // Whatever they were before (here, a friend), Incoming is new.
    let was = relate(&mut store, "u-ada", "Incoming");
    assert_eq!(was, RelationshipStatus::Friend);
    assert!(friend_request_notice(&store, "u-ada", &was, false, true).is_some());
}

#[test]
fn a_request_that_was_already_pending_does_not_notify_again() {
    let mut store = default_store();
    relate(&mut store, "u-carol", "Incoming");
    let was = relate(&mut store, "u-carol", "Incoming");
    assert_eq!(was, RelationshipStatus::Incoming);
    assert_eq!(
        friend_request_notice(&store, "u-carol", &was, false, false),
        None
    );
}

#[test]
fn busy_silences_friend_requests_and_focus_does_not() {
    let mut busy = store("{}", Some("Busy"));
    let was = relate(&mut busy, "u-carol", "Incoming");
    assert_eq!(
        friend_request_notice(&busy, "u-carol", &was, false, false),
        None
    );

    let mut focus = store("{}", Some("Focus"));
    let was = relate(&mut focus, "u-carol", "Incoming");
    assert!(friend_request_notice(&focus, "u-carol", &was, false, false).is_some());
}

#[test]
fn looking_at_pending_skips_only_while_the_window_is_active() {
    let mut store = default_store();
    let was = relate(&mut store, "u-carol", "Incoming");
    assert_eq!(
        friend_request_notice(&store, "u-carol", &was, true, true),
        None
    );
    assert!(friend_request_notice(&store, "u-carol", &was, true, false).is_some());
    assert!(friend_request_notice(&store, "u-carol", &was, false, true).is_some());
}

#[test]
fn other_relationship_changes_do_not_notify() {
    let mut store = default_store();
    for relationship in ["Outgoing", "Friend", "Blocked", "BlockedOther", "None"] {
        let was = relate(&mut store, "u-carol", relationship);
        assert_eq!(
            friend_request_notice(&store, "u-carol", &was, false, false),
            None,
            "{relationship}"
        );
    }
    // An Incoming request turning into a friendship doesn't either.
    relate(&mut store, "u-carol", "Incoming");
    let was = relate(&mut store, "u-carol", "Friend");
    assert_eq!(
        friend_request_notice(&store, "u-carol", &was, false, false),
        None
    );
    // Nor does an unknown user, or the user themselves.
    assert_eq!(
        friend_request_notice(&store, "u-nobody", &RelationshipStatus::None, false, false),
        None
    );
    assert_eq!(
        friend_request_notice(&store, "u-me", &RelationshipStatus::None, false, false),
        None
    );
}

#[test]
fn a_request_then_its_acceptance_through_the_store() {
    use stoat_gpui::friends::{friend_lists, incoming_count};
    let mut store = default_store();
    let (_, mentions) = store.home_summary();
    let attention = store.attention_count();

    let was = relate(&mut store, "u-carol", "Incoming");
    assert_eq!(incoming_count(&store), 1);
    assert_eq!(friend_lists(&store).incoming.len(), 1);
    assert_eq!(store.home_summary().1, mentions + 1);
    assert_eq!(store.attention_count(), attention + 1);
    assert!(friend_request_notice(&store, "u-carol", &was, false, false).is_some());

    // The route's answer after Accept.
    relate(&mut store, "u-carol", "Friend");
    let lists = friend_lists(&store);
    assert!(lists.incoming.is_empty());
    assert!(lists.all.iter().any(|row| row.user_id == "u-carol"));
    assert!(lists.online.iter().any(|row| row.user_id == "u-carol"));
    assert_eq!(incoming_count(&store), 0);
    assert_eq!(store.home_summary().1, mentions);
    assert_eq!(store.attention_count(), attention);
}

/// The fixture store with the user holding `role-mods` in Alpha Team,
/// whose channels default to mentions only.
fn store_with_role() -> ChatStore {
    let mut ready = ready();
    ready.notifications = Some(NotificationSettings::parse("{}"));
    let me = ready
        .members
        .iter_mut()
        .find(|m| m.id.server == "srv-alpha" && m.id.user == "u-me")
        .unwrap();
    me.roles = vec!["role-mods".into()];
    let mut store = ChatStore::new("u-me");
    store.apply_ready(ready);
    store
}

#[test]
fn role_mentions_notify_only_with_the_role() {
    let store = store_with_role();
    let held = message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "role_mentions": ["role-mods"] }),
    );
    assert!(notify(&store, &held, AWAY).is_some());
    let other = message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "role_mentions": ["role-admins"] }),
    );
    assert!(notify(&store, &other, AWAY).is_none());
}

#[test]
fn everyone_notifies_under_mentions_only() {
    // MessageFlags::MentionsEveryone = 2.
    let everyone = message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "flags": 1 << 2 }),
    );
    assert!(notify(&default_store(), &everyone, AWAY).is_some());
}

#[test]
fn online_notifies_only_while_we_appear_online() {
    // MessageFlags::MentionsOnline = 3.
    let online = message(
        "ch-general",
        "u-ada",
        serde_json::json!({ "flags": 1 << 3 }),
    );
    assert!(notify(&store("{}", Some("Online")), &online, AWAY).is_some());
    assert!(
        notify(&default_store(), &online, AWAY).is_some(),
        "online, no presence set"
    );
    // Invisible counts as offline, as in the member list.
    assert!(notify(&store("{}", Some("Invisible")), &online, AWAY).is_none());
}
