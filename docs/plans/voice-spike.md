# Plan: Voice spike, a go/no-go gate (RuSToat milestone 32)

## Summary
Milestone 32 of the [roadmap](roadmap.md) finds out whether the LiveKit Rust SDK (`livekit` crate) is a sound base for voice and video in RuSToat:
- **Add the SDK** next to GPUI on macOS and measure what it costs: build time, binary size, what it needs.
- **Join one voice channel, audio only,** on the default microphone and speakers, from a small Join / Leave button in a voice channel's header. Talk to someone on Stoat's web client.
- **Write up the result and decide go or no-go.** Milestones 33–37 happen only on "go".

The code is merged and switched on (Decision 2), so a "go" leaves milestone 33 a working join to build on. A "no-go" means a follow-up change removes it.

The references are Stoat's server (`stoatchat/stoatchat` v0.16.2: `crates/delta/src/routes/channels/voice_join.rs`, `crates/delta/src/routes/root.rs`, `crates/core/database/src/voice/{mod,voice_client}.rs`, `crates/core/models/src/v0/channels.rs`, `crates/core/result/src/lib.rs`, `crates/core/database/src/events/client.rs`, `crates/daemons/voice-ingress/src/api.rs`, `livekit.example.yml`) and the web client (`stoatchat/for-web` `stoat-for-web-v0.16.3`: `packages/client/components/rtc/{state.tsx,components/RoomAudioManager.tsx}`, `packages/client/components/ui/components/features/voice/callCard/VoiceCallCardPreview.tsx`, `packages/client/package.json`). Both were read on 2026-11-08 from shallow clones of those tags. The SDK is `livekit` 0.10.1 with `libwebrtc` 0.4.0, `webrtc-sys` 0.3.53 and `webrtc-sys-build` 0.3.21, read from the cargo registry the same day.

