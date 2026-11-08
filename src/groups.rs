//! Making and changing groups: who can be picked for one and how many,
//! who takes over from an owner who leaves, and why Stoat refused, in
//! words. Plain data with no GPUI, so it's unit-testable. The dialogs are
//! in [`crate::ui::group_dialogs`].

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::friends::{FriendRow, friend_lists};
use crate::profile_edit::error_text;
use crate::store::ChatStore;

/// The most people a new group can be made with, besides the user: Stoat
/// takes at most 49, so a new group has at most 50 members.
pub const NEW_GROUP_MAX: usize = 49;

/// Stoat's default `group_size` (the most members a group may have, its
/// owner included), when `GET /` doesn't say.
pub const DEFAULT_GROUP_SIZE: u32 = 100;

/// How many people can be picked for a new group where groups hold
/// `group_size` members, the user being one: [`NEW_GROUP_MAX`], or fewer
/// on an instance with smaller groups.
pub fn new_group_limit(group_size: Option<u32>) -> usize {
    let size = usize::try_from(group_size.unwrap_or(DEFAULT_GROUP_SIZE)).unwrap_or(usize::MAX);
    NEW_GROUP_MAX.min(size.saturating_sub(1))
}

/// How many more people a group with `members` members can take, where
/// groups hold `group_size`: none once it's full.
pub fn add_limit(group_size: Option<u32>, members: usize) -> usize {
    let size = usize::try_from(group_size.unwrap_or(DEFAULT_GROUP_SIZE)).unwrap_or(usize::MAX);
    size.saturating_sub(members)
}

/// The user's friends, sorted by name, who can be picked: everyone but
/// those in `exclude` (a group's members, when adding to it). Stoat only
/// lets the user put friends in a group.
pub fn pickable_friends(store: &ChatStore, exclude: &[String]) -> Vec<FriendRow> {
    friend_lists(store)
        .all
        .into_iter()
        .filter(|row| !exclude.contains(&row.user_id))
        .collect()
}

/// The friends in `rows` whose display name or username contains `query`
/// (trimmed), ignoring case, in their order. A blank query keeps them all.
pub fn filter_friends<'a>(rows: &'a [FriendRow], query: &str) -> Vec<&'a FriendRow> {
    let query = query.trim().to_lowercase();
    rows.iter()
        .filter(|row| {
            query.is_empty()
                || row.name.to_lowercase().contains(&query)
                || row.username.to_lowercase().contains(&query)
        })
        .collect()
}

/// Pick `user`, or unpick them if they're picked. `picked` keeps the order
/// they were picked in, and never grows past `limit`. Whether it changed.
pub fn toggle_pick(picked: &mut Vec<String>, user: &str, limit: usize) -> bool {
    if let Some(ix) = picked.iter().position(|id| id == user) {
        picked.remove(ix);
        true
    } else if picked.len() < limit {
        picked.push(user.to_string());
        true
    } else {
        false
    }
}

/// What the picker says once `limit` people are picked.
pub fn limit_text(limit: usize) -> String {
    match limit {
        0 => "This group is full.".into(),
        1 => "You can add at most 1 person.".into(),
        n => format!("You can add at most {n} people."),
    }
}

/// Why a request about a group was refused, for the refusals any of them
/// can meet, else `fallback`. `name` is the person it was about, if any.
/// Callers match their own refusals (such as Missing Permission) first.
pub fn group_error_text(err: &ApiError, name: Option<&str>, fallback: &str) -> String {
    match err.error_type() {
        Some(ErrorType::NotFriends) => "You can only add your friends to a group.".into(),
        Some(ErrorType::GroupTooLarge { max }) => {
            format!("Groups can have at most {max} members.")
        }
        Some(ErrorType::NotOwner) => "Only the group's owner can do that.".into(),
        Some(ErrorType::NotInGroup) => match name {
            Some(name) => format!("{name} isn't in this group any more."),
            None => "They aren't in this group any more.".into(),
        },
        _ => error_text(err, fallback),
    }
}

/// Why `POST /channels/create` was refused.
pub fn create_group_error_text(err: &ApiError) -> String {
    group_error_text(err, None, "Couldn't create the group. Try again.")
}

/// Why `PUT /channels/{id}/recipients/{user}` was refused, naming the
/// person (`name`) it was for: "Couldn't add {name}: {why}".
pub fn add_member_error_text(err: &ApiError, name: &str) -> String {
    let fallback = format!("Couldn't add {name}. Try again.");
    let why = match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => "You can't add people to this group.".into(),
        _ => group_error_text(err, Some(name), &fallback),
    };
    if why == fallback {
        why
    } else {
        format!("Couldn't add {name}: {why}")
    }
}

