use std::{
    collections::HashMap,
    time::{Duration, UNIX_EPOCH},
};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use tracing::warn;
use url::Url;

use crate::{
    error::AppError,
    player::track::ResolvedTrack,
    sources::resolver::{PreparedStream, TrackResolution},
};

/// Protocols Songbird can play through a plain HTTP request. Segmented and live
/// protocols need a different input and would only produce unplayable audio.
const SUPPORTED_STREAM_PROTOCOLS: [&str; 2] = ["https", "http"];

/// `--flat-playlist` entries carry the watch page in `url`, not a media stream.
const FLAT_PLAYLIST_ENTRY_TYPE: &str = "url";

const STREAM_EXPIRY_MARGIN_SECONDS: u64 = 300;

#[derive(Deserialize)]
struct YoutubeMetadata {
    id: Option<String>,
    title: Option<String>,
    webpage_url: Option<String>,
    original_url: Option<String>,
    url: Option<String>,
    duration: Option<f64>,
    channel: Option<String>,
    uploader: Option<String>,
    thumbnail: Option<String>,
    protocol: Option<String>,
    filesize: Option<u64>,
    http_headers: Option<HashMap<String, String>>,
    #[serde(rename = "_type")]
    entry_type: Option<String>,
    #[serde(default)]
    entries: Vec<Option<YoutubeMetadata>>,
}

pub fn parse_tracks(
    document: &str,
    start_at_seconds: Option<u64>,
) -> Result<TrackResolution, AppError> {
    let metadata = parse_document(document)?;
    if metadata.entries.is_empty() {
        let track = resolved_track(metadata, start_at_seconds).map_err(invalid_track_error)?;
        return Ok(TrackResolution {
            tracks: vec![track],
            unavailable: 0,
        });
    }

    parse_collection(metadata.entries)
}

pub fn parse_prepared_stream(document: &str) -> Result<PreparedStream, AppError> {
    let mut metadata = parse_document(document)?;
    let protocol = metadata.protocol.clone();
    prepared_stream(&mut metadata)
        .map(|stream| *stream)
        .ok_or_else(|| unusable_stream_error(protocol.as_deref()))
}

fn parse_document(document: &str) -> Result<YoutubeMetadata, AppError> {
    serde_json::from_str(document).map_err(|source| AppError::Resolution {
        context: format!(
            "invalid yt-dlp JSON at line {}, column {}",
            source.line(),
            source.column()
        ),
    })
}

fn parse_collection(entries: Vec<Option<YoutubeMetadata>>) -> Result<TrackResolution, AppError> {
    let entry_count = entries.len();
    let mut unavailable = 0_usize;
    let mut tracks = Vec::with_capacity(entry_count);

    for entry in entries {
        let Some(metadata) = entry else {
            unavailable += 1;
            continue;
        };
        match resolved_track(metadata, None) {
            Ok(track) => tracks.push(track),
            Err(_) => unavailable += 1,
        }
    }

    log_skipped_entries(entry_count, unavailable);
    if tracks.is_empty() {
        return Err(AppError::Resolution {
            context: "yt-dlp collection did not contain playable entries".to_owned(),
        });
    }

    Ok(TrackResolution {
        tracks,
        unavailable,
    })
}

fn resolved_track(
    mut metadata: YoutubeMetadata,
    start_at_seconds: Option<u64>,
) -> Result<ResolvedTrack, &'static str> {
    let prepared_stream = prepared_stream(&mut metadata);
    let id = required_value(metadata.id, "track ID")?;
    let title = required_value(metadata.title, "track title")?;
    let webpage_url = required_value(
        metadata
            .webpage_url
            .or(metadata.original_url)
            .or_else(|| flat_playlist_url(metadata.entry_type.as_deref(), &id)),
        "webpage URL",
    )?;
    let thumbnail_url = resolved_thumbnail_url(metadata.thumbnail, &id);

    Ok(ResolvedTrack {
        id,
        title,
        webpage_url,
        duration_seconds: duration_seconds(metadata.duration),
        start_at_seconds,
        channel_name: optional_value(metadata.channel.or(metadata.uploader)),
        thumbnail_url,
        prepared_stream,
    })
}

/// Resolution runs with `--format`, so a single video already carries the media
/// URL yt-dlp selected. Playlist entries only carry watch pages, so they get no
/// stream and are prepared individually when they reach the front of the queue.
fn prepared_stream(metadata: &mut YoutubeMetadata) -> Option<Box<PreparedStream>> {
    if metadata.entry_type.as_deref() == Some(FLAT_PLAYLIST_ENTRY_TYPE) {
        return None;
    }
    let protocol = metadata.protocol.as_deref()?;
    if !SUPPORTED_STREAM_PROTOCOLS.contains(&protocol) {
        return None;
    }

    let url = validate_stream_url(metadata.url.as_deref()?).ok()?;
    Some(Box::new(PreparedStream {
        content_length: metadata.filesize.or_else(|| content_length_from_url(&url)),
        reuse_until: stream_reuse_until(&url),
        url,
        headers: stream_headers(metadata.http_headers.take()),
    }))
}

