use serenity::model::id::{ChannelId, UserId};
use tracing::info;

use super::GuildPlayer;
use crate::{
    error::AppError,
    player::play_requests::{
        PendingPlayRequest, PlayRequestCancellation, PlayRequestReservation, PlayRequestTicket,
    },
};

impl GuildPlayer {
    pub async fn reserve_play_request(
        &self,
        channel_id: ChannelId,
        requested_by: UserId,
    ) -> Result<PlayRequestTicket, AppError> {
        let state = self.inner.lock().await;
        state.ensure_active(self.guild_id)?;
        Ok(self
            .play_requests
            .reserve(state.session_epoch, channel_id, requested_by)
            .await)
    }

    pub async fn play_request_session_is_current(&self, session_epoch: u64) -> bool {
        let state = self.inner.lock().await;
        state.ensure_active(self.guild_id).is_ok() && state.session_epoch == session_epoch
    }

    pub async fn install_play_request_abort(
        &self,
        reservation: PlayRequestReservation,
        abort_handle: tokio::task::AbortHandle,
    ) -> bool {
        self.play_requests
            .install_abort_handle(reservation, abort_handle)
            .await
    }

    pub async fn publish_play_resolution(
        &self,
        reservation: PlayRequestReservation,
        resolution: Result<crate::sources::resolver::TrackResolution, AppError>,
    ) -> bool {
        self.play_requests.publish(reservation, resolution).await
    }

    pub async fn cancel_play_request(
        &self,
        reservation: PlayRequestReservation,
        requested_by: UserId,
    ) -> PlayRequestCancellation {
        self.play_requests.cancel(reservation, requested_by).await
    }

    pub async fn take_next_play_request(&self) -> Option<PendingPlayRequest> {
        self.play_requests.take_next().await
    }

    pub(crate) async fn invalidate_play_requests(&self, current_epoch: u64) {
        let canceled = self
            .play_requests
            .invalidate_before_epoch(current_epoch)
            .await;
        if canceled > 0 {
            info!(
                guild_id = %self.guild_id,
                current_epoch,
                canceled_requests = canceled,
                "pending play requests invalidated"
            );
        }
    }
}
