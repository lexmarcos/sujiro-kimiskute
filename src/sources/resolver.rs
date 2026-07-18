use async_trait::async_trait;

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

#[async_trait]
pub trait TrackResolver: Send + Sync {
    fn classify(&self, input: &str) -> Result<TrackInputKind, AppError>;

    async fn resolve(&self, input: &str) -> Result<TrackResolution, AppError>;

    async fn prepare_stream(&self, track: &ResolvedTrack) -> Result<String, AppError>;
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