fn stream_reuse_until(stream_url: &str) -> Option<std::time::SystemTime> {
    let expires_at = Url::parse(stream_url)
        .ok()?
        .query_pairs()
        .find_map(|(key, value)| (key == "expire").then(|| value.parse::<u64>().ok()))??;
    let safe_expiry = expires_at.checked_sub(STREAM_EXPIRY_MARGIN_SECONDS)?;
    UNIX_EPOCH.checked_add(Duration::from_secs(safe_expiry))
}

/// YouTube media URLs carry their exact byte length in `clen`, which yt-dlp does
/// not always report as `filesize`. Only an exact length is usable: Songbird turns
/// it into the upper bound of a `range` header, so a guess would cut audio short.
fn content_length_from_url(stream_url: &str) -> Option<u64> {
    Url::parse(stream_url)
        .ok()?
        .query_pairs()
        .find_map(|(key, value)| (key == "clen").then(|| value.parse().ok()))?
}

fn stream_headers(source_headers: Option<HashMap<String, String>>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in source_headers.unwrap_or_default() {
        let parsed_name = HeaderName::from_bytes(name.as_bytes()).ok();
        let parsed_value = HeaderValue::from_str(&value).ok();
        match (parsed_name, parsed_value) {
            (Some(parsed_name), Some(parsed_value)) => {
                headers.insert(parsed_name, parsed_value);
            }
            _ => warn!(header_name = %name, "yt-dlp stream header could not be used"),
        }
    }
    headers
}

fn flat_playlist_url(entry_type: Option<&str>, video_id: &str) -> Option<String> {
    if entry_type != Some(FLAT_PLAYLIST_ENTRY_TYPE) {
        return None;
    }

    let mut url = Url::parse("https://www.youtube.com/watch").ok()?;
    url.query_pairs_mut().append_pair("v", video_id);
    Some(url.to_string())
}

/// Flat playlist results can omit `thumbnail`, even for otherwise playable
/// videos. Deriving the standard YouTube artwork keeps the player panel useful
/// without trusting an invalid URL returned in the metadata.
fn resolved_thumbnail_url(thumbnail: Option<String>, video_id: &str) -> Option<String> {
    thumbnail
        .filter(|url| valid_thumbnail_url(url))
        .or_else(|| youtube_thumbnail_url(video_id))
}

fn valid_thumbnail_url(value: &str) -> bool {
    let Ok(parsed) = Url::parse(value) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
}

pub(crate) fn youtube_thumbnail_url(video_id: &str) -> Option<String> {
    let valid_video_id = video_id.len() == 11
        && video_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    valid_video_id.then(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"))
}

fn duration_seconds(duration: Option<f64>) -> Option<u64> {
    duration
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= u64::MAX as f64)
        .map(|value| value.round() as u64)
}

fn validate_stream_url(stream_url: &str) -> Result<String, AppError> {
    let parsed = Url::parse(stream_url).map_err(|source| AppError::Resolution {
        context: format!("yt-dlp stream URL is invalid: {source}"),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::Resolution {
            context: format!(
                "yt-dlp stream URL uses unsupported protocol: {}",
                parsed.scheme()
            ),
        });
    }
    if parsed.host_str().is_none() {
        return Err(AppError::Resolution {
            context: "yt-dlp stream URL does not contain a host".to_owned(),
        });
    }
    Ok(parsed.to_string())
}

fn required_value(value: Option<String>, field: &'static str) -> Result<String, &'static str> {
    optional_value(value).ok_or(field)
}

fn optional_value(value: Option<String>) -> Option<String> {
    value.and_then(|candidate| {
        let trimmed = candidate.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

fn unusable_stream_error(protocol: Option<&str>) -> AppError {
    AppError::Resolution {
        context: format!(
            "yt-dlp result has no playable HTTP stream URL, protocol was {}",
            protocol.unwrap_or("absent")
        ),
    }
}

fn invalid_track_error(field: &'static str) -> AppError {
    AppError::Resolution {
        context: format!("yt-dlp result is missing {field}"),
    }
}

fn log_skipped_entries(entry_count: usize, skipped_count: usize) {
    if skipped_count == 0 {
        return;
    }

    warn!(
        entry_count,
        skipped_count, "unavailable yt-dlp collection entries skipped"
    );
}
