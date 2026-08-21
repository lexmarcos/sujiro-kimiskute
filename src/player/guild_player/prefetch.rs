use std::time::SystemTime;

use super::GuildPlayer;
use crate::player::{
    playback_state::{StreamPrefetchOutcome, StreamPrefetchResult},
    track::ResolvedTrack,
};

/// Bookkeeping for the background preparation of the next queued track's stream.
#[derive(Default)]
pub(super) struct StreamPrefetchState {
    /// Track whose stream is being prefetched. Only one runs per guild, so a
    /// burst of enqueues cannot pile up yt-dlp processes.
    in_flight_track_id: Option<String>,
    /// Track whose last prefetch failed. It is not retried while it stays at the
    /// front of the queue; playback still resolves it normally when its turn comes.
    failed_track_id: Option<String>,
}

impl GuildPlayer {
    /// Picks the track that will play next when it still lacks a usable stream
    /// and no prefetch is running, marking it as in flight.
    pub(crate) async fn claim_stream_prefetch(&self) -> Option<ResolvedTrack> {
        let mut guard = self.inner.lock().await;
        let state = &mut *guard;
        if state.ensure_active(self.guild_id).is_err()
            || state.stream_prefetch.in_flight_track_id.is_some()
        {
            return None;
        }

        let next = state.queue.peek_next()?;
        if has_reusable_stream(&next.track)
            || state.stream_prefetch.failed_track_id.as_deref() == Some(next.track.id.as_str())
        {
            return None;
        }
        let track = next.track.clone();
        state.stream_prefetch.failed_track_id = None;
        state.stream_prefetch.in_flight_track_id = Some(track.id.clone());
        Some(track)
    }

    pub(crate) async fn finish_stream_prefetch(
        &self,
        track_id: &str,
        result: StreamPrefetchResult,
    ) -> StreamPrefetchOutcome {
        let mut guard = self.inner.lock().await;
        let state = &mut *guard;
        if state.stream_prefetch.in_flight_track_id.as_deref() == Some(track_id) {
            state.stream_prefetch.in_flight_track_id = None;
        }

        match result {
            StreamPrefetchResult::Ready(stream) => {
                if state.queue.attach_stream(track_id, stream) {
                    StreamPrefetchOutcome::Attached
                } else {
                    StreamPrefetchOutcome::Skipped
                }
            }
            StreamPrefetchResult::Busy => StreamPrefetchOutcome::Skipped,
            StreamPrefetchResult::Failed => {
                state.stream_prefetch.failed_track_id = Some(track_id.to_owned());
                StreamPrefetchOutcome::Failed
            }
        }
    }
}

fn has_reusable_stream(track: &ResolvedTrack) -> bool {
    track
        .prepared_stream
        .as_ref()
        .is_some_and(|stream| stream.is_reusable_at(SystemTime::now()))
}
