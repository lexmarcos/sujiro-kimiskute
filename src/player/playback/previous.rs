use std::sync::Arc;

use tracing::{info, warn};

use super::PlaybackService;
use crate::{
    error::AppError,
    player::{
        guild_player::GuildPlayer,
        playback_state::{PreviousPlayback, PreviousPlaybackClaim},
        track::QueuedTrack,
    },
};

impl PlaybackService {
    pub async fn previous(
        self: &Arc<Self>,
        player: Arc<GuildPlayer>,
    ) -> Result<PlaybackPreviousResult, AppError> {
        self.validate_player(&player).await?;
        let previous = match player.claim_previous().await? {
            PreviousPlaybackClaim::NoPrevious => return Ok(PlaybackPreviousResult::NoPrevious),
            PreviousPlaybackClaim::Ready(previous) => previous,
        };
        self.start_previous_track(player, previous).await
    }

    async fn start_previous_track(
        self: &Arc<Self>,
        player: Arc<GuildPlayer>,
        previous: PreviousPlayback,
    ) -> Result<PlaybackPreviousResult, AppError> {
        stop_interrupted_handle(&player, &previous);
        if let Err(error) = self
            .start_claimed_track(&player, previous.operation, &previous.track.track)
            .await
        {
            self.spawn_queue_advancer_if_claimed(Arc::clone(&player))
                .await;
            return Err(error);
        }
        Ok(previous_started(&player, previous))
    }

    async fn spawn_queue_advancer_if_claimed(self: &Arc<Self>, player: Arc<GuildPlayer>) {
        if !player.claim_queue_advancer().await {
            return;
        }
        let playback = Arc::clone(self);
        tokio::spawn(async move {
            playback.advance_claimed_queue(player).await;
        });
    }
}

pub enum PlaybackPreviousResult {
    NoPrevious,
    Started { track: QueuedTrack },
}

fn previous_started(player: &GuildPlayer, previous: PreviousPlayback) -> PlaybackPreviousResult {
    info!(
        guild_id = %player.guild_id(),
        track_id = %previous.track.track.id,
        playback_id = previous.operation.playback_id,
        "previous track started"
    );
    PlaybackPreviousResult::Started {
        track: previous.track,
    }
}

fn stop_interrupted_handle(player: &GuildPlayer, previous: &PreviousPlayback) {
    let Some(handle) = previous.interrupted_handle.as_ref() else {
        return;
    };
    if let Err(source) = handle.stop() {
        warn!(
            guild_id = %player.guild_id(),
            track_id = previous.interrupted_track_id.as_deref(),
            playback_id = previous.operation.playback_id,
            error = %source,
            "failed to stop track interrupted by previous request"
        );
    }
}
