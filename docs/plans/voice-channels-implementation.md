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
