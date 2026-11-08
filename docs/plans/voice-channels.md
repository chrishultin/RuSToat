# Plan: Voice channels (RuSToat milestone 33)

## Summary
Milestone 33 of the [roadmap](roadmap.md) turns the voice spike (milestone 32, [plan](voice-spike.md), [results](voice-spike-implementation.md#results-and-decision): "go, with changes") into voice channels you can live with:
- **First, the spike's conditions:** settle a silent call's CPU target by measuring what WebRTC's audio processing costs, with the web client on the same Mac as the reference. Then run the checks the spike skipped.
- **Then the feature:**
  - who's in each voice channel, listed under it in the channel list;
  - a "Voice connected" panel with Mute, Deafen and Leave;
  - one call at a time across all accounts;
  - speaking indicators;
  - call sounds;
  - following a moderator's move.

## Approach
**What Stoat sends** (`stoatchat/stoatchat` v0.16.2, read from `/tmp/stoatchat-0.16.2` on 2026-11-08: `crates/core/database/src/events/client.rs`, `crates/daemons/voice-ingress/src/api.rs`, `crates/core/database/src/voice/mod.rs`, `crates/delta/src/routes/servers/member_edit.rs`):
- **Who's in a channel:**
  - `Ready` has `voice_states: [ChannelVoiceState { id, participants: [UserVoiceState] }]`, but only when asked for: RuSToat's `READY_FIELDS` (`src/api/events.rs` ~37) leaves it out today.
  - `ServerCreate` carries `voice_states` too.
  - `UserVoiceState { id, joined_at, is_receiving, is_publishing, screensharing, camera }` and `ChannelVoiceState` are in `revolt-models` 0.16.2.
- **Live changes:**
  - `VoiceChannelJoin { id, state }` and `VoiceChannelLeave { id, user }`.
  - `VoiceChannelMove { user, from, to, state }`: one event instead of a leave and a join when a moderator moves someone.
  - `UserVoiceStateUpdate { id, channel_id, data: PartialUserVoiceState }`.
  - `UserMoveVoiceChannel { node, from, to, token }`: sent only to the moved user's own sessions, with a fresh token for the new room.
- **Where those come from:** Stoat makes all of them from LiveKit webhooks.
  - `participant_joined` / `participant_left` give Join / Leave / Move.
  - `track_published` / `track_unpublished` / `track_muted` / `track_unmuted` give `UserVoiceStateUpdate`. Microphone → `is_publishing`, camera → `camera`, screen share → `screensharing`.
- **Mute vs deafen:**
  - Muting your microphone track in LiveKit becomes `is_publishing: false` for everyone.
  - **Nothing in 0.16.2 ever sets `is_receiving`** (always `true`), so deafen is local only, as in the web client.
- **Moderators:**
  - Server-wide mute and deafen (`Member.can_publish` / `can_receive`) already arrive as missing Speak / Listen through `revolt-permissions` (`src/permissions.rs` ~333–337).
  - A moderator can move a member to another voice channel (`member_edit` with `voice_channel`), which produces `UserMoveVoiceChannel` for them and `VoiceChannelMove` for everyone.

**What the web client does** (`stoatchat/for-web` `stoat-for-web-v0.16.3`, from `/tmp/for-web-0.16.3`: `packages/client/components/rtc/state.tsx`, `rtc/components/RoomAudioManager.tsx`, `ui/components/features/voice/VoiceChannelPreview.tsx`):
- **The sidebar:**
  - Lists participants under each voice channel: avatar, name, and muted / deafened / camera / screen-share icons.
  - For channels you aren't in, it uses Stoat's voice states, with no speaking indicator.
  - For the one you're in, it uses LiveKit, with speaking rings (`useIsSpeaking`) and mute from the track.
- **Mute:** `setMicrophoneEnabled(!enabled)`, which mutes the published track.
- **Deafen:**
  - Local only: mutes every `<audio>` element and turns the microphone off; undeafening restores the mic only if it was on.
  - Muting while deafened undeafens.
  - Both are saved in the client's voice settings (`micOn`, `deafen`) and applied when the next call connects.
- **Sounds:** join / leave (yours and others'), mute / unmute, deafen / undeafen. They're Stoat's own audio files, which `src/sound.rs` already notes aren't licensed for other clients.

**What exists in RuSToat** (after milestone 32, line numbers as of 2026-11-08):
- `src/voice.rs`:
  - `VoiceCall::join(channel, url, token, publish_mic, subscribe)`: `PlatformAudio`, `Room::connect` with `auto_subscribe: false`, publishing the microphone, subscribing to audio only.
  - `read_events` logs room events and sends `VoiceEvent::Disconnected`.
  - `Drop` closes the room on `runtime()`.
- `src/ui/chat.rs`:
  - `ChatView.voice: Option<ActiveCall { channel, state: Joining | Connected(VoiceCall), _task }>`, plus `join_voice` / `leave_voice` / `voice_state`.
  - A Join / Joining… / Leave button in a voice channel's header (`render_main`).
  - The accent-coloured row icon (`render_channel_row` ~8400).
  - `voice_node: Option<String>` from the root config.
- **The channel list and user panel:** `render_channel_list` (~8223) builds sections of `ChannelEntry` rows; `render_user_panel` (~8465) is at the bottom of the sidebar.
- **Navigation and users:** `ChatView::open_channel(channel, cx)` (~6870) navigates; `fetch_user` (~1425) loads an unknown user.
- `src/api/client.rs`: `RootConfig::voice_node()` keeps only the first node's name, but `VoiceNode` already has `public_url`.
- `src/sound.rs` plays macOS system sounds through `NSSound` (`play(name)`), with a throttle. `Prefs.notification_sounds` (`src/prefs.rs` ~182) is its switch in Settings (`src/ui/settings.rs` ~397).
- `src/ui/app_root.rs` `AppRoot` holds every account's `Entity<ChatView>`, and `src/ui/app_prefs.rs` `AppPrefs` is the global for app-wide preferences.
- **Icons:** Lucide's `mic`, `mic-off`, `headphones`, `headphone-off` and `phone-off` ship in `gpui-kit-assets` 0.9.0. They're added to `ExtraIcons` in `src/assets.rs` when used.

**LiveKit 0.10.1** (from the cargo registry):
- `LocalTrackPublication::mute()` / `unmute()`.
- `RemoteTrackPublication::set_enabled(bool)`: disables a subscribed track locally, which silences it.
- `PlatformAudio::stop_recording()` / `start_recording()`.
- `PlatformAudio::set_echo_cancellation` / `set_noise_suppression` / `set_auto_gain_control(enable, prefer_hardware)`.
- `RoomEvent::ActiveSpeakersChanged { speakers: Vec<Participant> }` includes the local participant.

**Decisions** (user, 2026-11-08):
1. **CPU target:** keep WebRTC's audio processing (echo cancellation, noise suppression, gain control) on. The target becomes **no worse than the web client on the same Mac** in the same silent call, replacing the spike's 5%.
2. **A "Voice connected" panel** above the user panel at the bottom of the channel sidebar, like the web client's.
   - It shows the channel's name with Mute, Deafen and Leave, and stays visible anywhere in that account.
   - Clicking the channel name opens the channel.
   - The voice channel's header keeps only "Join voice".
3. **One call app-wide:** joining from account B leaves account A's call.
4. **Sounds in this milestone:** joining and leaving (yours and others' in your call), mute and unmute, deafen and undeafen.
5. **Mute and deafen are remembered** across calls and restarts, app-wide, as the web client does.

**Rejected alternatives.**
- **Turning off audio processing to hit 5%** (Decision 1). Echo on speakers is worse than a few percent of a core.
- **LiveKit as the source of the participant list for the call you're in**, as the web client does. Stoat's voice states already cover every channel, including yours. Using one source everywhere means one code path and one look. LiveKit is used only for what Stoat can't know: who's speaking, and your own deafen.
- **Stoat's sound files.** They're not licensed for other clients (`src/sound.rs`), so macOS system sounds are used, as for message notices.
- **Unpublishing the microphone track to mute.** Muting the publication is what the web client does, and it keeps unmuting instant. Stopping WebRTC's recording while muted (Assumption 8) frees the microphone without republishing.

## Assumptions
1. **The participant list shows Stoat's voice states for every voice channel**, ordered by `joined_at`.
   - Each participant shows avatar, name (as the member list shows it, nickname first), a mic-off icon when `!is_publishing`, and, for yourself only, a headphone-off icon when deafened.
   - Camera and screen share aren't shown until milestones 36–37.
   - No "n / max" count.
2. **Unknown participants are fetched one by one** with the existing `fetch_user` (each user ID once). Until then they show a placeholder avatar and their ID's first characters, as other unknown users do. `fetch_members` isn't used: it fetches a whole server.
3. **Speaking indicators exist only for the call you're in**, from LiveKit's `ActiveSpeakersChanged`, as in the web client. They show as a ring around the participant's avatar in `theme.success` (green), including your own.
4. **The call panel:**
   - First line: "Voice connected", or "Connecting…" while joining. Second line: the channel's name and its server's, which opens the channel when clicked.
   - Buttons: Mute (`Mic` / `MicOff`), Deafen (`Headphones` / `HeadphoneOff`), Leave (`PhoneOff`, danger), each with a tooltip.
   - Mute is disabled without Speak.
   - It sits in the account whose call it is. Other accounts show nothing.
5. **The header button** reads "Join voice" in a voice channel you aren't in, and "Joining…" while joining it. There's no button in the channel you're connected to; the panel has Leave.
6. **One call app-wide** (Decision 3): an app global (`ActiveVoice`, a GPUI `Global`) holds a weak handle to the `ChatView` that's in a call or joining one. Joining in another view first makes that one leave, without a toast, and plays the leave sound once.
7. **Mute and deafen behave as in the web client:**
   - **Muting** mutes the microphone publication. Stoat then sends `is_publishing: false` to everyone.
   - **Deafening** silences everyone (`set_enabled(false)` on each remote audio publication, and on any published while deafened) and also mutes the mic.
   - **Undeafening** restores the mic only if it wasn't muted before.
   - **Muting while deafened** undeafens with the mic staying off.
   - **Saved state:** `Prefs.voice_muted` and `Prefs.voice_deafened`, applied when the next call connects. Without Speak, `voice_muted` is left as it is and the mic simply isn't published.
8. **While muted (or deafened), WebRTC stops recording** (`PlatformAudio::stop_recording`, then `start_recording` on unmute). The microphone is freed, so the macOS mic indicator goes off and the capture side's processing stops. If that fails or misbehaves (rust-sdks #1638 is about devices, but the same audio-device-module state is involved), fall back to only muting the publication and record it.
9. **Sounds** (Decision 4) are macOS system sounds, behind a new **"Call sounds"** switch in Settings beside "Notification sounds" (`Prefs.call_sounds`, default on). The mapping can be tuned by ear in chunk 6:

   | Event | Sound |
   |---|---|
   | You join | `Pop` |
   | You leave or are disconnected | `Bottle` |
   | Someone joins your call | `Tink` |
   | Someone leaves your call | `Purr` |
   | Mute, deafen | `Morse` |
   | Unmute, undeafen | `Tink` |

   The existing throttle doesn't apply. Others joining and leaving are throttled together to one sound a second.
10. **A moderator's move** (`UserMoveVoiceChannel`) is followed:
    - If this view is in `from`, it leaves and connects to `to` with the given token.
    - The URL is the node's `public_url` from the root config (so `ChatView` keeps the node list, not just the first name).
    - No toast; the panel's channel name changes.
    - If the token has expired (10 s), it toasts the usual join error.
11. **Being removed because the same account joined elsewhere** (LiveKit's `DisconnectReason::ParticipantRemoved`, seen in the spike) toasts "You joined this call somewhere else." instead of "Disconnected from voice." Other reasons keep the old text.

    **Amended during implementation (2026-11-08, user's option A):** Stoat removes the user from the old room *before* sending `UserMoveVoiceChannel` (`member_edit.rs` ~285), and uses `ParticipantRemoved` for takeovers, moves and a moderator's disconnect alike. So a removed call is held for 3 s ("Connecting…" in the panel). A move from that channel in that time is followed (join sound only). Otherwise the call ends with "You joined this call somewhere else." if Stoat reported this account joining a call meanwhile, else "You were removed from the call."
12. **Your own entry in a list comes from Stoat**, like everyone else's. It appears about a second after joining, when Stoat's webhook arrives. The panel shows "Connecting…" / "Voice connected" from the call itself, so nothing waits on it.
13. **CPU is measured on the release bundle launched with `open`**, as in the spike (`top -l 13 -s 5 … -stats cpu`, first sample dropped, plus the `ps` CPU-time delta over 60 s).
    - The web client is measured in the same silent call on the same Mac: its browser's processes (renderer, GPU and audio-service helpers), summed. RuSToat is in the call as the other account.
    - If the browser is Chrome or a Chromium browser, its processes are found with `pgrep -f`. In Safari they are `com.apple.WebKit.WebContent` / `GPU` / `Networking`.
14. **Group and DM calls stay in milestone 35;** device choice, per-person volume and processing settings in milestone 34.

## Open Questions
- **What if RuSToat costs clearly more CPU than the web client** after chunk 1? Deferred, not a blocker. The plan's answer is to stop at the end of chunk 1 and decide (more investigation, or accept). "Clearly more" means over 1.5× the web client's figure, in the same call.

## Plan

### Chunk 1: Settle the spike's conditions
**Goal:** The CPU target is settled against the web client with numbers, and the checks the spike skipped are run, so the rest of the milestone builds on a known base.
**Amended during implementation (2026-11-08):** the `RUSTOAT_VOICE_PROCESSING` switch was dropped. In LiveKit 0.10.1 the `set_*` calls only switch *hardware* effects (none on macOS), and `webrtc-sys` 0.3.53 hard-codes WebRTC's software echo cancellation, gain control and noise suppression on for the device track (`create_device_audio_track`). Nothing in the SDK can turn them off per part, so the comparison is RuSToat (all on) against the web client. See the implementation record.

**Files likely touched:**
- `docs/plans/voice-channels-implementation.md` (new): the finding above, the measurements and check results.
- `docs/plans/roadmap.md`: milestone 33's first item, and milestone 32's line about the target, record the outcome.

**Depends on:** None.
**Risks:**
- **The `set_*` calls may not take effect on a running audio module.** Check the log: LiveKit prints "Audio processing configured: AEC=…". If they don't, apply them before `Room::connect`, or note that they can't be changed per call.
- **Measuring the web client fairly:** the browser's process set varies, so sum every process of that browser, and take an idle reading of it outside the call too, for the delta.
- **The waived checks may find a bug** (a hang on the third cycle, say). Fix it here if small. If it's #1468, apply the spike's Assumption 11 fallback (one `PlatformAudio` for the process) and record it.

**How to verify:**
- `cargo test && cargo clippy --all-targets -- -D warnings`.
- The implementation record has a table of CPU figures for the same silent call, each with the `top` mean and the `ps` delta:
  - RuSToat (processing on, the only option);
  - the web client on this Mac;
  - both idle.

**Manual check:**
- The user and the implementer run the silent-call measurements together.
- Then the spike's waived checks, each recorded with log evidence:
  - five Join → Leave cycles in a row in one process;
  - joining, then logging out;
  - joining, then closing the window, with Keep running off and then on;
  - joining one voice channel, then another;
  - joining in the web client with RuSToat's account, then joining from RuSToat: the web call ends.

**Shippable when:** the record compares RuSToat with the web client on the same Mac, the target is set by Decision 1 (or the Open Question has been decided), and every waived check has a result.

### Chunk 2: Who's in each voice channel
**Goal:** Every voice channel in the channel list shows who's in it, live, from Stoat's voice states.
**Files likely touched:**
- `src/api/events.rs`:
  - `"voice_states"` added to `READY_FIELDS`, and `Ready.voice_states: Vec<ChannelVoiceState>` (items that don't parse count in `skipped`, as the other lists do).
  - `ServerCreate` gains `voice_states`.
  - New `ServerEvent` variants: `VoiceChannelJoin { channel, state }`, `VoiceChannelLeave { channel, user }`, `VoiceChannelMove { user, from, to, state }`, `UserVoiceStateUpdate { user, channel, data }` and `UserMoveVoiceChannel { node, from, to, token }`. The last is parsed here and used in chunk 7. The token is never logged: `Debug` is hand-written or the token redacted, since events are logged at debug.
- `src/store.rs`:
  - `voice: HashMap<String, Vec<UserVoiceState>>` (channel → participants, ordered by `joined_at`).
  - Set from `Ready` and `ServerCreate`, and changed by the four live events (a join removes the user from any other channel first, defensively). Cleared for a deleted channel or server.
  - `voice_participants(channel) -> &[UserVoiceState]`.
- `src/ui/chat.rs`:
  - Handles the events (re-rendering the list).
  - `render_channel_row` gets participant rows under a voice channel: an indented avatar of 18 px, name and mic-off icon.
  - Unknown users are fetched with `fetch_user`, once per ID.
- `src/assets.rs`: `MicOff` added to `ExtraIcons` and to the "every icon loads" test.
- `tests/fixtures/`: voice event frames as Stoat sends them, if the tests' style uses fixtures.

**Depends on:** None (independent of chunk 1).
**Risks:**
- **`Ready` gets bigger** with `voice_states`. It's small (only channels with someone in them), but check that `Ready` still parses on the live account (the log's `skipped` count stays 0).
- **The JSON shapes:** the events' field names are from the server source (`id` is the channel in Join, the user in `UserVoiceStateUpdate`). Test each against a frame captured from the live server during the trial, not just the source.
- **A participant whose channel isn't known** (a channel this account can't see): keep the state but don't draw it.
- **Missing a leave while disconnected:** the next `Ready` replaces everything, so stale entries clear on reconnect.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with:
- `tests/live_events.rs`: each new event parses from a JSON frame, including `PartialUserVoiceState` with only `is_publishing`.
- `tests/store_voice.rs` (new):
  - `Ready` voice states fill channels.
  - Join adds and keeps `joined_at` order. A join for a user already elsewhere moves them.
  - Leave removes; Move moves.
  - Update merges only the sent fields.
  - `ServerCreate` adds its channels' states, and a channel's deletion drops its list.

**Manual check:**
- With someone in a voice channel on the web client, RuSToat lists them under it at launch.
- They leave: the row goes. They mute: the mic-off icon appears within about a second.
- A channel with nobody in it shows nothing extra.

**Shippable when:** voice channels list their participants live, and the tests pass.

### Chunk 3: The call panel, and one call app-wide
**Goal:** A call is controlled from a "Voice connected" panel that stays in view, and joining anywhere ends any other call in the app.
**Files likely touched:**
- `src/ui/chat.rs`:
  - `render_voice_panel`, above `render_user_panel` in the sidebar, shown while `voice` is `Some` (Assumption 4). Leave (`PhoneOff`), and the channel's name opening it via `open_channel`.
  - The header button loses "Leave voice" (Assumption 5).
- A new small module for the global, `src/ui/voice_global.rs` (or in `src/ui/chat.rs` if it stays a few lines): `ActiveVoice(Option<WeakEntity<ChatView>>)`.
  - `join_voice` makes the previous holder leave (Assumption 6), then sets itself.
  - `leave_voice` and the `Disconnected` path clear it if it's this view.
- `src/assets.rs`: `PhoneOff`.

**Depends on:** None (it reuses milestone 32's `ActiveCall`). Chunk 2's list isn't needed.
**Risks:**
- **Updating another `ChatView` from inside `join_voice`** (it's a different entity, so no double borrow). But it must not happen while that view is being updated. Upgrade the weak handle and call `update` from the app context, and don't hold any borrow of `self` across it.
- **A dropped view** (logged out) leaves a dead weak handle. That's fine: upgrading fails, and the global is overwritten on the next join.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`. Log: joining in account B logs account A's "voice: leaving" before B's "voice: joining".
**Manual check:**
- The panel shows "Connecting…", then "Voice connected", with the channel and server name. Clicking the name from another server opens the channel. Leave works.
- With two accounts, joining from B ends A's call: the web client sees A leave, and A's panel disappears.
- Light and dark themes are readable.

**Shippable when:** calls are left from the panel, and only one call runs app-wide.

### Chunk 4: Mute and deafen
**Goal:** Mute and Deafen in the panel work as in the web client, are seen by others (mute), and are remembered.
**Files likely touched:**
- `src/voice.rs`:
  - `VoiceCall` keeps the microphone publication.
  - `set_muted(bool)`: publication `mute` / `unmute`, plus `stop_recording` / `start_recording` (Assumption 8).
  - `set_deafened(bool)`: `set_enabled` on remote audio publications. `read_events` keeps a shared "deafened" flag so publications subscribed while deafened start disabled.
  - Both run on `runtime()`.
- `src/prefs.rs`: `voice_muted: bool`, `voice_deafened: bool` (default `false`, `#[serde(default)]` so old files load).
- `src/ui/chat.rs`:
  - The panel's Mute and Deafen buttons, with the web client's rules (Assumption 7).
  - Applied once `Connected` from `AppPrefs`.
  - Your own participant row shows headphone-off while deafened.
- `src/assets.rs`: `Mic`, `Headphones`, `HeadphoneOff`.

**Depends on:** Chunk 3 (the panel). Chunk 2 for your own row's deafen icon; without it that part waits.
**Risks:**
- **`stop_recording` / `start_recording` mid-call** may not resume capture, or may glitch (an audio-device-module state issue in the #1468 / #1638 family). Test five mute/unmute cycles. If capture doesn't come back, fall back to muting the publication only (Assumption 8).
- **Publishing with recording stopped:** if a call starts muted, the publication must still be made so unmuting is instant. Publish, then mute and stop recording. Check that the web client shows the user as muted (`is_publishing: false`), not as "no mic".
- **`set_enabled(false)` on a remote audio track** must silence playout. If it doesn't, fall back to unsubscribing (`set_subscribed(false)`) while deafened.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with:
- `src/prefs.rs` tests: the new fields default to `false` and load from an old prefs file without them.
- `src/voice.rs` unit tests for the pure mute/deafen rules, as a small state function `(muted, deafened, action) -> (muted, deafened)`: deafen mutes; undeafen restores; mute while deafened undeafens and stays muted.

**Manual check:**
- Mute: the web client shows RuSToat's user muted, and hears nothing. Unmute: it hears again.
- Deafen: RuSToat hears nothing and is muted. Undeafen: it hears again, and the mic comes back only if it was on.
- The macOS mic indicator goes off while muted.
- Quit and relaunch while muted: the next call starts muted.
- CPU while muted is measured once and recorded (it should fall, since capture stops).

**Shippable when:** mute and deafen work, are seen by others (mute) and are remembered.

### Chunk 5: Speaking indicators
**Goal:** In the call you're in, whoever is speaking (you included) has a ring around their avatar in the channel list.
**Files likely touched:**
- `src/voice.rs`: `VoiceEvent::Speaking(Vec<String>)` (user IDs, from `ParticipantIdentity`), sent on `ActiveSpeakersChanged`.
- `src/ui/chat.rs`:
  - The listening task keeps `ActiveCall.speaking: HashSet<String>` and re-renders.
  - Participant rows in the connected channel draw the ring (Assumption 3).
  - Cleared when the call ends.

**Depends on:** Chunk 2 (the rows).
**Risks:**
- **Event rate:** `ActiveSpeakersChanged` can fire several times a second. Only re-render when the set changes, and check that CPU in a talking call doesn't jump against chunk 1's figures.
- **Your own speaking while muted:** LiveKit shouldn't report it (the track is muted). If it does, filter yourself out while muted.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`. With `RUST_LOG=stoat_gpui=debug`, `voice: speaking […]` lines appear as people talk (debug level, IDs only).
**Manual check:**
- Talking on the web client rings their avatar; talking in RuSToat rings yours. The ring clears when talking stops.
- CPU in a talking call is recorded next to the silent figure.

**Shippable when:** speaking shows in the connected channel's list.

### Chunk 6: Call sounds
**Goal:** Joining, leaving, others joining and leaving your call, mute and deafen are heard, with a switch to turn them off.
**Files likely touched:**
- `src/sound.rs`: `CallSound` (`Join`, `Leave`, `OtherJoin`, `OtherLeave`, `Mute`, `Unmute`), its system sound name (Assumption 9), `play_call_sound(sound)`, and a throttle for others' join/leave.
- `src/prefs.rs`: `call_sounds: bool` (default `true`).
- `src/ui/settings.rs`: a "Call sounds" switch beside "Notification sounds".
- `src/voice.rs`: `VoiceEvent::ParticipantJoined` / `ParticipantLeft`, from LiveKit's events, so only your call's people count.
- `src/ui/chat.rs`: plays the sounds at each point, on the main thread (`NSSound` is AppKit's).

**Depends on:** Chunks 3 and 4 (the leave, mute and deafen paths).
**Risks:**
- **The microphone picks up the sound on speakers**, and echo cancellation won't remove it: `NSSound` plays outside WebRTC, so it isn't in the echo reference. Others may hear a faint click. Note it if they do; the web client has the same issue.
- **Joining a busy call** gets a burst of "others" events for people already there. LiveKit reports existing participants at connect, not as `ParticipantConnected`, so this shouldn't happen. Check it.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with `src/sound.rs` unit tests:
- each `CallSound` maps to a system sound name;
- the others-throttle allows one per second;
- `call_sounds: false` plays nothing (a `should_play_call` helper, like `should_play`).

**Manual check:**
- Each sound plays at its moment, and the switch silences them all.
- Five people joining at once (or quick join/leave on the web client) plays at most one sound a second.

**Shippable when:** call sounds play, and can be turned off.

### Chunk 7: Moves and takeovers
**Goal:** A moderator moving you takes your call to the new channel, and losing the call to another of your clients says so.
**Files likely touched:**
- `src/ui/chat.rs`:
  - Keeps the root config's voice nodes (`Vec<VoiceNode>`), not just the first name.
  - On `ServerEvent::UserMoveVoiceChannel` where `from` is this view's call: drop the current call and `VoiceCall::join` to `to` with the node's `public_url` and the event's token (Assumption 10).
  - On `VoiceEvent::Disconnected` with `ParticipantRemoved`, holds the call for 3 s for a move, then toasts the takeover or neutral text (Assumption 11, as amended).
- `src/voice.rs`: `VoiceEvent::Disconnected` carries the reason, as an enum of what the UI cares about: `Removed`, `Other`.
- `src/api/client.rs`: `RootConfig::voice_node_url(name) -> Option<&str>`.

**Depends on:** Chunk 2 (the event parsing), chunk 3 (the panel and global).
**Risks:**
- **Testing a move needs a moderator:** the user's account must own (or moderate) a server, with a second account to move. The user's test server works if both accounts are in it.
- **The token's 10 seconds:** the event goes through the UI thread first, so start the join at once in the handler, as `join_voice` does.
- **`ParticipantRemoved` also covers a moderator kicking you from the call.** The text "You joined this call somewhere else." would be wrong then. If Stoat's moves or kicks also produce `ParticipantRemoved`, show the plain "Disconnected from voice." unless this account's own `force_disconnect` explains it. That can't be known locally, so use the neutral "You were removed from the call." if the trial shows ambiguity.

**How to verify:** `cargo test && cargo clippy --all-targets -- -D warnings`, with `tests/api_instance.rs` testing `voice_node_url("hel1")` and an unknown name.
**Manual check:**
- The web client (moderator) moves RuSToat's user to another voice channel. RuSToat's panel shows the new channel, audio continues, and the web client lists the user in the new channel only.
- Joining in the web client with RuSToat's account: RuSToat toasts the takeover text.
- A moderator disconnecting RuSToat's user from the call: after about 3 s, "You were removed from the call."

**Shippable when:** moves are followed and takeovers are explained.

## Test Plan

**Full-suite command:** `cargo test && cargo clippy --all-targets -- -D warnings`.
- Don't run `cargo fmt` across the crate. Format only files you touch with `rustfmt --edition 2024 <file>`, and never on `mod.rs`, `lib.rs` or `main.rs`.
- For files that already have drift, hand-format new code and check the `Diff in` count from `rustfmt --edition 2024 --check <file>` hasn't grown. Starting counts on 2026-11-08:
  - with drift: `src/api/events.rs` 3, `src/api/client.rs` 3, `src/store.rs` 15, `src/ui/chat.rs` 17;
  - clean: `src/prefs.rs`, `src/ui/settings.rs`, `src/sound.rs`, `src/assets.rs`, `src/ui/app_root.rs`, `src/voice.rs`, `tests/live_events.rs`, `tests/store_live.rs`;
  - new files must be clean.
- **Test the microphone through the bundle launched with `open`** (`scripts/bundle.sh release`, then `open -n --env RUST_LOG=stoat_gpui=debug,livekit=info --stdout <log> --stderr <log> target/release/RuSToat.app`). A `cargo run` started by a tool inherits its host's microphone permission (see the spike's record).

### Unit Tests
- `src/voice.rs`:
  - the mute/deafen rules function;
  - the disconnect reason mapping.
- `src/sound.rs`: the call sound mapping, the others-throttle, and `should_play_call`.
- `src/prefs.rs`: the new fields' defaults, and loading an old file.

### Integration Tests
- `tests/live_events.rs`: `Ready` with `voice_states`, `ServerCreate` with `voice_states`, and each voice event, parsed from frames.
- `tests/store_voice.rs` (new): voice states through `Ready`, join / leave / move / update, and channel and server deletion.
- `tests/api_instance.rs`: `voice_node_url`.
- LiveKit itself isn't tested automatically (it needs a server). The manual checks cover it.

### Manual Tests
You need RuSToat with two accounts, a second browser session on stoat.chat (ideally on this Mac, for chunk 1's reference), and a test server with two voice channels where one account can move members.
1. Every chunk's manual check.
2. A 10-minute call with mute, deafen and speaking used throughout: no drop, the list and panel stay right, and CPU in the call is noted against chunk 1's figures.
3. Leave the app running overnight in a call, then check the call's state and the log in the morning. This tests reconnects across sleep: expected `Reconnecting` / `Reconnected`, or a disconnect toast and a clean state.
4. Both themes: the participant rows, the ring, the panel and its icons are readable.

## Definition of Done
- [ ] Chunk 1's record compares a silent call's CPU in RuSToat (processing on; LiveKit 0.10.1 can't turn its parts off) with the web client on the same Mac, and the target follows Decision 1 (or the Open Question was decided). The roadmap's milestone 32/33 entries say so.
- [ ] Five join/leave cycles in a row, leaving by logging out, closing the window (Keep running off), switching channels and taking a call over from the web client all have recorded results.
- [ ] `Ready` asks for `voice_states`. Voice channels list their participants, live, from `Ready`, `ServerCreate` and the four voice events, with mic-off for `!is_publishing`. Tested in `tests/live_events.rs` and `tests/store_voice.rs`.
- [ ] A "Voice connected" panel above the user panel has Mute, Deafen, Leave and the channel's name (which opens it). The header has only "Join voice" / "Joining…".
- [ ] Joining from one account leaves any call in another.
- [ ] Mute is seen by the web client as muted. Deafen silences everyone and mutes. Both follow the web client's rules (unit-tested) and survive a restart (`Prefs.voice_muted`, `voice_deafened`).
- [ ] Whoever is speaking in your call has a green ring around their avatar.
- [ ] Call sounds play for join, leave, others joining and leaving your call, mute, unmute, deafen and undeafen, and the "Call sounds" switch in Settings turns them off.
- [ ] A moderator's move takes the call to the new channel; losing the call to another client says so.
- [ ] `cargo test && cargo clippy --all-targets -- -D warnings` pass, and no touched file's rustfmt drift grows.
