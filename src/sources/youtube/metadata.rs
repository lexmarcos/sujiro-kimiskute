use serde::Deserialize;
use tracing::warn;
use url::Url;

use crate::{error::AppError, player::track::ResolvedTrack};

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
    #[serde(rename = "_type")]
    entry_type: Option<String>,
    #[serde(default)]
    entries: Vec<Option<YoutubeMetadata>>,
}

pub fn parse_tracks(document: &str) -> Result<Vec<ResolvedTrack>, AppError> {
    let metadata = parse_document(document)?;
    if is_collection(&metadata) {
        return parse_collection(metadata.entries);
    }

    Ok(vec![resolved_track(metadata).map_err(invalid_track_error)?])
}

pub fn parse_stream_url(document: &str) -> Result<String, AppError> {
    let metadata = parse_document(document)?;
    let stream_url = required_value(metadata.url, "stream URL").map_err(invalid_track_error)?;
    validate_stream_url(&stream_url)
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

fn is_collection(metadata: &YoutubeMetadata) -> bool {
    !metadata.entries.is_empty()
        || matches!(
            metadata.entry_type.as_deref(),
            Some("playlist" | "multi_video")
        )
}

fn parse_collection(entries: Vec<Option<YoutubeMetadata>>) -> Result<Vec<ResolvedTrack>, AppError> {
    let entry_count = entries.len();
    let mut skipped_count = 0_usize;
    let mut tracks = Vec::with_capacity(entry_count);

    for entry in entries {
        let Some(metadata) = entry else {
            skipped_count += 1;
            continue;
        };
        match resolved_track(metadata) {
            Ok(track) => tracks.push(track),
            Err(_) => skipped_count += 1,
        }
    }

    log_skipped_entries(entry_count, skipped_count);
    if tracks.is_empty() {
        return Err(AppError::Resolution {
            context: "yt-dlp collection did not contain playable entries".to_owned(),
        });
    }

    Ok(tracks)
}

fn resolved_track(metadata: YoutubeMetadata) -> Result<ResolvedTrack, &'static str> {
    let id = required_value(metadata.id, "track ID")?;
    let title = required_value(metadata.title, "track title")?;
    let webpage_url = required_value(
        metadata
            .webpage_url
            .or(metadata.original_url)
            .or_else(|| flat_playlist_url(metadata.entry_type.as_deref(), &id)),
        "webpage URL",
    )?;

    Ok(ResolvedTrack {
        id,
        title,
        webpage_url,
        duration_seconds: duration_seconds(metadata.duration),
        channel_name: optional_value(metadata.channel.or(metadata.uploader)),
        thumbnail_url: optional_value(metadata.thumbnail),
    })
}

fn flat_playlist_url(entry_type: Option<&str>, video_id: &str) -> Option<String> {
    if entry_type != Some("url") {
        return None;
    }

    let mut url = Url::parse("https://www.youtube.com/watch").ok()?;
    url.query_pairs_mut().append_pair("v", video_id);
    Some(url.to_string())
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
