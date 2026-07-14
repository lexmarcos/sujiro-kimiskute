pub mod config;
pub mod discord;
pub mod error;
pub mod localization;
pub mod player;
pub mod sources;
pub mod state;
pub mod voice;

use std::{process::ExitCode, sync::Arc};

use config::{AppConfig, ConfigError};
use discord::client::{build_client, run_until_shutdown};
use error::AppError;
use songbird::Songbird;
use state::AppState;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    let logging_filter = match AppConfig::logging_filter() {
        Ok(filter) => filter,
        Err(error) => {
            eprintln!("failed to load configuration: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = initialize_logging(&logging_filter) {
        eprintln!("failed to initialize logging: {error}");
        return ExitCode::FAILURE;
    }

    match run_bot().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(error = %error, "application stopped with an error");
            ExitCode::FAILURE
        }
    }
}

fn initialize_logging(filter: &str) -> Result<(), AppError> {
    let environment_filter = EnvFilter::try_new(filter).map_err(|_| ConfigError::InvalidRustLog)?;
    tracing_subscriber::fmt()
        .with_env_filter(environment_filter)
        .try_init()
        .map_err(|source| AppError::Internal {
            context: format!("could not install tracing subscriber: {source}"),
        })
}

async fn run_bot() -> Result<(), AppError> {
    let configuration = Arc::new(AppConfig::load()?);
    info!(
        application_id = configuration.discord_application_id,
        bot_language = %configuration.bot_language,
        "configuration loaded"
    );

    let voice_manager = Songbird::serenity();
    let state = AppState::build(configuration, voice_manager)?;
    let client = build_client(state).await?;

    info!("starting Discord client");
    run_until_shutdown(client).await
}
