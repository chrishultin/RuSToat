//! Reporting a message, a user or a server to Stoat's moderators
//! (`POST /safety/report`): what can be reported, the reasons in the web
//! client's order, the request, and why Stoat refused, in words. Plain data
//! with no GPUI, so it's unit-testable. The dialog is
//! [`crate::ui::report_dialog`].

use revolt_result::ErrorType;
use serde::Serialize;

use crate::api::ApiError;
use crate::api::models::{ContentReportReason, ReportedContent, UserReportReason};
use crate::profile_edit::error_text;
use crate::store::ChatStore;

/// The longest detail Stoat takes with a report, in characters.
pub const DETAIL_MAX: usize = 1000;

/// What's being reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportTarget {
    Message {
        id: String,
        /// Who sent it.
        author: String,
    },
    User {
        id: String,
        /// The message it was reported from, as context.
        message: Option<String>,
    },
    Server {
        id: String,
    },
}

impl ReportTarget {
    /// The dialog's title.
    pub fn title(&self) -> &'static str {
        match self {
            ReportTarget::Message { .. } => "Tell us what's wrong with this message",
            ReportTarget::User { .. } => "Tell us what's wrong with this user",
            ReportTarget::Server { .. } => "Tell us what's wrong with this server",
        }
    }

    /// The reasons to choose from, in the web client's order, with Other
    /// last.
    pub fn reasons(&self) -> Vec<Reason> {
        match self {
            ReportTarget::User { .. } => USER_REASONS.iter().cloned().map(Reason::User).collect(),
            _ => CONTENT_REASONS
                .iter()
                .cloned()
                .map(Reason::Content)
                .collect(),
        }
    }
}

/// Why a message or server is reported, in the web client's order.
const CONTENT_REASONS: [ContentReportReason; 14] = [
    ContentReportReason::Illegal,
    ContentReportReason::IllegalGoods,
    ContentReportReason::IllegalExtortion,
    ContentReportReason::IllegalPornography,
    ContentReportReason::IllegalHacking,
    ContentReportReason::ExtremeViolence,
    ContentReportReason::PromotesHarm,
    ContentReportReason::UnsolicitedSpam,
    ContentReportReason::Raid,
    ContentReportReason::SpamAbuse,
    ContentReportReason::ScamsFraud,
    ContentReportReason::Malware,
    ContentReportReason::Harassment,
    ContentReportReason::NoneSpecified,
];

/// Why a user is reported, in the web client's order.
const USER_REASONS: [UserReportReason; 7] = [
    UserReportReason::UnsolicitedSpam,
    UserReportReason::SpamAbuse,
    UserReportReason::InappropriateProfile,
    UserReportReason::Impersonation,
    UserReportReason::BanEvasion,
    UserReportReason::Underage,
    UserReportReason::NoneSpecified,
];

/// A reason for a report: a message's or server's, or a user's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    Content(ContentReportReason),
    User(UserReportReason),
}

impl Reason {
    /// The web client's words for it.
    pub fn label(&self) -> &'static str {
        match self {
            Reason::Content(reason) => match reason {
                ContentReportReason::Illegal => "Content breaks one or more laws",
                ContentReportReason::IllegalGoods => "Drugs or illegal goods",
                ContentReportReason::IllegalExtortion => "Extortion or blackmail",
                ContentReportReason::IllegalPornography => "Revenge or underage pornography",
                ContentReportReason::IllegalHacking => "Illegal hacking or cracking",
                ContentReportReason::ExtremeViolence => "Extreme violence, gore or animal cruelty",
                ContentReportReason::PromotesHarm => "Promotes harm",
                ContentReportReason::UnsolicitedSpam => "Unsolicited advertising or spam",
                ContentReportReason::Raid => "Raid or spam attack",
                ContentReportReason::SpamAbuse => "Spam or similar platform abuse",
                ContentReportReason::ScamsFraud => "Scams or fraud",
                ContentReportReason::Malware => "Malware or phishing",
                ContentReportReason::Harassment => "Harassment or cyberbullying",
                ContentReportReason::NoneSpecified => "Other",
            },
            Reason::User(reason) => match reason {
                UserReportReason::UnsolicitedSpam => "Unsolicited advertising or spam",
                UserReportReason::SpamAbuse => "Spam or similar platform abuse",
                UserReportReason::InappropriateProfile => {
                    "User's profile has inappropriate content"
                }
                UserReportReason::Impersonation => "Impersonation",
                UserReportReason::BanEvasion => "Ban evasion",
                UserReportReason::Underage => "Not of minimum age to use the platform",
                UserReportReason::NoneSpecified => "Other",
            },
        }
    }

    /// Other, which needs some detail.
    pub fn is_other(&self) -> bool {
        matches!(
            self,
            Reason::Content(ContentReportReason::NoneSpecified)
                | Reason::User(UserReportReason::NoneSpecified)
        )
    }
}

