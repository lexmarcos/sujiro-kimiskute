use std::{sync::Arc, time::Duration};

use songbird::tracks::{ControlError, PlayError, TrackHandle};
use tracing::warn;

use super::{PlaybackService, stop_created_handle};
use crate::{
    error::AppError,
    player::{guild_player::GuildPlayer, playback_state::PlaybackOperation, track::ResolvedTrack},
    sources::resolver::PreparedStream,
};

impl PlaybackService {
    pub(super) async fn apply_initial_position(
        self: &Arc<Self>,
        player: &Arc<GuildPlayer>,
        operation: PlaybackOperation,
        track: &ResolvedTrack,
        handle: TrackHandle,
        fallback_stream: Option<PreparedStream>,
    ) -> Result<TrackHandle, AppError> {
        let Some(start_at_seconds) = track.start_at_seconds else {
            return Ok(handle);
        };
        match handle
            .seek_async(Duration::from_secs(start_at_seconds))
            .await
        {
            Ok(_) => self.finish_initial_seek(player, operation, track, handle),
            Err(source @ ControlError::Play(PlayError::Seek(_))) => {
                self.fallback_after_initial_seek(
                    player,
                    operation,
                    track,
                    handle,
                    fallback_stream,
                    source,
                )
                .await
            }
            Err(source) => {
                stop_created_handle(&handle);
                Err(initial_seek_error(track, start_at_seconds, source))
            }
        }
    }

    fn finish_initial_seek(
        self: &Arc<Self>,
        player: &GuildPlayer,
        operation: PlaybackOperation,
        track: &ResolvedTrack,
        handle: TrackHandle,
    ) -> Result<TrackHandle, AppError> {
        self.attach_playback_events(player, operation, &handle)
            .map_err(|source| {
                stop_created_handle(&handle);
                AppError::Voice {
                    context: format!(
                        "could not observe track {} after its initial seek: {source}",
                        track.id
                    ),
                }
            })?;
        Ok(handle)
    }

    async fn fallback_after_initial_seek(
        self: &Arc<Self>,
        player: &Arc<GuildPlayer>,
        operation: PlaybackOperation,
        track: &ResolvedTrack,
        failed_handle: TrackHandle,
        fallback_stream: Option<PreparedStream>,
        source: ControlError,
    ) -> Result<TrackHandle, AppError> {
        warn!(
            guild_id = %player.guild_id(),
            track_id = %track.id,
            start_at_seconds = track.start_at_seconds,
            error = %source,
            "stream does not support the initial seek; playing from the beginning"
        );
        stop_created_handle(&failed_handle);
        let stream = fallback_stream.ok_or_else(|| AppError::Internal {
            context: format!("track {} has no stream for initial-seek fallback", track.id),
        })?;
        self.install_paused_track(player, operation, stream, true)
            .await
    }
}

fn initial_seek_error(
    track: &ResolvedTrack,
    start_at_seconds: u64,
    source: ControlError,
) -> AppError {
    AppError::Voice {
        context: format!(
            "could not seek track {} to {start_at_seconds} seconds: {source}",
            track.id
        ),
    }
}
