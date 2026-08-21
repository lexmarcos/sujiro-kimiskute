use serenity::model::id::UserId;

use crate::sources::resolver::PreparedStream;

#[derive(Clone)]
pub struct ResolvedTrack {
    pub id: String,
    pub title: String,
    pub webpage_url: String,
    pub duration_seconds: Option<u64>,
    pub start_at_seconds: Option<u64>,
    pub channel_name: Option<String>,
    pub thumbnail_url: Option<String>,
    /// Stream selected while resolving or prefetching the track, when the source
    /// provided one. Signed source URLs expire, so `PreparedStream::is_reusable_at`
    /// decides whether it is still usable.
    /// Boxed to keep queued tracks small: most of them carry no stream yet.
    pub prepared_stream: Option<Box<PreparedStream>>,
}

#[derive(Clone)]
pub struct QueuedTrack {
    pub track: ResolvedTrack,
    pub requested_by: UserId,
}
