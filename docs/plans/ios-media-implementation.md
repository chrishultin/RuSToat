# Implementation Record: RuSToat for iPhone, milestone 3 (media)

_Plan: docs/plans/ios-media.md_
_Started: 2026-11-14_

Mode: continuous, with the manual checks on a real iPhone saved for the end, as in milestones 1 and 2. Branch `i-d-like-to-port-this-app-to-ios-so-that-i-can-u-24de`, starting from `02ce20b`. The plan file was the only uncommitted file at the start, and it's committed with chunk 1.

Simulator checks use a throwaway script that does these steps:
- start `fake_stoat`;
- build the app with `xcodebuild`;
- reinstall it with a fresh keychain;
- launch it with `SIMCTL_CHILD_RUSTOAT_DEV_LOGIN=<fake's address>`, plus `RUSTOAT_DEV_OPEN` and the like;
- take screenshots with `xcrun simctl io … screenshot`.

## Chunk 1: Images inline, with ThumbHash previews

### Changes Made
- `ios/stoat-ffi/src/timeline.rs`:
  - `AttachmentItem` is now an enum mirroring the store's `AttachmentView`. Each case holds a record: `Image { image: ImageAttachment }`, `File { file: FileAttachment }`, `Audio { audio: AudioAttachment }`, `Video { video: VideoAttachment }` and `Local { local: LocalAttachment }`.
  - The records carry every field desktop draws: ID, URL, name, size label, display size, original size, ThumbHash, `too_big`, `spoiler`, the file kind (`FileKindItem`), a local image path, and progress.
  - `EmbedItem` is now an enum mirroring `EmbedView`: `Card { card: EmbedCardItem }`, `Image { image: EmbedImageItem }`, `Video { url }` and `GifPoster { image, link }`.
  - `EmbedCardItem` holds the colour (`0xRRGGBBAA`), icon, site name, title, link, plain description, rich description, thumbnail, large image, `playable`, and a bot's `media` attachment.
  - `attachment()` is public, because the embed mapping reuses it.
- `ios/stoat-ffi/src/media.rs` (new): `#[uniffi::export] thumbhash_image(hash) -> Option<ThumbImage { width, height, rgba }>`, using `thumbhash::thumb_hash_to_rgba`.
- `ios/stoat-ffi/src/actions.rs`: `reveal_image(channel, file_id)` and `reveal_attachment(channel, file_id)`. Each calls the store's reveal, then `timeline_changed`.
- `ios/stoat-ffi/src/lib.rs`: `pub mod media`.
- `ios/stoat-ffi/Cargo.toml`: added `thumbhash = "0.1"`, the version already in the tree, and `url = "2"` as a dev-dependency (for the proxied addresses in the tests).
- `tests/fixtures/media/landscape.jpg` and `portrait.jpg` (new): pictures drawn with PIL, 25 kB and 13 kB.
- `tests/fixtures/messages_media.json` (new): Beta's #chat, with seven messages:
  - two photos;
  - a 9.1 MB scan;
  - a spoiler photo;
  - a PDF, a voice note and a video;
  - a news card, a YouTube card and an image embed;
  - a bot embed with media, a Gifbox GIF, a bare video link and a card with a thumbnail;
  - a "slow" photo.

  The ThumbHashes are real ones, computed from the fixture pictures.
- `ios/stoat-ffi/examples/fake_stoat.rs`:
  - serves `messages_media.json` as `ch-beta`'s history;
  - is its own media proxy (`january`) as well as its file server;
  - serves `/attachments/<id>` from the fixture pictures, with `f-slow` taking 15 s;
  - answers `/proxy?url=…` with the portrait or landscape picture according to the URL's ending, or else the app icon.
- `ios/stoat-ffi/tests/media.rs` (new, 8 tests):
  - an image's every field (display size 400×267 from 1200×800, and its ThumbHash);
  - the 8 MB guard and `reveal_image`, which also tells Swift about the change;
  - spoiler covers and `reveal_attachment`;
  - file, audio and video, with the originals' URLs;
  - website cards with every field, YouTube's `playable`, and image embeds;
  - a bot embed's Markdown and media, a Gifbox poster, a bare video, and a thumbnail;
  - ThumbHash decoding to 32×23 RGBA;
  - junk hashes don't panic.
