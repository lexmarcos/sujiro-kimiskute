use std::{sync::Arc, time::Instant};

use tracing::{info, warn};

use super::PlaybackService;
use crate::player::{guild_player::GuildPlayer, playback_state::StreamPrefetchResult};

impl PlaybackService {
    /// Prepares the next queued track's stream in the background, so the
    /// transition between tracks does not wait for a yt-dlp run. Neither the
    /// command reply nor the current playback start should wait for it.
    pub(super) fn spawn_next_stream_prefetch(self: &Arc<Self>, player: Arc<GuildPlayer>) {
        let playback = Arc::clone(self);
        tokio::spawn(async move {
            playback.prefetch_next_stream(player).await;
        });
    }

    async fn prefetch_next_stream(&self, player: Arc<GuildPlayer>) {
        let Some(track) = player.claim_stream_prefetch().await else {
            return;
        };
        let started_at = Instant::now();

        let result = match self.resolver.prefetch_stream(&track).await {
            Ok(Some(stream)) => StreamPrefetchResult::Ready(stream),
            Ok(None) => StreamPrefetchResult::Busy,
            Err(error) => {
                warn!(
                    guild_id = %player.guild_id(),
                    track_id = %track.id,
                    error = %error,
                    "next track stream prefetch failed"
                );
                StreamPrefetchResult::Failed
            }
        };
        let outcome = player.finish_stream_prefetch(&track.id, result).await;
        info!(
            guild_id = %player.guild_id(),
            track_id = %track.id,
            outcome = outcome.as_str(),
            duration_ms = started_at.elapsed().as_secs_f64() * 1_000.0,
            "next track stream prefetch finished"
        );
    }
}
