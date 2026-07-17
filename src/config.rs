use std::{env, num::ParseIntError, path::PathBuf, time::Duration};

use thiserror::Error;

use crate::localization::BotLanguage;

const DEFAULT_AUTO_LEAVE_SECONDS: &str = "120";
const DEFAULT_BOT_ACTIVITY_MESSAGE: &str = "música";
const DEFAULT_BOT_ACTIVITY_TYPE: &str = "listening";
const DEFAULT_BOT_LANGUAGE: &str = "pt-BR";
const DEFAULT_MAX_CONCURRENT_RESOLUTIONS: &str = "4";
const DEFAULT_MAX_QUEUE_SIZE: &str = "50";
const DEFAULT_PLAYER_PANEL_UPDATE_SECONDS: &str = "5";
const DEFAULT_RUST_LOG: &str = "info";
const DEFAULT_YT_DLP_PATH: &str = "yt-dlp";
const DEFAULT_YT_DLP_TIMEOUT_SECONDS: &str = "20";

pub struct AppConfig {
    pub discord_token: String,
    pub discord_application_id: u64,
    pub bot_language: BotLanguage,
    pub(crate) bot_activity: BotActivityConfig,
    pub yt_dlp_path: PathBuf,
    pub yt_dlp_extra_args: Vec<String>,
    pub yt_dlp_timeout: Duration,
    pub auto_leave_timeout: Duration,
    pub player_panel_update_interval: Option<Duration>,
    pub max_queue_size: usize,
    pub max_concurrent_resolutions: usize,
    pub rust_log: String,
}

impl AppConfig {
    pub fn logging_filter() -> Result<String, ConfigError> {
        load_dotenv()?;
        configured_rust_log()
    }

    pub fn load() -> Result<Self, ConfigError> {
        load_dotenv()?;

        let discord_token = required_value("DISCORD_TOKEN")?;
        let discord_application_id = positive_u64(
            "DISCORD_APPLICATION_ID",
            required_value("DISCORD_APPLICATION_ID")?,
        )?;
        let bot_language = configured_bot_language()?;
        let bot_activity = configured_bot_activity()?;
        let yt_dlp_path = non_empty_value(
            "YT_DLP_PATH",
            optional_value("YT_DLP_PATH", DEFAULT_YT_DLP_PATH)?,
        )?;
        let yt_dlp_extra_args = extra_arguments()?;
        let yt_dlp_timeout =
            configured_duration("YT_DLP_TIMEOUT_SECONDS", DEFAULT_YT_DLP_TIMEOUT_SECONDS)?;
        let auto_leave_timeout =
            configured_duration("AUTO_LEAVE_SECONDS", DEFAULT_AUTO_LEAVE_SECONDS)?;
        let player_panel_update_interval = configured_optional_duration(
            "PLAYER_PANEL_UPDATE_SECONDS",
            DEFAULT_PLAYER_PANEL_UPDATE_SECONDS,
        )?;
        let max_queue_size = configured_usize("MAX_QUEUE_SIZE", DEFAULT_MAX_QUEUE_SIZE)?;
        let max_concurrent_resolutions = configured_usize(
            "MAX_CONCURRENT_RESOLUTIONS",
            DEFAULT_MAX_CONCURRENT_RESOLUTIONS,
        )?;
        let rust_log = configured_rust_log()?;

        Ok(Self {
            discord_token,
            discord_application_id,
            bot_language,
            bot_activity,
            yt_dlp_path: PathBuf::from(yt_dlp_path),
            yt_dlp_extra_args,
            yt_dlp_timeout,
            auto_leave_timeout,
            player_panel_update_interval,
            max_queue_size,
            max_concurrent_resolutions,
            rust_log,
        })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not load the .env file")]
    Dotenv {
        #[source]
        source: dotenvy::Error,
    },

    #[error("required environment variable {name} is missing or empty")]
    MissingRequired { name: &'static str },

    #[error("environment variable {name} contains non-Unicode data")]
    NonUnicode { name: &'static str },

    #[error("environment variable {name} cannot be empty")]
    Empty { name: &'static str },

    #[error("environment variable {name} has invalid integer value {value:?}")]
    InvalidInteger {
        name: &'static str,
        value: String,
        #[source]
        source: ParseIntError,
    },

    #[error("environment variable {name} must be positive, received {value}")]
    NotPositive { name: &'static str, value: u64 },

    #[error("environment variable YT_DLP_EXTRA_ARGS contains unmatched quotes")]
    InvalidExtraArguments,

    #[error("environment variable RUST_LOG contains an invalid tracing filter")]
    InvalidRustLog,

    #[error(
        "environment variable BOT_LANGUAGE has unsupported value {value:?}; expected pt-BR or en-US"
    )]
    InvalidBotLanguage { value: String },

    #[error(
        "environment variable BOT_ACTIVITY_TYPE has unsupported value {value:?}; expected playing, watching, listening, or competing"
    )]
    InvalidBotActivityType { value: String },
}

pub(crate) struct BotActivityConfig {
    activity_type: BotActivityType,
    message: String,
}

impl BotActivityConfig {
    pub(crate) fn activity_type(&self) -> BotActivityType {
        self.activity_type
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BotActivityType {
    Playing,
    Watching,
    Listening,
    Competing,
}

impl BotActivityType {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "playing" => Some(Self::Playing),
            "watching" => Some(Self::Watching),
            "listening" => Some(Self::Listening),
            "competing" => Some(Self::Competing),
            _ => None,
        }
    }
}

fn load_dotenv() -> Result<(), ConfigError> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ConfigError::Dotenv { source }),
    }
}

