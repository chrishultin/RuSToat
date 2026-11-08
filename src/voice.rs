//! Voice calls, through LiveKit's Rust SDK.
//!
//! Stoat's voice runs on LiveKit: Stoat hands out a room token, and the client
//! talks to LiveKit directly. Audio goes through WebRTC's own platform audio
//! module ([`PlatformAudio`]): the default microphone and speakers, with
//! WebRTC's echo cancellation, noise suppression and gain control.
//!
//! Everything here runs on the Tokio IO runtime (see [`crate::runtime`]),
//! never on the UI thread: rooms and the audio module are made and dropped
//! there.

use futures::channel::mpsc;
use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use livekit::{AudioError, PlatformAudio};
use revolt_result::ErrorType;

use crate::api::ApiError;
use crate::profile_edit::error_text;
use crate::runtime::runtime;

/// Why joining a call failed.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    /// Stoat wouldn't hand out a room token.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// LiveKit wouldn't connect, or wouldn't take the microphone.
    #[error("LiveKit: {0}")]
    LiveKit(#[from] RoomError),
    /// WebRTC couldn't start the microphone and speakers.
    #[error("audio: {0}")]
    Audio(#[from] AudioError),
}

impl VoiceError {
    /// What a toast says.
    pub fn text(&self) -> String {
        match self {
            VoiceError::Api(err) => join_error_text(err),
            VoiceError::LiveKit(_) => "Couldn't connect to the voice server. Try again.".into(),
            VoiceError::Audio(_) => "Couldn't start your microphone and speakers.".into(),
        }
    }
}

/// Something that happened to a call that the UI must act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceEvent {
    /// The call ended without the user leaving it: LiveKit lost the
    /// connection for good, or the user was removed (by joining on another
    /// device, say).
    Disconnected,
}

/// A call's [`VoiceEvent`]s, as they happen. Ends when the call does.
pub type VoiceEvents = mpsc::UnboundedReceiver<VoiceEvent>;

/// A call in progress: a LiveKit room, and the platform audio module playing
/// it and capturing the microphone.
///
/// Dropping it leaves the call: the room is closed and the audio module
/// released on the IO runtime, so dropping never blocks the UI.
pub struct VoiceCall {
    channel: String,
    room: Option<Room>,
    audio: Option<PlatformAudio>,
    /// The task reading the room's events.
    events: tokio::task::AbortHandle,
}

impl VoiceCall {
    /// Join `channel`'s room at `url` with `token` (from
    /// [`StoatClient::join_call`](crate::api::StoatClient::join_call), at
    /// once: it lasts 10 seconds). With `publish_mic`, the default
    /// microphone is sent (Stoat allows it with Speak); with `subscribe`,
    /// everyone else's audio plays on the default speakers (with Listen).
    /// Video isn't subscribed to.
    ///
    /// Must run on the IO runtime.
    pub async fn join(
        channel: &str,
        url: &str,
        token: &str,
        publish_mic: bool,
        subscribe: bool,
    ) -> Result<(VoiceCall, VoiceEvents), VoiceError> {
        let audio = PlatformAudio::new()?;
        let mut options = RoomOptions::default();
        // Audio only: what's subscribed to is picked below.
        options.auto_subscribe = false;
        let (room, room_events) = Room::connect(url, token, options).await?;
        log::info!(
            "voice: joined {channel} as {}",
            room.local_participant().identity().0
        );

        if subscribe {
            for participant in room.remote_participants().values() {
                log::info!("voice: {} is in {channel}", participant.identity().0);
                for publication in participant.track_publications().values() {
                    subscribe_audio(publication, participant);
                }
            }
        }
        let (tx, events) = mpsc::unbounded();
        let task = runtime().spawn(read_events(channel.to_string(), room_events, subscribe, tx));
        // From here on, dropping `call` (an error below, or the join being
        // cancelled) closes the room.
        let call = VoiceCall {
            channel: channel.to_string(),
            room: Some(room),
            audio: Some(audio),
            events: task.abort_handle(),
        };

        if publish_mic && let (Some(room), Some(audio)) = (&call.room, &call.audio) {
            let track = LocalAudioTrack::create_audio_track("microphone", audio.rtc_source());
            let options = TrackPublishOptions {
                source: TrackSource::Microphone,
                ..TrackPublishOptions::default()
            };
            room.local_participant()
                .publish_track(LocalTrack::Audio(track), options)
                .await?;
            log::info!("voice: sending the microphone in {channel}");
        }
        Ok((call, events))
    }

