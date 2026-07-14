use super::{GuildPlayer, GuildPlayerState};
use crate::{
    error::AppError,
    player::{
        lifecycle::{
            AutoLeaveCancellation, AutoLeaveTimer, AutoLeaveToken, LeaveClaim, LeaveOperation,
            PlayerLifecycle, closing_error,
        },
        playback_state::PlaybackState,
    },
};

impl GuildPlayer {
    pub(crate) async fn claim_leave(&self) -> LeaveClaim {
        let mut state = self.inner.lock().await;
        if state.lifecycle == PlayerLifecycle::Closing {
            return LeaveClaim::AlreadyClosing;
        }

        let auto_leave_abort = state
            .invalidate_auto_leave()
            .and_then(|timer| timer.abort_handle);
        LeaveClaim::Ready(state.begin_leave(auto_leave_abort))
    }

    pub(crate) async fn cancel_auto_leave_for_activity(
        &self,
    ) -> Result<AutoLeaveCancellation, AppError> {
        let mut state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        Ok(cancellation_from(state.invalidate_auto_leave()))
    }

    pub(crate) async fn cancel_auto_leave_timer(&self) -> AutoLeaveCancellation {
        let mut state = self.inner.lock().await;
        cancellation_from(state.invalidate_auto_leave())
    }

    pub(crate) async fn claim_auto_leave_timer(
        &self,
        channel_id: serenity::model::id::ChannelId,
    ) -> Option<AutoLeaveToken> {
        let mut state = self.inner.lock().await;
        if state.lifecycle != PlayerLifecycle::Active
            || state.voice_connection.channel_id() != Some(channel_id)
            || state.auto_leave_timer.is_some()
        {
            return None;
        }

        state.auto_leave_generation = state.auto_leave_generation.wrapping_add(1);
        let token = AutoLeaveToken {
            generation: state.auto_leave_generation,
            channel_id,
        };
        state.auto_leave_timer = Some(AutoLeaveTimer {
            token,
            abort_handle: None,
        });
        Some(token)
    }

    pub(crate) async fn install_auto_leave_abort(
        &self,
        token: AutoLeaveToken,
        abort_handle: tokio::task::AbortHandle,
    ) -> Option<tokio::task::AbortHandle> {
        let mut state = self.inner.lock().await;
        let Some(timer) = state.auto_leave_timer.as_mut() else {
            return Some(abort_handle);
        };
        if timer.token != token {
            return Some(abort_handle);
        }
        timer.abort_handle = Some(abort_handle);
        None
    }

    pub(crate) async fn discard_auto_leave_token(&self, token: AutoLeaveToken) -> bool {
        let mut state = self.inner.lock().await;
        if !state.auto_leave_matches(token) {
            return false;
        }
        state.invalidate_auto_leave();
        true
    }

    pub(crate) async fn claim_auto_leave_expiration(
        &self,
        token: AutoLeaveToken,
    ) -> Option<LeaveOperation> {
        let mut state = self.inner.lock().await;
        if state.lifecycle != PlayerLifecycle::Active
            || state.voice_connection.channel_id() != Some(token.channel_id)
            || !state.auto_leave_matches(token)
        {
            return None;
        }

        state.invalidate_auto_leave();
        Some(state.begin_leave(None))
    }

    pub(crate) async fn reopen_after_failed_leave(&self, session_epoch: u64) -> bool {
        let mut state = self.inner.lock().await;
        if state.lifecycle != PlayerLifecycle::Closing || state.session_epoch != session_epoch {
            return false;
        }

        state.lifecycle = PlayerLifecycle::Active;
        true
    }
}

impl GuildPlayerState {
    fn auto_leave_matches(&self, token: AutoLeaveToken) -> bool {
        self.auto_leave_timer
            .as_ref()
            .is_some_and(|timer| timer.token == token)
    }

    fn invalidate_auto_leave(&mut self) -> Option<AutoLeaveTimer> {
        self.auto_leave_generation = self.auto_leave_generation.wrapping_add(1);
        self.auto_leave_timer.take()
    }

    fn begin_leave(
        &mut self,
        auto_leave_abort: Option<tokio::task::AbortHandle>,
    ) -> LeaveOperation {
        self.lifecycle = PlayerLifecycle::Closing;
        self.session_epoch = self.session_epoch.wrapping_add(1);
        let current = self.current.take();
        let removed_from_queue = self.queue.len();
        self.queue.clear();
        self.history.clear();
        self.playback_state = PlaybackState::Idle;
        self.queue_advancer_active = false;

        LeaveOperation {
            track: current.as_ref().map(|current| current.track.clone()),
            handle: current.and_then(|current| current.handle),
            removed_from_queue,
            session_epoch: self.session_epoch,
            auto_leave_abort,
        }
    }

    pub(super) fn ensure_active(
        &self,
        guild_id: serenity::model::id::GuildId,
    ) -> Result<(), AppError> {
        if self.lifecycle == PlayerLifecycle::Active {
            return Ok(());
        }
        Err(closing_error(guild_id))
    }
}

fn cancellation_from(timer: Option<AutoLeaveTimer>) -> AutoLeaveCancellation {
    AutoLeaveCancellation {
        canceled: timer.is_some(),
        abort_handle: timer.and_then(|timer| timer.abort_handle),
    }
}