fn required_value(name: &'static str) -> Result<String, ConfigError> {
    let Some(value) = environment_value(name)? else {
        return Err(ConfigError::MissingRequired { name });
    };
    if value.trim().is_empty() {
        return Err(ConfigError::MissingRequired { name });
    }
    Ok(value)
}

fn optional_value(name: &'static str, default: &str) -> Result<String, ConfigError> {
    Ok(environment_value(name)?.unwrap_or_else(|| default.to_owned()))
}

fn environment_value(name: &'static str) -> Result<Option<String>, ConfigError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError::NonUnicode { name }),
    }
}

fn non_empty_value(name: &'static str, value: String) -> Result<String, ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::Empty { name });
    }
    Ok(value)
}

fn configured_duration(name: &'static str, default: &str) -> Result<Duration, ConfigError> {
    let seconds = positive_u64(name, optional_value(name, default)?)?;
    Ok(Duration::from_secs(seconds))
}

fn configured_optional_duration(
    name: &'static str,
    default: &str,
) -> Result<Option<Duration>, ConfigError> {
    let value = optional_value(name, default)?;
    let seconds = value
        .trim()
        .parse::<u64>()
        .map_err(|source| ConfigError::InvalidInteger {
            name,
            value: value.clone(),
            source,
        })?;
    Ok((seconds > 0).then(|| Duration::from_secs(seconds)))
}

fn configured_usize(name: &'static str, default: &str) -> Result<usize, ConfigError> {
    let value = optional_value(name, default)?;
    let parsed = value
        .trim()
        .parse::<usize>()
        .map_err(|source| ConfigError::InvalidInteger {
            name,
            value: value.clone(),
            source,
        })?;
    if parsed == 0 {
        return Err(ConfigError::NotPositive { name, value: 0 });
    }
    Ok(parsed)
}

fn positive_u64(name: &'static str, value: String) -> Result<u64, ConfigError> {
    let parsed = value
        .trim()
        .parse::<u64>()
        .map_err(|source| ConfigError::InvalidInteger {
            name,
            value: value.clone(),
            source,
        })?;
    if parsed == 0 {
        return Err(ConfigError::NotPositive {
            name,
            value: parsed,
        });
    }
    Ok(parsed)
}

fn extra_arguments() -> Result<Vec<String>, ConfigError> {
    let value = optional_value("YT_DLP_EXTRA_ARGS", "")?;
    shlex::split(&value).ok_or(ConfigError::InvalidExtraArguments)
}

fn configured_rust_log() -> Result<String, ConfigError> {
    non_empty_value("RUST_LOG", optional_value("RUST_LOG", DEFAULT_RUST_LOG)?)
}

fn configured_bot_language() -> Result<BotLanguage, ConfigError> {
    let value = optional_value("BOT_LANGUAGE", DEFAULT_BOT_LANGUAGE)?;
    BotLanguage::parse(&value).ok_or(ConfigError::InvalidBotLanguage { value })
}

fn configured_bot_activity() -> Result<BotActivityConfig, ConfigError> {
    let activity_type_value = optional_value("BOT_ACTIVITY_TYPE", DEFAULT_BOT_ACTIVITY_TYPE)?;
    let activity_type = BotActivityType::parse(&activity_type_value).ok_or(
        ConfigError::InvalidBotActivityType {
            value: activity_type_value,
        },
    )?;
    let message = non_empty_value(
        "BOT_ACTIVITY_MESSAGE",
        optional_value("BOT_ACTIVITY_MESSAGE", DEFAULT_BOT_ACTIVITY_MESSAGE)?,
    )?;

    Ok(BotActivityConfig {
        activity_type,
        message,
    })
}
