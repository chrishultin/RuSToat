//! Voice calls, through LiveKit's Rust SDK.
//!
//! Stoat's voice runs on LiveKit: Stoat hands out a room token, and the client
//! talks to LiveKit directly. Audio goes through WebRTC's own platform audio
//! module ([`PlatformAudio`]): the default microphone and speakers, with
//! WebRTC's echo cancellation, noise suppression and gain control.

use livekit::PlatformAudio;

/// Start WebRTC's platform audio and name the devices it sees: the
/// microphones, then the speakers. A check that the SDK links and starts;
/// nothing is recorded or played.
pub fn webrtc_ready() -> Result<(Vec<String>, Vec<String>), livekit::AudioError> {
    let audio = PlatformAudio::new()?;
    let inputs = audio.recording_devices().map(|d| d.name).collect();
    let outputs = audio.playout_devices().map(|d| d.name).collect();
    Ok((inputs, outputs))
}
