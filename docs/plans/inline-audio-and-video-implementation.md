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

## Chunk 3: Video posters

### Changes Made
- `src/attachment.rs`:
  - `AttachmentView::Video { id, url, name, size_label, width, height, original, spoiler }`, for `Metadata::Video` when there's a file server. `url` is the original's (`MediaUrls::original`). The display size comes from `fit_within` (400 × 300 at most, `FALLBACK_IMAGE_SIZE` without dimensions).
  - `uncovered` reveals spoiler videos as it does images. `file_target()` gives the original, not as a picture (no Copy image).
- `src/ui/video_poster.rs` (new):
  - `VideoPoster`, a GPUI `Asset` keyed by `PosterSource { url, max }`. It runs `media_player::poster` on the background executor, logs a failure (`no poster for {url}: {AVFoundation error}`), and returns a BGRA `RenderImage`, or `None`.
  - `render_image` (RGBA → BGRA) and `poster(url, max) -> Img`, as `thumbhash.rs` does. A unit test checks the channel order and size.
- `src/ui/mod.rs`: `pub mod video_poster`.
- `src/ui/chat.rs`:
  - The `Video` arm:
    - a spoiler is `covered_image` (no ThumbHash, no poster made) until clicked (`RowAction::RevealAttachment`);
    - otherwise `video_poster`, which opens the file in the browser on click until chunk 4;
    - the file actions' right-click menu comes from `render_attachment`'s `file_target()`.
  - `video_poster(id, url, size, colors)`:
    - the poster at the display size, letterboxed (`ObjectFit::Contain`) on black, made at twice that size for Retina;
    - it pulses (`image_loading`) while being made;
    - if it can't be made, it stays black with a "Video" label in the corner;
    - `play_badge()` sits over the middle: the same 48 px round badge as the link-preview thumbnails.
- `tests/store_attachments.rs`:
  - a video is `AttachmentView::Video` at its fitted size (landscape, portrait, small), with the fallback box without dimensions, the original's encoded URL, and a non-picture file target;
  - without a file server it's a card;
  - a `video/*` MIME type without `Video` metadata stays a card (this replaces the old "videos are cards" check);
  - a spoiler video is covered, then revealed by file ID through the store at the same size.

