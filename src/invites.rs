//! Invites: which links and codes are invites, what one leads to, and why
//! joining failed, in words. Plain data with no GPUI, so it's
//! unit-testable.
//!
//! An invite link is `{app}/invite/{code}` on the account's own web client.
//! On the official instance (`https://stoat.chat`), the short `stt.gg` and
//! `rvlt.gg` forms and the older Stoat and Revolt hosts count too, as in
//! Stoat's web clients. A self-hosted instance can't know those codes, so
//! they're ordinary links there.

use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::api::models::{Embed, File, InviteResponse};

/// The official instance's web client: the only one the short links and
/// the older hosts lead to.
pub const OFFICIAL_APP: &str = "https://stoat.chat";

/// Longest code accepted. Stoat's own are 8 characters; a discoverable
/// server's 26-character ID works as one too.
pub const CODE_MAX: usize = 32;

/// Short links: `https://stt.gg/{code}`.
const SHORT_HOSTS: [&str; 2] = ["stt.gg", "rvlt.gg"];
/// Hosts whose `/invite/{code}` leads to the official instance.
const STOAT_HOSTS: [&str; 5] = [
    "stoat.chat",
    "beta.stoat.chat",
    "old.stoat.chat",
    "revolt.chat",
    "app.revolt.chat",
];
/// A path on a short host that isn't an invite.
const NOT_A_CODE: &str = "discover";

/// Most invite cards under one message.
pub const CARDS_MAX: usize = 5;

/// What the Join dialog says when what's typed isn't an invite.
pub const NOT_AN_INVITE: &str = "That isn't an invite link or code.";
/// … when it's an invite on another instance.
pub const OTHER_INSTANCE: &str = "That invite is for a different Stoat instance.";
/// An unknown, expired or used-up invite (Stoat can't tell them apart).
pub const INVALID_INVITE: &str = "This invite is invalid or has expired.";

/// Which links are invites for one account: its web client's host, and
/// whether it's the official instance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InviteHosts {
    /// The web client's host and port, lower case. `None` when the instance
    /// doesn't say where its web client is.
    own: Option<(String, Option<u16>)>,
    official: bool,
}

impl InviteHosts {
    /// The invite links of the instance whose web client is at `app` (the
    /// root config's `app`, e.g. `https://stoat.chat`).
    pub fn new(app: Option<&str>) -> Self {
        let own = app
            .and_then(|app| url::Url::parse(app).ok())
            .and_then(|url| Some((url.host_str()?.to_ascii_lowercase(), url.port())));
        let official = app.is_some_and(is_official_app);
        Self { own, official }
    }

    /// Whether this is the official instance, whose links are `stt.gg`.
    pub fn is_official(&self) -> bool {
        self.official
    }

    /// Whether any link can be an invite (the instance said where its web
    /// client is).
    pub fn has_links(&self) -> bool {
        self.own.is_some() || self.official
    }

    /// The example in the Join dialog's field.
    pub fn placeholder(&self) -> String {
        match &self.own {
            _ if self.official => "stt.gg/wVEJDGVs".into(),
            Some((host, _)) => format!("{host}/invite/wVEJDGVs"),
            None => "wVEJDGVs".into(),
        }
    }
}

/// Whether `app` is the official web client, `https://stoat.chat` (with
/// or without a trailing `/`).
pub fn is_official_app(app: &str) -> bool {
    app.trim_end_matches('/').eq_ignore_ascii_case(OFFICIAL_APP)
}