## Approach
**What Stoat does** (from the server source):
- **`GET /` lists the voice nodes** under `features.livekit`: `{ enabled, nodes: [{ name, lat, lon, public_url }] }`. On `api.stoat.chat` today that's one node, `hel1` at `wss://livekit.hel1.voice.stoat.chat`.
- **Joining:** `POST /channels/{id}/join_call` with `DataJoinCall { node?, force_disconnect?, recipients? }` returns `CreateVoiceUserResponse { token, url }`. Both types are in `revolt-models` 0.16.2, which RuSToat already pins.
  - Checks, in order:
    - `LiveKitUnavailable` when the instance has no nodes.
    - `IsBot` for a bot asking to force-disconnect.
    - `NotAVoiceChannel` for a channel without `voice`.
    - Missing **`Connect`** gives `MissingPermission`.
    - `CannotJoinCall` when the channel is at `voice.max_users` (Manage Channel bypasses it).
    - `UnknownNode` for an unknown node.
    - Without `force_disconnect`, `AlreadyConnected` when you're already in a call anywhere, on any device. **With `force_disconnect: true`**, Stoat instead removes you from every voice channel you're in (through LiveKit's `remove_participant`, ignoring failures) and deletes those voice states, so the other session's call ends and `VoiceChannelLeave` goes out before your join.
  - **If the channel already has a node, Stoat uses that one** and ignores the one you send.
  - **The token lasts 10 seconds** (`with_ttl(10 s)`), so the LiveKit connection must start straight after the response.
  - The room's name is the channel ID. Your LiveKit identity is your user ID, your name is `username#discriminator`, and the metadata is your user object as JSON.
  - The grants are: `can_publish_sources` holds `microphone` if you have **`Speak`**, plus `camera`/`screen_share`/`screen_share_audio` with **`Video`** and the instance's `video` limit; `can_subscribe` needs **`Listen`**; `can_publish_data` is false.
- **There's no "leave" endpoint.** Leaving is disconnecting from LiveKit. Its webhook reaches Stoat's `voice-ingress` daemon, which removes your voice state and sends `VoiceChannelLeave`.
- **Who's in voice:** `VoiceChannelJoin` / `VoiceChannelLeave` / `VoiceChannelMove` events and `voice_states` in `Ready`. That's milestone 33, not this spike.
- **Stoat's default channel permissions** (`DEFAULT_PERMISSION`) include `Connect`, `Speak`, `Listen` and `Video`, so ordinary members can join and talk.

**What the web client does** (`livekit-client` ~2.22.3):
- **Picking a node:** it fetches each node's `public_url` (with `wss` replaced by `https`) and takes the first that answers. It then calls `joinCall(node)` and `room.connect(url, token, { autoSubscribe: false })`.
- **Audio:** it subscribes to every remote **audio** publication itself (microphone, screen-share audio), and plays it through hidden `<audio>` elements with per-person volume and mute.
- **Microphone:** when connected, it turns the mic on only if you may speak and your saved mic setting is on. Capture uses the browser's echo cancellation, noise suppression and gain control (plus optional RNNoise).
- **Leaving:** `room.disconnect()`. It plays join and leave sounds.

**What the SDK needs** (from the crates, plus a web search on 2026-11-08):
- **Prebuilt WebRTC.** `webrtc-sys`'s build script downloads `webrtc-mac-arm64-release.zip` (about 264 MB) from the `webrtc-a545de4` GitHub release into a scratch directory under the build tree. `LK_CUSTOM_WEBRTC` points it at a local copy instead. It compiles a C++20/ObjC++ bridge with `cxx` and links `libwebrtc.a` statically, plus 17 system frameworks (Foundation, AVFoundation, Security, CoreAudio, AudioToolbox, AppKit, CoreMedia, CoreGraphics, VideoToolbox, CoreVideo, OpenGL, Metal, MetalKit, QuartzCore, IOKit, IOSurface, ScreenCaptureKit).
- **`-ObjC` on the final link.** libwebrtc ships Objective-C categories. Without the flag the app aborts at startup with "unrecognized selector" (rust-sdks #795). The SDK's own `cargo:rustc-link-arg` doesn't reach our binary, so we emit it ourselves.
- **Tokio.** `Room::connect` and its events run on tokio. RuSToat's `runtime()` (`src/runtime.rs`) is a 2-worker multi-thread runtime, and `spawn_io` already bridges it to GPUI. Zed does the same with its own `gpui_tokio`.
- **TLS:** the feature `rustls-tls-native-roots`, matching our `tokio-tungstenite`.
- **Dependency resolution** was checked in a scratch copy: it resolves cleanly with one `rustls` 0.23 and one `prost` 0.14. It adds about 65 crates, including second copies of `reqwest` (0.12) and `tokio-tungstenite` (0.29). Duplicated crates go from 118 to 135.
- **`PlatformAudio` (since 0.8.0, rust-sdks PR #1037)** switches WebRTC's audio device module to CoreAudio:
  - Remote audio that's subscribed plays on the default output.
  - A track made from `audio.rtc_source()` captures the default microphone, with WebRTC's software echo cancellation, noise suppression and gain control.
  - Without it, remote audio is decoded and dropped, and no microphone is opened.
- **Known open `PlatformAudio` bugs:**
  - #1468: after the last peer connection is dropped, reconnecting may hang and `recording_devices()` can spin.
  - #1560: publishing any audio track opens the mic.
  - #1638: `set_*_device` fails while audio is running.
- **Microphone permission:**
  - Under `cargo run`, macOS asks on behalf of the terminal.
  - The app bundle needs `NSMicrophoneUsageDescription` in `Info.plist`.
  - A hardened-runtime (release-signed) build needs the `com.apple.security.device.audio-input` entitlement; without it the mic is silently denied.
- **Zed** (also GPUI) uses a fork of the SDK and `cpal`/`rodio` instead of `PlatformAudio`. It passes `-Wl,-ObjC` from its binary's `build.rs`.

**What exists already** (line numbers as of 2026-11-08):
- **Root config** (`src/api/client.rs` ~37–135):
  - `RootConfig` / `RootFeatures` read `autumn`, `january` and `limits`, but not `livekit`.
  - `ChatView::new` fetches it with `spawn_io` (`src/ui/chat.rs` ~556–594) and keeps what it needs on `ChatView` (`web_app`, `invite_max_days`, `group_size`).
  - The test fixture `tests/fixtures/root.json` has no `livekit` section.
- **Client:**
  - `create_channel` (~966) is the model for a request with a private `Serialize` body and a typed response.
  - `send` / `send_raw` are at ~1247.
  - `ApiError` (`src/api/error.rs`) keeps Stoat errors as `Api(revolt_result::Error)`.
- **Permissions:**
  - `src/permissions.rs` has `can_view` (~85), `can_pin` (~95), `can_invite` (~140) and `can_manage_channel` (~161) on top of `channel_permissions`.
  - The `ChatStore` wrappers are at `src/store.rs` ~2604–2680.
  - The tests are in `tests/permissions.rs`.
- **Voice channels:**
  - `ChannelKind::Text { voice: bool }` (`src/store.rs` ~43).
  - The header icon is `Volume2` (`channel_icon`, `src/ui/chat.rs` ~11896).
  - Voice channels already open their text chat.
- **The header:** in `render_main` (`src/ui/chat.rs` ~8416), the header's right side has search, then "Add friends to group", "Create invite", Pins and Members buttons, each gated by a store check.
- **Errors and toasts:**
  - Errors are shown with `window.push_notification(Notification::error(..))`.
  - `profile_edit::error_text(err, fallback)` words rate limits and validation.
- **Teardown:**
  - Each account's `ChatView` is dropped when it's signed out, logged out or the window really closes. That already drops its connection.
  - Nothing is torn down explicitly.
- **Bundle and signing:**
  - `scripts/bundle.sh` writes `Info.plist` inline (~48–76) and, with `--release-signing`, signs with the hardened runtime and `scripts/entitlements.plist` (currently an empty `<dict/>`).
  - There's no `build.rs`.
  - `.cargo/config.toml` only sets the runner.

**Decisions** (user, 2026-11-08):
1. **The spike lives in the app:** a Join / Leave button in a voice channel's header. There's no separate example program.
2. **The code is merged switched on,** with no cargo feature.
3. **Audio goes through `PlatformAudio`** (WebRTC's own CoreAudio audio module).
4. **"Go" means all of these,** measured on this Mac. Build-time and size figures beyond the clean-build limit are measured and reported, but don't decide anything.
   - Two-way audio with a web client user stays clear for 5 minutes.
   - Five join/leave/join cycles finish without a hang or crash.
   - A clean release build takes no more than 5 minutes longer than before.
   - CPU stays under 5% in a silent call.
5. **The audio partner is the user,** on stoat.chat's web client with a second account, against the official `hel1` node.
6. **Windows and Linux will come later** but aren't priorities now. Nothing in this spike should close that door: keep macOS-only bits behind `cfg(target_os = "macos")` or in the bundle scripts. `PlatformAudio` also covers WASAPI and PulseAudio/ALSA.
7. **Joining from RuSToat takes over the call** (user, after the plan's first draft). RuSToat always sends `force_disconnect: true`, so a call you're in on another device or client (the web client, another RuSToat) ends and this one joins, with no error and no prompt.

**Rejected alternatives.**
- **`cpal` plus `NativeAudioStream` / `NativeAudioSource`, as Zed does.** It's more code: resampling, a playout mixer, our own echo cancellation. `PlatformAudio` already does all of it and is what milestone 34 would want. If `PlatformAudio` fails the go criteria, the write-up says so, and trying `cpal` becomes the first item of a "go, with changes" verdict rather than part of this spike.
- **Zed's fork of the SDK.** Its patches (rustls config hooks, clang builds, a libyuv fork, AGC2 settings) solve Zed's problems. Upstream 0.10.1 is 405 commits ahead and has `PlatformAudio`; the fork doesn't.
- **`-ObjC` in `.cargo/config.toml` `rustflags`** (the SDK README's suggestion). That passes it to every link, including the 60-odd integration test binaries, which never touch WebRTC. `cargo:rustc-link-arg-bins=-ObjC` from a `build.rs` puts it only on the app binary, the way Zed does.
- **`auto_subscribe: true`.** That would also subscribe to camera and screen-share video that someone on the web client publishes, decoding video nobody draws. Like the web client, subscribe to audio publications only.
- **Pinging every node and picking the fastest,** as the web client does. With one node there's nothing to pick, and Stoat overrides the choice for a channel that already has a node. Use the first node; milestone 33 can add the ping when an instance has several.

## Assumptions
1. **Pin `livekit = { version = "=0.10.1", features = ["rustls-tls-native-roots"] }`** with default features (`native`), as the revolt crates are pinned. Upstream, not Zed's fork.
2. **Measure on this Mac only** (Apple Silicon, `aarch64-apple-darwin`). Intel Macs and other platforms aren't checked.
3. **Voice is offered only when the instance has it:** `features.livekit.enabled` and at least one node. The node sent is the first one listed.
4. **The Join button shows** in a voice channel's header when the instance has voice and you have `Connect` there. **You publish your microphone only with `Speak`**, and **remote audio is subscribed only with `Listen`** (without it LiveKit refuses anyway).
5. **Always `force_disconnect: true`** (Decision 7). It also clears a stale voice state left by a crash, which would otherwise block joining until LiveKit timed out. `AlreadyConnected` shouldn't happen any more; if it does, it falls to the generic text in Assumption 8. Stoat refuses `force_disconnect` from bots (`IsBot`), but RuSToat logs in as a user, so it's never a bot.
6. **One call per account.** Joining another voice channel leaves the current one first. Across accounts nothing is coordinated: two accounts can each be in a call, and both play. Milestone 33 decides how that should work.
7. **Where you see it:**
   - The header button reads "Join voice" (or "Joining…", disabled) in a voice channel you aren't in, and "Leave voice" in the one you're in.
   - The channel's row in the list shows its voice icon in the accent colour while you're connected.
   - There's no call bar, mute button, participant list or sounds; those are milestone 33.
8. **Errors toast**, and the button goes back to "Join voice". The texts:
   - `LiveKitUnavailable`, `UnknownNode`: "This Stoat instance can't host voice calls right now."
   - `NotAVoiceChannel`: "This isn't a voice channel."
   - `CannotJoinCall`: "This voice channel is full."
   - `MissingPermission`: "You can't join this voice channel."
   - A LiveKit connection failure: "Couldn't connect to the voice server. Try again."
   - Anything else: `profile_edit::error_text(err, "Couldn't join the call. Try again.")`.
   - The LiveKit error itself is logged at warn.
9. **An unexpected disconnect** (LiveKit's `Disconnected` event, or the room closing under us) toasts "Disconnected from voice." and resets the button. There's no automatic rejoin; the SDK's own reconnect (`Reconnecting` / `Reconnected`) is left to do its work and logged.
10. **Leaving and teardown:**
    - "Leave voice" closes the room (`Room::close`) on the tokio runtime, then drops the `PlatformAudio` handle.
    - Dropping the account's `ChatView` (log out, sign out, window really closed) does the same from `Drop`, spawned onto `runtime()` so it doesn't block the UI thread.
    - Quitting the app doesn't wait; LiveKit's own timeout ends the participant.
11. **`PlatformAudio` is created when joining and dropped after leaving,** so it doesn't hold the microphone or the audio device between calls. If #1468 makes join → leave → join hang, the fallback is to keep one `PlatformAudio` alive for the process. The write-up records which was needed.
12. **The microphone permission:**
    - `NSMicrophoneUsageDescription` ("RuSToat uses the microphone in voice calls.") is added to `scripts/bundle.sh`'s `Info.plist`.
    - `com.apple.security.device.audio-input` is added to `scripts/entitlements.plist`, with its comment updated.
    - The first-run prompt is checked both under `cargo run` (where it's the terminal's) and in the bundle.
13. **Logging:**
    - Join, leave and LiveKit room events go out at info, with channel and participant IDs.
    - Every audio track subscribed or unsubscribed is logged at debug.
    - The token is never logged.
14. **The write-up lives in `docs/plans/voice-spike-implementation.md`,** this repo's usual implementation record, under a final "Results and decision" section. The roadmap's milestone 32 entry gets the decision and a link.
15. **Baselines are measured on the commit before the SDK is added,** with the same commands, so the numbers compare.

## Plan

### Chunk 1: Add the SDK and measure what it costs
**Goal:** `livekit` builds and links into RuSToat without breaking it, and the build-time and size cost is measured against today's numbers.
**Files likely touched:**
- `docs/plans/voice-spike-implementation.md` (new): the baseline figures, taken **before** any change (Assumption 15):
  - `cargo clean && time cargo build --release`
  - `ls -l target/release/stoat-gpui`
  - `scripts/bundle.sh release && du -sh target/release/RuSToat.app`
  - `touch src/ui/chat.rs && time cargo build --release`
  - `cargo clean && time cargo build`
  - `time cargo test --no-run`
  - `du -sh target`
- `Cargo.toml`: `livekit` (Assumption 1), with a comment in the file's style: what it is, why it's pinned, that it downloads WebRTC.
- `build.rs` (new): on macOS targets (`CARGO_CFG_TARGET_OS == "macos"`), `cargo:rustc-link-arg-bins=-ObjC`, with a comment on why (rust-sdks #795) and why only the binary.
- `src/voice.rs` (new, small): a `webrtc_ready()` smoke function that touches the SDK. For example, create and drop a `PlatformAudio` and return the default devices' names. `main.rs` calls it once at startup at debug level, under `RUST_LOG`, so the binary really links and initialises WebRTC. Remove the startup call in chunk 3.
- `src/lib.rs`: `pub mod voice;`.

**Depends on:** None.
**Risks:**
- **Link failures or a crash at launch:** missing `-ObjC`, duplicate symbols between libwebrtc's bundled code (BoringSSL, libyuv, abseil, protobuf) and anything else in the tree, or libc++ mismatches. If it doesn't link, record the exact error in the write-up before trying fixes. That's spike data.
- **The 264 MB download** happens inside the build. Check whether `cargo clean` removes the scratch copy (it lives under the target directory). If it does, every clean build downloads again: record both the download time and a clean build with the archive already present (`LK_CUSTOM_WEBRTC`, or a warm scratch directory).
- **Test binaries:** with `-ObjC` only on the binary, `cargo test` must still link and pass. If any test binary fails to link because the library references WebRTC, record it, then fall back to `cargo:rustc-link-arg=-ObjC` (all targets) and measure `cargo test --no-run` again.
- **Toolchain:** the SDK's C++ needs Xcode's clang with C++20. The pinned Rust 1.99.1 and Xcode should be fine; if not, that's a finding.

**How to verify:**
- `cargo test && cargo clippy --all-targets -- -D warnings` pass.
- `RUST_LOG=stoat_gpui=debug cargo run` starts the app, logs the default input and output device names from `webrtc_ready()`, and logs in as before.
- The same figures as the baseline, taken after the change, are written next to it in the implementation record.

**Manual check:** the app launches from `scripts/run-bundled.sh release` and works as before. If macOS asks for the microphone at launch, note it (it shouldn't, since no track is published).
**Shippable when:** RuSToat builds, launches and passes its tests with `livekit` linked, and the before/after build figures are recorded.

### Chunk 2: Ask Stoat for a call
**Goal:** RuSToat knows whether an instance has voice, and can ask Stoat for a LiveKit URL and token for a voice channel.
**Files likely touched:**
- `src/api/client.rs`:
  - `RootFeatures.livekit: Option<VoiceFeature>`, with `VoiceFeature { enabled, nodes: Vec<VoiceNode> }` and `VoiceNode { name, public_url }` (other fields ignored).
  - `RootConfig::voice_node() -> Option<&str>`: the first node's name when enabled (Assumption 3).
  - `join_call(channel, node) -> CreateVoiceUserResponse` (`POST /channels/{channel}/join_call`), sending a private struct with just `node` and `force_disconnect: true` (Decision 7), not `DataJoinCall`, which would send `"recipients": null`. Add `CreateVoiceUserResponse` to the `revolt_models::v0` import list. Its doc says the token lasts 10 seconds.
- `src/voice.rs`: `join_error_text(&ApiError) -> String` (Assumption 8, minus the LiveKit one).
- `src/store.rs`, `src/permissions.rs`: `can_connect(channel)`, `can_speak(channel)`, `can_listen(channel)`, like `can_pin`.
- `src/ui/chat.rs`: keep `voice_node: Option<String>` on `ChatView` from the root config, beside `group_size`.
- `tests/fixtures/root.json`: add the live `livekit` section (one `hel1` node).

**Depends on:** None (independent of chunk 1).
**Risks:**
- **Older or self-hosted instances without `livekit`** must still parse. `#[serde(default)]` everywhere, as the other features do.
- **`revolt_models`'s `CreateVoiceUserResponse` behind a feature:** if it doesn't derive `Deserialize` with the features enabled, use a private struct `{ token, url }` instead.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`:
- `tests/api_instance.rs`: the fixture gives `voice_node() == Some("hel1")`; `"livekit": {"enabled": false, "nodes": [...]}`, an empty node list and a missing `livekit` give `None`.
- `tests/api_voice.rs` (new, wiremock): `POST /channels/c1/join_call` sends exactly `{"node":"hel1","force_disconnect":true}` with the session token and parses `{"token":"t","url":"wss://x"}`; 400 `{"type":"CannotJoinCall"}` and 403 `{"type":"MissingPermission","permission":"Connect"}` stay `ApiError::Api`.
- `src/voice.rs` unit tests: each error in Assumption 8 gives its text; a rate limit gives `error_text`'s wording.
- `tests/permissions.rs`:
  - `can_connect`, `can_speak` and `can_listen` are true with Stoat's default permissions.
  - Each is false when a channel override denies it.

**Shippable when:** the root config's voice nodes and `join_call` are parsed and tested, and the permission helpers exist.

### Chunk 3: Join and leave a voice channel
**Goal:** In a voice channel, "Join voice" connects you to its LiveKit room with your default microphone and speakers, and "Leave voice" disconnects.
**Files likely touched:**
- `src/voice.rs` (the tokio side, no GPUI):
  - `VoiceCall::join(url, token, publish_mic: bool, subscribe: bool) -> Result<(VoiceCall, VoiceEvents), VoiceError>`. It creates `PlatformAudio` and calls `Room::connect` with `auto_subscribe: false`. If `publish_mic`, it publishes `LocalAudioTrack::create_audio_track("microphone", audio.rtc_source())` with `TrackSource::Microphone` (the source Stoat's grant allows).
  - A task on `runtime()` drains `RoomEvent`s:
    - Subscribes (when `subscribe`) to each remote **audio** publication, both those present at connect and those arriving with `TrackPublished`.
    - Logs participants joining and leaving, tracks subscribed, `Reconnecting` / `Reconnected`.
    - Forwards `Disconnected` (and the stream ending) on a `futures` channel, `VoiceEvents`.
  - `VoiceCall::leave(self)`: `room.close()`, then drop the audio. `Drop` spawns the same on `runtime()` (Assumption 10).
  - `VoiceError` is either the API's or LiveKit's.
  - Remove chunk 1's `webrtc_ready()` and its startup call.
- `src/ui/chat.rs`:
  - `voice: Option<ActiveCall>` on `ChatView`, holding the channel ID, the state (`Joining` / `Connected`), the `VoiceCall` once connected, and the task listening to `VoiceEvents`.
  - `join_voice(channel, window, cx)`: leave any current call (Assumption 6), set `Joining`, then in one `spawn_io`: `join_call` followed at once by `VoiceCall::join` (the 10-second token). On success set `Connected`; on failure toast (Assumption 8) and clear.
  - `leave_voice(cx)`.
  - A `Disconnected` event toasts "Disconnected from voice." and clears (Assumption 9).
  - The header button in `render_main` (Assumption 7), shown for voice channels when `voice_node` is known and `can_connect`.
  - The connected channel's row icon in the accent colour.
- `scripts/bundle.sh`: `NSMicrophoneUsageDescription`. `scripts/entitlements.plist`: `com.apple.security.device.audio-input`, and its comment (Assumption 12).

**Depends on:** Chunks 1 and 2.
**Risks:**
- **The SDK's API in 0.10.1** may differ from the README snippets (names of `TrackSource`, `TrackPublishOptions`, how a remote publication is subscribed). Read the crate's source in the cargo registry rather than guessing.
- **Where things run.**
  - `Room` and `PlatformAudio` are created and dropped on tokio worker threads, never on GPUI's main thread. WebRTC's own threads call CoreAudio.
  - Check that nothing the SDK does blocks the UI: the window must stay responsive while joining.
  - If `PlatformAudio::new()` must run on the main thread, record that as a rough edge and move it.
- **#1468: join → leave → join hangs.** Test it right away (chunk 4 does it five times). If it hangs, apply Assumption 11's fallback.
- **#1560: the mic opens on publish.** Expected: the permission prompt appears at the first join, not at launch.
- **The permission prompt under `cargo run`** is the terminal's. If the terminal was denied before, `tccutil reset Microphone` (for the terminal's bundle ID) resets it.
- **The 10-second token:** `join_call` and `Room::connect` must run back to back in one task, with nothing that waits on the UI in between.
- **Deafen / echo.** With speakers instead of headphones, WebRTC's echo cancellation should stop the web client user hearing themselves. If it doesn't, that's a finding, not a blocker for the spike.

**How to verify:**
- `cargo test && cargo clippy --all-targets -- -D warnings`.
- `RUST_LOG=stoat_gpui=debug cargo run`. In a voice channel on a test server, click "Join voice": the log shows the join, the room, and (with someone there) their audio track subscribed. "Leave voice" logs the close.

**Manual check:**
- With the web client's user in the same voice channel, each hears the other.
- The button reads Join / Joining… / Leave correctly.
- The row icon is coloured while you're connected.
- Log out while in a call: the web client sees you leave.
- In a text channel, or on an instance without voice, there's no button.

**Shippable when:** you can join a voice channel, hear and be heard by a web client user, and leave, and the tests pass.

### Chunk 4: Trial, write-up and decision
**Goal:** The go criteria are checked, the findings are written up, and the roadmap records go or no-go.
**Files likely touched:**
- `docs/plans/voice-spike-implementation.md`: a "Results and decision" section with:
  - **Build:** clean release and debug build times, before and after; the WebRTC download size and time, and whether `cargo clean` removes it; the incremental release rebuild after touching `src/ui/chat.rs`; `cargo test --no-run`; `target` on disk.
  - **Size:** the release binary and `.app`, before and after.
  - **What the crate needs:** the dependency count and duplicates, the frameworks, `-ObjC`, tokio, the download, Xcode/C++20, the mic usage description and entitlement.
  - **Audio:** clarity both ways over 5 minutes, latency as it felt, echo on speakers, behaviour when the default device changes mid-call (plug in or unplug headphones; just observed, not gated), and the permission prompt under `cargo run` and in the bundle.
  - **Stability:** five join/leave/join cycles, and leaving by logging out and by closing the window.
  - **CPU:** in a silent call (connected, mic published, nobody speaking) for 60 s, measured with `top -l 13 -s 5 -pid $(pgrep -n stoat-gpui) -stats cpu`, ignoring the first sample. Note the idle figure outside a call for comparison.
  - **Rough edges:** anything surprising, with the issue links above where they applied.
  - **The verdict** against Decision 4, criterion by criterion, then **go**, **go with changes** (what must change first) or **no-go** (what to remove).
- `docs/plans/roadmap.md`: milestone 32's entry gets the verdict and a link to the record. If no-go, mark milestones 33–37 as not planned and say why.
- Any small fixes the trial turns up (log noise, a toast's wording) in `src/voice.rs` / `src/ui/chat.rs`.

**Depends on:** Chunk 3.
**Risks:**
- **A borderline result** (for example, CPU at 4–6%, or one hang in five cycles). Repeat the measurement once. If it's still borderline, write "go with changes" with the specific concern rather than rounding it away.
- **A failure that might be our code, not the SDK.** Before calling no-go, check the same thing against the SDK's own example if it's quick (for example, `examples/` in `livekit/rust-sdks` at the 0.10.1 tag), and say which it was.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings` pass. The record has every figure and a verdict per criterion. The roadmap links to it.
**Manual check:** the user and the implementer run the trial together: the 5-minute two-way call with the web client, five join/leave cycles, and the CPU measurement.
**Shippable when:** the write-up and the decision are committed, and the roadmap says whether milestones 33–37 go ahead.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate: older drift makes it reformat unrelated files.
- Format only the files you touch, with `rustfmt --edition 2024 <file>`. Never run it on a `mod.rs`, `lib.rs` or `main.rs`, since it formats their child modules too. For `src/main.rs` and `src/lib.rs`, use `--check` and hand-format.
- A file that already had drift is left that way: hand-format your new code in it, and check its count of `Diff in` lines (from `rustfmt --edition 2024 --check <file>`) hasn't grown. Starting counts on 2026-11-08:
  - with drift: `src/api/client.rs` 3, `src/ui/chat.rs` 17, `src/store.rs` 15, `src/lib.rs` 63 (from its child modules; check by hand instead)
  - clean: `src/permissions.rs`, `src/runtime.rs`, `tests/permissions.rs`, `tests/api_instance.rs`
  - New files (`build.rs`, `src/voice.rs`, `tests/api_voice.rs`) must be clean.
- After chunk 1, the suite builds WebRTC too. The first run after `cargo clean` downloads it.

### Unit Tests
- **`src/voice.rs`:** `join_error_text` for every case in Assumption 8.

### Integration Tests
- **`tests/api_instance.rs`:** `voice_node()` with the live fixture, disabled voice, no nodes, and no `livekit` key.
- **`tests/api_voice.rs`** (new, wiremock): `join_call`'s exact path and body, the response parsed, and Stoat errors kept as `Api`.
- **`tests/permissions.rs`:** `can_connect`, `can_speak` and `can_listen` with the defaults and with a denying override.
- The LiveKit connection itself isn't tested automatically: it needs a LiveKit server. The trial in chunk 4 covers it.

### Manual Tests
You need a stoat.chat account in RuSToat, a second account in the web client (another browser or device, ideally with headphones), and a test server with a voice channel that both can join. RuSToat can create one (milestone 19).
1. Run every chunk's manual check.
2. Join from RuSToat first, then from the web client; then the other way round. Both hear each other either way.
3. In RuSToat, join one voice channel, then another: you leave the first (the web client shows it).
4. Join a voice channel in the web client with the same account RuSToat uses, then join from RuSToat: the web client's call ends, RuSToat connects, and the other account sees one of you in the channel, not two. Then try it the other way round. The web client doesn't force-disconnect, so it should refuse with its own "already connected" error, and RuSToat's call should carry on. Record what actually happens.
5. With the bundled app on first launch: the microphone prompt names RuSToat and its usage text.
6. Light and dark themes: the button and the coloured row icon are readable.

## Definition of Done
- [ ] `livekit` 0.10.1 is in `Cargo.toml`, `build.rs` passes `-ObjC` to the app binary on macOS only, and RuSToat builds, launches and passes `cargo test` and `cargo clippy --all-targets -- -D warnings`.
- [ ] `RootConfig` reads `features.livekit`. An instance without voice, or with it disabled, shows no Join button.
- [ ] `join_call` sends exactly `{"node":…,"force_disconnect":true}` to `/channels/{id}/join_call`, and each Stoat voice error has its own message. Joining from RuSToat ends the same account's call on another client.
- [ ] In a voice channel where you have Connect, "Join voice" connects with the default microphone (only with Speak) and plays others' audio (only with Listen); "Leave voice" disconnects. Joining another channel leaves the first.
- [ ] Logging out, signing out or closing the window while in a call disconnects it.
- [ ] The bundle's `Info.plist` has `NSMicrophoneUsageDescription`, and `scripts/entitlements.plist` has `com.apple.security.device.audio-input`.
- [ ] `docs/plans/voice-spike-implementation.md` records before/after build times, binary and `.app` sizes, the WebRTC download, what the crate needs, audio quality, five join/leave cycles, CPU in a silent call, and the rough edges, with a verdict per criterion of Decision 4 and an overall go, go with changes, or no-go.
- [ ] `docs/plans/roadmap.md` records the decision for milestone 32 and links to the record.
- [ ] No touched file's rustfmt drift grows.
