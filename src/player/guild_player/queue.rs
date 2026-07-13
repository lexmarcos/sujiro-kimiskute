use super::GuildPlayer;
use crate::{
    error::AppError,
    player::{
        playback_state::{ClaimedPlayback, PlaybackOperation, PlaybackState},
        queue::QueueInsertionReceipt,
        track::QueuedTrack,
    },
};

impl GuildPlayer {
    pub async fn enqueue(&self, track: QueuedTrack) -> Result<usize, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        state.queue.add(track)
    }

    pub async fn enqueue_prefix(
        &self,
        tracks: Vec<QueuedTrack>,
    ) -> Result<QueueInsertionReceipt, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        Ok(state.queue.add_prefix(tracks))
    }

    pub(crate) async fn enqueue_for_playback(
        &self,
        track: QueuedTrack,
    ) -> Result<(usize, bool), AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        let position = state.queue.add(track)?;
        let claimed_advancer = state.claim_queue_advancer();
        Ok((position, claimed_advancer))
    }

    pub(crate) async fn enqueue_prefix_for_playback(
        &self,
        tracks: Vec<QueuedTrack>,
        expected_session_epoch: u64,
    ) -> Result<(QueueInsertionReceipt, bool), AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        if state.session_epoch != expected_session_epoch {
            return Err(AppError::Internal {
                context: format!(
                    "guild {} play enqueue expected session epoch {}, found {}",
                    self.guild_id, expected_session_epoch, state.session_epoch
                ),
            });
        }
        let receipt = state.queue.add_prefix(tracks);
        let claimed_advancer = state.claim_queue_advancer();
        Ok((receipt, claimed_advancer))
    }

    pub async fn pop_next(&self) -> Option<QueuedTrack> {
        let mut state = self.inner.lock().await;
        if state.ensure_active(self.guild_id).is_err() {
            return None;
        }
        state.queue.pop_next()
    }

    pub async fn queue_snapshot(&self) -> Vec<QueuedTrack> {
        self.inner.lock().await.queue.iter().cloned().collect()
    }

    pub async fn clear_queue(&self) -> usize {
        let mut state = self.inner.lock().await;
        let removed = state.queue.len();
        state.queue.clear();
        removed
    }

    pub(crate) async fn take_next_for_advancer(&self) -> Option<ClaimedPlayback> {
        let mut state = self.inner.lock().await;
        if state.ensure_active(self.guild_id).is_err()
            || !state.queue_advancer_active
            || state.current.is_some()
            || state.playback_state != PlaybackState::Idle
        {
            state.queue_advancer_active = false;
            return None;
        }

        let Some(track) = state.queue.pop_next() else {
            state.queue_advancer_active = false;
            return None;
        };
        let operation = state.begin_playback(track.clone());
        Some(ClaimedPlayback { operation, track })
    }

    pub(crate) async fn claim_queue_advancer(&self) -> bool {
        let mut state = self.inner.lock().await;
        if state.ensure_active(self.guild_id).is_err() {
            return false;
        }
        state.claim_queue_advancer()
    }

    pub(crate) async fn finish_advancer_after_start(&self) -> bool {
        let mut state = self.inner.lock().await;
        if state.ensure_active(self.guild_id).is_err() || !state.queue_advancer_active {
            return false;
        }
        if state.current.is_none()
            && state.playback_state == PlaybackState::Idle
            && !state.queue.is_empty()
        {
            return true;
        }

        state.queue_advancer_active = false;
        false
    }

    pub(crate) async fn finish_playback_and_claim_advancer(
        &self,
        operation: PlaybackOperation,
    ) -> Option<(QueuedTrack, bool)> {
        let mut state = self.inner.lock().await;
        if state.ensure_active(self.guild_id).is_err() || !state.current_matches(operation) {
            return None;
        }

        let current = state.current.take()?;
        state.playback_state = PlaybackState::Idle;
        let claimed_advancer = state.claim_queue_advancer();
        Some((current.track, claimed_advancer))
    }
}
