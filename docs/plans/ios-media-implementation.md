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

## Chunk 4: Link embeds and GIFs

### Changes Made
- `ios/RuSToat/Chat/Media/EmbedCardView.swift` (new), replacing `EmbedCard` in `MessageRowView.swift`:
  - Cards have a 4 pt colour bar (the embed's `0xRRGGBBAA`, or the separator colour) and the site's icon (16 pt, through `RemoteImage`) beside its name.
  - The title is a tinted button that opens `link`.
  - The description is a bot's Markdown through `RichTextView`, or a website's plain text (5 lines).
  - A thumbnail sits beside the text at its 80 pt box. A large image goes under the text, sized inside the bar and padding.
  - A `playable` picture gets a play badge, and opens the link: the system hands YouTube links to their app when it's installed. Any other large image opens the page too.
  - A bot's `media` goes through `AttachmentView`, so images, video and audio work there as well.
  - Image embeds open the viewer. GIF posters keep their badge and open their page. Bare video links are cards.
  - `Color(rgba:)`.
- `ios/RuSToat/Chat/Media/AnimatedImage.swift` (new):
  - `AnimatedFrames`:
    - `isGIF` (the `GIF8` signature) and `frameCount`;
    - `decode`, which reads frames with `CGImageSourceCreateThumbnailAtIndex` at the drawn size. It gives nil for stills and for GIFs over 200 frames.
    - Frame delays work as in browsers: an unclamped delay, and anything under 20 ms becomes 100 ms.
    - `side(maxPixels:width:height:frames:)` decodes smaller when all the frames would pass a 48 MB budget.
    - `index(at:)` loops through the frames by their delays.
  - `AnimatedImage` shows the still until the frames are decoded, then plays them in `TimelineView(.animation)`. It decodes in its `.task` and drops the frames `onDisappear`, so only GIFs on screen hold frames.
- `ios/RuSToat/Shared/ImageLoader.swift`:
  - when it downloads a picture, it notes animated GIFs by their bytes (`isAnimated(url)`);
  - `frames(_:fitting:scale:)` decodes them from the bytes in the disk cache, off the main actor, without caching the frames.
- `ios/RuSToat/Chat/Media/InlineImage.swift`: `RemoteImage` plays a picture that turned out to be animated through `AnimatedImage`. That covers GIF attachments, GIF links and bot media alike.
- `tests/fixtures/media/orbit.gif` (new, 5 kB): 16 frames at 160×120, a ball circling, made with PIL.
- `tests/fixtures/messages_media.json`: message M08 has `orbit.gif` uploaded (with its ThumbHash) and linked (a `special: GIF` website, which the core makes an image embed).
- `ios/stoat-ffi/examples/fake_stoat.rs`: serves `/attachments/f-gif`, and proxied `.gif` addresses as `image/gif`.
- `ios/stoat-ffi/tests/media.rs`: one more test, 9 in all. It checks that a GIF upload is an `Image` at 160×120, and a GIF link is a bare picture embed.
- `ios/RuSToat/Chat/ChatView.swift`: DEBUG `RUSTOAT_DEV_SCROLL` scrolls twice, 400 ms apart, because the lazy list only knows the rows' real heights once it has laid them out.
- `ios/RuSToatTests/AnimatedImageTests.swift` (new, 5 tests):
  - the frames and delays of a two-frame GIF built in the test with `CGImageDestination`, decoded at the size drawn;
  - 201 frames aren't animated, but 200 are;
  - stills and PNGs aren't animated;
  - looping by delays;
  - the memory budget.

### Divergences
- **A memory budget of 48 MB per GIF** was added on top of the plan's "decode at the displayed size". 200 frames at a 300×200 pt row at 3× would be about 430 MB, so big or long GIFs are decoded smaller.
- **A GIF is detected when it's downloaded**, by its bytes, and every remote picture plays if it turns out to be animated. No URL or content-type checks, as the plan's risk said.
- **A card's large image opens the page** when it isn't a video, like the title, rather than opening the viewer.
- **The bot description's Markdown keeps `RichTextView`'s own sizes.** `.font(.footnote)` doesn't reach its blocks, so it's drawn at body size, as desktop draws it.

### Verification Performed
- `cargo test -p stoat-ffi`: 85 passed (media 9).
- Swift: 39 tests in 11 suites passed (AnimatedImageTests 5).
- Desktop `cargo test`: 1499 passed, 0 failed, 5 ignored. Both clippy runs are clean.
- Simulator against `fake_stoat`:
  - the news card with its orange bar, site icon, "News", the title link, the description and the large sunset;
  - the YouTube card with its title link and a play badge on the picture;
  - the tower image embed;
  - the CI bot card with its green bar, title link, rich description (bold "42", code "3.1 s") and media image;
  - the Gifbox poster with its badge, the bare video card, and the repo card with its thumbnail.
- Both GIFs in M08, uploaded and linked, were animating. Two screenshots a third of a second apart show the ball moved from (583, 1555) to (571, 1513) in both.

### Manual Validation
Deferred to the end: paste a YouTube link and a news article on desktop and compare the cards on the phone; send a GIF from desktop's GIF picker and watch it play on the phone.

## Chunk 5: Sending files, with progress

### Changes Made
- `src/api/error.rs`: `ApiError::upload_failure_text()`, moved from `src/ui/chat.rs`'s `upload_error_text`, which desktop now calls.
- `ios/stoat-ffi/src/controller.rs`:
  - `OutgoingFile { path, name, spoiler }` (a UniFFI record).
  - `OutgoingFile::draft()` makes a `DraftAttachment`:
    - sized from disk;
    - typed by the file name;
    - with its dimensions from `attachment::local_image_size`;
    - `temporary: true`, so the store hands it back when its message leaves the pending list;
    - spoiler as marked.
  - `check_files()` applies desktop's `Draft::add_attachment` rules and texts.
  - `send(channel, text, reply_to, attachments)`:
    - `check_send` lets a message of files alone through (the text is still trimmed at both ends, as before), and gives too-long text desktop's text;
    - the files are checked against the limits before anything shows;
    - the pending message carries them as `PendingAttachment`s.
  - `Shared::post` runs desktop's loop:
    - each file not yet uploaded goes through `Shared::upload` in order;
    - a failure is `fail_pending(…, "Couldn't upload {name}: {reason}", false)`, and nothing is posted;
    - then the POST carries the IDs.
  - `Shared::upload`:
    - `upload_attachment` with the file's `upload_name()` (so spoilers go up as `SPOILER_…`);
    - progress through `set_upload_progress` at most every 100 ms (`PROGRESS_INTERVAL`), and always the last one;
    - then `set_uploaded`.
  - `Shared.limits`: the `AttachmentLimits` from `GET /` (defaults until then). `fetch_media` now sets the file server before `web_app`, so a message link being available means uploads can start.
  - `attachment_limits()`.
  - `released_files()`: `take_released_files()` as paths.
- `ios/stoat-ffi/src/actions.rs`: `check_attachments(files)`, which applies the limits with desktop's texts.
- `ios/stoat-ffi/tests/sending.rs`: five new tests (12 in all), and the existing calls pass `Vec::new()` for files.
  - Two files upload in order before the message, whose POST carries their IDs. The spoiler goes up as `SPOILER_plot.txt`. Files alone are a message. The pending row shows `Local` items (name, "4.0 KB", text kind) at 1000 once uploaded. The copies are handed back once, after confirmation.
  - A 413 fails the row with "Couldn't upload huge.mov: it's too big.", posts nothing, and resets progress. Discarding hands the file back.
  - A 502 on the second file's first try leaves the first at 1000. Retry uploads only the second, and posts once with both IDs.
  - A 16 MB upload (256 chunks) redraws at most 12 times and ends at 1000.
  - A limit of 10 bytes and 2 files gives "A message can have at most 2 files." (from both `check_attachments` and `send`) and "big.bin is too big to send. Files can be up to 10 B.". Nothing is sent or uploaded.
- `ios/stoat-ffi/examples/fake_stoat.rs`:
  - `POST /attachments` keeps uploads in memory and answers after 2 s, so the pending row can be seen.
  - Sent messages echo their files: images with no dimensions, other files as `File`.
  - `GET /attachments/up-N` serves uploads back, with range support.
  - Sent messages get time-based ULIDs (`ulid_now`). The old fixed `01JE…S0000` IDs sorted before the fixture history, so a confirmed message jumped to the top of the channel.
- Swift:
  - `ios/RuSToat/Chat/OutgoingFiles.swift` (new):
    - `TrayFile` (the URL of our copy, name, size, spoiler, `outgoing`);
    - `OutgoingFiles` keeps copies in `tmp/outgoing/<uuid>/<name>`:
      - `copy(picked:)` copies inside `startAccessingSecurityScopedResource`;
      - `write(_:named:)`;
      - `safeName`;
      - `remove(path)` deletes the copy's folder, and only inside `outgoing`;
      - `removeAll()` runs at launch.
  - `ios/RuSToat/Chat/AttachmentTray.swift` (new):
    - `AttachmentTray`: chips with a 36 pt preview (images decoded with `ImageLoader.decode`, blurred when marked) or an icon, the name and size, a spoiler toggle, and remove;
    - `LocalAttachmentView`: a pending image from its local copy at the size the sent one will have (blurred if a spoiler), with an `UploadRing` on a dark disc; other files as a card with the ring;
    - `UploadRing`: progress in thousandths, with a tick at 1000.
  - `ios/RuSToat/Chat/Composer.swift`: a ＋ `Menu` (Files for now), hidden while editing. The tray sits above the field. Send is enabled with files alone.
  - `ios/RuSToat/Chat/ChatView.swift`:
    - `.fileImporter` with multiple selection. Each picked file is copied and checked with `check_attachments` against the tray plus itself; the first that doesn't fit is deleted and its reason shown.
    - `send` passes the tray and clears it; the core now owns the copies.
    - Each send gets a `beginBackgroundTask`, ended once its row is no longer sending.
    - DEBUG `RUSTOAT_DEV_ATTACH=<path>` attaches and sends once connected. DEBUG `RUSTOAT_DEV_TRAY=<path>,<path>` attaches without sending.
  - `ios/RuSToat/Model/AccountModel.swift`:
    - `trays` per channel, kept for the session like drafts;
    - after any timeline change or `Ready`, it deletes whatever `releasedFiles()` hands back, whichever channel is open.
  - `ios/RuSToat/RuSToatApp.swift`: clears `tmp/outgoing` at launch.
  - `ios/RuSToat/Chat/MessageRowView.swift`: `.local` attachments use `LocalAttachmentView`.
  - `ios/RuSToatTests/OutgoingFilesTests.swift` (new, 4 tests, serialized):
    - safe names;
    - copies keep their names in their own folders, and removing deletes the folder;
    - files outside `outgoing` are never deleted;
    - picked files are copied out and the user's own left alone.

### Divergences
- **Released files are deleted by `AccountModel` on any change**, not only by the open chat. Messages confirmed or discarded while another screen is up still have their copies removed.
- **`check_attachments` takes `OutgoingFile`s, not bare paths**, so its "too big" text can use the file's own name.
- **No Rust change is needed in chunk 6 for photo dimensions.** `OutgoingFile::draft` reads them from the file with `local_image_size`, so a re-encoded JPEG's pending row already has the sent one's shape. The plan's chunk 6 was updated to say so.
- **Text is still trimmed at both ends.** Desktop's `check_send` keeps a first line's leading spaces, but the existing iOS test (and behaviour) trims them, so `send` trims before checking.
- **Fixed in passing:** `fake_stoat`'s sent-message IDs, described above.
- **Not done:** a background `URLSession` (out of scope, as planned).

### Verification Performed
- `cargo test -p stoat-ffi --test sending`: 12 passed. Full `stoat-ffi`: 90 passed.
- Swift: 43 tests in 12 suites passed.
- Desktop `cargo test`: 1499 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings` (desktop, after moving `upload_error_text`) and the `stoat-ffi` clippy run are both clean.
- Simulator against `fake_stoat`, in #general:
  - `RUSTOAT_DEV_ATTACH=…/portrait.jpg`: the pending "Captain Me" row with the tower photo at its final 2:3 shape, faded, with the ring's tick on its dark disc. After the fake's 2 s, it's confirmed at the bottom of the channel, drawn from the uploaded copy.
  - `RUSTOAT_DEV_TRAY=landscape.jpg,README.md`: the tray with a sunset thumbnail chip and a document chip ("25 KB", "15 KB"), each with the spoiler eye and remove. The ＋ button is beside the field, and Send is enabled with no text.
  - The ring's partial progress isn't visible over loopback, because the upload finishes at once. The throttling test covers progress.

### Manual Validation
Deferred to the end:
- send a PDF from Files and open it on desktop;
- send one over the size limit and read the reason;
- send in Airplane mode, then retry.

## Chunk 6: Photos and the camera

### Changes Made
- `ios/RuSToat/Chat/OutgoingFiles.swift`:
  - `PhotoEncoding`:
    - `jpeg(Data)` makes a JPEG with `CGImageSourceCreateThumbnailAtIndex` (`WithTransform`, so EXIF orientation is applied to the pixels; at most 4096 px on the long side, never scaled up);
    - `jpeg(CGImage)` writes it with `CGImageDestination` at quality 0.85 and no metadata dictionary, so no GPS, EXIF or orientation tag goes with it;
    - `flattened` puts transparent pictures onto white;
    - `jpeg(UIImage)` draws a camera shot upright (in its own orientation), capped at 4096 px, then encodes it the same way.
  - `OutgoingFiles.photo(Data)` (encoded off the main actor), `photo(UIImage)`, and `file(from: PhotosPickerItem)`. A movie comes through `PickedMovie`, a `Transferable` `FileRepresentation` that copies it into the outgoing folder, then `mp4()`. Anything else loads as `Data` and is re-encoded.
  - `mp4(_:)` exports a non-MP4 video to MP4 with `AVAssetExportSession`: `Passthrough` first, then `HighestQuality`. The original is deleted, and it's sent as it was if neither works.
  - `photoName()`: "IMG_yyyyMMdd_HHmmss_n.jpg", with a counter under a `Mutex`.
  - `AttachFailure`: user-facing reasons.
  - `CameraPicker`: a `UIImagePickerController` wrapper. It uses the `.camera` source, or `.photoLibrary` where there's no camera (the simulator).
- `ios/RuSToat/Chat/Composer.swift`: the ＋ menu offers Photo Library, Camera, then Files.
- `ios/RuSToat/Chat/ChatView.swift`:
  - `.photosPicker` for images and videos, allowing as many as there's room for in the tray;
  - the camera in a `fullScreenCover`;
  - `addPhotos` and `attachPrepared` add items in order and stop at the first that doesn't fit, showing why;
  - DEBUG `RUSTOAT_DEV_PHOTO=<path>` runs a photo through the same re-encoding and sends it once connected.
- `ios/project.yml`: `INFOPLIST_KEY_NSCameraUsageDescription: RuSToat uses the camera to take photos to send.` No Photos-library key, because `PhotosPicker` runs out of process.
- `ios/RuSToatTests/OutgoingFilesTests.swift`: `PhotoEncodingTests` (6 tests):
  - a 6000×4000 JPEG comes out at 4096×2731, and a small one isn't scaled up;
  - EXIF orientation 6 on a 40×20 picture gives a 20×40 JPEG with no turning left to do;
  - a photo with a GPS dictionary comes out without one;
  - a transparent PNG becomes a JPEG with white where it was clear;
  - a camera `UIImage` with `.right` orientation comes out upright;
  - photo names are distinct.

### Divergences
- **No Rust change.** As found in chunk 5, the core reads a re-encoded JPEG's dimensions from its header (`local_image_size`), so the pending row already has the sent one's shape. `OutgoingFile` didn't need a `dimensions` field. The plan was amended in chunk 5.
- **The simulator check doesn't drive the photo picker.** `PhotosPicker` runs out of process and `simctl` can't tap it. DEBUG `RUSTOAT_DEV_PHOTO` sends a file through the same `OutgoingFiles.photo` pipeline that picked photos use instead. The picker itself, videos and the camera are left to the manual check on a device, as the plan expected for the camera.
- **Transparency is flattened onto white**, so a transparent PNG doesn't turn black as a JPEG. The plan said only that PNGs are sent as JPEG.

### Verification Performed
- Swift: 49 tests in 13 suites passed (PhotoEncodingTests 6, OutgoingFilesTests 4).
- `cargo test -p stoat-ffi`: 90 passed. Desktop `cargo test`: 1499 passed, 0 failed, 5 ignored. Both clippy runs are clean.
- Simulator against `fake_stoat`, with `RUSTOAT_DEV_PHOTO` and a 6000×4000 JPEG stored sideways (EXIF orientation 6) with a GPS location (51°30′N, 0°7′W):
  - The pending row showed the picture upright, turned as its orientation says, at 2:3 portrait, with the upload tick.
  - The file the fake server received (`/attachments/up-0`) was a JPEG of 2731×4096, with no orientation tag and no GPS. Its only EXIF tag left is ImageIO's own EXIF pointer.
- The built app's Info.plist has `NSCameraUsageDescription` and no Photos-library key.

### Manual Validation
Deferred to the end:
- send a photo from the library and a camera shot from your iPhone;
- check they're upright on desktop;
- check (by downloading one) that location isn't included.

## Summary

_Completed: 2026-11-14_

### Chunks Completed
1. Images inline, with ThumbHash previews (`dc5d471`).
2. The full-screen image viewer (`5dc9929`).
3. Video and audio (`deb8836`).
4. Link embeds and GIFs (`ad5409d`).
5. Sending files, with progress (`3dea8ab`).
6. Photos and the camera (`ab2785d`).

### Divergences
- Attachment and embed enum cases hold one record each (`Image { image: ImageAttachment }`), with the plan's fields.
- `ImageLoader` is awaited, not observed. `EmojiImages` stays observable over it.
- `messages_media.json` and the fixture media were made in chunk 1 and extended in chunks 3 and 4.
- DEBUG launch variables were added for screenshots, since `simctl` can't tap or scroll:
  - `RUSTOAT_DEV_SCROLL`
  - `RUSTOAT_DEV_VIEW`
  - `RUSTOAT_DEV_PLAY`
  - `RUSTOAT_DEV_ATTACH`
  - `RUSTOAT_DEV_TRAY`
  - `RUSTOAT_DEV_PHOTO`
- The viewer's drag-to-close is a UIKit pan that the pager's pan waits on. Its top bar is an overlay. Image embeds open in the viewer too.
- Full-screen video presents `AVPlayerViewController` modally. Videos have no ThumbHash (Stoat stores none).
- GIFs have a 48 MB decoded budget on top of the 200-frame cap. They're detected by their bytes when downloaded.
- A card's large image opens the page, as its title does.
- Released files are deleted by `AccountModel` on any change. `check_attachments` takes `OutgoingFile`s.
- Photo dimensions come from the JPEG's header in the core, so `OutgoingFile.dimensions` wasn't needed and chunk 6 had no Rust change.
- Transparent PNGs are flattened onto white.
- Fixed along the way:
  - card images overflowed the card's padding (chunk 1);
  - `fake_stoat`'s sent messages sorted above the history (chunk 5).
- Pre-existing, not addressed: 18 `#ActorIsolatedCall` warnings in `RichTextTests` on a clean build (milestone 2's test code).

### Definition of Done
- [x] `cargo test && cargo clippy --all-targets -- -D warnings && scripts/ios/test.sh` passes: 1499 desktop, 90 `stoat-ffi` and 49 Swift tests.
- [x] Image attachments and embeds render inline at the store's display size from the first frame, with a ThumbHash preview, the "Show image" card over 8 MB, and spoiler covers. Shown on the simulator; the scroll check on real data is still to do.
- [x] Images load through one cached, de-duplicated, downsampling loader (`ImageLoaderTests`), and rows scrolled back draw from memory (`cached`). Confirming there's no visible reload on a device is part of the manual check.
- [x] The full-screen viewer zooms, pages between a message's images, closes when dragged down, and shares the original file. The parts that need gestures are **pending your check on a device**.
- [x] Video attachments show a poster and play inline and full screen, audio plays inline, and only one item plays at a time (`PlaybackTests`, simulator). Full screen and large real videos are **pending a device check**.
- [x] Link embeds render as desktop's cards (colour, site, title, description, thumbnail or image, play badge), and GIFs animate inline, capped at 200 frames.
- [x] Files can be attached with a spoiler toggle, uploaded with visible progress through the core's `upload_attachment`, and sent. Failed uploads retry without re-uploading finished files, and the instance's limits are enforced with desktop's texts (sending tests). Library photos, videos and camera shots go through the same path; the pickers themselves are **pending a device check**.
- [x] Re-encoded photos are JPEG, at most 4096 px, upright and without GPS (`PhotoEncodingTests`, and the fake server's copy). Temporary files are deleted after sending or discarding (`released_files` tests, `OutgoingFilesTests`).
- [x] `NSCameraUsageDescription` is set, and no Photos-library permission is asked for (checked in the built Info.plist).

### Remaining Manual Steps
On your iPhone, against a real Stoat account (the plan's Manual Tests):
1. Scroll a busy media channel. Expect blurred previews, no jumps, and no reloading when you scroll back.
2. Open a photo full screen, then pinch and double-tap to zoom, swipe to the next, share one to Messages, and drag down to close.
3. Play a video inline and full screen, play a voice note, and start a second item while one plays. Check that a large video streams.
4. Compare link cards with desktop: a news article, a YouTube link and a bot embed. Send a GIF from desktop's GIF picker and watch it play.
5. Send a PDF from Files, a few photos and a video from the library, and a camera shot. Check on desktop that they arrive upright, at a sensible size, and (download one) without location.
6. Send something over the size limit and read the reason. Then send in Airplane mode, turn it off, and retry.
