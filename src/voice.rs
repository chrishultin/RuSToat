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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEvent {
    /// Who's speaking now, by user ID (LiveKit identities are Stoat user
    /// IDs), the user included. Empty when nobody is.
    Speaking(Vec<String>),
    /// The call ended without the user leaving it: LiveKit lost the
    /// connection for good, or the user was removed (by joining on another
    /// device, say).
    Disconnected,
}

/// A call's [`VoiceEvent`]s, as they happen. Ends when the call does.
pub type VoiceEvents = mpsc::UnboundedReceiver<VoiceEvent>;

/// The user's mute and deafen, with the web client's rules. `muted` is
/// the user's own choice for the microphone; deafening turns it off too
/// without changing that choice, so undeafening brings it back only if it
/// was on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MuteState {
    pub muted: bool,
    pub deafened: bool,
}

impl MuteState {
    /// Whether the microphone is sent.
    pub fn mic_on(self) -> bool {
        !self.muted && !self.deafened
    }

    /// The Mute button. While deafened, it undeafens and leaves the
    /// microphone off.
    pub fn toggle_mute(self) -> Self {
        if self.deafened {
            MuteState {
                muted: true,
                deafened: false,
            }
        } else {
            MuteState {
                muted: !self.muted,
                ..self
            }
        }
    }

    /// The Deafen button.
    pub fn toggle_deafen(self) -> Self {
        MuteState {
            deafened: !self.deafened,
            ..self
        }
    }
}

/// A call in progress: a LiveKit room, and the platform audio module playing
/// it and capturing the microphone.
///
/// Dropping it leaves the call: the room is closed and the audio module
/// released on the IO runtime, so dropping never blocks the UI.
pub struct VoiceCall {
    channel: String,
    room: Option<Arc<Room>>,
    audio: Option<PlatformAudio>,
    /// The task reading the room's events.
    events: tokio::task::AbortHandle,
    /// Mute and deafen changes, applied in order by the task that
    /// `controls` aborts (started once the microphone is published).
    mute: tokio::sync::mpsc::UnboundedSender<MuteState>,
    controls: Option<tokio::task::AbortHandle>,
}

impl VoiceCall {
    /// Join `channel`'s room at `url` with `token` (from
    /// [`StoatClient::join_call`](crate::api::StoatClient::join_call), at
    /// once: it lasts 10 seconds). With `publish_mic`, the default
    /// microphone is sent (Stoat allows it with Speak); with `subscribe`,
    /// everyone else's audio plays on the default speakers (with Listen).
    /// Video isn't subscribed to. `mute` is applied from the start: a muted
    /// microphone is still published, so unmuting is instant.
    ///
    /// Must run on the IO runtime.
    pub async fn join(
        channel: &str,
        url: &str,
        token: &str,
        publish_mic: bool,
        subscribe: bool,
        mute: MuteState,
    ) -> Result<(VoiceCall, VoiceEvents), VoiceError> {
        let audio = PlatformAudio::new()?;
        let mut options = RoomOptions::default();
        // Audio only: what's subscribed to is picked below.
        options.auto_subscribe = false;
        let (room, room_events) = Room::connect(url, token, options).await?;
        let room = Arc::new(room);
        // Before subscribing, so nothing plays while deafened.
        let deafened = Arc::new(AtomicBool::new(mute.deafened));
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
        let task = runtime().spawn(read_events(
            channel.to_string(),
            room_events,
            subscribe,
            deafened.clone(),
            tx,
        ));
        let (send_mute, changes) = tokio::sync::mpsc::unbounded_channel();
        // From here on, dropping `call` (an error below, or the join being
        // cancelled) closes the room.
        let mut call = VoiceCall {
            channel: channel.to_string(),
            room: Some(room.clone()),
            audio: Some(audio.clone()),
            events: task.abort_handle(),
            mute: send_mute,
            controls: None,
        };

        let mut mic = None;
        if publish_mic {
            let track = LocalAudioTrack::create_audio_track("microphone", audio.rtc_source());
            let options = TrackPublishOptions {
                source: TrackSource::Microphone,
                ..TrackPublishOptions::default()
            };
            mic = Some(
                room.local_participant()
                    .publish_track(LocalTrack::Audio(track), options)
                    .await?,
            );
            log::info!("voice: sending the microphone in {channel}");
        }
        let controls = Controls {
            channel: channel.to_string(),
            room,
            audio,
            mic,
            deafened,
            applied: MuteState::default(),
        };
        call.controls = Some(runtime().spawn(controls.run(changes)).abort_handle());
        call.set_mute(mute);
        Ok((call, events))
    }

