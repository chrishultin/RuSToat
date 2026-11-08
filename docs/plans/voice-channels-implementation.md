# Implementation Record: Voice channels (RuSToat milestone 33)

_Plan: docs/plans/voice-channels.md_
_Started: 2026-11-08_

Supervision mode: continuous. **Every chunk's manual check is batched into one pass at the end** (the user's choice), so each chunk below records its manual check as pending until that pass.

Pre-existing dirty files, kept out of every commit:
- `.gitignore`: a duplicated, uncommitted "Riptide artifacts (cloud-synced)" block (`.humanlayer/tasks/`), added by tooling.
- `.shadow/`: untracked tooling folder.

The plan itself (`docs/plans/voice-channels.md`) was untracked at the start and is committed with chunk 1.

Baseline before any change: `cargo test` 1061 passed, 0 failed, 5 ignored (71 suites); `cargo clippy --all-targets -- -D warnings` clean.

## Chunk 1: Settle the spike's conditions

### Changes Made
- `docs/plans/voice-channels.md`: chunk 1 amended (the `RUSTOAT_VOICE_PROCESSING` switch dropped, see Divergences); the switch's unit test removed from the Test Plan; the Definition of Done's first item reworded.
- No code changes.

### Divergences
- **Significant, resolved by the plan's own risk ("note that they can't be changed per call"): WebRTC's audio processing can't be turned off from LiveKit 0.10.1 on macOS.**
  - `PlatformAudio::set_echo_cancellation` / `set_noise_suppression` / `set_auto_gain_control` (`livekit-0.10.1/src/platform_audio/mod.rs` ~1347–1390) only call `enable_builtin_aec/agc/ns`, the *hardware* effects, and only if `is_hardware_*_available()`. `configure_audio_processing` does the same and just logs "Audio processing configured: …". LiveKit's own docs (`platform_audio/processing.rs`): "Desktop (macOS, Windows, Linux): Hardware processing is not available. WebRTC's software Audio Processing Module (APM) is always used."
  - The software APM is configured where the microphone track is made: `webrtc-sys-0.3.53/src/peer_connection_factory.cpp` `create_device_audio_track` sets `audio_options.echo_cancellation = auto_gain_control = noise_suppression = true`, with no way in from Rust.
  - So the switch would have been a no-op that logged a misleading line. It wasn't added. Measuring each part's cost would need a patched `webrtc-sys` (out of scope; milestone 34's processing settings face the same wall and should plan for it).
  - Effect on the plan: chunk 1's CPU table is RuSToat (processing on) against the web client and idle; no per-part rows. Decision 1 (keep processing on, target "no worse than the web client") is unaffected. Later chunks: no change.

### Verification Performed
- Source reading as above (LiveKit 0.10.1, webrtc-sys 0.3.53 from the cargo registry).
- `cargo test`: 1061 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean.

### Manual Validation
Pending, in the end-of-milestone pass:
- The silent-call CPU table: RuSToat (release bundle via `open`) and the web client on this Mac in the same call, each with the `top` mean (13 × 5 s, first dropped) and the `ps` CPU-time delta over 60 s; both idle.
- The spike's waived checks, each with log evidence: five Join → Leave cycles in one process; join then log out; join then close the window (Keep running off, then on); join one voice channel then another; join in the web client with RuSToat's account, then join from RuSToat.
- Then the roadmap's milestone 32/33 entries record the outcome.

## Chunk 2: Who's in each voice channel

### Changes Made
- `src/api/events.rs`:
  - `"voice_states"` added to `READY_FIELDS` (the comment above it updated); `Ready.voice_states: Vec<ChannelVoiceState>`, parsed leniently (bad items count in `skipped`). The `events: Ready (…)` log line gains "N voice channels in use".
  - `ServerCreate` gains `voice_states` (lenient too).
  - New `ServerEvent` variants: `VoiceChannelJoin { channel, state }`, `VoiceChannelLeave { channel, user }`, `VoiceChannelMove { user, from, to, state }`, `UserVoiceStateUpdate { user, channel, data }` (Stoat's `id` is the user and `channel_id` the channel here), and `UserMoveVoiceChannel(Box<VoiceMove>)`. `VoiceMove { node, from, to, token }` has a hand-written `Debug` that prints the token as `…`. All five are forwarded to the UI.
  - Unit tests: join/leave/move in one `Bulk`, an update with only `is_publishing`, the move's token kept out of `Debug`, `Ready` with a bad voice state skipped, `ServerCreate`'s voice states (and their absence); the `events_url` test lists `voice_states`.
- `src/store.rs`:
  - `ChatStore.voice: HashMap<String, Vec<UserVoiceState>>` (channel → participants by `joined_at`; empty channels not kept). Replaced wholesale by `apply_ready`.
  - `set_server_voice(server, states)` (replaces that server's channels' lists, for `ServerCreate`), `apply_voice_join` (removes the user from any other channel first, inserts in `joined_at` order), `apply_voice_leave`, `apply_voice_move`, `apply_voice_update` (`UserVoiceState::apply_options`), `voice_participants(channel)`.
  - `VoiceParticipant { user_id, name, avatar_url, muted }` and `voice_rows(channel)`: nickname, else display name, else the ID's first 8 characters; member avatar, else user avatar, else the generated default; `muted = !is_publishing`. `unknown_voice_users()` lists participants not loaded.
  - `drop_channel_state` also drops the channel's list, so a deleted channel or a left server clears it.
- `src/ui/chat.rs`:
  - Handles the four voice events (re-rendering only if something changed for leave/update); `Ready`, `ServerCreate`, join and move call `fetch_voice_users`, which fetches each unknown participant once (`voice_user_fetches`).
  - `render_channel_row` puts `render_voice_participant` rows under a voice channel with anyone in it: 18 px avatar lined up under the channel name, name, and `MicOff` when muted.
- `src/assets.rs`: `MicOff` in `ExtraIcons` and the "icons load" test.
- `tests/store_voice.rs` (new): `Ready` fills in `joined_at` order; rows' names (nickname for the user, display name, ID start for an unknown) and mute; join order; a join elsewhere moves; leave (and a repeat); move; update merges only what's sent; a new `Ready` replaces; `ServerCreate` adds and replaces its channels only; channel deletion and leaving the server drop the list.
- `tests/store_servers.rs`: the `ServerCreate` pattern gains `..`.

### Divergences
- **Minor: where the parsing tests live.** The plan put them in `tests/live_events.rs`, but that file holds only opt-in checks against the live service. They're unit tests in `src/api/events.rs` (beside the other events' tests), and `tests/store_voice.rs` parses every frame it applies.
- **Minor: `ServerCreate`'s voice states go through `set_server_voice`,** not a new `add_server` parameter, so `add_server`'s other callers (create/join responses, tests) are unchanged.
- **Minor: unknown participants are named by their ID's first 8 characters** (the plan said "first characters"); their avatar is the generated default `user_avatar_url` already gives.
- **Note:** `log::trace!("events: frame {text}")` (existing) logs raw frames, so a `UserMoveVoiceChannel`'s token appears at trace level. Debug and above never show it. Left as is: trace is off in every documented run, and the token lasts 10 s.
- Look-ahead: no later chunk affected. Chunk 7 uses `ServerEvent::UserMoveVoiceChannel(Box<VoiceMove>)` rather than a struct variant.

### Verification Performed
- `cargo test`: 1077 passed, 0 failed, 5 ignored (72 suites; +16 tests). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift (`Diff in` count): `src/api/events.rs` 3 (was 3), `src/store.rs` 14 (was 15: the import block it touched is now formatted), `src/ui/chat.rs` 17 (was 17), `src/assets.rs` 0, `tests/store_voice.rs` 0 (new, `rustfmt`ed), `tests/store_servers.rs` 0.

### Manual Validation
Pending, in the end-of-milestone pass: someone in a voice channel on the web client is listed under it at launch; their leave removes the row; their mute shows the mic-off icon within about a second; an empty channel shows nothing extra; `Ready`'s `skipped` stays 0 on the live account; each voice event's JSON checked against a frame captured live (`RUST_LOG=stoat_gpui::api::events=trace`).

## Chunk 3: The call panel, and one call app-wide

### Changes Made
- `src/ui/voice_global.rs` (new): `ActiveVoice(Option<WeakEntity<ChatView>>)`, a GPUI `Global`, with `holder(cx)`, `set(view, cx)` and `clear(view, cx)` (clears only if `view` still holds it).
- `src/ui/mod.rs`: `pub mod voice_global;`.
- `src/ui/chat.rs`:
  - `join_voice`: if another view holds the call, upgrades it and calls its `leave_voice` (a different entity, so no re-entry; no toast), then leaves this view's own call and sets itself as the holder.
  - `leave_voice` clears the global; new `voice_ended` (used where a join fails and where the call drops by itself) sets `voice = None` and clears it.
  - The header button: "Join voice", or "Joining…" (disabled, spinner) while joining that channel; nothing once connected there.
  - `render_voice_panel`, between the channel list and the user panel while `voice` is `Some`: "Voice connected" in `theme.success`, or "Connecting…" muted; "channel / server" below it (opens the channel via `open_channel`, tooltip "Open channel"); a danger `PhoneOff` Leave button (tooltip "Leave call").
- `src/assets.rs`: `PhoneOff` in `ExtraIcons` and the icons test.

### Divergences
- None. (The global went in its own module, the plan's first option.)

### Verification Performed
- `cargo test`: 1077 passed, 0 failed, 5 ignored (no new tests; the global needs a GPUI app with two accounts, covered by the manual check). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt drift: `src/ui/chat.rs` 17 (was 17), `src/assets.rs` 0, `src/ui/voice_global.rs` 0 (new). `src/ui/mod.rs` edited by hand (one line), not formatted.

### Manual Validation
Pending, in the end-of-milestone pass: the panel shows "Connecting…" then "Voice connected" with channel and server; clicking the name from another server opens the channel; Leave works; with two accounts, joining from B ends A's call (the web client sees A leave, A's panel goes) and the log shows A's `voice: leaving` before B's `voice: joining`; both themes readable.

## Chunk 4: Mute and deafen

### Changes Made
- `src/voice.rs`:
  - `MuteState { muted, deafened }` with the web client's rules: `mic_on()` (`!muted && !deafened`), `toggle_mute()` (while deafened: undeafen, stay muted), `toggle_deafen()` (keeps `muted`, so undeafening restores the mic only if it was on). Unit tests for each rule.
  - `VoiceCall::join` takes the starting `MuteState`. The room is now an `Arc<Room>`; a `deafened: Arc<AtomicBool>` is set before subscribing.
  - The microphone publication is kept, and a `Controls` task (one per call, on the IO runtime, aborted on drop) applies `MuteState` changes in order from a channel: deafen → `set_enabled(!deafened)` on every remote audio publication; mic off → publication `mute()` then `PlatformAudio::stop_recording()`, mic on → `start_recording()` then `unmute()`. Each change logs `voice: deafened/undeafened …` / `voice: microphone on/off …`. Failures to stop/start recording are logged at warn.
  - `VoiceCall::set_mute(MuteState)` sends to it. The starting state goes through it after the microphone is published, so a muted call publishes, then mutes and stops recording (the plan's order).
  - `read_events` disables audio subscribed while the flag is set.
- `src/prefs.rs`: `voice_muted`, `voice_deafened` (default `false`, loaded from older files by `#[serde(default)]`); test for defaults, an old file, and a round trip; the full round-trip test sets them.
- `src/ui/chat.rs`:
  - `mute_state(cx)` reads the saved state from `AppPrefs`; `join_voice` passes it to `VoiceCall::join` (and logs it).
  - `change_mute(change)` saves the new state and applies it to a connected call.
  - The panel gains ghost Mute (`Mic` / `MicOff` in `theme.danger`; tooltip Mute / Unmute; disabled with "You can't speak here" without Speak) and Deafen (`Headphones` / `HeadphoneOff` in danger; tooltip Deafen / Undeafen), before Leave.
  - The user's own row in the channel they're connected to shows `HeadphoneOff` while deafened (instead of `MicOff`).
- `src/assets.rs`: `Mic`, `Headphones`, `HeadphoneOff` in `ExtraIcons` and the icons test.

### Divergences
- **Minor: the saved state is applied at join, not "once Connected".** It's passed into `VoiceCall::join`, so a call that starts muted never sends audio, and deafen is set before anything is subscribed.
- **Minor: mute/deafen run on a per-call control task fed by a channel,** not one `runtime().spawn` per change, so quick toggles can't apply out of order on the multi-threaded runtime.
- **Minor: a deafened user's own row shows only `HeadphoneOff`,** not `MicOff` as well (deafened implies muted).
- **Note:** a remote track subscribed in the instant a deafen is being applied could miss both the controls' sweep (not subscribed yet) and the subscribe-time check (flag not set yet). Toggling deafen again fixes it. Accepted as rare.
- Look-ahead: chunk 6's mute/deafen sounds go in `change_mute`; no other effect.

### Verification Performed
- `cargo test`: 1080 passed, 0 failed, 5 ignored (+3: two `MuteState` rule tests, one prefs test). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/voice.rs` and `src/prefs.rs` formatted (clean before, clean after); `src/ui/chat.rs` 17 (was 17); `src/assets.rs` 0.

### Manual Validation
Pending, in the end-of-milestone pass: mute → the web client shows RuSToat's user muted and hears nothing, unmute → hears again; deafen → RuSToat hears nothing and is muted, undeafen → hears again, mic back only if it was on; macOS mic indicator off while muted; five mute/unmute cycles (capture comes back each time, `PlatformAudio: started recording` in the log); quit and relaunch while muted → next call starts muted, and the web client shows muted rather than "no mic"; CPU while muted measured once.

## Chunk 5: Speaking indicators

### Changes Made
- `src/voice.rs`: `VoiceEvent::Speaking(Vec<String>)` (no longer `Copy`), sent from `RoomEvent::ActiveSpeakersChanged` with each speaker's identity, which Stoat sets to the user ID (`crates/core/database/src/voice/voice_client.rs:82`, `.with_identity(&user.id)`). Logged at debug as `voice: speaking [...]`.
- `src/ui/chat.rs`:
  - `ActiveCall.speaking: HashSet<String>`, cleared with the call (it lives in `ActiveCall`).
  - The call's task now loops over its events: `Speaking` calls `set_speaking`, anything else (or the stream ending) is the disconnect it handled before.
  - `set_speaking` drops the user while their mic is off, and redraws only if the set changed.
  - `render_voice_participant` wraps the avatar in a 2 px round border, `theme.success` while speaking and transparent otherwise (so rows don't shift; the indent drops 2 px to keep the avatar under the channel name). Only the channel the user is connected to passes speakers.

### Divergences
- **Minor: the user is always filtered out while their mic is off,** rather than only "if LiveKit reports it": cheaper than checking whether it does, and the result is the same.

### Verification Performed
- `cargo test`: 1080 passed, 0 failed, 5 ignored. `cargo clippy --all-targets -- -D warnings`: clean (a `loop`/`match` became `while let` for `clippy::while_let_loop`).
- rustfmt: `src/ui/chat.rs` 17 (was 17), `src/voice.rs` 0.

### Manual Validation
Pending, in the end-of-milestone pass: talking on the web client rings their avatar, talking in RuSToat rings the user's; the ring clears when talking stops; `voice: speaking [...]` lines appear at debug; CPU in a talking call recorded next to the silent figure.

## Chunk 6: Call sounds

### Changes Made
- `src/sound.rs`: `CallSound` (`Join` Pop, `Leave` Bottle, `OtherJoin` Tink, `OtherLeave` Purr, `Mute` Morse, `Unmute` Tink: all in `/System/Library/Sounds`), `should_play_call(sounds_on, sound, others_throttle, now)` (nothing with sounds off; others' join/leave share one `Throttle`, one a second; the user's own always play), and `play_call_sound(sound, sounds_on)` with its own app-wide throttle, separate from the message sound's. Tests: each sound maps to a system sound, the others-throttle, and sounds off.
- `src/prefs.rs`: `call_sounds: bool` (default `true`; an older file loads it as `true`); round-trip test covers it.
- `src/ui/settings.rs`: a "Call sounds" switch under "Notification sounds" in Notifications → Sounds.
- `src/voice.rs`: `VoiceEvent::ParticipantJoined` / `ParticipantLeft` from LiveKit's `ParticipantConnected` / `ParticipantDisconnected` (only this call's people).
- `src/ui/chat.rs`: `play_call_sound(sound, cx)` reads `AppPrefs.call_sounds`. Join on connecting; Leave when leaving a connected call (so taking over from another account plays it once, from the account that leaves) and when the call drops; OtherJoin/OtherLeave from the call's events; Mute/Unmute from `change_mute` (deafen's change decides when both change, so muting while deafened plays Unmute, the undeafen). The call task's loop now handles all four event kinds and stops if the view is gone.

### Divergences
- None.

### Verification Performed
- `cargo test`: 1083 passed, 0 failed, 5 ignored (+3 sound tests). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/sound.rs` formatted (was clean); `src/ui/chat.rs` 17 (was 17); `src/voice.rs`, `src/prefs.rs`, `src/ui/settings.rs` 0.

### Manual Validation
Pending, in the end-of-milestone pass: each sound at its moment and the switch silencing them all; quick join/leave on the web client plays at most one sound a second; joining a busy call doesn't play a burst (LiveKit shouldn't report people already there as `ParticipantConnected`); whether others hear the sounds through the user's mic (echo cancellation can't remove `NSSound`).

