# Implementation Record: Inline audio and video (RuSToat milestone 31)

_Plan: docs/plans/inline-audio-and-video.md_
_Started: 2026-11-13_

Supervision: continuous, with the manual tests batched at the end (user's choice). The plan and the roadmap's link to it were uncommitted when this started; they're committed with chunk 1.

**Nothing makes a sound unless the user runs it.** One early spike run played an 8-second 440 Hz tone through the speakers without warning, which was uncomfortable for the user. From then on, every player in the spike and the tests is muted (`Player::set_muted`); the spike plays aloud only with `--audible`, and the test asserts it's muted before playing.

## Chunk 1: Spike: AVFoundation next to GPUI

### Changes Made
- `Cargo.toml` (macOS section):
  - `objc2-av-foundation` 0.3.2 (default features off; `AVAsset`, `AVAssetImageGenerator`, `AVPlayer`, `AVPlayerItem`, `AVTime`, and its CoreFoundation, CoreGraphics and CoreMedia parts);
  - `objc2-core-media` 0.3.2 (`CMTime`);
  - `objc2-core-graphics` 0.3.2 (`CGBitmapContext`, `CGColorSpace`, `CGContext`, `CGImage`), for reading a poster's pixels;
  - `objc2-av-kit` 0.3.2 (`AVPlayerView`), for chunk 4;
  - `objc2-foundation` gains `NSArray`, `NSDate`, `NSRunLoop`, `NSURL`.
  - A `[[test]] media_player` with `harness = false` (see Divergences).
- `Cargo.lock`: adds `objc2-av-foundation`, `objc2-av-kit`, `objc2-core-media`, `objc2-core-audio`, `objc2-core-audio-types`.
- `src/media_player.rs` (new):
  - `PlayerStatus { Loading, Ready, Failed(String) }`; `PlayerSnapshot { status, position, duration, playing, ended }` with `LOADING`.
  - `Player::open(url)` (main thread only, else `Err`), `play`, `pause`, `seek(seconds)` (zero tolerance, so the time shown is the time asked for), `set_muted`, `is_muted`, `snapshot()`. Dropping a player pauses it.
  - `poster(url, max) -> Result<RgbaImage, String>`: `AVAssetImageGenerator` at 0.5 s with zero tolerance, falling back to 0 s; preferred track transform applied; drawn into an RGBA bitmap.
  - `run_main_loop_for(duration)`, for code waiting on AppKit outside an app.
  - Errors read "domain code: description (reason) <- underlying…". `underlyingErrors` is only called where it exists (macOS 11.3+; the app's minimum is 11.0).
  - A failed player reports `playing: false` (AVFoundation still says it's waiting to play).
  - Stubs on other platforms: `open` and `poster` fail with "playback needs macOS".
  - A unit test for `at_end`.
- `src/lib.rs`: `pub mod media_player`.
- `examples/media_spike.rs` (new): `cargo run --example media_spike -- [--audible] [--sleep] <url> [poster.png]`. It's muted unless `--audible` is given, and stops if muting didn't take. `--sleep` sleeps between polls instead of running the run loop.
- `tests/media_player.rs` (new, `harness = false`, muted). On the main thread:
  - a player can't be opened off the main thread;
  - a generated 1-second 8 kHz silent WAV reaches `Ready` with a duration of 1.0 ± 0.05 s, paused at 0;
  - a junk `.wav` fails with an `AVFoundationErrorDomain` error and isn't "playing".
- `docs/plans/inline-audio-and-video.md`: amended (see Divergences). `docs/plans/roadmap.md`: links the plan.

### Spike results

**Polling works, but only while the main run loop runs.**
- With the run loop running between 250 ms polls, `status`, `currentTime`, `duration` and `timeControlStatus` all update. Every file was `Ready` by the first poll (≤ 250 ms).
- With the thread sleeping instead (`--sleep`), nothing updates: m4a, mp4 and webm all stayed `Loading` with no duration for 5 s. AVFoundation delivers its state on the main queue.
- GPUI's main thread runs the run loop, so the app can poll as planned with no KVO. Tests have to run on the main thread and run the loop themselves (hence `harness = false`).

**Formats** (local files made with ffmpeg, played from `file://`, muted; macOS 26.6.2):

| File | Plays | Duration | Poster |
|---|---|---|---|
| mp3 | yes | 8.05 s | – |
| m4a (AAC) | yes | 7.98 s | – |
| wav | yes | 8.00 s | – |
| flac | yes | 8.05 s | – |
| ogg (Vorbis) | **yes** | 8.18 s | – |
| opus (Ogg Opus, `.opus` and `.ogg`) | **yes** | 8.24 s | – |
| webm (Opus audio only) | no: `AVFoundationErrorDomain -11828: Cannot Open (This media format is not supported.) <- NSOSStatusErrorDomain -12847` | – | – |
| mp4 (H.264 + AAC) | yes | 6.00 s | 640×360 in ~0.2 s |
| mp4 (HEVC, `hvc1`) | yes | 6.00 s | 640×360 in ~0.2 s |
| mov (H.264, 360×640 portrait) | yes | 6.00 s | 337×600 (upright, fitted within 800×600) in ~0.2 s |
| webm (VP9) | no: the same -11828 / -12847, within ~7 ms | – | fails with the same error |
| mp4 shorter than 0.5 s (0.2 s) | yes; reports `ended` | 0.20 s | its first frame (the 0.5 s request falls back) |

- Ogg Vorbis and Opus play on macOS 26, against the plan's expectation. They may not on older systems; if not, they fail like webm and fall back to the card. **WebM is the format that doesn't play** (audio or video).
- Seeking to the middle and playing on worked for every playable file.
- Posters matched an ffmpeg-extracted reference frame at 0.5 s (colours, orientation). With the generator's default tolerance, they came from the nearest keyframe (frame 0) instead; zero tolerance fixed it at the same ~0.2 s.

**Streaming: yes. The live CDN answers byte ranges, even on a cache miss.**
- autumn itself (0.16.4, `crates/services/autumn/src/api.rs` `fetch_file`) always sends the whole file with `Content-Type` and `Content-Disposition: attachment`, and no `Accept-Ranges`.
- A local server that does the same (whole body, `200`, never `206`) breaks AVPlayer for most formats:
  - m4a, wav, mp4 and mov fail with `AVFoundationErrorDomain -11850: Operation Stopped (The server is not correctly configured.) <- NSOSStatusErrorDomain -12939`, and so do their posters;
  - only mp3 and ogg (readable front to back) played.
  - A local server that honours `Range` played everything the `file://` run did, and made the same posters (2–7 requests each).
- `cdn.stoatusercontent.com` is Cloudflare in front of autumn. A range request for a public file on the original's route (the "Stoat Discussion" server's icon, `/icons/{id}/original` → 308 → `/icons/{id}/{filename}`) came back `206 Partial Content` with `Content-Range: bytes 100-199/45341` on `cf-cache-status: MISS` and again on `HIT`. The banner (1,087,452 B) did the same.
- So Assumption 2's main path stands: stream the CDN's URL, and no download fallback is built. Not yet confirmed with an actual audio or video attachment; that's in the batched manual tests.

**Which URL to play:**
- `MediaUrls::file` builds `{files}/{tag}/{id}`, which is autumn's preview route (`fetch_preview`):
  - for images, it serves a webp preview (at most 1280×1280 for attachments, `Revolt.toml` `attachments = [1280, 1280]`), not the original;
  - for audio and video, it answers `308` to `/{tag}/{id}/{filename}` with the filename **not percent-encoded** (unlike `/original`'s redirect).
- Audio and video should use the original's own URL, `{files}/{tag}/{id}/{percent-encoded filename}`. That skips the redirect and its unencoded `Location`, and it's the route that was seen answering ranges. The plan is amended to add `MediaUrls::original` in chunk 2.
- Not changed here (outside this milestone): Save… and the image viewer use `MediaUrls::file`, so they get the 1280 px webp preview of an image, not the original.

**Build and size** (aarch64, release):

| Measure | Before | After | Change |
|---|---|---|---|
| Release binary (`target/release/stoat-gpui`) | 84,158,128 B | 84,162,576 B | +4,448 B (nothing calls AVKit yet; AVFoundation and CoreMedia are linked dynamically, AVKit isn't linked until it's used) |
| The three new binding crates, compiled alone (release, their dependencies already built) | – | 0.8 s | ≈ +1 s on a clean build |
| Release rebuild after adding them (rebuilt `objc2-app-kit` and GPUI for the new features, then the app) | 1 min 15 s (HEAD, app only) | 1 min 1 s | within noise |

A fully clean release build wasn't timed: `cargo clean` deletes the 277 MB WebRTC download (see the voice spike's record), and the new crates' own cost is measured above instead.

**Decision: go for chunks 2–4**, as planned:
- poll on a 250 ms GPUI timer (no KVO);
- stream the CDN's original URL (no download fallback);
- WebM falls back to the card ("Can't play this format here") and the video toast.

### Divergences
- **The WAV test is a `harness = false` integration test** (`tests/media_player.rs`), not a unit test in `src/media_player.rs`. `AVPlayer` is main-thread-only in objc2 (`MainThreadMarker`), and it only updates while the main run loop runs. Unit tests run on worker threads. The plan's Test Plan is amended.
- **Players are muted in the spike and tests**, after the unannounced audible run described at the top. `Player::set_muted` / `is_muted` were added, which the plan didn't list. The plan's Test Plan now says no test or spike run makes a sound.
- **`--sleep` in the spike example**, to answer the run-loop risk directly.
- **Ogg Vorbis and Opus play** (on macOS 26). The plan's manual checks used ogg as the unplayable example; they're amended to use webm.
- **Audio and video need the original's URL** (`MediaUrls::original`, `{files}/{tag}/{id}/{encoded filename}`), not `MediaUrls::file`. Chunks 2 and 3 are amended to add and use it.
- **Clean build time** measured as the new crates' own compile time, not a full `cargo clean` build (see above).
- **Format and streaming checks used generated files** (`file://`, a local server with and without ranges) and a public CDN file's headers, not uploads to a test channel. Confirming with real attachments is in the batched manual tests.

### Verification Performed
- `cargo build --example media_spike`: builds.
- The spike over every format, muted, from `file://` and from `http://127.0.0.1` with and without byte ranges: results above.
- Posters compared by eye with ffmpeg's frame at 0.5 s (`portrait.mov`): same frame, colours and orientation.
- `curl -r 100-199` against the CDN (icon and banner, on the original's route and the preview route): `206` on MISS and HIT.
- `cargo build --release`: binary sizes above. `otool -L`: AVFoundation and CoreMedia linked.
- `cargo test --test media_player`: `media_player: ok (1.000 s; junk: AVFoundationErrorDomain -11800 …)`.
- Full suite, `cargo test`: 89 suites, 1,463 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.
- The `ld: __eh_frame section too large` warning in debug builds was already there at HEAD (checked in a scratch worktree); it isn't from this change.
- rustfmt: the new files (`src/media_player.rs`, `examples/media_spike.rs`, `tests/media_player.rs`) are clean. `lib.rs` was edited by hand, and no other `.rs` file was touched.

### Manual Validation
Deferred to the end, batched (user's choice): play real mp3, m4a, ogg, mp4 and webm attachments from a test channel, by link, through the spike (`--audible` only when the user runs it).

## Chunk 2: Audio in the message

### Changes Made
- `src/playback.rs` (new, plain data):
  - `MediaKind { Audio, Video }`, `NowPlaying { channel, file_id, kind }`;
  - `Change { Play(&NowPlaying), OpenChannel(Option<&str>), SwitchAccount, HideWindow }` and `stops(current, change)`:
    - another file stops what's playing, and the same file resumes;
    - another channel (or none) stops audio but not a video;
    - closing (hiding) the main window stops audio;
    - switching account stops anything.
  - `time_label(position, duration)` ("0:42 / 3:15", "1:02 / 1:02:03", "–:–– / –:––" while the length is unknown; the position never shows past the length; NaN reads as 0) and `clock(seconds)`;
  - `seek_target(fraction, duration)` (clamped; NaN reads as 0), `progress(position, duration)` (the bar's fill), and `fraction_along(x, start, width)` (a click or drag on the bar).
- `src/media.rs`: `MediaUrls::original(file)`, `{files}/{tag}/{id}/{name}`, with the name percent-encoded (everything but RFC 3986's unreserved characters).
- `src/attachment.rs`: `AttachmentView::Audio { id, url, name, size_label, spoiler }`, for `Metadata::Audio` when there's a file server. `url` is the original's. `file_target()` gives that URL (not a picture).
- `src/ui/chat.rs`:
  - `ChatView` has three new fields:
    - `playing: Option<Playing { now, player, snapshot }>`;
    - `playback_polling`;
    - `unplayable: Rc<HashSet<String>>` (file IDs, for the session).
  - `RowAction::{PlayAudio { file_id, url }, PauseAudio, SeekAudio(fraction)}`, handled by `play_audio`, `pause_playback`, `seek_playback`. No `Player` exists until play is pressed.
  - `poll_playback` / `read_player`:
    - reads the player at once, then every 250 ms (`PLAYBACK_POLL`) on a detached task, while it's playing or loading;
    - stops polling when it's paused, gone, or the view is dropped;
    - on a failure, logs AVFoundation's error (`can't play {file}: {domain code: …}`), marks the file unplayable and drops the player;
    - at the end, pauses and goes back to 0:00.
  - `stop_playback(change)` drops the player when `playback::stops` says so (dropping a `Player` pauses it). It's called:
    - at the top of `render`, with the open channel (`OpenChannel`), which catches every way of changing channel;
    - from `set_active(false)` (`SwitchAccount`);
    - from `AppRoot::window_hidden` (`HideWindow`).
    - Signing out drops the view, and the player with it.
  - `Pickers.playback: PlaybackRows { current: Option<(file ID, snapshot)>, unplayable }`, so the playing row redraws without its row data changing. Search results and embeds get `PlaybackRows::default()` or no pickers.
  - `render_attachment` / `attachment_element` take `Option<&Pickers>` instead of `Option<&TextScrolls>` (text previews read `pickers.text_scrolls`).
  - The `Audio` arm:
    - the card (music icon, name, size, spoiler tag), then a 24 px line, 420 px wide (`AUDIO_WIDTH`);
    - the line holds a play/pause button (with tooltip), the seek bar and the time label;
    - an unplayable file's line reads "Can't play this format here" with Open in browser, at the same height;
    - without pickers (embeds, search results), it's the card, which opens the file.
    - The right-click menu wraps the whole block.
  - `seek_bar`:
    - a 4 px track in a 16 px hit strip, filled in the theme's primary colour;
    - only the file playing or paused (with a known length) is seekable;
    - a click seeks via the bar's bounds, recorded by a `canvas` while it's laid out, and stops propagation;
    - a drag (`on_drag(SeekDrag)` with an empty preview, `on_drag_move` filtered by file ID) seeks as it moves.
    - GPUI calls `on_drag`'s pending-press listener before `on_mouse_down`'s in the bubble phase, so stopping propagation doesn't block the drag.
  - `cant_play_line`, `seek_to`, and constants `AUDIO_WIDTH`, `AUDIO_LINE_HEIGHT`, `SEEK_TRACK_HEIGHT`, `SEEK_HIT_HEIGHT`.
- `src/ui/app_root.rs`: `window_hidden()` stops audio in every account's view.
- `src/main.rs`: the close handler, when it hides the window (Keep running on), calls `AppRoot::window_hidden` through `MainWindow.current`.
- `src/lib.rs`: `pub mod playback`.
- `tests/playback.rs` (new): clocks, time labels (0, 59 s, 1 h, unknown, NaN, position past the end), seek targets, the bar's fill, click fractions, and the stop rules for a new play, a channel change, an account switch and closing the window.
- `tests/store_attachments.rs`:
  - an `Audio` file becomes `AttachmentView::Audio` with the original's encoded URL, its file target and the spoiler tag (shown without the prefix, unaffected by revealing);
  - without a file server it's a card with no link;
  - an `audio/*` MIME type without `Audio` metadata stays a card;
  - timeline rows carry it.
- `tests/media_urls.rs`: `original` encodes spaces, `#`, brackets, `/` and non-ASCII, and keeps the spoiler prefix; there's no URL without a file server.

### Divergences
- **Redraws:** each poll notifies the whole `ChatView`, as the relative-time tick does, not "only the pane": panes aren't their own entities. The notify happens only when the snapshot changed, and only while something plays.
- **Channel changes are caught in `render`**, not at each place the channel changes (there are several: `select_channel`, places, `open_channel`, Friends). `stop_playback` is a no-op unless audio from another channel is playing.
- **Closing the window** needed `AppRoot::window_hidden` and a line in `main.rs`'s close handler, neither listed in the plan. With Keep running off, closing quits, and the player goes with the view.
- **Closing the window stops audio but not a video** (`Change::HideWindow`), following Assumption 3's "closing the window stops audio". The video window is separate.
- **`render_attachment`'s last parameter is now `Option<&Pickers>`**, so the audio arm can reach `Pickers.playback`. Chunk 3's video arm can use the same.
- **On other platforms**, an audio row shows its controls until Play is pressed, and only then says "Can't play this format here" (`Player::open` fails). The plan said such files fall back to the card at once. Only macOS is shipped.
- **The audio card itself isn't clickable** in the timeline (its player is the action). Save…, Copy link and Open in browser stay in its right-click menu, and on the unplayable line.

### Verification Performed
- `cargo test --test playback --test store_attachments --test media_urls`: 10, 23 and 6 passed.
- Full suite, `cargo test`: 90 suites, 1,476 passed, 0 failed. It includes `media_player` (muted). Nothing in it plays aloud: the view's players aren't exercised by tests.
- `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt:
  - `src/ui/chat.rs` 12 → 12; my two hunks were formatted by hand.
  - `src/media.rs`, `src/attachment.rs`, `src/ui/app_root.rs`, `src/main.rs` (checked only), `tests/store_attachments.rs`, `tests/media_urls.rs`: 0 → 0.
  - New `src/playback.rs` and `tests/playback.rs`: clean.

### Manual Validation
Deferred to the batched manual tests at the end (user's choice):
- play an mp3 and an m4a: pause, seek by click and drag, play to the end;
- a second file stops the first; changing channel or account stops it;
- a webm says "Can't play this format here".
