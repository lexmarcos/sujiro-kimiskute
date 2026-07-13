use super::GuildPlayer;
use crate::{
    error::AppError,
    player::playback_state::{
        PlaybackControl, PlaybackControlClaim, PlaybackOperation, PlaybackState,
    },
};

impl GuildPlayer {
    pub(crate) async fn claim_playback_control(
        &self,
        control: PlaybackControl,
    ) -> Result<PlaybackControlClaim, AppError> {
        let state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        let Some(current) = state.current.as_ref() else {
            return Ok(PlaybackControlClaim::NoTrack);
        };

        Ok(match (control, state.playback_state) {
            (_, PlaybackState::Idle | PlaybackState::Starting) => PlaybackControlClaim::NoTrack,
            (PlaybackControl::Pause, PlaybackState::Paused) => PlaybackControlClaim::AlreadyPaused,
            (PlaybackControl::Resume, PlaybackState::Playing) => {
                PlaybackControlClaim::AlreadyPlaying
            }
            _ => current
                .handle
                .clone()
                .map(|handle| PlaybackControlClaim::Ready {
                    handle,
                    operation: PlaybackOperation {
                        playback_id: current.playback_id,
                        session_epoch: current.session_epoch,
                    },
                })
                .unwrap_or(PlaybackControlClaim::NoTrack),
        })
    }

    pub(crate) async fn confirm_playback_control(
        &self,
        operation: PlaybackOperation,
        expected_state: PlaybackState,
        new_state: PlaybackState,
    ) -> bool {
        let mut state = self.inner.lock().await;
        if state.session_epoch != operation.session_epoch
            || !state.current_matches(operation)
            || state.playback_state != expected_state
        {
            return false;
        }

        state.playback_state = new_state;
        true
    }
}