/// Why `DELETE /channels/{id}/recipients/{user}` was refused; `name` is
/// the person. (`NotInGroup` isn't a refusal: they're already out.)
pub fn remove_member_error_text(err: &ApiError, name: &str) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => {
            "Only the group's owner can remove members.".into()
        }
        _ => group_error_text(
            err,
            Some(name),
            &format!("Couldn't remove {name}. Try again."),
        ),
    }
}

/// Who Stoat makes the owner when `owner` leaves a group with these
/// `recipients`: the first other member, in their order. `None` when the
/// owner is alone (leaving deletes the group).
pub fn default_next_owner(recipients: &[String], owner: &str) -> Option<String> {
    recipients.iter().find(|id| *id != owner).cloned()
}

/// Why `PATCH /channels/{id}` making `name` the owner was refused.
pub fn transfer_error_text(err: &ApiError, name: &str) -> String {
    match err.error_type() {
        Some(ErrorType::MissingPermission { .. }) => "Only the group's owner can do that.".into(),
        _ => group_error_text(
            err,
            Some(name),
            &format!("Couldn't make {name} the owner. Try again."),
        ),
    }
}

/// Why `DELETE /channels/{id}` (leaving a group) was refused. `handed_to`
/// is who the user made the owner just before, if anyone: they stay the
/// owner, and the user stays a member. (`NotFound` isn't a refusal: the
/// user is already out.)
pub fn leave_group_error_text(err: &ApiError, handed_to: Option<&str>) -> String {
    let fallback = "Couldn't leave the group. Try again.";
    let why = group_error_text(err, None, fallback);
    match handed_to {
        None => why,
        Some(name) if why == fallback => {
            format!("{name} is now the owner, but leaving failed. Try again.")
        }
        Some(name) => format!("{name} is now the owner, but leaving failed: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::RelationshipStatus;
    use crate::store::PresenceKind;

    fn api_error(error_type: ErrorType) -> ApiError {
        ApiError::Api(revolt_result::Error {
            error_type,
            location: None,
        })
    }

    fn row(id: &str, name: &str, username: &str) -> FriendRow {
        FriendRow {
            user_id: id.into(),
            name: name.into(),
            username: username.into(),
            avatar_url: String::new(),
            presence: PresenceKind::Online,
            status: String::new(),
            relationship: RelationshipStatus::Friend,
        }
    }

    fn ids(rows: Vec<&FriendRow>) -> Vec<&str> {
        rows.into_iter().map(|row| row.user_id.as_str()).collect()
    }

    #[test]
    fn friends_are_found_by_display_name_or_username_ignoring_case() {
        let rows = [
            row("u1", "Ada Lovelace", "ada"),
            row("u2", "Bob", "builder"),
            row("u3", "Carol", "carol"),
        ];
        assert_eq!(ids(filter_friends(&rows, "LOVE")), ["u1"]);
        assert_eq!(ids(filter_friends(&rows, "build")), ["u2"]);
        // Either name; in their order.
        assert_eq!(ids(filter_friends(&rows, "o")), ["u1", "u2", "u3"]);
        assert_eq!(ids(filter_friends(&rows, " carol ")), ["u3"]);
        assert!(filter_friends(&rows, "zed").is_empty());
    }

    #[test]
    fn a_blank_search_keeps_everyone() {
        let rows = [row("u1", "Ada", "ada"), row("u2", "Bob", "bob")];
        assert_eq!(ids(filter_friends(&rows, "")), ["u1", "u2"]);
        assert_eq!(ids(filter_friends(&rows, "   ")), ["u1", "u2"]);
    }

    #[test]
    fn picks_keep_their_order_and_stop_at_the_limit() {
        let mut picked = Vec::new();
        assert!(toggle_pick(&mut picked, "u2", 2));
        assert!(toggle_pick(&mut picked, "u1", 2));
        assert_eq!(picked, ["u2", "u1"]);
        // Full: nobody more, but unpicking still works.
        assert!(!toggle_pick(&mut picked, "u3", 2));
        assert_eq!(picked, ["u2", "u1"]);
        assert!(toggle_pick(&mut picked, "u2", 2));
        assert_eq!(picked, ["u1"]);
        assert!(toggle_pick(&mut picked, "u3", 2));
        assert_eq!(picked, ["u1", "u3"]);
    }

    #[test]
    fn a_new_group_takes_49_people_or_fewer_where_groups_are_smaller() {
        assert_eq!(new_group_limit(None), 49);
        assert_eq!(new_group_limit(Some(100)), 49);
        assert_eq!(new_group_limit(Some(10)), 9);
        assert_eq!(new_group_limit(Some(1)), 0);
        assert_eq!(new_group_limit(Some(0)), 0);
    }

    #[test]
    fn a_group_takes_people_until_its_full() {
        assert_eq!(add_limit(None, 3), 97);
        assert_eq!(add_limit(Some(10), 4), 6);
        assert_eq!(add_limit(Some(10), 10), 0);
        // Fuller than the instance now allows: still none.
        assert_eq!(add_limit(Some(10), 12), 0);
    }

    #[test]
    fn the_limit_is_said_in_words() {
        assert_eq!(limit_text(49), "You can add at most 49 people.");
        assert_eq!(limit_text(1), "You can add at most 1 person.");
        assert_eq!(limit_text(0), "This group is full.");
    }

    #[test]
    fn group_refusals_say_why() {
        assert_eq!(
            create_group_error_text(&api_error(ErrorType::NotFriends)),
            "You can only add your friends to a group."
        );
        assert_eq!(
            create_group_error_text(&api_error(ErrorType::GroupTooLarge { max: 50 })),
            "Groups can have at most 50 members."
        );
        assert_eq!(
            group_error_text(&api_error(ErrorType::NotOwner), None, "x"),
            "Only the group's owner can do that."
        );
        assert_eq!(
            group_error_text(&api_error(ErrorType::NotInGroup), Some("ada"), "x"),
            "ada isn't in this group any more."
        );
        assert_eq!(
            group_error_text(&api_error(ErrorType::NotInGroup), None, "x"),
            "They aren't in this group any more."
        );
        let limited = ApiError::RateLimited {
            retry_after_ms: Some(4200),
        };
        assert_eq!(
            create_group_error_text(&limited),
            "Stoat is limiting changes. Try again in 5 s."
        );
        assert_eq!(
            create_group_error_text(&api_error(ErrorType::InternalError)),
            "Couldn't create the group. Try again."
        );
    }

    #[test]
    fn adding_refusals_name_the_person() {
        let missing = api_error(ErrorType::MissingPermission {
            permission: "InviteOthers".into(),
        });
        assert_eq!(
            add_member_error_text(&missing, "ada"),
            "Couldn't add ada: You can't add people to this group."
        );
        assert_eq!(
            add_member_error_text(&api_error(ErrorType::NotFriends), "ada"),
            "Couldn't add ada: You can only add your friends to a group."
        );
        assert_eq!(
            add_member_error_text(&api_error(ErrorType::GroupTooLarge { max: 100 }), "ada"),
            "Couldn't add ada: Groups can have at most 100 members."
        );
        let limited = ApiError::RateLimited {
            retry_after_ms: Some(2000),
        };
        assert_eq!(
            add_member_error_text(&limited, "ada"),
            "Couldn't add ada: Stoat is limiting changes. Try again in 2 s."
        );
        assert_eq!(
            add_member_error_text(&api_error(ErrorType::NotFound), "ada"),
            "Couldn't add ada. Try again."
        );
    }

    #[test]
    fn removing_refusals_say_why() {
        let missing = api_error(ErrorType::MissingPermission {
            permission: "ManageChannel".into(),
        });
        assert_eq!(
            remove_member_error_text(&missing, "ada"),
            "Only the group's owner can remove members."
        );
        assert_eq!(
            remove_member_error_text(&api_error(ErrorType::InternalError), "ada"),
            "Couldn't remove ada. Try again."
        );
    }

    #[test]
    fn the_next_owner_is_the_first_other_member() {
        let recipients = ["u2", "u-me", "u3"].map(String::from);
        assert_eq!(
            default_next_owner(&recipients, "u-me").as_deref(),
            Some("u2")
        );
        assert_eq!(
            default_next_owner(&recipients, "u2").as_deref(),
            Some("u-me")
        );
        // Alone: nobody takes over.
        assert_eq!(default_next_owner(&["u-me".into()], "u-me"), None);
        assert_eq!(default_next_owner(&[], "u-me"), None);
    }

    #[test]
    fn handing_over_refusals_say_why() {
        assert_eq!(
            transfer_error_text(&api_error(ErrorType::NotOwner), "ada"),
            "Only the group's owner can do that."
        );
        let missing = api_error(ErrorType::MissingPermission {
            permission: "ManageChannel".into(),
        });
        assert_eq!(
            transfer_error_text(&missing, "ada"),
            "Only the group's owner can do that."
        );
        assert_eq!(
            transfer_error_text(&api_error(ErrorType::NotInGroup), "ada"),
            "ada isn't in this group any more."
        );
        assert_eq!(
            transfer_error_text(&api_error(ErrorType::InternalError), "ada"),
            "Couldn't make ada the owner. Try again."
        );
    }

    #[test]
    fn leaving_refusals_say_whether_the_group_was_handed_on() {
        let failed = api_error(ErrorType::InternalError);
        assert_eq!(
            leave_group_error_text(&failed, None),
            "Couldn't leave the group. Try again."
        );
        assert_eq!(
            leave_group_error_text(&failed, Some("ada")),
            "ada is now the owner, but leaving failed. Try again."
        );
        let limited = ApiError::RateLimited {
            retry_after_ms: Some(3000),
        };
        assert_eq!(
            leave_group_error_text(&limited, Some("ada")),
            "ada is now the owner, but leaving failed: Stoat is limiting changes. Try again in 3 s."
        );
    }
}
