use super::GuildPlayer;
use crate::error::AppError;
use crate::player::playback_state::{
    PlaybackOperation, PlaybackSkipClaim, PlaybackState, PreviousPlayback, PreviousPlaybackClaim,
    SkippedPlayback, StoppedPlayback,
};

impl GuildPlayer {
    pub(crate) async fn claim_skip(&self) -> Result<PlaybackSkipClaim, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        if state.current.is_none() {
            return Ok(PlaybackSkipClaim::NoTrack);
        }
        if state.queue.is_empty() {
            return Ok(PlaybackSkipClaim::NoNext);
        }

        let current = state.current.take().ok_or(AppError::Internal {
            context: format!(
                "guild {} lost current track during skip claim",
                self.guild_id
            ),
        })?;
        let operation = PlaybackOperation {
            playback_id: current.playback_id,
            session_epoch: current.session_epoch,
        };
        state.record_completed_track(current.track.clone());
        state.playback_state = PlaybackState::Idle;
        let claimed_advancer = state.claim_queue_advancer();

        Ok(PlaybackSkipClaim::Ready(SkippedPlayback {
            track: current.track,
            handle: current.handle,
            operation,
            claimed_advancer,
        }))
    }

    pub(crate) async fn claim_previous(&self) -> Result<PreviousPlaybackClaim, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        let Some(previous_track) = state.history.pop_back() else {
            return Ok(PreviousPlaybackClaim::NoPrevious);
        };

        state.queue_advancer_active = false;
        let interrupted = state.current.take();
        if let Some(current) = interrupted.as_ref() {
            state.queue.restore_current_to_front(current.track.clone());
        }
        state.playback_state = PlaybackState::Idle;
        let operation = state.begin_playback(previous_track.clone());

        Ok(PreviousPlaybackClaim::Ready(PreviousPlayback {
            track: previous_track,
            operation,
            interrupted_track_id: interrupted
                .as_ref()
                .map(|current| current.track.track.id.clone()),
            interrupted_handle: interrupted.and_then(|current| current.handle),
        }))
    }

    pub(crate) async fn claim_stop(&self) -> Result<StoppedPlayback, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        state.session_epoch = state.session_epoch.wrapping_add(1);
        let current = state.current.take();
        let removed_from_queue = state.queue.len();
        state.queue.clear();
        state.history.clear();
        state.playback_state = PlaybackState::Idle;
        state.queue_advancer_active = false;

        Ok(StoppedPlayback {
            track: current.as_ref().map(|current| current.track.clone()),
            handle: current.and_then(|current| current.handle),
            removed_from_queue,
            session_epoch: state.session_epoch,
        })
    }
}
