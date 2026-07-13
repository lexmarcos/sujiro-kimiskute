use std::{sync::Arc, time::Instant};

use async_trait::async_trait;
use tracing::{info, warn};
use url::Url;

use crate::{
    error::AppError,
    player::track::ResolvedTrack,
    sources::{
        resolver::{TrackResolver, normalize_track_input},
        youtube::{
            metadata::{parse_stream_url, parse_tracks},
            process::YoutubeProcess,
        },
    },
};

const AUDIO_FORMAT: &str = "bestaudio[acodec=opus]/bestaudio";

pub struct YoutubeResolver {
    process: Arc<YoutubeProcess>,
    max_playlist_size: usize,
}

impl YoutubeResolver {
    pub fn new(process: Arc<YoutubeProcess>, max_playlist_size: usize) -> Result<Self, AppError> {
        if max_playlist_size == 0 {
            return Err(AppError::InvalidInput {
                reason: "YouTube playlist maximum size must be positive, received 0".to_owned(),
            });
        }
        Ok(Self {
            process,
            max_playlist_size,
        })
    }

    async fn resolve_input(&self, input: ResolvedInput) -> Result<Vec<ResolvedTrack>, AppError> {
        let arguments = resolution_arguments(&input, self.max_playlist_size);
        let document = self.process.execute(&arguments).await?;
        let mut tracks = parse_tracks(&document)?;
        tracks.truncate(input.result_limit(self.max_playlist_size));

        if tracks.is_empty() {
            return Err(AppError::Resolution {
                context: "yt-dlp returned no playable YouTube results".to_owned(),
            });
        }
        Ok(tracks)
    }
}

#[async_trait]
impl TrackResolver for YoutubeResolver {
    async fn resolve(&self, input: &str) -> Result<Vec<ResolvedTrack>, AppError> {
        let resolved_input = ResolvedInput::parse(input)?;
        let input_kind = resolved_input.kind_name();
        let input_length = resolved_input.value().len();
        let started_at = Instant::now();

        let result = self.resolve_input(resolved_input).await;
        log_resolution(input_kind, input_length, started_at, &result);
        result
    }

    async fn prepare_stream(&self, track: &ResolvedTrack) -> Result<String, AppError> {
        let input_length = track.webpage_url.len();
        let started_at = Instant::now();
        let input = ResolvedInput::parse_video_url(&track.webpage_url)?;
        let document = self.process.execute(&resolution_arguments(&input, 1)).await;
        let result = match document {
            Ok(document) => parse_stream_url(&document),
            Err(error) => Err(error),
        };

        log_stream_preparation(input_length, started_at, &result);
        result
    }
}

enum ResolvedInput {
    Search(String),
    VideoUrl(String),
    PlaylistUrl(String),
}

impl ResolvedInput {
    fn parse(input: &str) -> Result<Self, AppError> {
        let normalized = normalize_track_input(input)?;

        match Url::parse(normalized) {
            Ok(url) => Self::from_url(url),
            Err(source) if looks_like_url(normalized) => Err(AppError::InvalidInput {
                reason: format!("invalid YouTube URL: {source}"),
            }),
            Err(_) => Ok(Self::Search(normalized.to_owned())),
        }
    }

    fn parse_video_url(input: &str) -> Result<Self, AppError> {
        let url = Url::parse(input.trim()).map_err(|source| AppError::InvalidInput {
            reason: format!("track webpage URL is invalid: {source}"),
        })?;
        validate_youtube_url(&url)?;
        Ok(Self::VideoUrl(url.to_string()))
    }

    fn from_url(url: Url) -> Result<Self, AppError> {
        validate_youtube_url(&url)?;
        let normalized = url.to_string();
        if is_playlist_url(&url) {
            return Ok(Self::PlaylistUrl(normalized));
        }
        Ok(Self::VideoUrl(normalized))
    }

