//! Links into the instance's web client, for "Copy message link".
//!
//! The paths are for-web's routes, which it copies too: `/server/{server}`
//! (for server channels) then `/channel/{channel}/{message}`. Checked against
//! `https://stoat.chat` on 2026-11-06: no `/app` prefix, though the client's
//! files are served from there.

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