    /// The voice channel this call is in.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Leave the call (the same as dropping it).
    pub fn leave(self) {
        log::info!("voice: leaving {}", self.channel);
    }
}

impl Drop for VoiceCall {
    fn drop(&mut self) {
        // Leaving isn't a disconnect to tell the UI about.
        self.events.abort();
        let channel = std::mem::take(&mut self.channel);
        let room = self.room.take();
        let audio = self.audio.take();
        runtime().spawn(async move {
            if let Some(room) = room {
                match room.close().await {
                    Ok(()) | Err(RoomError::AlreadyClosed) => {}
                    Err(err) => log::warn!("voice: closing {channel}'s room: {err}"),
                }
            }
            // After the room, so the microphone and speakers stop last.
            drop(audio);
            log::info!("voice: left {channel}");
        });
    }
}

/// Subscribe to `publication` if it's audio (a microphone, or a screen
/// share's sound).
fn subscribe_audio(publication: &RemoteTrackPublication, participant: &RemoteParticipant) {
    if publication.kind() == TrackKind::Audio {
        log::debug!(
            "voice: subscribing to {}'s {:?} audio ({})",
            participant.identity().0,
            publication.source(),
            publication.sid()
        );
        publication.set_subscribed(true);
    }
}

/// Log `channel`'s room events, subscribe to audio as it's published (with
/// `subscribe`), and pass on what the UI needs to `tx`.
async fn read_events(
    channel: String,
    mut events: tokio::sync::mpsc::UnboundedReceiver<RoomEvent>,
    subscribe: bool,
    tx: mpsc::UnboundedSender<VoiceEvent>,
) {
    while let Some(event) = events.recv().await {
        match event {
            RoomEvent::ParticipantConnected(participant) => {
                log::info!("voice: {} joined {channel}", participant.identity().0);
            }
            RoomEvent::ParticipantDisconnected(participant) => {
                log::info!("voice: {} left {channel}", participant.identity().0);
            }
            RoomEvent::TrackPublished {
                publication,
                participant,
            } if subscribe => subscribe_audio(&publication, &participant),
            RoomEvent::TrackSubscribed {
                publication,
                participant,
                ..
            } => log::debug!(
                "voice: subscribed to {}'s {:?} ({})",
                participant.identity().0,
                publication.source(),
                publication.sid()
            ),
            RoomEvent::TrackUnsubscribed {
                publication,
                participant,
                ..
            } => log::debug!(
                "voice: unsubscribed from {}'s {:?} ({})",
                participant.identity().0,
                publication.source(),
                publication.sid()
            ),
            RoomEvent::TrackSubscriptionFailed {
                participant,
                error,
                track_sid,
            } => log::warn!(
                "voice: couldn't subscribe to {}'s {track_sid}: {error}",
                participant.identity().0
            ),
            RoomEvent::Reconnecting => log::info!("voice: reconnecting to {channel}"),
            RoomEvent::Reconnected => log::info!("voice: reconnected to {channel}"),
            RoomEvent::Disconnected { reason } => {
                log::info!("voice: disconnected from {channel} ({reason:?})");
                break;
            }
            _ => {}
        }
    }
    // Disconnected, or the room went away under us.
    let _ = tx.unbounded_send(VoiceEvent::Disconnected);
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
    fn a_failed_connection_or_audio_says_which() {
        assert_eq!(
            VoiceError::LiveKit(RoomError::Internal("boom".into())).text(),
            "Couldn't connect to the voice server. Try again."
        );
        assert_eq!(
            VoiceError::Audio(AudioError::PlatformInitFailed).text(),
            "Couldn't start your microphone and speakers."
        );
        assert_eq!(
            VoiceError::Api(stoat(ErrorType::CannotJoinCall)).text(),
            "This voice channel is full."
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