/// Whether `text` has the shape of a code: 1 to [`CODE_MAX`] ASCII letters
/// and digits.
pub fn is_code(text: &str) -> bool {
    (1..=CODE_MAX).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// `text` as a web address, with or without its `http(s)://`. `None` for
/// other schemes, and for addresses with a user name (`stt.gg@evil.com`).
fn web_url(text: &str) -> Option<url::Url> {
    let lower = text.get(..8).unwrap_or(text).to_ascii_lowercase();
    let has_scheme = lower.starts_with("https://") || lower.starts_with("http://");
    let url = if has_scheme {
        url::Url::parse(text).ok()?
    } else if text.contains("://") {
        return None;
    } else {
        url::Url::parse(&format!("https://{text}")).ok()?
    };
    (url.username().is_empty() && url.password().is_none() && url.host_str().is_some())
        .then_some(url)
}

/// The code in a path of `/{code}` (`short`) or `/invite/{code}`, with any
/// trailing `/`.
fn path_code(path: &str, short: bool) -> Option<&str> {
    let path = path.trim_end_matches('/');
    let code = if short {
        path.strip_prefix('/')?
    } else {
        path.strip_prefix("/invite/")?
    };
    (is_code(code) && !(short && code == NOT_A_CODE)).then_some(code)
}

/// The invite code in `link`, if it's an invite link of this instance
/// (see the module docs). The scheme is optional; a query or fragment is
/// ignored.
pub fn code_from_link(link: &str, hosts: &InviteHosts) -> Option<String> {
    let url = web_url(link)?;
    let host = url.host_str()?.to_ascii_lowercase();
    let port = url.port();
    if hosts.own.as_ref() == Some(&(host.clone(), port))
        && let Some(code) = path_code(url.path(), false)
    {
        return Some(code.to_string());
    }
    if !hosts.official || port.is_some() {
        return None;
    }
    let code = if SHORT_HOSTS.contains(&host.as_str()) {
        path_code(url.path(), true)
    } else if STOAT_HOSTS.contains(&host.as_str()) {
        path_code(url.path(), false)
    } else {
        None
    };
    code.map(str::to_string)
}

/// The codes of the invite links in a message's text, for its cards: in
/// order, each once, at most [`CARDS_MAX`]. Links in code, math or
/// spoilers don't count (a card would give a hidden link away). The scheme
/// is optional, as in revite.
pub fn invite_codes(content: &str, hosts: &InviteHosts) -> Vec<String> {
    let mut codes = Vec::new();
    if !hosts.has_links() || !content.contains('/') {
        return codes;
    }
    for span in crate::markdown::visible_text_spans(content) {
        let words = span.split(|c: char| c.is_whitespace() || "<>()[]\"'".contains(c));
        for word in words {
            let word = word
                .trim_start_matches(['*', '_', '~'])
                .trim_end_matches(['*', '_', '~', '.', ',', '!', '?', ';', ':']);
            if !word.contains('/') {
                continue;
            }
            if let Some(code) = code_from_link(word, hosts)
                && !codes.contains(&code)
            {
                codes.push(code);
                if codes.len() == CARDS_MAX {
                    return codes;
                }
            }
        }
    }
    codes
}

/// Whether `embed` is a website preview of one of the invites in `codes`
/// (its card already shows what it leads to).
pub fn is_invite_embed(embed: &Embed, codes: &[String], hosts: &InviteHosts) -> bool {
    let Embed::Website(site) = embed else {
        return false;
    };
    [&site.url, &site.original_url]
        .into_iter()
        .flatten()
        .filter_map(|url| code_from_link(url, hosts))
        .any(|code| codes.contains(&code))
}

/// What's typed in the Join dialog, as a code: a bare code, or an invite
/// link of this instance. Else what to say under the field.
pub fn parse_input(text: &str, hosts: &InviteHosts) -> Result<String, &'static str> {
    let text = text.trim();
    if is_code(text) {
        return Ok(text.to_string());
    }
    if let Some(code) = code_from_link(text, hosts) {
        return Ok(code);
    }
    // An invite link, but not to here.
    let elsewhere = web_url(text).is_some_and(|url| {
        let short = url
            .host_str()
            .is_some_and(|host| SHORT_HOSTS.contains(&host.to_ascii_lowercase().as_str()));
        path_code(url.path(), false).is_some() || (short && path_code(url.path(), true).is_some())
    });
    Err(if elsewhere {
        OTHER_INSTANCE
    } else {
        NOT_AN_INVITE
    })
}

/// "1 member", "1,234 members".
pub fn member_count_text(count: i64) -> String {
    let digits = count.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (ix, digit) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let sign = if count < 0 { "-" } else { "" };
    let noun = if count == 1 { "member" } else { "members" };
    format!("{sign}{grouped} {noun}")
}

/// What an invite leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InviteTarget {
    /// A server, with about this many members.
    Server { id: String, members: i64 },
    /// A group.
    Group,
}

