use songbird::tracks::TrackHandle;

use crate::player::track::QueuedTrack;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub enum PlaybackState {
    #[default]
    Idle,
    Starting,
    Playing,
    Paused,
}

pub(super) struct CurrentTrack {
    pub track: QueuedTrack,
    pub playback_id: u64,
    pub session_epoch: u64,
    pub handle: Option<TrackHandle>,
}

#[derive(Clone, Copy)]
pub(crate) struct PlaybackOperation {
    pub playback_id: u64,
    pub session_epoch: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum PlaybackControl {
    Pause,
    Resume,
}

pub(crate) enum PlaybackControlClaim {
    NoTrack,
    AlreadyPaused,
    AlreadyPlaying,
    Ready {
        handle: TrackHandle,
        operation: PlaybackOperation,
    },
}

pub(crate) struct ClaimedPlayback {
    pub operation: PlaybackOperation,
    pub track: QueuedTrack,
}

pub(crate) struct SkippedPlayback {
    pub track: QueuedTrack,
    pub handle: Option<TrackHandle>,
    pub operation: PlaybackOperation,
    pub claimed_advancer: bool,
}

pub(crate) enum PlaybackSkipClaim {
    NoTrack,
    NoNext,
    Ready(SkippedPlayback),
}

pub(crate) enum PreviousPlaybackClaim {
    NoPrevious,
    Ready(PreviousPlayback),
}

pub(crate) struct PreviousPlayback {
    pub track: QueuedTrack,
    pub operation: PlaybackOperation,
    pub interrupted_track_id: Option<String>,
    pub interrupted_handle: Option<TrackHandle>,
}

pub(crate) struct StoppedPlayback {
    pub track: Option<QueuedTrack>,
    pub handle: Option<TrackHandle>,
    pub removed_from_queue: usize,
    pub session_epoch: u64,
}
