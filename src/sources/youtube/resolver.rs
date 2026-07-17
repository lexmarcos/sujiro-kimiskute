use std::{sync::Arc, time::Instant};

use async_trait::async_trait;
use tracing::{info, warn};
use url::Url;

use crate::{
    error::AppError,
    player::track::ResolvedTrack,
    sources::{
        resolver::{TrackInputKind, TrackResolution, TrackResolver, normalize_track_input},
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

    async fn resolve_input(&self, input: ResolvedInput) -> Result<TrackResolution, AppError> {
        let arguments = resolution_arguments(&input, self.max_playlist_size);
        let document = self.process.execute(&arguments).await?;
        let mut resolution = parse_tracks(&document, input.start_at_seconds())?;
        resolution
            .tracks
            .truncate(input.result_limit(self.max_playlist_size));

        if resolution.tracks.is_empty() {
            return Err(AppError::Resolution {
                context: "yt-dlp returned no playable YouTube results".to_owned(),
            });
        }
        Ok(resolution)
    }
}

#[async_trait]
impl TrackResolver for YoutubeResolver {
    fn classify(&self, input: &str) -> Result<TrackInputKind, AppError> {
        Ok(ResolvedInput::parse(input)?.kind())
    }

    async fn resolve(&self, input: &str) -> Result<TrackResolution, AppError> {
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
    VideoUrl {
        url: String,
        start_at_seconds: Option<u64>,
    },
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
        Ok(Self::VideoUrl {
            start_at_seconds: start_at_seconds(&url),
            url: url.to_string(),
        })
    }

    fn from_url(url: Url) -> Result<Self, AppError> {
        validate_youtube_url(&url)?;
        if is_playlist_url(&url) {
            return Ok(Self::PlaylistUrl(url.to_string()));
        }
        Ok(Self::VideoUrl {
            start_at_seconds: start_at_seconds(&url),
            url: url.to_string(),
        })
    }

    fn value(&self) -> &str {
        match self {
            Self::Search(value) | Self::PlaylistUrl(value) => value,
            Self::VideoUrl { url, .. } => url,
        }
    }

    fn kind(&self) -> TrackInputKind {
        match self {
            Self::Search(_) => TrackInputKind::Search,
            Self::VideoUrl { .. } => TrackInputKind::Track,
            Self::PlaylistUrl(_) => TrackInputKind::Collection,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self {
            Self::Search(_) => "search",
            Self::VideoUrl { .. } => "video_url",
            Self::PlaylistUrl(_) => "playlist_url",
        }
    }

    fn result_limit(&self, max_playlist_size: usize) -> usize {
        match self {
            Self::PlaylistUrl(_) => max_playlist_size,
            Self::Search(_) | Self::VideoUrl { .. } => 1,
        }
    }

    fn start_at_seconds(&self) -> Option<u64> {
        match self {
            Self::VideoUrl {
                start_at_seconds, ..
            } => *start_at_seconds,
            Self::Search(_) | Self::PlaylistUrl(_) => None,
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
        ResolvedInput::VideoUrl { url, .. } => {
            arguments.push("--no-playlist".to_owned());
            arguments.push(url.clone());
        }
        ResolvedInput::PlaylistUrl(url) => {
            arguments.push("--flat-playlist".to_owned());
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
    let path = url.path().trim_end_matches('/');
    if path == "/playlist" {
        return true;
    }
    if has_selected_video(url) || is_direct_video_path(url, path) {
        return false;
    }
    url.query_pairs().any(|(key, _)| key == "list")
}

fn has_selected_video(url: &Url) -> bool {
    url.query_pairs()
        .any(|(key, value)| key == "v" && !value.is_empty())
}

fn is_direct_video_path(url: &Url, path: &str) -> bool {
    url.host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("youtu.be"))
        && path.len() > 1
        || path.starts_with("/shorts/")
        || path.starts_with("/live/")
        || path.starts_with("/embed/")
}

fn start_at_seconds(url: &Url) -> Option<u64> {
    let query_value = url
        .query_pairs()
        .find_map(|(key, value)| matches!(key.as_ref(), "t" | "start").then_some(value));
    let raw = query_value.as_deref().or_else(|| {
        url.fragment().and_then(|fragment| {
            fragment
                .strip_prefix("t=")
                .or_else(|| fragment.strip_prefix("start="))
        })
    })?;
    parse_timestamp(raw)
}

fn parse_timestamp(raw: &str) -> Option<u64> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return None;
    }
    if let Ok(seconds) = normalized.parse::<u64>() {
        return (seconds > 0).then_some(seconds);
    }

    let mut total = 0_u64;
    let mut number = String::new();
    for character in normalized.chars() {
        if character.is_ascii_digit() {
            number.push(character);
            continue;
        }
        let value = number.parse::<u64>().ok()?;
        number.clear();
        let multiplier = match character.to_ascii_lowercase() {
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        total = total.checked_add(value.checked_mul(multiplier)?)?;
    }
    if !number.is_empty() {
        return None;
    }
    (total > 0).then_some(total)
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
    result: &Result<TrackResolution, AppError>,
) {
    let duration_ms = started_at.elapsed().as_secs_f64() * 1_000.0;
    match result {
        Ok(resolution) => info!(
            input_kind,
            input_length,
            result_count = resolution.tracks.len(),
            unavailable_count = resolution.unavailable,
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
