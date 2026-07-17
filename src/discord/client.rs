use std::sync::Arc;

use serenity::{
    Client,
    model::{gateway::GatewayIntents, id::ApplicationId},
};
use songbird::SerenityInit;
use tokio::task::{JoinError, JoinHandle};
use tracing::info;

use crate::{error::AppError, state::AppState};

use super::handler::DiscordEventHandler;

pub async fn build_client(state: Arc<AppState>) -> Result<Client, AppError> {
    let intents: GatewayIntents = GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES;
    let application_id: ApplicationId = ApplicationId::new(state.config.discord_application_id);

    let client = Client::builder(&state.config.discord_token, intents)
        .application_id(application_id)
        .event_handler(DiscordEventHandler::new(Arc::clone(&state)))
        .register_songbird_with(Arc::clone(&state.songbird))
        .await
        .map_err(AppError::from)?;
    if !state.player_panels.initialize(Arc::clone(&client.http)) {
        return Err(AppError::Internal {
            context: "player panel HTTP client was already initialized".to_owned(),
        });
    }
    Ok(client)
}

pub async fn run_until_shutdown(mut client: Client) -> Result<(), AppError> {
    let shard_manager = Arc::clone(&client.shard_manager);
    let mut client_task: JoinHandle<Result<(), serenity::Error>> =
        tokio::spawn(async move { client.start().await });

    tokio::select! {
        task_result = &mut client_task => {
            task_result.map_err(client_task_error)??;
        }
        signal_result = tokio::signal::ctrl_c() => {
            signal_result.map_err(shutdown_signal_error)?;
            info!("shutdown signal received");
            shard_manager.shutdown_all().await;
            client_task.await.map_err(client_task_error)??;
        }
    }

    info!("Discord client stopped");
    Ok(())
}

fn client_task_error(source: JoinError) -> AppError {
    AppError::Internal {
        context: format!("Discord client task failed: {source}"),
    }
}

fn shutdown_signal_error(source: std::io::Error) -> AppError {
    AppError::Internal {
        context: format!("could not listen for shutdown signal: {source}"),
    }
}
