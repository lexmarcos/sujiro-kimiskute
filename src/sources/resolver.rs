use async_trait::async_trait;
use reqwest::header::HeaderMap;
use std::time::SystemTime;

use crate::{error::AppError, player::track::ResolvedTrack};

pub const MAX_TRACK_INPUT_CHARS: u16 = 500;

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TrackInputKind {
    Search,
    Track,
    Collection,
}

pub struct TrackResolution {
    pub tracks: Vec<ResolvedTrack>,
    pub unavailable: usize,
}

/// Everything the audio driver needs to fetch a track's compressed stream.
#[derive(Clone)]
pub struct PreparedStream {
    pub url: String,
    /// Byte length of the stream when the source reported it. YouTube throttles
    /// range-less requests to a trickle, and Songbird only sends a bounded
    /// `range` header when it knows the length.
    pub content_length: Option<u64>,
    /// Headers the source requires, such as the user agent its URL was signed for.
    pub headers: HeaderMap,
    /// Last instant at which a stream selected during resolution may safely be
    /// reused. Sources compute this from their own URL lifetime rules.
    pub reuse_until: Option<SystemTime>,
}

/// Whether a stream already attached to a track may be replayed. Resolution
/// yields a usable stream for single tracks, so the common path can skip a
/// second process launch; a playback failure must not reuse the same URL.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum StreamReuse {
    Allowed,
    Forbidden,
}

#[async_trait]
pub trait TrackResolver: Send + Sync {
    fn classify(&self, input: &str) -> Result<TrackInputKind, AppError>;

    async fn resolve(&self, input: &str) -> Result<TrackResolution, AppError>;

    async fn prepare_stream(
        &self,
        track: &ResolvedTrack,
        reuse: StreamReuse,
    ) -> Result<PreparedStream, AppError>;
}

pub fn normalize_track_input(input: &str) -> Result<&str, AppError> {
    let normalized = input.trim();
    if normalized.is_empty() {
        return Err(AppError::InvalidInput {
            reason: "track input must not be empty".to_owned(),
        });
    }
    if normalized.chars().count() > usize::from(MAX_TRACK_INPUT_CHARS) {
        return Err(AppError::InvalidInput {
            reason: format!("track input exceeds {MAX_TRACK_INPUT_CHARS} characters"),
        });
    }
    Ok(normalized)
}