/// An invite, as the Join dialog and invite cards show it.
#[derive(Debug, Clone, PartialEq)]
pub struct InvitePreview {
    pub code: String,
    pub target: InviteTarget,
    /// The server's name, or the group's.
    pub name: String,
    /// The server's icon (groups have none here).
    pub icon: Option<File>,
    /// The channel it opens: the server channel, or the group.
    pub channel_id: String,
    pub channel_name: String,
    /// Who made it (their username).
    pub inviter: String,
}

impl InvitePreview {
    pub fn new(response: InviteResponse) -> Self {
        match response {
            InviteResponse::Server {
                code,
                server_id,
                server_name,
                server_icon,
                channel_id,
                channel_name,
                user_name,
                member_count,
                ..
            } => Self {
                code,
                target: InviteTarget::Server {
                    id: server_id,
                    members: member_count,
                },
                name: server_name,
                icon: server_icon,
                channel_id,
                channel_name,
                inviter: user_name,
            },
            InviteResponse::Group {
                code,
                channel_id,
                channel_name,
                user_name,
                ..
            } => Self {
                code,
                target: InviteTarget::Group,
                name: channel_name.clone(),
                icon: None,
                channel_id,
                channel_name,
                inviter: user_name,
            },
        }
    }

    /// The line under the name: "1,234 members · #general", or "Group".
    pub fn detail(&self) -> String {
        match &self.target {
            InviteTarget::Server { members, .. } => {
                format!("{} · #{}", member_count_text(*members), self.channel_name)
            }
            InviteTarget::Group => "Group".into(),
        }
    }

    /// The server's ID, for a server invite.
    pub fn server_id(&self) -> Option<&str> {
        match &self.target {
            InviteTarget::Server { id, .. } => Some(id),
            InviteTarget::Group => None,
        }
    }
}

/// Whether Stoat refused a join because the user is already in: the
/// store hadn't caught up, so open it instead.
pub fn already_joined(err: &ApiError) -> bool {
    matches!(
        err.error_type(),
        Some(ErrorType::AlreadyInServer | ErrorType::AlreadyInGroup)
    )
}

