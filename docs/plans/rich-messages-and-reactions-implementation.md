# Implementation Record: Rich messages and reactions (RuSToat milestone 2)

_Plan: docs/plans/rich-messages-and-reactions.md_
_Started: 2026-11-04_

## Chunk 1: Avatars and media URLs

### Changes Made
- `Cargo.toml`: added `url = "2"` (already in the dependency tree) for percent-encoding.
- `src/api/client.rs`: `RootFeatures.january`, `RootConfig::proxy_server()`, and a shared `enabled_url` helper.
- `src/media.rs` (new): `MediaUrls { api, files, proxy }` with trailing slashes trimmed.
  - `from_root`
  - `file(&File)` gives `{files}/{tag}/{id}`
  - `file_by_id(tag, id)`
  - `emoji(id)` gives `{files}/emojis/{id}`
  - `default_avatar(user)` gives `{api}/users/{id}/default_avatar`
  - `external(url)`: own file-server URLs are used directly; anything else goes through `{proxy}/proxy?url=<form-encoded>`. A look-alike host such as `cdn….evil.example` isn't treated as ours, because it's matched on `"{files}/"`. With no proxy, external images aren't loaded.
- `src/store.rs`:
  - `ChatStore.media`, `set_media()` and `media()`.
  - `user_avatar_url(id)` returns the user's avatar or their generated default.
  - `avatar_url(&Message)` follows plan Assumption 7: masquerade avatar (via `external`), then webhook avatar (`avatars/{id}`, or `None` because webhooks have no default), then server member avatar (embedded or from the store), then the embedded `message.user` avatar (only if the user isn't cached), then `user_avatar_url`. System messages get `None`.
  - `MessageRow.avatar` and `ChannelEntry.avatar`: DMs carry the other person's avatar, groups their icon file, and text channels and Saved Notes `None`.
- `src/ui/chat.rs`:
  - `ChatView.media: MediaUrls` replaces `file_server`. It's seeded with the API base at construction, so default avatars work at once. `set_media()` runs after `GET /` loads and re-syncs loaded panes, so avatar fields fill in. Server icons now use `media.file()`.
  - New `avatar(url, name, size, bg, fg)`: a round `img()` with an initial-letter placeholder for loading, fallback, or no URL.
  - `render_message_row` is now `h_flex[gutter 36 px, content]`. Header rows show the avatar; continuation rows keep an empty gutter so text lines up. System rows put the "→" in the same gutter.
  - DM and group rows in the channel list show a 20 px avatar instead of the glyph when there is one. The user panel shows your 32 px avatar next to your name and status.
- `tests/media_urls.rs` (new), 5 tests:
  - file, emoji and default-avatar URLs, including with no file server
  - external proxying and encoding (space, `&`, `é`), own-CDN passthrough, the look-alike host, and no-proxy giving `None`
  - avatar precedence for masquerade, webhook with and without an avatar, member over user in a server, user in a DM, the default, and system
  - DM entry avatars
  - timeline rows carrying avatars
- `tests/live_media.rs` (new): 2 opt-in live tests. `default_avatar` returns an image without auth for any ULID-shaped ID, and the January proxy serves a public PNG.
- `tests/store_live.rs`: the test row helper gains `avatar: None`.

### Divergences
- **Minor: webhook avatar tag.** `MessageWebhook.avatar` is a bare file ID, not a `File`, so the URL is built as `{files}/avatars/{id}`, assuming webhook avatars use the `avatars` tag. This was not separately verified live, because no webhook avatar ID was at hand.
- **Minor:** the embedded `message.user` avatar is a fallback for authors who aren't in the cache. It isn't in the plan's precedence list.
- **Minor: the live test uses a made-up ID.** `default_avatar` works for any ULID-shaped ID (the server generates the image), so the live test doesn't contain a real account ID.
- No effect on later chunks. Chunk 3 can use `MediaUrls::emoji`, and chunk 5 can use `MediaUrls::external`, as planned.

### Verification Performed
- `cargo test --test media_urls`: 5 passed.
- `cargo test --test live_media -- --ignored`: 2 passed against the live services.
- `cargo test`: 86 passed, 0 failed, 5 ignored:
  - 27 unit tests
  - 9 `api_login`
  - 7 contract
  - 5 `media_urls`
  - 10 `session_restore`
  - 8 `store_history`
  - 12 `store_live`
  - 8 `store_ready`
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Live run: the app logged `media: MediaUrls { api: "https://api.stoat.chat", files: Some("https://cdn.stoatusercontent.com"), proxy: Some("https://proxy.stoatusercontent.com") }`. It then fetched 7 uploaded avatars (`cdn…/avatars/<file>`) and 1 generated default (`api…/users/<id>/default_avatar`), with no warnings or errors.

### Manual Validation
The user checked the running app and confirmed "Looks right": avatars match the web app.