    fn value(&self) -> &str {
        match self {
            Self::Search(value) | Self::VideoUrl(value) | Self::PlaylistUrl(value) => value,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self {
            Self::Search(_) => "search",
            Self::VideoUrl(_) => "video_url",
            Self::PlaylistUrl(_) => "playlist_url",
        }
    }

    fn result_limit(&self, max_playlist_size: usize) -> usize {
        match self {
            Self::PlaylistUrl(_) => max_playlist_size,
            Self::Search(_) | Self::VideoUrl(_) => 1,
        }
    }
}

fn resolution_arguments(input: &ResolvedInput, max_playlist_size: usize) -> Vec<String> {
    let mut arguments = base_arguments();
    match input {
        ResolvedInput::Search(query) => {
            arguments.push("--no-playlist".to_owned());
            arguments.push(format!("ytsearch1:{query}"));
        }
        ResolvedInput::VideoUrl(url) => {
            arguments.push("--no-playlist".to_owned());
            arguments.push(url.clone());
        }
        ResolvedInput::PlaylistUrl(url) => {
            arguments.push("--playlist-end".to_owned());
            arguments.push(max_playlist_size.to_string());
            arguments.push(url.clone());
        }
    }
    arguments
}

fn base_arguments() -> Vec<String> {
    [
        "--dump-single-json",
        "--no-warnings",
        "--no-progress",
        "--no-call-home",
        "--skip-download",
        "--format",
        AUDIO_FORMAT,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn validate_youtube_url(url: &Url) -> Result<(), AppError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::InvalidInput {
            reason: format!("unsupported URL protocol: {}", url.scheme()),
        });
    }

    let Some(host) = url.host_str() else {
        return Err(AppError::InvalidInput {
            reason: "YouTube URL does not contain a host".to_owned(),
        });
    };
    if is_youtube_host(host) {
        return Ok(());
    }

    Err(AppError::InvalidInput {
        reason: format!("unsupported YouTube URL host: {host}"),
    })
}

fn is_youtube_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("youtube.com")
        || host
            .to_ascii_lowercase()
            .strip_suffix(".youtube.com")
            .is_some_and(|prefix| !prefix.is_empty())
        || host.eq_ignore_ascii_case("youtu.be")
}

fn is_playlist_url(url: &Url) -> bool {
    url.path().trim_end_matches('/') == "/playlist"
        || url.query_pairs().any(|(key, _)| key == "list")
}

fn looks_like_url(input: &str) -> bool {
    let lowercase = input.to_ascii_lowercase();
    lowercase.contains("://")
        || lowercase.starts_with("//")
        || lowercase.starts_with("http//")
        || lowercase.starts_with("https//")
        || lowercase.starts_with("http:")
        || lowercase.starts_with("https:")
        || lowercase.starts_with("www.")
        || lowercase == "youtube.com"
        || lowercase.starts_with("youtube.com/")
        || lowercase.starts_with("youtube.com:")
        || lowercase.starts_with("youtube.com?")
        || lowercase.starts_with("youtube.com#")
        || lowercase == "youtu.be"
        || lowercase.starts_with("youtu.be/")
        || lowercase.starts_with("youtu.be:")
        || lowercase.starts_with("youtu.be?")
        || lowercase.starts_with("youtu.be#")
}

fn log_resolution(
    input_kind: &str,
    input_length: usize,
    started_at: Instant,
    result: &Result<Vec<ResolvedTrack>, AppError>,
) {
    let duration_ms = started_at.elapsed().as_secs_f64() * 1_000.0;
    match result {
        Ok(tracks) => info!(
            input_kind,
            input_length,
            result_count = tracks.len(),
            duration_ms,
            "YouTube resolution finished"
        ),
        Err(_) => warn!(
            input_kind,
            input_length,
            result_count = 0,
            duration_ms,
            "YouTube resolution failed"
        ),
    }
}

fn log_stream_preparation(
    input_length: usize,
    started_at: Instant,
    result: &Result<String, AppError>,
) {
    let duration_ms = started_at.elapsed().as_secs_f64() * 1_000.0;
    match result {
        Ok(_) => info!(
            input_kind = "stream_refresh",
            input_length,
            result_count = 1,
            duration_ms,
            "YouTube stream preparation finished"
        ),
        Err(_) => warn!(
            input_kind = "stream_refresh",
            input_length,
            result_count = 0,
            duration_ms,
            "YouTube stream preparation failed"
        ),
    }
}