/// Why looking up or joining an invite failed, in words.
pub fn join_error_text(err: &ApiError) -> String {
    match err {
        ApiError::RateLimited {
            retry_after_ms: Some(ms),
        } => format!(
            "Stoat is limiting requests. Try again in {} s.",
            ms.div_ceil(1000).max(1)
        ),
        ApiError::RateLimited {
            retry_after_ms: None,
        } => "Stoat is limiting requests. Try again in a moment.".into(),
        ApiError::Api(e) => match &e.error_type {
            ErrorType::NotFound => INVALID_INVITE.into(),
            ErrorType::Banned => "You're banned from this server.".into(),
            ErrorType::TooManyServers { max } => {
                format!(
                    "You're in {max} servers, the most Stoat allows. Leave one to join another."
                )
            }
            ErrorType::GroupTooLarge { max } => {
                format!("This group is full ({max} members at most).")
            }
            _ => "Couldn't join. Try again.".into(),
        },
        ApiError::Network(_) => "Couldn't reach Stoat. Try again.".into(),
        _ => "Couldn't join. Try again.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn official() -> InviteHosts {
        InviteHosts::new(Some("https://stoat.chat"))
    }

    fn api_error(error_type: ErrorType) -> ApiError {
        ApiError::Api(revolt_result::Error {
            error_type,
            location: None,
        })
    }

    #[test]
    fn official_links_give_their_code() {
        let hosts = official();
        for link in [
            "https://stt.gg/wVEJDGVs",
            "stt.gg/wVEJDGVs",
            "http://rvlt.gg/wVEJDGVs",
            "https://stoat.chat/invite/wVEJDGVs/",
            "https://app.revolt.chat/invite/wVEJDGVs?x=1",
            "https://beta.stoat.chat/invite/wVEJDGVs#top",
            "HTTPS://STT.GG/wVEJDGVs",
        ] {
            assert_eq!(
                code_from_link(link, &hosts).as_deref(),
                Some("wVEJDGVs"),
                "{link}"
            );
        }
    }

    #[test]
    fn other_links_on_the_official_instance_are_not_invites() {
        let hosts = official();
        for link in [
            "https://stt.gg/discover",
            "https://evilstt.gg/abc",
            "https://stt.gg.example.com/abc",
            "https://stoat.chat/server/abc",
            "https://stoat.chat/invite/",
            "https://stt.gg/",
            "https://stt.gg/a-b",
            "https://stt.gg/abc/def",
            "https://stt.gg:8080/abc",
            "https://stt.gg@evil.com/abc",
            "https://evil.com@stt.gg/abc",
            "ftp://stt.gg/abc",
            "hello",
        ] {
            assert_eq!(code_from_link(link, &hosts), None, "{link}");
        }
    }

    #[test]
    fn a_self_hosted_instance_only_has_its_own_links() {
        let hosts = InviteHosts::new(Some("https://chat.example.org/"));
        assert!(!hosts.is_official());
        assert_eq!(
            code_from_link("https://chat.example.org/invite/abc", &hosts).as_deref(),
            Some("abc")
        );
        assert_eq!(
            code_from_link("chat.EXAMPLE.org/invite/abc", &hosts).as_deref(),
            Some("abc")
        );
        assert_eq!(code_from_link("https://stt.gg/abc", &hosts), None);
        assert_eq!(
            code_from_link("https://stoat.chat/invite/abc", &hosts),
            None
        );
    }

    #[test]
    fn a_port_is_part_of_the_host() {
        let hosts = InviteHosts::new(Some("http://localhost:5173"));
        assert_eq!(
            code_from_link("http://localhost:5173/invite/abc", &hosts).as_deref(),
            Some("abc")
        );
        assert_eq!(code_from_link("http://localhost/invite/abc", &hosts), None);
    }

    #[test]
    fn without_an_app_only_bare_codes_work() {
        let hosts = InviteHosts::new(None);
        assert_eq!(code_from_link("https://stt.gg/abc", &hosts), None);
        assert_eq!(parse_input(" abc ", &hosts).as_deref(), Ok("abc"));
        assert_eq!(hosts.placeholder(), "wVEJDGVs");
    }

    #[test]
    fn the_official_app_is_recognised_with_or_without_a_slash() {
        assert!(is_official_app("https://stoat.chat"));
        assert!(is_official_app("https://stoat.chat/"));
        assert!(!is_official_app("https://beta.stoat.chat"));
        assert_eq!(official().placeholder(), "stt.gg/wVEJDGVs");
        assert_eq!(
            InviteHosts::new(Some("https://chat.example.org")).placeholder(),
            "chat.example.org/invite/wVEJDGVs"
        );
    }

    #[test]
    fn input_is_a_code_or_a_link() {
        let hosts = official();
        assert_eq!(parse_input(" wVEJDGVs ", &hosts).as_deref(), Ok("wVEJDGVs"));
        assert_eq!(
            parse_input("01J7Z3K5W9QXHCV4N8TBRM2E6F", &hosts).as_deref(),
            Ok("01J7Z3K5W9QXHCV4N8TBRM2E6F")
        );
        assert_eq!(
            parse_input("https://stt.gg/wVEJDGVs", &hosts).as_deref(),
            Ok("wVEJDGVs")
        );
        assert_eq!(parse_input("two words", &hosts), Err(NOT_AN_INVITE));
        assert_eq!(parse_input("", &hosts), Err(NOT_AN_INVITE));
        assert_eq!(
            parse_input("https://stoat.chat/server/abc", &hosts),
            Err(NOT_AN_INVITE)
        );
        assert_eq!(
            parse_input("https://other.example/invite/abc", &hosts),
            Err(OTHER_INSTANCE)
        );
        assert_eq!(parse_input(&"a".repeat(33), &hosts), Err(NOT_AN_INVITE));
    }

    #[test]
    fn short_links_are_another_instance_on_a_self_hosted_one() {
        let hosts = InviteHosts::new(Some("https://chat.example.org"));
        assert_eq!(parse_input("stt.gg/abc", &hosts), Err(OTHER_INSTANCE));
    }

    #[test]
    fn invite_links_in_text_give_cards() {
        let hosts = official();
        let codes = |text: &str| invite_codes(text, &hosts);
        assert_eq!(codes("join https://stt.gg/abc!"), ["abc"]);
        assert_eq!(codes("(stt.gg/abc), <https://stt.gg/def>"), ["abc", "def"]);
        assert_eq!(
            codes("[here](https://stt.gg/abc) and **stt.gg/def**"),
            ["abc", "def"]
        );
        assert_eq!(codes("stt.gg/abc then https://stt.gg/abc"), ["abc"]);
        let seven = (1..=7)
            .map(|n| format!("stt.gg/c{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(codes(&seven), ["c1", "c2", "c3", "c4", "c5"]);
        assert!(codes("`https://stt.gg/abc`").is_empty());
        assert!(codes("```\nstt.gg/abc\n```").is_empty());
        assert!(codes("||stt.gg/abc||").is_empty());
        assert_eq!(codes("||secret|| stt.gg/abc"), ["abc"]);
        assert!(codes("$$stt.gg/abc$$").is_empty());
        assert!(codes("https://stt.gg/discover").is_empty());
        assert!(codes("no links here").is_empty());
    }

    #[test]
    fn a_self_hosted_instance_only_cards_its_own_links() {
        let hosts = InviteHosts::new(Some("https://chat.example.org"));
        assert!(invite_codes("stt.gg/abc", &hosts).is_empty());
        assert_eq!(
            invite_codes("https://chat.example.org/invite/abc", &hosts),
            ["abc"]
        );
        assert!(
            invite_codes(
                "https://chat.example.org/invite/abc",
                &InviteHosts::new(None)
            )
            .is_empty()
        );
    }

    #[test]
    fn member_counts_are_grouped_and_counted() {
        assert_eq!(member_count_text(0), "0 members");
        assert_eq!(member_count_text(1), "1 member");
        assert_eq!(member_count_text(999), "999 members");
        assert_eq!(member_count_text(1234), "1,234 members");
        assert_eq!(member_count_text(1_234_567), "1,234,567 members");
    }

    #[test]
    fn previews_of_servers_and_groups() {
        let server: InviteResponse = serde_json::from_value(serde_json::json!({
            "type": "Server", "code": "abc", "server_id": "s1", "server_name": "Rust",
            "channel_id": "c1", "channel_name": "general", "user_name": "ada",
            "member_count": 1234
        }))
        .unwrap();
        let preview = InvitePreview::new(server);
        assert_eq!(preview.server_id(), Some("s1"));
        assert_eq!(preview.name, "Rust");
        assert_eq!(preview.detail(), "1,234 members · #general");
        assert_eq!(preview.inviter, "ada");
        assert!(preview.icon.is_none());

        let group: InviteResponse = serde_json::from_value(serde_json::json!({
            "type": "Group", "code": "def", "channel_id": "g1", "channel_name": "Friends",
            "user_name": "ada"
        }))
        .unwrap();
        let preview = InvitePreview::new(group);
        assert_eq!(preview.server_id(), None);
        assert_eq!(preview.name, "Friends");
        assert_eq!(preview.channel_id, "g1");
        assert_eq!(preview.detail(), "Group");
    }

    #[test]
    fn join_errors_in_words() {
        assert_eq!(
            join_error_text(&api_error(ErrorType::NotFound)),
            INVALID_INVITE
        );
        assert_eq!(
            join_error_text(&api_error(ErrorType::Banned)),
            "You're banned from this server."
        );
        assert_eq!(
            join_error_text(&api_error(ErrorType::TooManyServers { max: 100 })),
            "You're in 100 servers, the most Stoat allows. Leave one to join another."
        );
        assert_eq!(
            join_error_text(&api_error(ErrorType::GroupTooLarge { max: 50 })),
            "This group is full (50 members at most)."
        );
        assert_eq!(
            join_error_text(&ApiError::RateLimited {
                retry_after_ms: Some(2500)
            }),
            "Stoat is limiting requests. Try again in 3 s."
        );
        assert_eq!(
            join_error_text(&api_error(ErrorType::InternalError)),
            "Couldn't join. Try again."
        );
    }

    #[test]
    fn already_in_is_not_an_error() {
        assert!(already_joined(&api_error(ErrorType::AlreadyInServer)));
        assert!(already_joined(&api_error(ErrorType::AlreadyInGroup)));
        assert!(!already_joined(&api_error(ErrorType::Banned)));
    }
}
