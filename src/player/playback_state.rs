use songbird::tracks::TrackHandle;

use crate::{player::track::QueuedTrack, sources::resolver::PreparedStream};

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
    pub recovery_attempted: bool,
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

pub(crate) enum PlaybackRecoveryClaim {
    Stale,
    Retry(ClaimedPlayback),
    Skip {
        track: QueuedTrack,
        claimed_advancer: bool,
    },
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
    Ready(Box<PreviousPlayback>),
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

pub(crate) enum StreamPrefetchResult {
    Ready(PreparedStream),
    /// Every resolution slot was busy; the track is retried on the next trigger.
    Busy,
    Failed,
}

#[derive(Clone, Copy)]
pub(crate) enum StreamPrefetchOutcome {
    Attached,
    /// The stream was not stored: the source was busy, or the track left the
    /// queue before the prefetch finished.
    Skipped,
    Failed,
}

impl StreamPrefetchOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Attached => "attached",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}
