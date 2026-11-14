//! Links into the instance's web client: made for "Copy message link", and
//! read back when one is clicked, to open it here instead of the browser.
//!
//! The paths are for-web's routes, which it copies too: `/server/{server}`
//! (for server channels) then `/channel/{channel}/{message}`. Checked against
//! `https://stoat.chat` on 2026-11-06: no `/app` prefix, though the client's
//! files are served from there.

use crate::invites::{InviteHosts, web_url};

/// Where a link into the web client leads: a channel, and in it a message
/// if the link names one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLink {
    pub channel: String,
    pub message: Option<String>,
}

/// A link to `message` in `channel` (in `server`, or `None` for a DM or
/// group), on the web client at `app` (the root config's `app`, e.g.
/// `https://stoat.chat`).
pub fn message_link(app: &str, server: Option<&str>, channel: &str, message: &str) -> String {
    let app = app.trim_end_matches('/');
    match server {
        Some(server) => format!("{app}/server/{server}/channel/{channel}/{message}"),
        None => format!("{app}/channel/{channel}/{message}"),
    }
}

/// The link to share for invite `code`, as Stoat's web client copies it:
/// the short `https://stt.gg/{code}` on the official instance, else
/// `{app}/invite/{code}`.
pub fn invite_link(app: &str, code: &str) -> String {
    if crate::invites::is_official_app(app) {
        return format!("https://stt.gg/{code}");
    }
    format!("{}/invite/{code}", app.trim_end_matches('/'))
}

/// The channel (and message) `link` leads to, if it's a channel or message
/// link on this instance's web client (see [`InviteHosts::is_web_client`]):
/// `/server/{server}/channel/{channel}`, or `/channel/{channel}` for a DM
/// or group, each with an optional `/{message}`. A trailing `/`, query or
/// fragment is ignored. The server isn't kept: the channel says which it's
/// in.
pub fn channel_from_link(link: &str, hosts: &InviteHosts) -> Option<ChannelLink> {
    let url = web_url(link)?;
    if !hosts.is_web_client(&url) {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    let mut parts = path.strip_prefix('/')?.split('/');
    let mut next = parts.next()?;
    if next == "server" {
        parts.next().filter(|server| is_id(server))?;
        next = parts.next()?;
    }
    if next != "channel" {
        return None;
    }
    let channel = parts.next().filter(|channel| is_id(channel))?;
    let message = match parts.next() {
        Some(message) if is_id(message) => Some(message.to_string()),
        Some(_) => return None,
        None => None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(ChannelLink {
        channel: channel.to_string(),
        message,
    })
}

/// The bot `link` adds, if it's a bot invite link on this instance's web
/// client (see [`InviteHosts::is_web_client`]): `/bot/{id}`, as the bot's
/// owner copies it. A trailing `/`, query or fragment is ignored.
pub fn bot_from_link(link: &str, hosts: &InviteHosts) -> Option<String> {
    let url = web_url(link)?;
    if !hosts.is_web_client(&url) {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    let id = path.strip_prefix("/bot/")?;
    is_id(id).then(|| id.to_string())
}

/// Whether `text` has the shape of a Stoat ID: a 26-character ULID.
fn is_id(text: &str) -> bool {
    text.len() == 26 && text.bytes().all(|b| b.is_ascii_alphanumeric())
}