/// `POST /safety/report`'s body. Stoat defines it only in its route, so
/// it's the client's own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DataReportContent {
    pub content: ReportedContent,
    pub additional_context: String,
}

/// Whether Report can be sent: a reason picked, the detail within
/// [`DETAIL_MAX`], and some detail for Other.
pub fn can_send(reason: Option<&Reason>, detail: &str) -> bool {
    let detail = detail.trim();
    reason.is_some_and(|reason| !reason.is_other() || !detail.is_empty())
        && detail.chars().count() <= DETAIL_MAX
}

/// The report of `target` for `reason`, with `detail` (trimmed). `None`
/// if the reason is for another kind of target.
pub fn report_body(
    target: &ReportTarget,
    reason: &Reason,
    detail: &str,
) -> Option<DataReportContent> {
    let content = match (target, reason) {
        (ReportTarget::Message { id, .. }, Reason::Content(reason)) => ReportedContent::Message {
            id: id.clone(),
            report_reason: reason.clone(),
        },
        (ReportTarget::Server { id }, Reason::Content(reason)) => ReportedContent::Server {
            id: id.clone(),
            report_reason: reason.clone(),
        },
        (ReportTarget::User { id, message }, Reason::User(reason)) => ReportedContent::User {
            id: id.clone(),
            report_reason: reason.clone(),
            message_id: message.clone(),
        },
        _ => return None,
    };
    Some(DataReportContent {
        content,
        additional_context: detail.trim().to_string(),
    })
}

/// Whether the user may report `target`: not themselves, not their own
/// message, and not a server they own (Stoat refuses all three), nor one
/// it doesn't know.
pub fn can_report(store: &ChatStore, target: &ReportTarget) -> bool {
    let me = store.me();
    match target {
        ReportTarget::Message { author, .. } => author != me,
        ReportTarget::User { id, .. } => id != me,
        ReportTarget::Server { id } => store.server(id).is_some_and(|server| server.owner != me),
    }
}

/// Why Stoat refused a report.
pub fn report_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::CannotReportYourself) => "You can't report yourself.".into(),
        Some(ErrorType::NotFound) => "That's gone, so it can't be reported.".into(),
        Some(ErrorType::FailedValidation { .. }) => {
            format!("Keep the detail to {DETAIL_MAX} characters.")
        }
        _ => error_text(err, "Couldn't send the report."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> ReportTarget {
        ReportTarget::Message {
            id: "m1".into(),
            author: "u-ada".into(),
        }
    }

    fn user() -> ReportTarget {
        ReportTarget::User {
            id: "u-ada".into(),
            message: None,
        }
    }

    #[test]
    fn every_reason_has_a_label_and_other_comes_last() {
        for target in [message(), user(), ReportTarget::Server { id: "s".into() }] {
            let reasons = target.reasons();
            assert!(reasons.iter().all(|reason| !reason.label().is_empty()));
            let (last, rest) = reasons.split_last().unwrap();
            assert!(last.is_other());
            assert_eq!(last.label(), "Other");
            assert!(rest.iter().all(|reason| !reason.is_other()));
        }
        assert_eq!(message().reasons().len(), 14);
        assert_eq!(user().reasons().len(), 7);
        assert_eq!(
            user().reasons()[2].label(),
            "User's profile has inappropriate content"
        );
    }

    #[test]
    fn the_titles_name_what_is_reported() {
        assert_eq!(message().title(), "Tell us what's wrong with this message");
        assert_eq!(user().title(), "Tell us what's wrong with this user");
    }

    #[test]
    fn report_needs_a_reason_and_detail_for_other() {
        let spam = Reason::Content(ContentReportReason::SpamAbuse);
        let other = Reason::Content(ContentReportReason::NoneSpecified);
        assert!(!can_send(None, "detail"));
        assert!(can_send(Some(&spam), ""));
        assert!(!can_send(Some(&other), "  "));
        assert!(can_send(Some(&other), "It's a bot"));
        assert!(can_send(Some(&spam), &"d".repeat(DETAIL_MAX)));
        assert!(!can_send(Some(&spam), &"d".repeat(DETAIL_MAX + 1)));
    }

    #[test]
    fn a_reason_for_another_kind_of_target_makes_no_report() {
        let spam = Reason::User(UserReportReason::SpamAbuse);
        assert_eq!(report_body(&message(), &spam, ""), None);
        let body = report_body(&user(), &spam, " why ").unwrap();
        assert_eq!(body.additional_context, "why");
    }
}
