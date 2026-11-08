//! Voice calls, through LiveKit's Rust SDK.
//!
//! Stoat's voice runs on LiveKit: Stoat hands out a room token, and the client
//! talks to LiveKit directly. Audio goes through WebRTC's own platform audio
//! module ([`PlatformAudio`]): the default microphone and speakers, with
//! WebRTC's echo cancellation, noise suppression and gain control.

use livekit::PlatformAudio;
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::profile_edit::error_text;

/// Start WebRTC's platform audio and name the devices it sees: the
/// microphones, then the speakers. A check that the SDK links and starts;
/// nothing is recorded or played.
pub fn webrtc_ready() -> Result<(Vec<String>, Vec<String>), livekit::AudioError> {
    let audio = PlatformAudio::new()?;
    let inputs = audio.recording_devices().map(|d| d.name).collect();
    let outputs = audio.playout_devices().map(|d| d.name).collect();
    Ok((inputs, outputs))
}

/// What a toast says when Stoat won't let the user join a call.
pub fn join_error_text(err: &ApiError) -> String {
    match err.error_type() {
        Some(ErrorType::LiveKitUnavailable | ErrorType::UnknownNode) => {
            "This Stoat instance can't host voice calls right now.".into()
        }
        Some(ErrorType::NotAVoiceChannel) => "This isn't a voice channel.".into(),
        Some(ErrorType::CannotJoinCall) => "This voice channel is full.".into(),
        Some(ErrorType::MissingPermission { .. }) => "You can't join this voice channel.".into(),
        _ => error_text(err, "Couldn't join the call. Try again."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use revolt_result::Error as StoatError;

    fn stoat(error_type: ErrorType) -> ApiError {
        ApiError::Api(StoatError {
            error_type,
            location: None,
        })
    }

    #[test]
    fn each_voice_error_has_its_own_text() {
        let no_voice = "This Stoat instance can't host voice calls right now.";
        assert_eq!(
            join_error_text(&stoat(ErrorType::LiveKitUnavailable)),
            no_voice
        );
        assert_eq!(join_error_text(&stoat(ErrorType::UnknownNode)), no_voice);
        assert_eq!(
            join_error_text(&stoat(ErrorType::NotAVoiceChannel)),
            "This isn't a voice channel."
        );
        assert_eq!(
            join_error_text(&stoat(ErrorType::CannotJoinCall)),
            "This voice channel is full."
        );
        assert_eq!(
            join_error_text(&stoat(ErrorType::MissingPermission {
                permission: "Connect".into()
            })),
            "You can't join this voice channel."
        );
    }

    #[test]
    fn anything_else_falls_back() {
        let fallback = "Couldn't join the call. Try again.";
        assert_eq!(
            join_error_text(&stoat(ErrorType::AlreadyConnected)),
            fallback
        );
        assert_eq!(
            join_error_text(&ApiError::Unexpected {
                status: 500,
                detail: "boom".into()
            }),
            fallback
        );
    }

    #[test]
    fn a_rate_limit_says_so() {
        let err = ApiError::RateLimited {
            retry_after_ms: Some(2500),
        };
        assert_eq!(join_error_text(&err), error_text(&err, "unused"));
        assert!(join_error_text(&err).contains("3 s"));
    }
}
