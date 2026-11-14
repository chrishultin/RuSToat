# Plan: Inline audio and video (RuSToat milestone 31)

## Summary
Milestone 31 of the [roadmap](roadmap.md) plays audio and video attachments:
- **audio** plays in the message, through AVFoundation, with play/pause, a seek bar and the time, drawn in GPUI;
- **video** shows a poster frame at its own size with a play button, which opens a native player window (AVKit).

Files that AVFoundation can't play keep their card, and say so.

## Approach
**What Stoat does** (0.16.4, `/tmp/for-web-0.16.3` and the backend's models, read on 2026-11-13):
- **Metadata:** an upload's metadata is `Video { width, height }` or `Audio`. There's no thumbnail or poster for a video, and no duration for either.
- **The web client** (`elements/Attachment.tsx`):
  - draws a video as `<video controls preload="metadata">` at its metadata size, covered by `Spoiler` when it's a spoiler;
  - draws audio as a card (`FileInfo`: name and size) with an `<audio controls>` bar under it, 420 px wide;
  - plays both from `{autumn}/attachments/{id}`.

**What exists in RuSToat** (as of 2026-11-13, after milestone 30):
- **`src/attachment.rs`:** audio and video are `AttachmentView::File` with `kind: FileKind::Audio | Video`. That's a card that opens the browser, with the file actions' right-click menu.
- **`AttachmentView::Image`** is the pattern for something drawn at a fixed size from its metadata (`fit_within`), with `spoiler` covering it (`covered_image` in `src/ui/chat.rs`).
- **Row heights:** they come from row data. Anything that changes while a row is on screen without changing its height (like invite cards' states) reaches the row through `Pickers`, not the store.
- **macOS bindings:** `objc2` 0.6, with `objc2-foundation` / `objc2-app-kit` 0.3 (`NSSound`, `NSWindow`). There are no AVFoundation bindings yet. `objc2-av-foundation`, `objc2-av-kit` and `objc2-core-media` 0.3.2 match these versions.
- **`src/ui/thumbhash.rs`:** a GPUI `Asset` that turns bytes into a cached `RenderImage` (BGRA). Poster frames can be cached the same way.
- **`LSMinimumSystemVersion`** is 11.0 (`scripts/bundle.sh`).

**The chosen shape** (user's choices on 2026-11-13):
- **Unplayable formats** (Ogg, Opus, WebM, …) aren't decoded by us. When AVFoundation says it can't play a file, its card stays, with "Can't play this format here" and Open in browser. A Rust decoder (symphonia + cpal) was considered and rejected: it's a second playback path for a minority of files.
- **One thing plays at a time:**
  - starting audio or a video stops whatever else is playing;
  - leaving the channel, switching account or closing the window stops audio;
  - there's no "now playing" control elsewhere.
- **Video means uploaded video attachments only.** Video links in link previews stay cards that open the browser. Video drawn inside the message is out of scope (the roadmap sizes it L).
- **Code split:**
  - `src/media_player.rs` (new, `objc2`, no GPUI): `AVPlayer` and poster frames;
  - `src/video_window.rs` (new, `objc2`, no GPUI): the native player window;
  - `src/playback.rs` (new, plain data): what's playing, times and seeking maths, all unit-tested;
  - `src/ui/chat.rs` draws everything and drives the player.
  - On other platforms, `media_player` and `video_window` are stubs that report "can't play", so everything falls back to the card.

## Assumptions
1. **Playback polls; it doesn't observe.** While something plays, a GPUI timer reads the player every 250 ms (status, current time, duration, rate), and stops when nothing does. That avoids KVO and `block2` time observers. The spike confirms the properties update without a run-loop observer.
2. **Streaming first, downloading as the fallback.** `AVPlayer` plays the file server's URL directly (it's public, with no token). If the spike finds the file server can't be streamed (no byte ranges, a wrong `Content-Type`), the file is downloaded to a temporary file first (attachments are at most 20 MB) and played from there. Temporary files are removed like pasted images (`crate::draft::remove_temporary`).
3. **The audio row** keeps one height in every state:
   - the card's line (icon, name, size);
   - under it, a 420 px controls line: a play/pause button, a seek bar, and "0:42 / 3:15" (or "–:–– / –:––" before the duration is known).
   - Clicking or dragging on the bar seeks.
   - Unplayable: the controls line becomes "Can't play this format here" with Open in browser.
4. **Before it's started, audio isn't loaded:** no `AVPlayer` exists until play is pressed, so a channel full of audio costs nothing. The duration shows once it's playing (Stoat's metadata has none).
5. **A video's display size** comes from its metadata, through `fit_within` (400 × 300 at most), else `FALLBACK_IMAGE_SIZE`. So the row never changes height when the poster arrives.
6. **Poster frames** are made with `AVAssetImageGenerator` at 0.5 s (or the first frame of a shorter video), scaled to at most twice the display size, on a background thread. They're cached as a GPUI asset keyed by URL.
   - While the poster is coming, the box pulses (`image_loading`).
   - If no poster can be made, the box stays dark with a "Video" label; the play button stays.
7. **Spoiler videos** are covered with `covered_image` until clicked (`ChatStore::reveal_attachment`, as images are). No poster is fetched while covered. Spoiler audio gets the card's tag only, as on the web.
8. **The player window:**
   - It's an `NSWindow` holding an `AVPlayerView` with its own controls. It's titled with the file's name, sized to the video (at most 80% of the screen), and starts playing.
   - There's only one: opening another video replaces its item and brings it forward.
   - Closing it stops playback.
   - If the item fails, the window closes and a toast says "Can't play this video here." with Open in browser.
9. **Right-click menus** keep the file actions (Save…, Copy link, Open in browser) on audio cards and video posters; Copy image doesn't apply.
10. **Logging:** a player's failure is logged with AVFoundation's error (domain, code, description), for the spike's write-up and later reports.

## Plan

### Chunk 1: Spike: AVFoundation next to GPUI
**Goal:** prove `AVPlayer` plays Stoat attachments and `AVAssetImageGenerator` makes posters from the file server, before any UI depends on it.
**Files likely touched:**
- `Cargo.toml`, macOS section:
  - `objc2-av-foundation` (`AVPlayer`, `AVPlayerItem`, `AVAsset`, `AVURLAsset`, `AVAssetImageGenerator`, `AVTime`);
  - `objc2-core-media` (`CMTime`);
  - `objc2-av-kit` (`AVPlayerView`, for chunk 4; added now so the spike measures build cost once);
  - `objc2-core-graphics` features for reading a `CGImage`'s pixels.
- `src/media_player.rs` (new):
  - `Player::open(url)`, `play`, `pause`, `seek(seconds)`, `snapshot() -> PlayerSnapshot { status: Loading | Ready | Failed(String), position, duration: Option<f64>, playing, ended }`;
  - `poster(url, max: (u32, u32)) -> Result<RgbaImage, String>`;
  - stubs on other platforms.
- `examples/media_spike.rs` (new): `cargo run --example media_spike -- <url> [poster.png]` plays the URL for 5 s, printing a snapshot every 250 ms, then seeks to the middle and plays 2 s more. With a second argument, it writes the poster to that PNG.
- `src/lib.rs`.
- `docs/plans/inline-audio-and-video-implementation.md`: the spike's results.

**Depends on:** None
**Risks:**
- **AVFoundation may need a running run loop** for `status` and `currentTime` to update. The example runs `NSRunLoop::mainRunLoop` between polls. GPUI's main thread has one already. If polling sees no updates, chunk 2 needs KVO, which adds `block2`/`define_class!` work: stop and re-plan.
- **The file server may not stream** (byte ranges, content type). Then Assumption 2's download fallback becomes the main path: note it, and amend chunks 2 and 4.
- **Formats:** record which of mp3, m4a, wav, flac, ogg, opus, mp4 (H.264, HEVC), mov and webm play, and the error for those that don't. Upload one of each to a test channel from the web client.
- **Build and size:** record the clean release build time and binary size change, as the voice spike did.

**How to verify:**
- `cargo build --example media_spike` (and `cargo build --release` for the size).
- A test, macOS only (amended in chunk 1: `tests/media_player.rs` with `harness = false`, since a player needs the main thread and its run loop, which unit tests don't run on; muted): write a 1-second 8 kHz WAV to a temporary file, `Player::open` its `file://` URL, poll (up to 5 s) until `Ready`, and check the duration is 1.0 ± 0.05 s. It runs offline.
- Then the full suite.

**Manual check:** run the example against an mp3, an m4a, an ogg, an mp4 and a webm uploaded to the test channel (copy each one's link with Copy link). Hear the audio, and open the poster PNGs.
**Shippable when:** the bindings build, the example plays and makes posters, and the record says which formats work and whether streaming does. Go/no-go for chunks 2–4 is decided there.

### Chunk 2: Audio in the message
**Goal:** audio attachments play in their message, with play/pause, seeking and the time.
**Files likely touched:**
- `src/playback.rs` (new):
  - `time_label(position, duration)` ("0:42 / 3:15", "1:02:03 / …", "–:––");
  - `seek_target(fraction, duration)`;
  - `NowPlaying { channel, file_id, kind }`, and the rules: what a new play stops, and what leaving a channel or switching account stops.
- `src/media.rs` (amended in chunk 1): `MediaUrls::original(file)`, the original's own URL, `{files}/{tag}/{id}/{percent-encoded filename}`. The bare `{files}/{tag}/{id}` that `MediaUrls::file` builds is autumn's preview route, which for audio and video answers with a 308 to the filename unencoded; the original's URL skips that, and the CDN answers byte ranges on it (chunk 1).
- `src/attachment.rs`: `AttachmentView::Audio { id, url, name, size_label, spoiler }`, made for `Metadata::Audio` with a URL (`MediaUrls::original`). Its `file_target()` gives the file actions.
- `src/ui/chat.rs`:
  - `ChatView.playing: Option<Playing { now: NowPlaying, player: Player, snapshot: PlayerSnapshot }>` and the 250 ms poll task (only while something plays). `RowAction::{PlayAudio, PauseAudio, SeekAudio(fraction)}`.
  - `Pickers.playback` hands the snapshot of the playing file to rows, so its row redraws without its data changing.
  - `render_attachment`'s `Audio` arm: the card line and the controls line (Assumption 3). The seek bar takes clicks and drags (`on_mouse_down` / `on_drag_move` on a 4 px track with a larger hit area).
  - Stops playback on channel change, account switch and `ChatView` drop. Marks a file unplayable (kept for the session, by file ID) when its player fails.
- `tests/playback.rs` (new), `tests/store_attachments.rs`.

**Depends on:** Chunk 1
**Risks:**
- **Redraws while playing:** notify only the pane, every 250 ms, and only while playing. Check an idle channel with paused audio costs nothing (the timer stops when nothing plays).
- **Dragging the seek bar inside a list row:** the timeline must not scroll or select while dragging. Stop propagation on mouse down.
- **A file that fails after starting** (a network drop) shows the unplayable line until the session ends. Acceptable: Open in browser still works.

**How to verify:**
- `cargo test --test playback`: time labels (0, 59 s, 1 h, unknown duration, position past the duration), seek targets (clamped 0–1), and what a new play, a channel change and an account switch stop.
- `cargo test --test store_attachments`: an `Audio` metadata file becomes `AttachmentView::Audio` with its URL and spoiler tag; without a file server it stays a card.
- Then the full suite.

**Manual check:** play an mp3 and an m4a; pause, seek by clicking and dragging, and play to the end (the button returns to play, at 0:00). Start a second one: the first stops. Change channel: it stops. Try a webm (audio-only Opus in WebM): "Can't play this format here", and Open in browser works. (Amended in chunk 1: Ogg Vorbis and Opus play on macOS 26, so they're no longer the unplayable example; they may not on older macOS.)
**Shippable when:** audio attachments play in the message, one at a time, and unplayable ones say so.

### Chunk 3: Video posters
**Goal:** video attachments show a poster frame at their own size with a play button, instead of a card.
**Files likely touched:**
- `src/attachment.rs`: `AttachmentView::Video { id, url, name, size_label, width, height, original, spoiler }`, made for `Metadata::Video` with a URL (`MediaUrls::original`, as audio's), sized by `fit_within`; `uncovered` and `file_target()` cover it too.
- `src/ui/video_poster.rs` (new): a GPUI `Asset` keyed by URL that runs `media_player::poster` on the background executor and returns a BGRA `RenderImage`, like `thumbhash.rs`; and `poster(url) -> Img`.
- `src/ui/chat.rs`:
  - the `Video` arm: the poster at the display size (pulsing while it loads, a dark box with "Video" if it fails), a round play badge, and the file actions' menu;
  - a covered spoiler with `covered_image`, revealed by `RowAction::RevealAttachment`;
  - until chunk 4, a click opens the file in the browser.
- `src/store.rs`: nothing new (`reveal_attachment` already exists); check `message_row` passes `uncovered` to videos.
- `tests/store_attachments.rs`.

**Depends on:** Chunk 1
**Risks:**
- **Posters download part of each video:** a channel of videos generates several at once. GPUI's asset cache dedupes by URL. If it's slow, cap concurrent generations at 2 with a semaphore in the asset.
- **Missing or zero metadata dimensions** use the fallback box, letterboxed (`ObjectFit::Contain`).

**How to verify:**
- `cargo test --test store_attachments`:
  - a video becomes `AttachmentView::Video` at its fitted size (and the fallback box without dimensions);
  - a spoiler video is covered, revealed by file ID, at the same size;
  - its file target isn't a picture.
- Then the full suite.

**Manual check:** an mp4 and a mov from the web client show their posters at the right shape; a spoiler video is covered until clicked; a webm shows the dark box (its poster can't be made) and still opens in the browser.
**Shippable when:** videos show posters at their own size, and spoiler videos are covered.

### Chunk 4: The video player window
**Goal:** clicking a video's poster plays it in a native window.
**Files likely touched:**
- `src/video_window.rs` (new):
  - `VideoWindow::open(url, title, size)` creates an `NSWindow` (titled, closable, resizable; `releasedWhenClosed` off) whose content view is an `AVPlayerView` with an `AVPlayer`, sized to the video within 80% of the main screen, centred, and playing;
  - `replace(url, title, size)`, `is_open()` (the window's `isVisible`), `close()`, `snapshot()` (the player's status, for failures);
  - stubs on other platforms.
- `src/ui/chat.rs`:
  - `RowAction::PlayVideo(FileTarget, (usize, usize))` opens or replaces the window and stops audio; playing audio closes it;
  - the poll task watches it, so closing the window ends playback, and a failure closes it with a toast "Can't play this video here." and Open in browser;
  - account switch and `ChatView` drop close it.
- `src/playback.rs`: `window_size(original, screen)` (fit within 80%, at least 320 × 180, the fallback shape when unknown).
- `tests/playback.rs`.

**Depends on:** Chunks 1 and 3
**Risks:**
- **A second AppKit window beside GPUI's:**
  - GPUI may take key-window or app-activation events oddly. Check ⌘W closes the video window (not the chat), and that ⌘Q still quits.
  - If menu shortcuts misroute, give the window its own `NSWindowController`, or fall back to closing on Escape only.
- **Keeping the window alive:** hold its `Retained<NSWindow>` in `ChatView` until it closes. Closing (or replacing) must pause and release the player, or sound keeps playing with no window.
- **Several accounts:** each `ChatView` owns at most one window. Switching account closes it (Assumption: one thing plays at a time).

**How to verify:**
- `cargo test --test playback`: `window_size` (landscape, portrait, tiny, huge, unknown).
- Then the full suite.

**Manual check:**
- Play an mp4: a window opens at the video's shape, playing, with AVKit's controls.
- Open another video: the same window switches to it.
- Close it: the sound stops.
- Play audio while a video plays: the window closes.
- ⌘W and ⌘Q behave.
- A webm: a toast, and Open in browser.

**Shippable when:** videos play in a native window, one at a time, and close cleanly.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`

**Formatting rule** (as in milestones 7–30):
- Never run `cargo fmt` across the crate.
- New files must be clean under `rustfmt --edition 2024 --check <file>`.
- For existing files, the number of `Diff in` hunks must not grow. Counts at the start: `src/ui/chat.rs` 12, `src/store.rs` 14, `src/api/client.rs` 3, `src/api/events.rs` 3, `src/ui/composer.rs` 13. Check any other file's count with `git show HEAD:<file> | rustfmt --edition 2024 --check | grep -c '^Diff in'` before editing it.
- To format only your own code in a file with drift, format the changed functions alone (as milestone 30 did), not the whole file.
- Don't run rustfmt on `lib.rs`, `mod.rs` or `main.rs`.

### Unit Tests
- `tests/media_player.rs` (macOS only, `harness = false`, muted): a generated 1-second WAV plays from `file://` and reports its duration; a player can't be made off the main thread; a junk file fails with AVFoundation's error.
- **No test or spike run makes a sound:** players in tests and the spike are muted (the spike plays aloud only with `--audible`). Audible checks are left to the manual tests.
- `tests/playback.rs` (new): time labels, seek targets, the one-at-a-time rules (what a new play, a channel change and an account switch stop), and the window size.
- `tests/store_attachments.rs`:
  - `Audio` and `Video` views from metadata (URL, size, fallback box, spoiler covering and reveal, file targets);
  - cards when there's no file server.

### Integration Tests
- None against Stoat: playback and posters need AVFoundation and real media, and are covered by the spike's example and the manual checks.
- If chunk 1 makes the download fallback the main path, `tests/api_files.rs` covers downloading a whole file (already there from milestone 30).

### Manual Tests
Run them with the web client in a test channel holding one each of mp3, m4a, wav, flac, ogg, opus, mp4, mov and webm, plus a spoiler mp4. Batch them at the end if the user asks.
1. **Audio:**
   - mp3, m4a, wav and flac play;
   - pause, seek (click and drag), and play to the end;
   - starting another stops the first, and so does changing channel;
   - ogg and opus play (on macOS 26; amended in chunk 1);
   - webm says "Can't play this format here" and opens in the browser.
2. **Video posters:** mp4 and mov at the right shape; webm as the dark box; the spoiler covered until clicked; the right-click menu's Save…, Copy link and Open in browser.
3. **The player window:** mp4 and mov play; another video replaces the first; closing stops the sound; audio closes the window; ⌘W and ⌘Q; webm toasts.
4. **Accounts:** play audio, switch account: it stops. Play a video, switch account: the window closes.
5. **Themes:** the audio controls, the poster's play badge and the dark box read well in light and dark.

## Definition of Done
- [ ] The spike's results (formats, streaming, build time, binary size) are written in the implementation record, with the go decision.
- [ ] Audio attachments in formats AVFoundation plays show play/pause, a seek bar that seeks on click and drag, and "position / duration". Starting one stops any other, and changing channel or account stops it.
- [ ] Audio and video that can't be played say "Can't play this format here" (audio) or toast "Can't play this video here." (video), and offer Open in browser.
- [ ] Video attachments show a poster at their metadata's shape (fitted within 400 × 300) with a play button; spoiler videos are covered until clicked.
- [ ] Clicking a poster plays the video in a native window with AVKit's controls; another video replaces it; closing it stops playback; ⌘W and ⌘Q behave.
- [ ] Audio cards and video posters keep the file actions' right-click menu (Save…, Copy link, Open in browser).
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass. No touched file's rustfmt drift grew, and new files are clean.
- [ ] The roadmap links this plan.
