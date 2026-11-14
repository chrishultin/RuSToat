# Plan: RuSToat for iPhone, milestone 3 (media)

## Summary
Make pictures, video, audio and links in messages work properly on the iPhone, and let you send them:
- images inline, at their own shape, with blurred ThumbHash previews while they load, and a full-screen viewer with zoom;
- videos with poster frames, playing in the system player, and audio with an inline player;
- rich link cards and GIFs that play;
- sending photos, camera shots and files, with progress.

This is milestone iOS 3 of the [iOS roadmap](ios-app.md#ios-roadmap), building on milestones 1 and 2 ([plan](ios-app.md), [record](ios-app-implementation.md); [plan](ios-rich-messages.md), [record](ios-rich-messages-implementation.md)).

## Approach
**What to show stays in the shared core; how to show it is native iOS.**
- The store already builds every attachment and embed view desktop draws: `AttachmentView::{Image, File, Audio, Video, Local}` and `EmbedView::{Card, Image, Video, GifPoster}`, with display sizes, ThumbHashes, the 8 MB "too big to load" rule, spoilers, and proxied URLs.
- Milestone 1 flattened those into thin `AttachmentItem`/`EmbedItem` records. This milestone carries all their fields through the FFI.
- ThumbHashes are decoded in Rust with the `thumbhash` crate already in the tree, and handed to Swift as small RGBA images.
- Playback uses AVKit's system player (`VideoPlayer`, and `AVPlayerViewController` for full screen) and `AVPlayer` for audio. Desktop's `media_player.rs` is AppKit-only and isn't needed on iOS.
- Pictures load through one shared image loader: memory cache, `URLCache` on disk, de-duplicated downloads. It generalises chunk 3's `EmojiImages`.
- GIFs decode their frames with ImageIO and play with a `TimelineView`.

**Sending follows desktop's `post_pending` exactly.**
- Attachments are files on disk (`draft::DraftAttachment`), added to the pending message (`ChatStore::add_pending` with `PendingAttachment`s).
- They're uploaded one at a time with `StoatClient::upload_attachment` and its progress callback (`set_upload_progress`, `set_uploaded`), then the message is posted with their IDs.
- iOS's only extra job is turning a picked photo or camera shot into a file in the app's temporary folder, as JPEG (see assumption 3), and copying a picked file out of its security scope.

**Rejected alternatives:**
- **Decoding ThumbHash in Swift.** That's a second implementation of the format; the Rust crate is already there and tested by desktop.
- **`AsyncImage` everywhere.** It has no shared memory cache, so every row reloads as it scrolls back into view and flickers. It also can't show a ThumbHash while loading, or play GIFs.
- **A third-party image library** (Kingfisher, Nuke). A new dependency for what a ~150-line loader plus `URLCache` does.
- **Uploading from Swift.** That duplicates the multipart upload, progress and error handling the core already has, and bypasses the pending-message state that drives the rows.

## Assumptions
1. **Images load automatically on Wi-Fi and cellular alike,** as on desktop, with the same 8 MB cut-off (`MAX_AUTO_LOAD_BYTES`) above which a "Show image" card appears. A data-saver setting can come later.
2. **The full-screen viewer** has pinch and double-tap zoom, swiping between the images of one message, drag-down to dismiss, and Share (the system share sheet, which includes Save Image). There's no separate "Save to Photos" button, so no Photos-library permission is needed.
3. **Photos and camera shots are sent as JPEG** (quality 0.85, at most 4096 px on the long side). HEIC doesn't display in Stoat's web client. Files picked from Files are sent exactly as they are. Videos from Photos are sent as the original file (exported with `AVAssetExportSession` to MP4 if they aren't one already).
4. **Video plays in the system player:** inline in the row once tapped (`VideoPlayer`, muted until unmuted), with a full-screen button. Audio plays inline (play/pause, scrubber, time), one item at a time across the app, as desktop does.
5. **Animated GIFs play inline,** whether attachments or embeds, decoded with ImageIO, and animate only while on screen. Gifbox GIFs that exist only as video (`EmbedView::GifPoster`) show their poster and open their page, as on desktop.
6. **Attachment spoilers:** covered with a "Spoiler" overlay until tapped (`ChatStore::reveal_attachment`). Sending as a spoiler is a per-attachment toggle in the tray (`DraftAttachment::spoiler`, sent as `spoiler_<name>`).
7. **Limits come from the instance** (`AttachmentLimits::from_root`: size and count). Picking past them shows why, and the composer won't send until it's fixed, as on desktop.
8. **Text-file previews** (desktop's `TextPreview` for `.txt`/`.md` under 50 kB) are out of scope: file cards open in Quick Look instead (`QLPreviewController`, after downloading to a temporary file). Quick Look covers PDFs and Office documents too.

## Open Questions
- **Deferrable:** should drafts with attachments survive leaving the channel? Text drafts already do (in memory). Attachments are temporary files, so this plan keeps them per channel in memory too, and deletes them when the app quits.

## Plan

### Chunk 1: Images inline, with ThumbHash previews
**Goal:**
- Image attachments and image embeds appear inline at their own shape, scaled to the row's width (at most 320 pt wide or 400 pt tall), with a blurred ThumbHash preview until the picture arrives.
- Too-big images show a "Show image" card, and spoiler images stay covered until tapped.
- Pictures load once and stay cached while scrolling.

**Files likely touched:**
- `ios/stoat-ffi/src/timeline.rs`:
  - `AttachmentItem` becomes an enum mirroring `AttachmentView`: `Image { id, url, name, size_label, width, height, original_width, original_height, thumbhash: Option<Vec<u8>>, too_big, spoiler }`, `File`, `Audio`, `Video`, `Local { …, progress }`.
  - `EmbedItem` likewise mirrors `EmbedView`: `Card { colour, icon, site_name, title, link, description (plain or rich), thumbnail, image, playable, media }`, `Image`, `Video`, `GifPoster`.
- `ios/stoat-ffi/src/media.rs` (new): `#[uniffi::export] fn thumbhash_image(hash: Vec<u8>) -> Option<ThumbImage { width, height, rgba }>`, from `thumbhash::thumb_hash_to_rgba`.
- `ios/stoat-ffi/src/actions.rs`: `reveal_image(channel, file_id)` and `reveal_attachment(channel, file_id)`, wrapping the store's reveals and reporting a timeline change.
- `ios/stoat-ffi/tests/media.rs` (new).
- Swift:
  - `ios/RuSToat/Shared/ImageLoader.swift` (new): `@MainActor @Observable`, an `NSCache`-backed memory cache, a 200 MB `URLCache` disk cache, de-duplicated requests, and downsampling to the displayed size with ImageIO (`CGImageSourceCreateThumbnailAtIndex`), so a 12-megapixel photo isn't decoded at full size for a 300 pt row.
  - `EmojiImages` becomes a thin use of it.
  - `ios/RuSToat/Chat/Media/InlineImage.swift` (new): the ThumbHash under a fade-in of the picture, the too-big card, and the spoiler cover.
  - `ios/RuSToat/Chat/MessageRowView.swift` and `EmbedCard`.
- `ios/RuSToatTests/ImageLoaderTests.swift` (new).

**Depends on:** None (milestones 1–2).
**Risks:**
- **Row heights changing as images load** would make the list jump. Every image is laid out at its known display size from the start (the store's `width`/`height`, or `FALLBACK_IMAGE_SIZE`), so loading never changes a row's height.
- **Memory.** Downsampling plus `NSCache` limits (about 150 MB, count-limited) keeps a long scroll through a media channel within iOS's limits. Check with Instruments' Allocations on a device if it seems heavy.
- **Changing `AttachmentItem` from a record to an enum** touches milestone 1's `AttachmentCard`. Replace it rather than keeping both.

**How to verify:**
- `cargo test -p stoat-ffi --test media`: a fixture image attachment gives `Image` with its display size and ThumbHash; over 8 MB gives `too_big`; `reveal_image` clears it; a `spoiler_` file is `spoiler: true` until `reveal_attachment`; `thumbhash_image` decodes a known hash to the expected size; each `EmbedView` kind maps across.
- `scripts/ios/test.sh`, with `ImageLoaderTests`: one URL asked twice loads once, and is downsampled to the requested size (stub `URLProtocol`, as in `EmojiImagesTests`).
- Simulator against `fake_stoat`, which serves the fixture images and a slow image so the ThumbHash shows; screenshot.

**Manual check:** Scroll a real channel full of photos quickly: no jumps, no flicker coming back to rows, and blurred previews before each picture.
**Shippable when:** Images and image embeds show inline at the right size with previews, and big and spoiler images behave as on desktop.

### Chunk 2: The full-screen image viewer
**Goal:** Tapping an image opens it full screen at full resolution, with pinch and double-tap zoom, swiping between that message's images, drag-down to dismiss, and Share.
**Files likely touched:**
- Swift:
  - `ios/RuSToat/Chat/Media/ImageViewer.swift` (new): a `fullScreenCover` with a paged `TabView` of zoomable pages. Each page is a `UIScrollView` wrapped with `UIViewRepresentable`, since SwiftUI's own zoom gestures don't handle panning a zoomed image well.
  - It loads the original (`url` without size limits) through `ImageLoader` at screen size × 3, with the inline copy shown meanwhile.
  - A toolbar with the file name, Share (`ShareLink` with the downloaded file), and Close.
  - `ios/RuSToat/Chat/ChatView.swift`: the viewer's state.
- `ios/RuSToatTests/ImageViewerTests.swift`: the paging index, and zoom-scale clamping (pure functions).

**Depends on:** Chunk 1.
**Risks:**
- Gesture conflicts between paging, zooming and drag-to-dismiss. Zoomed in, a drag pans; at 1×, a vertical drag dismisses and a horizontal one pages.
- Sharing needs the file on disk: download to a temporary file named after the attachment, so the share sheet shows the right name and type.

**How to verify:** `scripts/ios/test.sh`, plus simulator screenshots of the viewer opened by a DEBUG launch argument (`RUSTOAT_DEV_VIEW=<file id>`, alongside the existing `RUSTOAT_DEV_OPEN`).
**Manual check:** Pinch-zoom a photo, swipe to the next, drag down to close, and share one to Messages.
**Shippable when:** Every image can be viewed full screen, zoomed and shared.

### Chunk 3: Video and audio
**Goal:**
- Video attachments show a poster frame at their own shape with a play button, and play inline (with a full-screen button) in the system player.
- Audio attachments get an inline player.
- Only one item plays at a time.

**Files likely touched:**
- Swift:
  - `ios/RuSToat/Chat/Media/VideoAttachment.swift` (new):
    - The poster comes from `AVAssetImageGenerator` (`copyCGImage(at: .zero)`, asynchronously), cached in `ImageLoader` by URL.
    - It shows the ThumbHash, or a dark placeholder, until the poster is ready, then becomes `VideoPlayer(player:)` when tapped.
    - The full-screen button presents `AVPlayerViewController`.
  - `ios/RuSToat/Chat/Media/AudioAttachment.swift` (new): play/pause, a `Slider` scrubber and elapsed/total time, driven by `AVPlayer.addPeriodicTimeObserver`.
  - `ios/RuSToat/Shared/Playback.swift` (new): an `@Observable` "now playing" that pauses the previous item when another starts, and sets the audio session to `.playback`.
- `ios/stoat-ffi/src/timeline.rs`: `Audio`/`Video` already carried from chunk 1. Add the spoiler cover for videos.

**Depends on:** Chunk 1.
**Risks:**
- **Stoat's file server and range requests.** AVPlayer streams with HTTP `Range` requests. If the server doesn't honour them, playback starts only after a full download; acceptable, but check on device with a large video.
- **Audio session.** `.playback` makes audio play with the silent switch on, which is right for media the user chose to play. Don't activate the session until something plays, or other apps' audio stops when RuSToat opens.
- **Rows with players are expensive.** Keep posters as images until tapped, so a `LazyVStack` never holds many `AVPlayer`s.

**How to verify:**
- `scripts/ios/test.sh`, with `PlaybackTests`: starting a second item pauses the first (with fake players), and the time-label formatting.
- Simulator: `fake_stoat` serves a short MP4 and an M4A from `tests/fixtures/media/` (new, a few kB each, generated once with `ffmpeg`/`afconvert` and committed). Screenshot the poster and the playing states.

**Manual check:** Play a video inline and full screen; play a voice note; start one while another plays.
**Shippable when:** Videos and audio from desktop users play on the phone.

### Chunk 4: Link embeds and GIFs
**Goal:**
- Website embeds become proper cards: colour bar, site icon and name, title linking out, description (bot embeds' Markdown through `RichTextView`), and a thumbnail beside the text or a large image under it.
- Video links (YouTube and the like) show their picture with a play badge and open in their app or Safari.
- GIFs, as attachments or embeds, animate inline.

**Files likely touched:**
- Swift:
  - `ios/RuSToat/Chat/Media/EmbedCardView.swift` (new, replacing milestone 1's `EmbedCard`), built from chunk 1's `EmbedItem` enum. A bot embed's `media` attachment reuses chunk 1's and chunk 3's views.
  - `ios/RuSToat/Chat/Media/AnimatedImage.swift` (new): frames and delays from `CGImageSource`, drawn by a `TimelineView(.animation)` only while visible, with a cap of 200 frames (longer GIFs show their first frame and open in the viewer).
  - `ImageLoader` keeps the raw data for GIFs so they can be decoded into frames.

**Depends on:** Chunks 1 and 3.
**Risks:**
- **GIF memory:** decode frames at the displayed size, not the original, and drop them when the row leaves the screen.
- **Stoat's embed proxy (January) returns its own content types.** Detect GIFs by their data (the `GIF8` magic), not by URL or header.

**How to verify:**
- `scripts/ios/test.sh`, with `AnimatedImageTests`: frame count and delays from a two-frame fixture GIF, and the 200-frame cap.
- Simulator screenshots of a fixture message with a website embed, a large-image embed, a YouTube embed and a GIF (`tests/fixtures/messages_media.json`, made in chunk 1; add a GIF attachment and a GIF embed).

**Manual check:** Paste a YouTube link and a news article on desktop and compare the cards on the phone; send a GIF from desktop's GIF picker and watch it play on the phone.
**Shippable when:** Embeds look like desktop's cards, and GIFs move.

### Chunk 5: Sending files, with progress
**Goal:**
- The composer's ＋ button picks files from Files and adds them to an attachment tray (name, size, spoiler toggle, remove).
- Sending shows the message at once with each file's upload progress. Failed uploads can be retried, and files over the instance's limits are refused with the reason.

**Files likely touched:**
- `ios/stoat-ffi/src/controller.rs`:
  - `send` takes `attachments: Vec<OutgoingFile { path, name, spoiler }>`.
  - `Shared::post` gains desktop's upload loop: each file not yet uploaded goes through `upload_attachment(file_server, upload_name, content_type, path, progress)`. Progress is forwarded as `set_upload_progress` at most every 100 ms (desktop's `PROGRESS_INTERVAL`), then `set_uploaded`. A failed upload calls `fail_pending` with desktop's upload text, and nothing is posted.
  - Root's `AttachmentLimits` are kept, and an exported `attachment_limits()` returns them.
- `ios/stoat-ffi/src/actions.rs`: `check_attachments(paths) -> Result` (count and size against the limits, with desktop's texts).
- `src/api/error.rs`: `ApiError::upload_failure_text()`, moved from `src/ui/chat.rs`'s `upload_error_text`, as with the other texts in milestone 2.
- `ios/stoat-ffi/tests/sending.rs`:
  - an attachment uploads (wiremock `POST /attachments` on the file server) before the message, which carries its ID;
  - progress reaches 1000;
  - a 413 fails the row with "it's too big." and posts nothing;
  - retry re-uploads only what didn't finish;
  - over the count limit is refused.
- Swift:
  - `ios/RuSToat/Chat/Composer.swift`: ＋ opens a menu (Files for now; Photos and Camera in chunk 6), and the tray of chips above the field.
  - `ios/RuSToat/Chat/AttachmentTray.swift` (new).
  - `ios/RuSToat/Chat/OutgoingFiles.swift` (new): copy a `fileImporter` URL (inside `startAccessingSecurityScopedResource`) into `FileManager.temporaryDirectory/outgoing/<uuid>/<name>`, deleted once sent or discarded (`ChatStore::take_released_files`, through a new `released_files()` FFI call after each change).
  - `ios/RuSToat/Chat/MessageRowView.swift`: `Local` attachments show a progress ring and the file name.

**Depends on:** Chunk 1 (the `Local` attachment item).
**Risks:**
- **The upload's progress callback runs on the IO runtime for every chunk.** Throttle it in Rust (as desktop does), or Swift would redraw the list hundreds of times a second.
- **Background:** if the app goes to the background mid-upload, iOS may suspend it. Wrap sends in `UIApplication.beginBackgroundTask` so a short upload finishes. Long ones fail and can be retried; a background `URLSession` is out of scope.
- **Temporary files** must be removed after sending or discarding, or they'd pile up. The core already tracks released files; make sure Swift deletes them.

**How to verify:** `cargo test -p stoat-ffi --test sending`; `scripts/ios/test.sh`; and a simulator run against `fake_stoat` (which serves `POST /attachments`), sending a file through a DEBUG launch argument (`RUSTOAT_DEV_ATTACH=<path>`), with a screenshot of the progress row.
**Manual check:** Send a PDF from Files and open it on desktop; send one over the size limit and read the reason.
**Shippable when:** Files go from the phone to Stoat with visible progress, and failures are recoverable.

### Chunk 6: Photos and the camera
**Goal:** The ＋ menu also offers Photo Library (multiple photos and videos, `PhotosPicker`) and Camera. Photos are sent as JPEG at a sensible size, with their dimensions known so the pending row has the sent one's shape.
**Files likely touched:**
- Swift:
  - `ios/RuSToat/Chat/OutgoingFiles.swift`:
    - `PhotosPickerItem` → `Data` → a downscaled JPEG (ImageIO, 4096 px max, quality 0.85).
    - Videos via `Transferable` `movie` export to MP4.
    - Camera shots go through `UIImagePickerController` (`UIViewControllerRepresentable`; `.camera` source, with `.photoLibrary` as a fallback on the simulator).
  - `ios/RuSToat/Chat/Composer.swift`: the menu items.
  - `ios/project.yml`: `INFOPLIST_KEY_NSCameraUsageDescription` ("RuSToat uses the camera to take photos to send."). The photo picker runs out of process and needs no permission.
- `ios/stoat-ffi/src/controller.rs`: `OutgoingFile` gains `dimensions: Option<(u32, u32)>`, so `DraftAttachment.dimensions` gives the pending image its final size (`attachment::local_image_size` already reads it for desktop's files, and is also used as the fallback).

**Depends on:** Chunk 5.
**Risks:**
- **EXIF orientation:** re-encode with orientation applied (`UIImage` from the camera is already oriented; for library images use `CGImageSourceCreateThumbnailAtIndex` with `kCGImageSourceCreateThumbnailWithTransform`), or photos arrive sideways.
- **Location metadata:** strip GPS EXIF from re-encoded JPEGs (re-encoding through ImageIO without copying the metadata dictionary does this), so photos don't leak where they were taken. Files from Files are sent untouched, as the user chose them.
- The simulator has no camera, so the camera path is only checked on the device.

**How to verify:**
- `scripts/ios/test.sh`, with `OutgoingFilesTests`:
  - a 6000×4000 test JPEG (generated in the test) comes out at 4096×2731;
  - orientation is applied;
  - the output has no GPS dictionary;
  - a PNG with transparency is still sent as JPEG (Stoat shows no difference).
- Simulator: pick from the photo library (the simulator ships sample photos) through a DEBUG argument, and screenshot the pending row at its final shape.

**Manual check:** Send a photo from the library and a camera shot from your iPhone, check they're upright on desktop, and check (in desktop's file info, or by downloading) that location isn't included.
**Shippable when:** Photos and camera shots can be sent from the phone, upright, at a sensible size, without location data.

## Test Plan

**Full-suite command:**
```bash
cargo test && cargo clippy --all-targets -- -D warnings && scripts/ios/test.sh
```

### Unit Tests
- Rust (`ios/stoat-ffi/tests/media.rs`):
  - each `AttachmentView` and `EmbedView` kind across the FFI with all its fields;
  - ThumbHash decoding;
  - the too-big and spoiler reveals.
- Rust (`ios/stoat-ffi/tests/sending.rs`):
  - upload-then-post with the file ID;
  - progress throttling, ending at 1000;
  - upload failure text (413, network);
  - retry skipping files already uploaded;
  - attachment count and size limits.
- Swift:
  - `ImageLoader` de-duplication and downsampling;
  - the viewer's paging and zoom helpers;
  - `Playback` pausing the previous item;
  - GIF frame decoding and its cap;
  - photo re-encoding (size, orientation, no GPS).

### Integration Tests
Against `fake_stoat` / wiremock:
- a message with every attachment kind renders: image, too-big image, spoiler image, video, audio, file;
- embeds of each kind;
- send with two files, both uploaded in order and posted;
- a failed upload, then retry, then sent once.

### Manual Tests
1. Scroll a busy media channel: blurred previews, no jumps, no reloading when scrolling back.
2. Open a photo full screen, zoom, swipe, share, and dismiss.
3. Play a video inline and full screen; play a voice note; start a second item while one plays.
4. Compare link cards (an article, YouTube, a bot embed) with desktop; watch a GIF play.
5. Send a file from Files, a few photos from the library and a camera shot; check they arrive upright, at a sensible size, and without location.
6. Send something over the size limit, and send in Airplane mode, then retry.

## Definition of Done
- [ ] `cargo test && cargo clippy --all-targets -- -D warnings && scripts/ios/test.sh` passes.
- [ ] Image attachments and embeds render inline at the store's display size from the first frame (no row-height change on load), with a ThumbHash preview, the "Show image" card over 8 MB, and spoiler covers.
- [ ] Images load through one cached, de-duplicated, downsampling loader; scrolling back doesn't reload them.
- [ ] The full-screen viewer zooms, pages between a message's images, dismisses by dragging down, and shares the original file.
- [ ] Video attachments show a poster and play inline and full screen; audio plays inline; only one item plays at a time.
- [ ] Link embeds render as desktop's cards (colour, site, title, description, thumbnail or image, play badge), and GIFs animate inline, capped at 200 frames.
- [ ] Files, library photos and videos, and camera shots can be attached (with a spoiler toggle), uploaded with visible progress through the core's `upload_attachment`, and sent; failed uploads retry without re-uploading finished files; the instance's limits are enforced with desktop's texts.
- [ ] Re-encoded photos are JPEG, at most 4096 px, upright, and without GPS metadata; temporary files are deleted after sending or discarding.
- [ ] `NSCameraUsageDescription` is set, and no Photos-library permission is requested.