    /// Apply the user's mute and deafen to the call.
    pub fn set_mute(&self, mute: MuteState) {
        let _ = self.mute.send(mute);
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
        if let Some(controls) = &self.controls {
            controls.abort();
        }
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

/// What applies mute and deafen to a call, one change at a time.
struct Controls {
    channel: String,
    room: Arc<Room>,
    audio: PlatformAudio,
    /// The microphone's publication; `None` without Speak.
    mic: Option<LocalTrackPublication>,
    /// Shared with [`read_events`], which silences audio subscribed while
    /// deafened.
    deafened: Arc<AtomicBool>,
    /// What's in effect. The microphone starts on, as published.
    applied: MuteState,
}

impl Controls {
    async fn run(mut self, mut changes: tokio::sync::mpsc::UnboundedReceiver<MuteState>) {
        while let Some(mute) = changes.recv().await {
            self.apply(mute);
        }
    }

    fn apply(&mut self, mute: MuteState) {
        if mute.deafened != self.applied.deafened {
            self.deafened.store(mute.deafened, Ordering::Relaxed);
            for participant in self.room.remote_participants().values() {
                for publication in participant.track_publications().values() {
                    if publication.kind() == TrackKind::Audio {
                        publication.set_enabled(!mute.deafened);
                    }
                }
            }
            log::info!(
                "voice: {} in {}",
                if mute.deafened {
                    "deafened"
                } else {
                    "undeafened"
                },
                self.channel
            );
        }
        if let Some(mic) = &self.mic
            && mute.mic_on() != self.applied.mic_on()
        {
            // Recording stops too, so the microphone is let go (and macOS's
            // indicator goes off). Unmuting starts it before unmuting.
            if mute.mic_on() {
                if let Err(err) = self.audio.start_recording() {
                    log::warn!("voice: couldn't start the microphone again: {err}");
                }
                mic.unmute();
            } else {
                mic.mute();
                if let Err(err) = self.audio.stop_recording() {
                    log::warn!("voice: couldn't stop the microphone: {err}");
                }
            }
            log::info!(
                "voice: microphone {} in {}",
                if mute.mic_on() { "on" } else { "off" },
                self.channel
            );
        }
        self.applied = mute;
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
/// `subscribe`), silence it while `deafened`, and pass on what the UI needs
/// to `tx`.
async fn read_events(
    channel: String,
    mut events: tokio::sync::mpsc::UnboundedReceiver<RoomEvent>,
    subscribe: bool,
    deafened: Arc<AtomicBool>,
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
            } => {
                log::debug!(
                    "voice: subscribed to {}'s {:?} ({})",
                    participant.identity().0,
                    publication.source(),
                    publication.sid()
                );
                if deafened.load(Ordering::Relaxed) && publication.kind() == TrackKind::Audio {
                    publication.set_enabled(false);
                }
            }
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
            RoomEvent::ActiveSpeakersChanged { speakers } => {
                let speakers: Vec<String> = speakers.iter().map(|p| p.identity().0).collect();
                log::debug!("voice: speaking {speakers:?}");
                let _ = tx.unbounded_send(VoiceEvent::Speaking(speakers));
            }
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
    fn deafening_turns_the_microphone_off_and_undeafening_restores_it() {
        let open = MuteState::default();
        assert!(open.mic_on());
        let deaf = open.toggle_deafen();
        assert_eq!(
            deaf,
            MuteState {
                muted: false,
                deafened: true
            }
        );
        assert!(!deaf.mic_on());
        assert!(deaf.toggle_deafen().mic_on(), "it was on before");
        // Muted first: undeafening leaves it off.
        let muted = open.toggle_mute().toggle_deafen().toggle_deafen();
        assert_eq!(
            muted,
            MuteState {
                muted: true,
                deafened: false
            }
        );
        assert!(!muted.mic_on());
    }

    #[test]
    fn muting_while_deafened_undeafens_and_stays_muted() {
        for before in [false, true] {
            let deaf = MuteState {
                muted: before,
                deafened: true,
            };
            assert_eq!(
                deaf.toggle_mute(),
                MuteState {
                    muted: true,
                    deafened: false
                }
            );
        }
        // Not deafened, it's a plain toggle.
        assert!(MuteState::default().toggle_mute().muted);
        assert!(!MuteState::default().toggle_mute().toggle_mute().muted);
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