- Swift:
  - `ios/RuSToat/Shared/ImageLoader.swift` (new):
    - a `@MainActor` loader with a 200 MB `URLCache` (`.returnCacheDataElseLoad`) in its own session;
    - an `NSCache` of decoded pictures (150 MB by cost, at most 600);
    - downloads shared by URL, and decodes shared by URL and pixel size;
    - decoding off the main actor with `CGImageSourceCreateThumbnailAtIndex`, applying EXIF orientation and never scaling up;
    - `cached(_:fitting:scale:)`, so a row drawn again shows its picture at once;
    - `data(_:)`, for the raw bytes later chunks need (GIFs, sharing).
  - `ios/RuSToat/Shared/EmojiImages.swift`: still the observable store that text reads synchronously, but it now loads through `ImageLoader` at 3×.
  - `ios/RuSToat/Chat/Media/InlineImage.swift` (new):
    - `MediaLayout.fit`: the core's display size, scaled down to the text column, at most 320 pt wide and 400 pt tall.
    - The `mediaMaxWidth` environment value.
    - `ThumbHashes`: the core's RGBA as a cached `UIImage`.
    - `PicturePlaceholder`: the ThumbHash stretched to fill, or a plain box.
    - `RemoteImage`: a fixed-size picture that fades in over its placeholder and remembers which URL it loaded, because rows are reused.
    - `InlineImage`: the picture, the "Show image" card (the blurred preview with the name, size and button) or `SpoilerCover` (the preview blurred again, with a "Spoiler" tag).
  - `ios/RuSToat/Chat/MessageRowView.swift`:
    - `AttachmentView` replaces `AttachmentCard`. Images are drawn inline. Files, audio and video are cards that open in Safari until chunk 3. Local files are cards until chunk 5.
    - `FileCard` shows the kind's icon and a spoiler tag.
    - `EmbedCard` is built from the new enum:
      - image embeds and GIF posters (with a "GIF" badge) are drawn at their display size;
      - bare videos are cards;
      - cards get their thumbnail beside the text, a large image under it, and bot media under that.
    - `MessageActions` gains `revealImage` and `revealAttachment`.
  - `ios/RuSToat/Chat/ChatView.swift`:
    - passes on the two reveals;
    - measures its width for `mediaMaxWidth` (the width less 80 pt for padding and the avatar);
    - adds DEBUG `RUSTOAT_DEV_SCROLL=<message ID>`, which scrolls to a message at launch for screenshots.
  - `ios/RuSToatTests/ImageLoaderTests.swift` (new, 4 tests):
    - two simultaneous requests share one download, and later ones come from memory;
    - pictures are decoded at the size they're drawn and never bigger;
    - pixel sizes;
    - undecodable data gives `nil`.
  - `ios/RuSToatTests/EmojiImagesTests.swift`:
    - the stub PNG is now really 64×64 pixels (the renderer was making it 192×192 at the simulator's 3× scale);
    - `ImageLoader.stubbed()` is shared with the loader tests;
    - the test of the old `scaled` helper was dropped along with the helper.

### Divergences
- **The enum cases hold records instead of listing fields inline**, e.g. `Image { image: ImageAttachment }`. Swift can then pass a kind to its view whole (`case .image(let image)`). The fields are the ones the plan lists.
- **`ImageLoader` isn't `@Observable`.** Views await it in `.task` and check `cached(…)` synchronously first. Observing one shared dictionary would redraw every picture on screen whenever any one arrived. `EmojiImages` stays observable, because text has to be built synchronously.
- **`messages_media.json` and the fixture pictures were made here, not in chunk 4.** Chunk 1's tests and screenshots needed them. The plan's note for chunk 4 now says to extend the file.
- **Added DEBUG `RUSTOAT_DEV_SCROLL`.** `simctl` can't scroll, and the fixture is taller than one screen.
- **Embed cards got a first version of images and thumbnails here.** The plan had chunk 1 touching `EmbedCard` for image embeds only. Chunk 4 still replaces the card with `EmbedCardView`, adding the colour bar, site icon and play badge.
- **ThumbHashes keep only an approximate shape** (32×23 for a 3:2 picture). The placeholder is stretched to the display size, so this doesn't show.
- **Pre-existing:** a clean Swift build shows 18 `#ActorIsolatedCall` warnings in `RichTextTests`. They come from main-actor static helpers that milestone 2 added, so they predate this chunk. They're only warnings in test code; none of this chunk's files raise any.

### Verification Performed
- `cargo test -p stoat-ffi --test media`: 8 passed. Full `stoat-ffi`: 84 passed.
- `cargo clippy -p stoat-ffi --all-targets -- -D warnings`: clean. `EmbedItem` allows `large_enum_variant`, because UniFFI can't box a record.
- Swift: 22 tests in 8 suites passed.
- Desktop: `cargo test`: 1499 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.
- Simulator (iPhone 17 Pro) against `fake_stoat`, in #chat:
  - both holiday photos at their own shapes (sunset 3:2, tower 2:3);
  - the 9.1 MB scan as a blurred card with "scan.jpg · 9.1 MB · Show image";
  - the spoiler as a dark blurred cover with a "Spoiler" tag;
  - the slow photo as its blurred ThumbHash for its 15 s, then the picture;
  - the Gifbox poster with its "GIF" badge, the bare video link as a card, and the repo card with its thumbnail.

### Manual Validation
Deferred to the end: scroll a real channel full of photos quickly, checking for jumps, flicker and the blurred previews.

## Chunk 2: The full-screen image viewer

### Changes Made
- `ios/RuSToat/Chat/Media/ImageViewer.swift` (new):
  - `ViewerImage.images(of:available:)`: the message's pictures that are showing, in order. That's image attachments that aren't too big or covered, then image embeds and bot-embed media. Each comes with its inline size, so the inline copy cached at that size is drawn at once.
  - `ViewerRequest`: the images and the one to start on.
  - `ViewerMath`: the pure parts. Page clamping, zoom clamping (1–4×), the double-tap target (2.5× or back to 1×), aspect fitting, the rectangle to zoom into around a point, and the dismiss rule (more than 120 pt, or a flick over 900 pt/s after at least 20 pt).
  - `ImageViewer`:
    - a paged `TabView`, presented by `fullScreenCover` with a clear presentation background;
    - drag-down moves the pages and fades the black backdrop;
    - a single tap hides or shows the top bar;
    - the top bar has Close, the name with "2 of 3", and Share.
    - Share is a `ShareLink` to the downloaded file, disabled until the file is ready.
  - `ViewerFiles`: writes the file to `tmp/viewer/<id>/<name>`. A name without an extension (proxied embeds) gets one for its type, from ImageIO. The folder is removed when the viewer closes.
  - `ViewerPage`: shows the inline copy (or the ThumbHash) at once, then loads the picture through `ImageLoader` at the screen's size × 3.
  - `ZoomableImage` / `ZoomView`: a `UIScrollView` that zooms the picture by pinching and double-tapping, keeps it centred, and lays it out by the core's shape, so the preview and the full picture line up.
  - Drag-to-close is a `UIPanGestureRecognizer` that starts only at 1× and mostly downwards. Other pans, the pager's included, wait for it to fail (`shouldBeRequiredToFailBy`), so sideways drags still page and zoomed drags still pan.
- `ios/RuSToat/Chat/MessageRowView.swift`:
  - `MessageActions.viewImage(message, id)`.
  - `AttachmentView` and `EmbedCard` take a `view` closure. Inline images, image embeds and bot media open the viewer; GIF posters still open their page.
- `ios/RuSToat/Chat/ChatView.swift`:
  - presents the viewer from `viewing`;
  - adds DEBUG `RUSTOAT_DEV_VIEW=<file ID>`, which opens a picture full screen at launch.
- `ios/RuSToatTests/ImageViewerTests.swift` (new, 8 tests):
  - paging;
  - zoom clamping;
  - double tap in and out;
  - fitting;
  - the zoom rectangle;
  - dismiss thresholds;
  - which pictures a message offers, and starting on the right one;
  - share file names.

### Divergences
- **Drag-to-close is done in UIKit**, with a pan recognizer on the zoom view, rather than as a SwiftUI gesture over the `TabView`. That makes its priority over the pager's pan explicit (the plan's named risk). The rule is as planned: zoomed, a drag pans; at 1×, vertical closes and horizontal pages.
- **The top bar is an overlay, not a toolbar**, so it can fade with a tap and hide during a drag.
- **Image embeds and bot media open in the viewer too**, not just attachments. GIF posters and video links still open their pages.

