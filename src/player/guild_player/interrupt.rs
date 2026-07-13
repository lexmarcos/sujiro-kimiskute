use super::GuildPlayer;
use crate::error::AppError;
use crate::player::playback_state::{
    PlaybackOperation, PlaybackState, SkippedPlayback, StoppedPlayback,
};

impl GuildPlayer {
    pub(crate) async fn claim_skip(&self) -> Result<Option<SkippedPlayback>, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        let Some(current) = state.current.take() else {
            return Ok(None);
        };
        let operation = PlaybackOperation {
            playback_id: current.playback_id,
            session_epoch: current.session_epoch,
        };
        state.playback_state = PlaybackState::Idle;
        let claimed_advancer = state.claim_queue_advancer();

        Ok(Some(SkippedPlayback {
            track: current.track,
            handle: current.handle,
            operation,
            claimed_advancer,
        }))
    }

    pub(crate) async fn claim_stop(&self) -> Result<StoppedPlayback, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        state.session_epoch = state.session_epoch.wrapping_add(1);
        let current = state.current.take();
        let removed_from_queue = state.queue.len();
        state.queue.clear();
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
