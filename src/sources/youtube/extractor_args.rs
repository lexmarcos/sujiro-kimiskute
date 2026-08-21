//! Folds the bot's own YouTube extractor arguments into the ones configured
//! through `YT_DLP_EXTRA_ARGS`.
//!
//! yt-dlp keeps only the last `--extractor-args` value given for the same
//! extractor: its help text allows repeating the option "to give arguments for
//! different extractors", and a repeated `youtube:` value replaces the earlier
//! one instead of merging with it. Appending a separate flag after the user's
//! would therefore silently drop their `player_client` or `po_token` settings,
//! so the bot rewrites the user's `youtube:` value (or adds one) instead.

const EXTRACTOR_ARGS_FLAG: &str = "--extractor-args";
const EXTRACTOR_ARGS_INLINE_PREFIX: &str = "--extractor-args=";
const YOUTUBE_EXTRACTOR_KEY: &str = "youtube";
const SKIP_KEY_PREFIX: &str = "skip=";

/// Manifests the bot never benefits from: it only plays plain HTTP streams, so
/// HLS and DASH formats would be rejected after costing one request each, and a
/// format selector landing on one of them would fail playback outright.
const REQUIRED_SKIPS: [&str; 2] = ["hls", "dash"];

/// Returns the configured yt-dlp arguments with the bot's YouTube extractor
/// arguments merged in, preserving every user-provided argument.
pub fn with_required_youtube_arguments(extra_arguments: Vec<String>) -> Vec<String> {
    let mut merged: Vec<String> = Vec::with_capacity(extra_arguments.len() + 2);
    let mut youtube_value_seen = false;
    let mut arguments = extra_arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == EXTRACTOR_ARGS_FLAG {
            merged.push(argument);
            let Some(value) = arguments.next() else {
                break;
            };
            youtube_value_seen |= is_youtube_value(&value);
            merged.push(merge_youtube_value(value));
            continue;
        }
        if let Some(value) = argument.strip_prefix(EXTRACTOR_ARGS_INLINE_PREFIX) {
            youtube_value_seen |= is_youtube_value(value);
            let merged_value = merge_youtube_value(value.to_owned());
            merged.push(format!("{EXTRACTOR_ARGS_INLINE_PREFIX}{merged_value}"));
            continue;
        }
        merged.push(argument);
    }

    if !youtube_value_seen {
        merged.push(EXTRACTOR_ARGS_FLAG.to_owned());
        merged.push(format!("{YOUTUBE_EXTRACTOR_KEY}:{}", required_skip_pair()));
    }
    merged
}

/// Splits `IE_KEY:ARGS` at the first colon, the same way yt-dlp parses it.
fn split_extractor_value(value: &str) -> Option<(&str, &str)> {
    value.split_once(':')
}

fn is_youtube_value(value: &str) -> bool {
    split_extractor_value(value)
        .is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case(YOUTUBE_EXTRACTOR_KEY))
}

fn merge_youtube_value(value: String) -> String {
    if !is_youtube_value(&value) {
        return value;
    }
    let Some((key, arguments)) = split_extractor_value(&value) else {
        return value;
    };

    let mut pairs: Vec<String> = arguments
        .split(';')
        .filter(|pair| !pair.trim().is_empty())
        .map(str::to_owned)
        .collect();
    match pairs
        .iter_mut()
        .find(|pair| pair.trim_start().starts_with(SKIP_KEY_PREFIX))
    {
        Some(skip_pair) => *skip_pair = with_required_skips(skip_pair),
        None => pairs.push(required_skip_pair()),
    }
    format!("{key}:{}", pairs.join(";"))
}

/// Extends an existing `skip=a,b` pair instead of adding a second one, because
/// yt-dlp keeps only the last duplicate key inside a single value too.
fn with_required_skips(skip_pair: &str) -> String {
    let existing = skip_pair
        .trim_start()
        .strip_prefix(SKIP_KEY_PREFIX)
        .unwrap_or_default();
    let mut values: Vec<&str> = existing
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect();
    for required in REQUIRED_SKIPS {
        if !values
            .iter()
            .any(|value| value.eq_ignore_ascii_case(required))
        {
            values.push(required);
        }
    }
    format!("{SKIP_KEY_PREFIX}{}", values.join(","))
}

fn required_skip_pair() -> String {
    format!("{SKIP_KEY_PREFIX}{}", REQUIRED_SKIPS.join(","))
}