### Verification Performed
- Swift: 30 tests in 9 suites passed (ImageViewerTests 8).
- `cargo test -p stoat-ffi`: 84 passed. Desktop `cargo test`: 1499 passed, 0 failed, 5 ignored. Both clippy runs are clean.
- Simulator against `fake_stoat` with `RUSTOAT_DEV_VIEW=f-portrait`: the tower photo full screen on black, fitted and centred (402×603 pt on the iPhone 17 Pro). The bar showed "portrait.jpg" and "2 of 2", with Close, and Share enabled once the file had downloaded.

### Manual Validation
Deferred to the end, because `simctl` can't drive gestures: pinch-zoom a photo, swipe to the next, drag down to close, and share one to Messages.

## Chunk 3: Video and audio

### Changes Made
- `tests/fixtures/media/clip.mp4` (new, 28 kB) and `voice.m4a` (new, 14 kB), made once with `ffmpeg`:
  - the clip is 3 s of `testsrc2` at 320×180, H.264 baseline, with a 440 Hz tone;
  - the voice note is 4 s of a 330 Hz tone in AAC.

  Both use `+faststart`.
- `tests/fixtures/messages_media.json`: the clip's metadata is now its real 320×180, and both files have their real sizes. `ios/stoat-ffi/tests/media.rs` was updated to match (display 320×180, small enough as it is).
- `ios/stoat-ffi/examples/fake_stoat.rs`: serves the originals (`/attachments/f-clip/clip.mp4`, `/attachments/f-voice/voice.m4a`) through `ranged()`. That answers `Range: bytes=a-b` with 206 and `Content-Range`, which AVPlayer needs.
- `ios/RuSToat/Shared/Playback.swift` (new):
  - a `@MainActor @Observable` "now playing" with `current` (a file ID);
  - `start` pauses the previous player; `pause` and `stopped` clear `current` only for that item;
  - the `.playback` audio session is activated just before the first item plays (injectable for tests);
  - `Pausable`, which `AVPlayer` adopts;
  - `timeLabel`: "0:05", "12:34", "1:02:03", and "0:00" for unknown times.