### Divergences
- **No cap on concurrent poster generations.** The plan made a semaphore conditional on posters being slow. A poster took about 0.2 s per video in the spike (locally and over HTTP with ranges), and GPUI's asset cache dedupes by URL and size. It's left uncapped, to revisit if the manual test with a channel of videos is slow.
- **The fallback** is the black box with "Video" in the bottom-left corner, under the play badge, rather than a dark box with a centred label: the badge already sits in the middle.
- **Embedded text-embed media** (a bot's embed whose `media` is an uploaded video) is drawn as a poster too, since it's built by `AttachmentView::new`. Link-preview videos (`Embed::Video`) are untouched and stay cards, as planned.

### Verification Performed
- `cargo test --test store_attachments`: 25 passed. `cargo test --lib video_poster`: 1 passed.
- Full suite, `cargo test`: 90 suites, 1,479 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt:
  - `src/ui/chat.rs` stays at 12;
  - `src/attachment.rs` 0;
  - `tests/store_attachments.rs` 0 (0 at HEAD; formatting it whole only touched the new tests);
  - new `src/ui/video_poster.rs` clean;
  - `src/ui/mod.rs` gained one line, unformatted.

### Manual Validation
Deferred to the batched manual tests at the end:
- an mp4 and a mov show posters at their shape;
- a spoiler video is covered until clicked;
- a webm shows the "Video" box and still opens in the browser;
- the right-click menu has Save…, Copy link and Open in browser.

## Chunk 4: The video player window

### Changes Made
- `src/video_window.rs` (new):
  - `PlayerWindow`, an `NSWindow` subclass (`define_class!`, `RuSToatVideoWindow`). It overrides `performKeyEquivalent:`, so ⌘W closes it via `performClose:`, and `cancelOperation:`, so Escape does too.
  - `VideoWindow`:
    - `open(url, title, original)` makes the window (titled, closable, miniaturizable, resizable; `releasedWhenClosed` off) with an `AVPlayerView` showing a `media_player::Player`, titled with the file's name, sized by `playback::window_size` for the main screen's visible frame, centred, brought forward and playing;
    - `replace(url, title, original)` gives the view a new player (the old one pauses as it's dropped), then retitles, resizes and brings the window forward;
    - `bring_forward()`;
    - `is_open()`: visible, or minimised;
    - `snapshot()`: the player's, for failures.
    - Dropping it pauses, detaches the player and closes the window.
  - Stubs on other platforms (`open` fails).
- `src/media_player.rs`: `Player::av_player()` (crate-only), for the view.
- `src/playback.rs`: `window_size(original, screen)`:
  - the video's own size, grown to at least 320 × 180 (`MIN_VIDEO_WINDOW`), then shrunk to 80% of the screen (`MAX_SCREEN_SHARE`), keeping its shape;
  - 720 × 480 (`UNKNOWN_VIDEO_WINDOW`, the fallback box's 3:2) when the size is unknown;
  - no limit when no screen is reported.
- `src/ui/chat.rs`:
  - `Playing.output: Output { Audio(Player), Video { window: VideoWindow, url } }`. Pause and seek only apply to audio.
  - `RowAction::PlayVideo { file_id, url, name, original }` from the poster's click, handled by `play_video`:
    - the window open on this video comes forward;
    - on another video, it switches to this one;
    - otherwise whatever plays stops and a window opens;
    - if it can't open, a toast appears.
  - `poll_playback` uses `spawn_in`/`update_in`, so `read_player` can show toasts. It keeps polling while the video window is open:
    - a window the user closed (close button, ⌘W, Escape) drops the player;
    - a video that fails is logged, its window closes, and the toast "Can't play this video here." appears with Open in browser (`cant_play_video`, `CANT_PLAY_VIDEO_TEXT`).
  - Audio closes the window (`playback::stops`: another file). Switching account closes it (`set_active(false)`), and so does dropping the view.
- `Cargo.toml`: `objc2-app-kit` gains `NSEvent`, `NSGraphics`, `NSScreen`; `objc2-foundation` gains `NSGeometry`.
- `src/lib.rs`: `pub mod video_window`.
- `tests/playback.rs`: `window_size`:
  - videos that fit keep their size;
  - big landscape and portrait videos shrink to 80%;
  - tiny ones grow to the minimum (and the screen wins);
  - unknown sizes get the fallback, even on a tiny screen;
  - there's no limit without a screen.

### Divergences
- **⌘W and Escape are handled by an `NSWindow` subclass.** The app's menus have no Close item, so a plain `NSWindow` wouldn't get ⌘W at all. The plan's fallbacks were an `NSWindowController` or Escape only; the subclass gives both keys. ⌘Q goes through the app menu's key equivalent as before (to check by hand).
- **No `close()` method:** dropping the `VideoWindow` closes it, and every stop path already drops `Playing`.
- **`bring_forward()`** was added, for clicking the video that's already showing.
- **Minimised counts as open** (`isVisible || isMiniaturized`). Otherwise minimising the window would have stopped playback.
- **Closing the window takes up to 250 ms to stop sound:** the poll notices it, not a window delegate. That avoids a delegate class for a quarter-second.
- **Closing the main window (Keep running) leaves the video window playing**, as decided in chunk 2 (`Change::HideWindow` stops audio only). Switching account still closes it.
- **Embedded media videos** (a bot's text embed with an uploaded video) open in the window too, since they're `AttachmentView::Video`.

### Verification Performed
- `cargo test --test playback`: 14 passed.
- A throwaway example, run once and deleted, not committed. It used video-only files (checked with `ffprobe`, so silent), on the main thread with `NSApplication`:
  - `hevc.mp4` opened (`open=true status=Ready pos=1.13` after 1.2 s);
  - replacing it with `portrait.mov` worked (`Ready`, `pos=0.98`);
  - dropping it closed the window;
  - `webm.webm` opened and reported `Failed`.
- Full suite, `cargo test`: 90 suites, 1,483 passed, 0 failed. `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/ui/chat.rs` 12 → 12 (one import moved into order). New `src/video_window.rs` clean. `src/playback.rs`, `tests/playback.rs` and `src/media_player.rs` clean.

### Manual Validation
Deferred to the batched manual tests at the end:
- an mp4 opens at its shape, playing, with AVKit's controls;
- another video switches the window;
- closing it stops the sound; audio closes it;
- ⌘W and Escape close it, and ⌘Q quits;
- a webm toasts with Open in browser;
- switching account closes it.

## Summary

_Completed: 2026-11-13_

### Chunks Completed
1. Spike: AVFoundation next to GPUI. Bindings, `src/media_player.rs`, the muted spike example and main-thread WAV test; go decision (`5da7f7e`).
2. Audio in the message: play/pause, the seek bar, the time, one at a time, and "Can't play this format here" (`28b3ab0`).
3. Video posters at their own size with a play button; spoiler videos covered (`0ee94f0`).
4. The video player window: AVKit, one per account, replaced by the next video, closed by ⌘W/Escape, with a toast on failure (`25b9dd5`).

### Divergences
- Spike and tests are muted (after one unannounced audible run); `Player::set_muted` / `is_muted` were added.
- The WAV test is a `harness = false` integration test: players need the main thread and its run loop.
- Ogg Vorbis and Opus play on macOS 26; WebM is the unplayable format.
- Audio and video play from the original's own URL (`MediaUrls::original`), not the preview route's unencoded redirect.
- The clean build cost was measured as the new crates' own compile time.
- Polls notify the whole `ChatView`. Channel changes are caught in `render`.
- Closing the main window stops audio, but not a video.
- On other platforms, audio controls show until Play, then say it can't play.
- The audio card isn't clickable; its actions are in the right-click menu.
- No cap on concurrent poster generations.
- The poster fallback is a black box with "Video" in the corner.
- Embedded upload media gets posters and the window too.
- ⌘W and Escape close the video window through an `NSWindow` subclass.
- There's no `close()`; dropping closes. `bring_forward()` was added. Minimised counts as open.
- A closed window's sound stops within 250 ms (polled).

### Definition of Done
- [x] Spike results (formats, streaming, build time, binary size) and the go decision are in this record.
- [~] Audio plays with play/pause, a seek bar (click and drag) and "position / duration". Starting one stops another; changing channel or account stops it. Built and unit-tested; **not checked by hand**.
- [~] Unplayable audio says "Can't play this format here", and unplayable video toasts "Can't play this video here."; both offer Open in browser. Built; the failure path was seen in the spike and the window smoke test; **not checked in the app**.
- [~] Videos show a poster at their metadata's shape (within 400 × 300) with a play button; spoiler videos are covered until clicked. Built and unit-tested; **not checked by hand**.
- [~] Clicking a poster plays the video in a native window with AVKit's controls; another video replaces it; closing stops playback. ⌘W and ⌘Q **not checked by hand**. Opening, replacing and dropping were smoke-tested silently.
- [x] Audio cards and video posters keep the file actions' right-click menu (through `file_target()`, as images).
- [x] `cargo test` (90 suites, 1,483 passed) and `cargo clippy --all-targets -- -D warnings` pass. Drift: `src/ui/chat.rs` 12, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/composer.rs` 13, all unchanged. New files are clean.
- [x] The roadmap links this plan.

### Remaining Manual Steps
The user chose to merge into main without running the manual tests. All five of the plan's manual test groups are still to be done:
- audio playback and the stop rules;
- video posters;
- the player window (including ⌘W and ⌘Q);
- accounts;
- themes.

They also include confirming streaming with real attachments rather than generated files. Turn the volume down first, and don't use the generated test tones in `/tmp/m31`.

Out of scope, found on the way: Save… and the image viewer get the file server's 1280 px webp preview of an image, not the original (`MediaUrls::file` is the preview route).
