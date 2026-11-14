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
