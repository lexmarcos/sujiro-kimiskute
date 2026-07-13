pub mod leave;
pub mod pause;
pub mod play;
pub mod queue;
pub mod register;
pub mod resume;
pub mod skip;
pub mod stop;

use std::sync::Arc;

use serenity::{
    all::{CommandInteraction, Context},
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
};
use tracing::{error, info};

use crate::{error::AppError, state::AppState};

pub(super) const MAX_RESPONSE_CHARS: usize = 1_899;

pub async fn dispatch(context: &Context, command: &CommandInteraction, state: &Arc<AppState>) {
    info!(
        guild_id = ?command.guild_id,
        user_id = %command.user.id,
        command = %command.data.name,
        "slash command received"
    );

    let response = if command.guild_id.is_none() {
        respond(
            context,
            command,
            "Este comando só pode ser usado em um servidor.",
            true,
        )
        .await
    } else {
        dispatch_guild_command(context, command, state).await
    };

    if let Err(source) = response {
        error!(
            guild_id = ?command.guild_id,
            user_id = %command.user.id,
            command = %command.data.name,
            error = %source,
            "failed to respond to slash command"
        );
    }
}

async fn dispatch_guild_command(
    context: &Context,
    command: &CommandInteraction,
    state: &Arc<AppState>,
) -> Result<(), serenity::Error> {
    match command.data.name.as_str() {
        "play" => play::run(context, command, state).await,
        "pause" => pause::run(context, command, state).await,
        "resume" => resume::run(context, command, state).await,
        "skip" => skip::run(context, command, state).await,
        "stop" => stop::run(context, command, state).await,
        "queue" => queue::run(context, command, state).await,
        "leave" => leave::run(context, command, state).await,
        _ => respond(context, command, "Comando não reconhecido.", true).await,
    }
}

pub(super) async fn respond(
    context: &Context,
    command: &CommandInteraction,
    content: &str,
    ephemeral: bool,
) -> Result<(), serenity::Error> {
    let content = truncate_text(content, MAX_RESPONSE_CHARS);
    let message = CreateInteractionResponseMessage::new()
        .content(content)
        .ephemeral(ephemeral);
    command
        .create_response(&context.http, CreateInteractionResponse::Message(message))
        .await
}

pub(super) fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    if max_chars == 0 {
        return String::new();
    }

    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

pub(super) async fn respond_app_error(
    context: &Context,
    command: &CommandInteraction,
    source: AppError,
) -> Result<(), serenity::Error> {
    error!(
        guild_id = ?command.guild_id,
        user_id = %command.user.id,
        command = %command.data.name,
        error = %source,
        "slash command operation failed"
    );
    respond(context, command, &source.discord_message(), true).await
}