## Chunk 7: Moves and takeovers

### Changes Made
- `src/api/client.rs`: `RootConfig::voice_nodes()` (the enabled instance's nodes, else none), `voice_node()` now its first name, `RootConfig::voice_node_url(name)` and the free `voice_node_url(nodes, name)` (`None` for an unknown name or an empty `public_url`).
- `tests/api_instance.rs`: `voice_node_url("hel1")`, an unknown name, and voice off.
- `src/voice.rs`: `EndReason { Removed, Other }` from LiveKit's (protocol) `DisconnectReason` (`ParticipantRemoved` → `Removed`), carried by `VoiceEvent::Disconnected(EndReason)`; the room vanishing without a disconnect event is `Other`. Unit test of the mapping.
- `src/ui/chat.rs`:
  - `voice_nodes: Vec<VoiceNode>` replaces `voice_node: Option<String>`; `voice_node()` is the first's name.
  - `join_voice` now ends in a shared `start_call(channel, join, window, cx)`, which holds the call while `join` (a `CallJoin` IO task) connects and then follows its events.
  - `CallState::Removed { elsewhere }`: on `EndReason::Removed` the call's place is held for `MOVE_GRACE` (3 s), speakers cleared, the panel showing "Connecting…" and the header "Joining…". Then, unless something replaced it, the call ends with Leave and "You joined this call somewhere else." (`elsewhere`) or "You were removed from the call."; other ends keep "Disconnected from voice.".
  - A `VoiceChannelJoin` for the user while `Removed` sets `elsewhere`.
  - `follow_move` on `ServerEvent::UserMoveVoiceChannel`: if this view's call is in `from`, it joins `to` at once with the node's `public_url` and the event's token, replacing the old call (no leave sound or toast; the join sound plays on connecting). A node the instance doesn't list ends the call with "Couldn't join the call. Try again."; an expired token fails as any join does ("Couldn't connect to the voice server. Try again.").
  - `leave_voice` during `Removed` just logs.
- `docs/plans/voice-channels.md`: Assumption 11 and chunk 7 amended with option A.

### Divergences
- **Significant, decided by the user (option A): Stoat removes before it moves.** `member_edit.rs` ~285 calls `voice_client.remove_user(old_node, …)` and only then publishes `UserMoveVoiceChannel`, so LiveKit's `ParticipantRemoved` disconnect almost always arrives first. And `ParticipantRemoved` also means a takeover (`force_disconnect` in `voice_join.rs`) or a moderator's disconnect. Options put to the user: (A) hold a removed call 3 s for a move, then choose the toast by whether this account was seen joining a call; (B) end at once with a neutral toast and rejoin on a late move; (C) the plan as written. A was chosen, and the plan amended.
- **Minor: `EndReason::Removed`** rather than the plan's `RemovedElsewhere`, since removal doesn't say why.
- **Note:** a takeover whose new device takes longer than 3 s to join is reported with the neutral text.

### Verification Performed
- `cargo test`: 1085 passed, 0 failed, 5 ignored (+2: `voice_node_url`, the end-reason mapping). `cargo clippy --all-targets -- -D warnings`: clean.
- rustfmt: `src/ui/chat.rs` 17 (was 17), `src/api/client.rs` 3 (was 3), `src/voice.rs` formatted (clean), `tests/api_instance.rs` 0.

### Manual Validation
Pending, in the end-of-milestone pass: the web client (moderator) moves RuSToat's user → the panel shows the new channel, audio continues, the web client lists the user only there, no toast; joining in the web client with RuSToat's account → "You joined this call somewhere else."; a moderator disconnecting the user → after about 3 s, "You were removed from the call.".

## Summary

_Code completed: 2026-11-08. The manual pass is pending (the user batched every chunk's manual check to the end)._

### Chunks Completed
1. Settle the spike's conditions: no code; WebRTC's processing can't be turned off per part in LiveKit 0.10.1 on macOS, so the switch was dropped and the comparison is RuSToat vs the web client (measurements pending).
2. Who's in each voice channel: `Ready`/`ServerCreate` voice states, the five voice events, participant rows.
3. The call panel, and one call app-wide: "Voice connected" panel, `ActiveVoice` global.
4. Mute and deafen: the web client's rules, recording stopped while muted, saved in prefs.
5. Speaking indicators: green ring from LiveKit's active speakers.
6. Call sounds: macOS system sounds with a "Call sounds" switch.
7. Moves and takeovers: removed calls held 3 s for a move; takeover/removal toasts.

### Divergences
- Chunk 1 (significant, resolved by the plan's own risk): no per-part audio processing switch; plan amended.
- Chunk 7 (significant, user chose option A): removal arrives before the move; removed calls are held 3 s; plan amended.
- Minor: parsing tests in `src/api/events.rs` rather than `tests/live_events.rs`; `set_server_voice` instead of an `add_server` parameter; unknown participants named by 8 ID characters; mute state applied at join through an ordered control task; a deafened user's row shows only `HeadphoneOff`; the user is always left out of speakers while muted; `EndReason::Removed` naming.
- Notes: raw event frames at trace level include a move's token; a rare deafen/subscribe race; a takeover slower than 3 s gets the neutral text.

### Definition of Done
- [ ] Chunk 1's CPU comparison and roadmap entries: **pending the manual pass** (the finding that parts can't be turned off is recorded).
- [ ] The waived checks (five cycles, log out, close window, switch channels, takeover): **pending the manual pass**.
- [x] `Ready` asks for `voice_states`; participants listed live from `Ready`, `ServerCreate` and the four events, mic-off for `!is_publishing`; tested in `tests/store_voice.rs` and `src/api/events.rs` (not `tests/live_events.rs`, see chunk 2). Live behaviour pending the manual pass.
- [x] Panel with Mute, Deafen, Leave and the channel name; header only "Join voice" / "Joining…". (Looks pending the manual pass.)
- [x] Joining from one account leaves another's call (code; manual check pending).
- [x] Mute/deafen rules unit-tested and saved in prefs; seen-as-muted by the web client pending the manual pass.
- [x] Green speaking ring (code; manual check pending).
- [x] Call sounds and the Settings switch (code; by-ear check pending).
- [x] Moves followed, takeovers explained (code, as amended; manual check pending).
- [x] `cargo test && cargo clippy --all-targets -- -D warnings` pass (1085 passed, 0 failed, 5 ignored); no touched file's rustfmt drift grew (`src/store.rs` fell from 15 to 14).

### Remaining Manual Steps
The batched manual pass: every chunk's "Manual Validation" above, plus the plan's Manual Tests (a 10-minute call using mute, deafen and speaking, with CPU noted; overnight in a call; both themes). Then record the CPU table and the waived checks under chunk 1, and update the roadmap's milestone 32/33 entries.