- `ios/RuSToat/Shared/ImageLoader.swift`: `poster(_:fitting:scale:)` and `cachedPoster`. These take the first frame from `AVAssetImageGenerator` (`image(at: .zero)`, with the track's transform applied, limited to the pixel size). The frame is kept in the same memory cache, keyed by URL, and generation is shared like a decode.
- `ios/RuSToat/Chat/Media/VideoAttachment.swift` (new):
  - `VideoAttachmentView` is drawn at its display size. It shows a dark box, then the poster with a play button.
  - A tap swaps in `VideoPlayer(player:)`, muted, registered with `Playback`.
  - The full-screen button unmutes and presents `ClosingPlayerController` (an `AVPlayerViewController` that reports its dismissal) with the same player, over the top view controller.
  - Scrolling away pauses the player and drops it back to the poster, unless full screen is up.
  - A spoiler video is covered with `SpoilerCover`, and no poster is made until it's uncovered.
- `ios/RuSToat/Chat/Media/AudioAttachment.swift` (new):
  - `AudioAttachmentView`: the file card with play/pause, a `Slider` scrubber, and "elapsed / total".
  - `AudioPlayerModel`:
    - reads the length from the file's header on appear;
    - makes its `AVPlayer` on first play;
    - updates the time from a 0.25 s periodic observer (not while scrubbing) and seeks when the scrubber is let go;
    - rewinds to the start when it finishes;
    - lets the player go when the row leaves.
- `ios/RuSToat/Chat/MessageRowView.swift`: audio and video attachments use the new views. A card's large image and a bot's media are now sized inside the card's padding; before, they overflowed it by the padding.
- DEBUG `RUSTOAT_DEV_PLAY=<file ID>` starts a video or voice note at launch, for screenshots.
- `ios/RuSToatTests/PlaybackTests.swift` (new, 4 tests):
  - a second item pauses the first, and the session is activated once, only when something plays;
  - pausing and stopping affect only the current item;
  - restarting the same item doesn't pause it;
  - time labels.

### Divergences
- **No Rust change was needed for video spoilers.** The core's `AttachmentView::Video` already carries `spoiler` through `uncovered`, and chunk 1 maps it. The tests cover it with a plain video; the reveal is the same `reveal_attachment` as for images.
- **Videos have no ThumbHash**: Stoat's video metadata doesn't include one. They show a dark box until the poster is ready, which the plan allowed for.
- **Full screen presents `AVPlayerViewController` modally** from the top view controller, not through a SwiftUI `fullScreenCover`. That gives the system's own close button and swipe-down, and shares the inline player.
- **Fixed in passing (chunk 1 bug):** large images in embed cards were sized to the column, not the card, and overflowed by its padding.
- **The fixture clip is 28 kB**, a little more than the plan's "a few kB", to have both video and sound.

### Verification Performed
- Swift: 34 tests in 10 suites passed (PlaybackTests 4).
- `cargo test -p stoat-ffi`: 84 passed. Desktop `cargo test`: 1499 passed, 0 failed, 5 ignored. Both clippy runs are clean.
- Simulator against `fake_stoat`, in #chat, scrolled to bob's message:
  - the PDF card;
  - the voice note's player, reading "0:00 / 0:04";
  - the clip's poster (the first `testsrc2` frame) with its play button. This also shows range requests work.
  - With `RUSTOAT_DEV_PLAY=f-clip`: the clip playing inline at 2.5 s, with the full-screen button.
  - With `RUSTOAT_DEV_PLAY=f-voice`: the pause button, the scrubber advancing, and "0:03 / 0:04".

### Manual Validation
Deferred to the end: play a video inline and full screen; play a voice note; start one while another plays. Also check on a device that a large video from Stoat's real file server streams, which depends on its range support.
